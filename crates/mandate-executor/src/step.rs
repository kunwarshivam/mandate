//! The live step: the only producer of effects.

use crate::batch::Batch;
use crate::error::ExecutorError;
use crate::ids::{FLATTEN, WATCHDOG};
use crate::intent::{received, release_held, resume};
use crate::kill::kill_switch;
use crate::orders::{
    absent, account, cancelled, described, duplicate, fill, lookups_due, reject, silence,
};
use crate::payload::optional_text;
use crate::ports::{BindingGateSource, Ports};
use crate::protection::{
    bound, breach, cancel_openings, ladder_steps, new_day, overdue_openings, settle, watchdog,
};
use crate::reconcile::run;
use crate::state::{EVERY_AGENT, ExecutorState, UnresolvedAppend};
use crate::types::{
    AgentId, BrokerOutcome, BrokerRequest, BrokerUpdate, Command, Effect, FoldedEvent, Input,
    OrderState, ReconcileReason, WriterEpoch,
};

/// One step of the executor (ADR-0001 ES-06).
///
/// The **only** producer of effects, which is what makes a replay safe: [`crate::fold`] emits
/// nothing, so recovery cannot re-send, and nothing but this function can reach the connector.
///
/// The returned list is ordered and the shell runs it in order: it appends first, stopping at the
/// first append that is neither `Committed` nor `AlreadyCommitted` and discarding the rest, and
/// makes a broker request only after the append that records it has committed. Within one list
/// **every [`Effect::Broker`] that submits follows the `OrderSubmitted` draft that names it**, so
/// write-before-acting (journal spec §5.2, `AGENTS.md` rule 5, DEC-07) is structural rather than a
/// convention: there is no code path that can submit without the draft.
///
/// A whole sequence — cancel, confirmation, gate re-run, submit, re-placement — is one ordered
/// list from one call, so a crash inside a sequence leaves the journal saying exactly where it
/// stopped and [`Input::Started`] resumes from that point rather than restarting the sequence
/// (task brief interpretation 20).
///
/// `Input::Started` is recovery. It never resubmits: for every `OrderSubmitted` the fold carries
/// with no acknowledgment it emits a `GetOrderByClientId`, and an order the broker does not have
/// returns to `Intent` only after `unknown_absent_lookups` absences spanning
/// `unknown_absent_window_s` (trading-domain spec §5.7, interpretation 9).
pub fn handle(
    state: &mut ExecutorState,
    input: Input,
    ports: &Ports<'_>,
    binding_gate: &dyn BindingGateSource,
) -> Result<Vec<Effect>, ExecutorError> {
    if let Input::Started(epoch) = input {
        return started(state, epoch, ports, binding_gate);
    }
    if !state.started {
        return Err(ExecutorError::NotStarted);
    }
    if let Some(batch) = &state.unresolved
        && batch.input != input
        && !superseded(&batch.input, &input)
    {
        return Err(ExecutorError::AppendUnresolved { head: batch.head.0 });
    }
    if let Input::Tick(at) = &input {
        state.now = state.now.max(Some(*at));
    }
    if let Input::Market(observation) = &input {
        breach(state, observation);
        for (kept, carries) in [
            (&mut state.sane_bids, observation.bid.is_some()),
            (&mut state.trades, observation.last_trade.is_some()),
        ] {
            let newer = kept
                .get(&observation.instrument)
                .is_none_or(|held| held.observed_at <= observation.observed_at);
            if observation.sane && carries && newer {
                kept.insert(observation.instrument.clone(), observation.clone());
            }
        }
        state
            .quotes
            .insert(observation.instrument.clone(), observation.clone());
    }
    let head = state.account_head();
    let mut batch = Batch::new(state, ports)?;
    batch.bind(binding_gate);
    step(&mut batch, input.clone())?;
    settle(&mut batch)?;
    let effects = batch.effects;
    let drafts: Vec<_> = effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Journal(draft) => Some(draft.clone()),
            _ => None,
        })
        .collect();
    if !drafts.is_empty() {
        state.unresolved = Some(UnresolvedAppend {
            head,
            input,
            drafts,
        });
    }
    Ok(effects)
}

/// Whether a new input may replace an unresolved batch. Only a fresh snapshot may, and only one
/// whose predecessor was a snapshot too: a reconciliation is appended at the head its snapshot was
/// taken at, so a submission that landed in between makes the whole batch answer `HeadMismatch`
/// and commit nothing, and the run is recomputed against the fresh snapshot rather than retried
/// (interpretation 15). Were the earlier batch in fact committed, its first event id — derived from
/// the same epoch, head, and ordinal — would collide with the new batch's, and the journal refuses
/// a collision rather than appending a second event.
fn superseded(unresolved: &Input, input: &Input) -> bool {
    matches!(
        (unresolved, input),
        (Input::BrokerSnapshot(_), Input::BrokerSnapshot(_))
    )
}

/// `Input::Started`: the process folded the stream and took an epoch. Every order whose outcome
/// the journal does not know is queried, a received intent too old to submit is abandoned, and
/// the startup reconciliation is requested. The intents waiting at the start resume after it has
/// run, and the gate holds a new opening until it has run and an account has been journaled
/// (`ExecutorState::reconciled_since_start`).
fn started(
    state: &mut ExecutorState,
    epoch: WriterEpoch,
    ports: &Ports<'_>,
    binding_gate: &dyn BindingGateSource,
) -> Result<Vec<Effect>, ExecutorError> {
    if state.started {
        return Err(ExecutorError::AlreadyStarted);
    }
    state.epoch = Some(epoch);
    state.started = true;
    state.started_at = Some(state.account_head());
    let mut batch = Batch::new(state, ports)?;
    batch.bind(binding_gate);
    let unresolved: Vec<_> =
        batch
            .view
            .unacknowledged()
            .into_iter()
            .chain(batch.view.orders.values().filter(|order| {
                matches!(order.state, OrderState::Unknown | OrderState::PendingCancel)
            }))
            .map(|order| order.client_order_id.clone())
            .collect();
    for id in unresolved {
        batch.broker(BrokerRequest::GetOrderByClientId(id));
    }
    resume(&mut batch, true)?;
    batch.request_reconciliation();
    Ok(batch.effects)
}

fn step(batch: &mut Batch<'_, '_>, input: Input) -> Result<(), ExecutorError> {
    match input {
        Input::Started(_) => Err(ExecutorError::AlreadyStarted),
        Input::Journal(event) => copied(batch, &event),
        Input::Market(_) => watchdog(batch),
        Input::Tick(_) => {
            lookups_due(batch);
            release_held(batch)?;
            overdue_openings(batch)?;
            bound(batch)?;
            ladder_steps(batch)?;
            watchdog(batch)
        }
        Input::Intent(handoff)
            if [WATCHDOG, FLATTEN]
                .iter()
                .any(|own| handoff.intent_id.0.0.starts_with(own)) =>
        {
            Err(ExecutorError::MalformedClientOrderId {
                raw: handoff.intent_id.0.0,
            })
        }
        Input::Intent(handoff) => received(batch, handoff),
        Input::Broker(Err(_)) => silence(batch),
        Input::Broker(Ok(outcome)) => outcome_of(batch, outcome),
        Input::BrokerUpdate(BrokerUpdate::Order(order)) => described(batch, &order),
        Input::BrokerUpdate(BrokerUpdate::Fill(one)) => fill(batch, &one, None),
        Input::BrokerUpdate(BrokerUpdate::Account(snapshot)) => account(batch, &snapshot),
        Input::BrokerUpdate(BrokerUpdate::Reject(refused)) => reject(batch, &refused),
        Input::BrokerSnapshot(snapshot) => {
            run(batch, &snapshot)?;
            if snapshot.reason == ReconcileReason::Startup {
                resume(batch, false)?;
            }
            Ok(())
        }
        Input::Command(Command::Reconcile(_)) => {
            batch.request_reconciliation();
            Ok(())
        }
        Input::Command(Command::KillSwitch {
            scope,
            initiator,
            confirmation,
        }) => kill_switch(batch, scope, initiator, confirmation),
        Input::Command(Command::CancelOpenings { .. }) => {
            Err(ExecutorError::Unimplemented { story: "E7-19" })
        }
    }
}

fn outcome_of(batch: &mut Batch<'_, '_>, outcome: BrokerOutcome) -> Result<(), ExecutorError> {
    match outcome {
        BrokerOutcome::Submitted(order) | BrokerOutcome::Order(order) => described(batch, &order),
        BrokerOutcome::DuplicateClientOrderId { client_order_id } => {
            duplicate(batch, &client_order_id)
        }
        BrokerOutcome::Absent { client_order_id } => absent(batch, &client_order_id),
        BrokerOutcome::CancelAccepted { client_order_id } => cancelled(batch, &client_order_id),
        BrokerOutcome::Rejected(refused) => reject(batch, &refused),
        BrokerOutcome::Account(snapshot) => account(batch, &snapshot),
        BrokerOutcome::OpenOrders(_)
        | BrokerOutcome::Positions(_)
        | BrokerOutcome::Activities { .. }
        | BrokerOutcome::AccountWideAccepted => later_slice(),
    }
}

/// A journaled event from a followed stream is a fact the executor copies into the account stream
/// with its `causation_id` — `AgentModeApplied` from `AgentModeChanged`, `TradingDayStarted`,
/// `ClockAdvanced` crossing midnight New York, `OwnerAcknowledged` (journal spec §2). That copy is
/// E7-4 slice 5's (the trading day, DEC-160); until it lands the input answers its stub, never a
/// silent `Ok(())` that drops the fact (#244 round 1, the coordinator's ruling 5861479849).
///
/// Slice 3b takes one: an `AgentModeApplied` on this account's stream to `exits_only`, `paused` or
/// `stopped` cancels that agent's working openings, or every agent's for `*` (mandate spec §5.9,
/// interpretation 19). Protective orders stay. Any `to` other than `normal` is read as stricter,
/// one of §7.4's three or a string that names no mode: cancelling openings only reduces risk
/// (rule 3), so an unrecognised mode fails in that direction (#286 round 1, minor 4).
fn copied(batch: &mut Batch<'_, '_>, event: &FoldedEvent) -> Result<(), ExecutorError> {
    if event.stream == batch.view.account_stream() && event.event_type == "TradingDayStarted" {
        return new_day(batch);
    }
    let strict = event.stream == batch.view.account_stream()
        && event.event_type == "AgentModeApplied"
        && optional_text(&event.payload, "to").is_some_and(|to| to != "normal");
    let Some(agent) = optional_text(&event.payload, "agent").filter(|_| strict) else {
        return Err(ExecutorError::Unimplemented { story: "E7-4" });
    };
    let agent = (agent != EVERY_AGENT).then(|| AgentId(agent.to_owned()));
    cancel_openings(batch, agent.as_ref(), None)
}

/// Reconciliation's broker reads and the account-wide endpoints' answer (trading-domain spec
/// §5.5, §11): the later slices of this stack, the kill switch's account scope included.
fn later_slice() -> Result<(), ExecutorError> {
    Err(ExecutorError::Unimplemented { story: "E7-3" })
}

#[cfg(test)]
mod watch_call_tests {
    use mandate_accounting::InstrumentId;
    use mandate_num::{Price, Qty};

    use crate::error::ExecutorError;
    use crate::ports::Ports;
    use crate::reconcile::tests::{Everything, Ids, executor_config, fees, protected_by_an_oco};
    use crate::types::{Input, MarketObservation, RiskClock};

    fn quote(name: &str, at: &str) -> Result<Input, ExecutorError> {
        let at = Price::parse(at)?;
        Ok(Input::Market(MarketObservation {
            instrument: InstrumentId::new(name)?,
            bid: Some(at),
            bid_size: Some(Qty::parse("100")?),
            ask: Some(at),
            last_trade: Some(at),
            mark: Some(at),
            sane: true,
            observed_at: RiskClock::from_secs(0),
        }))
    }

    /// #258 round 1, major 1, carried into slice 4b: `handle` passes every quote to the
    /// watchdog's clock, so a sane mark at or below the resting stop starts a breach, a sane mark
    /// above it ends it, and a quote elsewhere touches nothing; none is refused.
    #[test]
    fn a_quote_where_protection_rests_reaches_the_watchdog() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = protected_by_an_oco(&ports)?;
        let aapl = InstrumentId::new("AAPL")?;
        assert_eq!(executor.run(quote("AAPL", "140")?, &ports), Ok(Vec::new()));
        assert!(executor.state.breaches.contains_key(&aapl));
        assert_eq!(executor.run(quote("AAPL", "150")?, &ports), Ok(Vec::new()));
        assert!(executor.state.breaches.is_empty());
        assert_eq!(executor.run(quote("MSFT", "100")?, &ports), Ok(Vec::new()));
        assert!(executor.state.breaches.is_empty());
        Ok(())
    }
}

#[cfg(test)]
mod copied_tests {
    use mandate_canon::Value;

    use crate::error::ExecutorError;
    use crate::payload::{clock, object};
    use crate::ports::Ports;
    use crate::reconcile::tests::{Everything, Executor, Ids, executor_config, fees};
    use crate::types::{BrokerRequest, Effect, EventId, FoldedEvent, Input, RiskClock, Seq};

    fn resting_buy(executor: &mut Executor, id: &str, agent: &str) -> Result<(), ExecutorError> {
        let text = |raw: &str| Value::Str(raw.to_owned());
        executor.commit_one(
            "OrderSubmitted",
            object(vec![
                ("client_order_id", text(id)),
                ("agent", text(agent)),
                ("instrument", text("AAPL")),
                ("side", text("buy")),
                ("qty", text("5")),
                ("limit", text("150")),
                ("purpose", text("open")),
                ("risk_clock", clock(RiskClock::from_secs(0))?),
            ])?,
        )?;
        executor.commit_one(
            "OrderStateChanged",
            object(vec![
                ("client_order_id", text(id)),
                ("state", text("accepted")),
                ("risk_clock", clock(RiskClock::from_secs(0))?),
            ])?,
        )
    }

    /// Slice 3b copies one fact: an `AgentModeApplied` on this account's stream to a stricter mode
    /// cancels that agent's working openings (mandate spec §5.9). Another stream, another fact, or
    /// a return to `normal` still answers slice 5's stub, never a silent `Ok`.
    fn cancelled(effects: &[Effect]) -> Vec<&str> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Broker(BrokerRequest::Cancel { client_order_id }) => {
                    Some(client_order_id.as_str())
                }
                _ => None,
            })
            .collect()
    }

    /// A stricter mode for one agent cancels that agent's openings only; one for every agent
    /// (`*`) cancels all of them (mandate spec §5.9).
    #[test]
    fn a_stricter_mode_cancels_only_its_agents_openings() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        for (agent, expected) in [
            ("agent-a", vec!["md-buy-1"]),
            ("*", vec!["md-buy-1", "md-buy-2"]),
        ] {
            let mut executor = Executor::opened(&ports)?;
            resting_buy(&mut executor, "md-buy-1", "agent-a")?;
            resting_buy(&mut executor, "md-buy-2", "agent-b")?;
            let event = FoldedEvent {
                stream: executor.state.account_stream(),
                seq: Seq(1),
                event_id: EventId("copied-1".to_owned()),
                event_type: "AgentModeApplied".to_owned(),
                causation_id: None,
                payload: object(vec![
                    ("agent", Value::Str(agent.to_owned())),
                    ("to", Value::Str("exits_only".to_owned())),
                ])?,
            };
            let ran = executor.run(Input::Journal(event), &ports)?;
            assert_eq!(cancelled(&ran), expected, "{agent}");
        }
        Ok(())
    }

    /// #286 round 1, minor 4: every mode string but `normal` — §7.4's three stricter modes, and
    /// strings that name no mode at all — is read as stricter and cancels the agent's openings,
    /// the risk-reducing direction; only `normal` does not.
    #[test]
    fn any_mode_but_normal_is_read_as_stricter() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        for to in [
            "exits_only",
            "paused",
            "stopped",
            "Normal",
            "normal ",
            "",
            "halted",
            "normal",
        ] {
            let mut executor = Executor::opened(&ports)?;
            resting_buy(&mut executor, "md-buy-1", "agent-a")?;
            let event = FoldedEvent {
                stream: executor.state.account_stream(),
                seq: Seq(1),
                event_id: EventId("copied-1".to_owned()),
                event_type: "AgentModeApplied".to_owned(),
                causation_id: None,
                payload: object(vec![
                    ("agent", Value::Str("agent-a".to_owned())),
                    ("to", Value::Str(to.to_owned())),
                ])?,
            };
            let answer = executor.run(Input::Journal(event), &ports);
            if to == "normal" {
                assert_eq!(
                    answer,
                    Err(ExecutorError::Unimplemented { story: "E7-4" }),
                    "a return to normal is slice 5's copy"
                );
            } else {
                assert_eq!(cancelled(&answer?), vec!["md-buy-1"], "{to:?}");
            }
        }
        Ok(())
    }

    #[test]
    fn only_a_stricter_mode_on_this_account_cancels_openings() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let stub = Err(ExecutorError::Unimplemented { story: "E7-4" });
        let account = |executor: &Executor| executor.state.account_stream();
        for (other_stream, event_type, to, cancels) in [
            (false, "AgentModeApplied", "exits_only", true),
            (false, "AgentModeApplied", "paused", true),
            (false, "AgentModeApplied", "normal", false),
            (true, "AgentModeApplied", "exits_only", false),
            (false, "TradingDayStarted", "exits_only", false),
        ] {
            let mut executor = Executor::opened(&ports)?;
            resting_buy(&mut executor, "md-buy-1", "agent-a")?;
            let stream = if other_stream {
                "agent:ws1:agent-a".to_owned()
            } else {
                account(&executor)
            };
            let event = FoldedEvent {
                stream,
                seq: Seq(1),
                event_id: EventId("copied-1".to_owned()),
                event_type: event_type.to_owned(),
                causation_id: None,
                payload: object(vec![
                    ("agent", Value::Str("agent-a".to_owned())),
                    ("to", Value::Str(to.to_owned())),
                ])?,
            };
            let answer = executor.run(Input::Journal(event), &ports);
            let case = format!("{other_stream} {event_type} {to}");
            if cancels {
                assert!(
                    answer?.iter().any(|effect| matches!(
                        effect,
                        Effect::Broker(BrokerRequest::Cancel { client_order_id })
                            if client_order_id.as_str() == "md-buy-1"
                    )),
                    "{case}"
                );
            } else if event_type == "TradingDayStarted" {
                assert!(
                    !answer?
                        .iter()
                        .any(|effect| matches!(effect, Effect::Broker(_))),
                    "{case}: a new trading day with nothing to re-place cancels no opening"
                );
            } else {
                assert_eq!(answer, stub.clone(), "{case}");
            }
        }
        Ok(())
    }
}
