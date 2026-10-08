//! The agent-scoped kill switch (E7-4 slice 7; trading-domain spec §5.5, `AGENTS.md` rule 13,
//! DEC-485): the final mode first, then the agent's own orders cancelled by id, then its
//! sub-ledger sold through the executor's own flatten intents, each step journaled.

use std::collections::BTreeSet;

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_canon::Value;
use mandate_num::{Price, Qty, SignedQty};

use crate::batch::Batch;
use crate::codec::{initiator_name, mode_name, purpose_name};
use crate::error::ExecutorError;
use crate::ids::{ClientOrderId, FLATTEN, IntentId};
use crate::intent::{order_tif, received};
use crate::payload::text;
use crate::protection::{ask_cancel, lowest_sane};
use crate::session::{Venue, venue};
use crate::state::{EVERY_AGENT, ExecutorState, IntentOutcome, Switch};
use crate::types::{
    AgentId, BrokerRequest, EventId, Initiator, IntentBody, IntentHandoff, KillScope, Mode, Order,
    OrderState, OwnerConfirmation, Purpose,
};

/// The restriction the kill switch's final mode is recorded under (mandate spec §5.9).
const KILL_SWITCH: &str = "kill_switch";

/// `Command::KillSwitch`. Only the agent scope is implemented: the account and workspace scopes,
/// which alone may name `cancel-all` and `close-position`, answer the next slice's stub, so no
/// path reaches the account-wide endpoints yet (DEC-485 item 1).
pub(crate) fn kill_switch(
    batch: &mut Batch<'_, '_>,
    scope: KillScope,
    initiator: Initiator,
    confirmation: Option<OwnerConfirmation>,
) -> Result<(), ExecutorError> {
    let KillScope::Agent(agent) = scope else {
        return Err(ExecutorError::Unimplemented { story: "E7-4" });
    };
    let mode = batch.journal(
        "AgentModeApplied",
        None,
        vec![
            ("agent", text(agent.0.clone())),
            ("to", text(mode_name(initiator.final_mode()))),
            ("restriction", text(KILL_SWITCH)),
            ("originated", Value::Bool(true)),
        ],
    )?;
    let purpose = initiator.sell_purpose();
    let closing = closes(&batch.view, &agent)?;
    let cancels = cancels(&batch.view, &agent, &closing);
    let (now, later): (Vec<InstrumentId>, Vec<InstrumentId>) = closing
        .into_iter()
        .partition(|instrument| !deferred(batch, instrument, purpose));
    let named = |ids: Vec<&str>| Value::Array(ids.into_iter().map(text).collect());
    let price = |price: Option<String>| price.map_or(Value::Null, text);
    let switch = batch.journal(
        "KillSwitchActivated",
        Some(mode),
        vec![
            ("scope", text("agent")),
            ("subject", text(agent.0.clone())),
            ("initiator", text(initiator_name(initiator))),
            ("purpose", text(purpose_name(purpose))),
            (
                "cancels",
                named(cancels.iter().map(ClientOrderId::as_str).collect()),
            ),
            (
                "closes",
                named(now.iter().map(InstrumentId::as_str).collect()),
            ),
            (
                "deferred",
                named(later.iter().map(InstrumentId::as_str).collect()),
            ),
            (
                "floor",
                price(confirmation.as_ref().map(|c| c.floor.to_string())),
            ),
            (
                "bid",
                price(confirmation.as_ref().map(|c| c.bid.to_string())),
            ),
            (
                "bid_size",
                price(confirmation.as_ref().map(|c| c.bid_size.to_string())),
            ),
        ],
    )?;
    batch.notify(switch, KILL_SWITCH);
    cancel_each(batch, cancels)
}

/// Asks the cancel of each order by its own id, once until confirmed (§5.5).
fn cancel_each(batch: &mut Batch<'_, '_>, ids: Vec<ClientOrderId>) -> Result<(), ExecutorError> {
    for id in ids {
        if ask_cancel(batch, &id)? {
            batch.broker(BrokerRequest::Cancel {
                client_order_id: id,
            });
        }
    }
    Ok(())
}

/// The instruments a switch closes: those where the agent holds attributed lots, and those where
/// a non-protective order of its may still fill while its cancel is pending, so lots a late fill
/// leaves are sold too.
fn closes(view: &ExecutorState, agent: &AgentId) -> Result<Vec<InstrumentId>, ExecutorError> {
    let mut held = BTreeSet::new();
    for order in view
        .orders
        .values()
        .filter(|o| o.agent.as_ref() == Some(agent))
    {
        if live(order) || sub_ledger(view, agent, &order.instrument)? > Qty::ZERO {
            held.insert(order.instrument.clone());
        }
    }
    Ok(held.into_iter().collect())
}

/// Whether a non-protective order is one the broker may still hold: neither terminal nor a bare
/// intent.
fn live(order: &Order) -> bool {
    order.purpose != Purpose::Protective
        && !order.state.is_terminal()
        && order.state != OrderState::Intent
}

/// Whether a bracket entry has filled shares whose legs are not yet recorded placed: the flatten
/// waits a step for them, so it finds them resting and cancels them before it sells (§5.4,
/// rule 12; DEC-485 item 6).
pub(crate) fn unplaced(view: &ExecutorState, order: &Order) -> bool {
    order.filled_qty > Qty::ZERO
        && view
            .details
            .get(&order.client_order_id)
            .is_some_and(|detail| {
                !detail.bracket_placed
                    && detail
                        .request
                        .as_ref()
                        .is_some_and(|sent| sent.bracket.is_some())
            })
}

/// Whether an order is, in an instrument the switch closes, the agent's own, or a `*` watchdog
/// exit's (§2.3, §5.5) where the agent holds lots for it to be selling: a switch whose agent holds
/// none there leaves another party's exit alone, since a risk exit is never held for it (rule 13;
/// #668 round 2). A switch's own flatten is never reached: it is what the switch sells through
/// (#668 round 1, blocker 1; DEC-485 item 5).
fn reached(view: &ExecutorState, order: &Order, agent: &AgentId, closing: &[InstrumentId]) -> bool {
    closing.contains(&order.instrument)
        && !order
            .intent_id
            .as_ref()
            .is_some_and(|intent| is_flatten(view, intent))
        && order.agent.as_ref().is_some_and(|of| {
            of == agent
                || (of.0 == EVERY_AGENT
                    && sub_ledger(view, agent, &order.instrument)
                        .is_ok_and(|lots| lots > Qty::ZERO))
        })
}

/// §5.5: the non-protective orders the switch reaches that the broker holds accepted and no
/// cancel is outstanding for, each by its own id and never another agent's. Protective orders
/// are the flatten's §5.4 sequence's to cancel, so an automated equity close deferred to the
/// session leaves its protection resting until then; an order still `Submitting` or `Unknown` is
/// cancelled once acknowledged ([`flatten_closes`]), never blind.
fn cancels(view: &ExecutorState, agent: &AgentId, closing: &[InstrumentId]) -> Vec<ClientOrderId> {
    view.orders
        .values()
        .filter(|order| {
            matches!(
                order.state,
                OrderState::Accepted | OrderState::PartiallyFilled
            ) && !order.cancel_unconfirmed
                && order.purpose != Purpose::Protective
                && reached(view, order, agent, closing)
        })
        .map(|order| order.client_order_id.clone())
        .collect()
}

/// §5.5: an automated kill switch sells equities only in the regular session (its closing
/// auction window included, where the ladder prices the sell), leaving protection in place until
/// then; crypto goes at once. An owner's sells are never deferred here.
fn deferred(batch: &Batch<'_, '_>, instrument: &InstrumentId, purpose: Purpose) -> bool {
    purpose == Purpose::RiskExit
        && batch.ports.instruments.asset_class(instrument) != Some(AssetClass::Crypto)
        && !matches!(
            venue(batch.ports, instrument, batch.at()),
            Venue::Regular | Venue::Closing
        )
}

/// After every step: an order the switch reaches that the broker acknowledged since is cancelled,
/// and each close not yet raised is raised as the executor's own flatten intent,
/// `k-<switch>-<n>` (DEC-485 item 2), once nothing the switch reaches is live in the instrument
/// any more, the session admits the sell, the agent holds lots there, and a limit can be named.
/// Raised earlier, the gate would count the sells still pending their cancel and deny the flatten
/// `sell_exceeds_available`.
pub(crate) fn flatten_closes(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    let switches: Vec<(EventId, Switch)> = batch
        .view
        .switches
        .iter()
        .filter(|(_, switch)| !switch.pending.is_empty())
        .map(|(id, switch)| (id.clone(), switch.clone()))
        .collect();
    for (id, switch) in switches {
        let pending: Vec<InstrumentId> = switch.pending.iter().cloned().collect();
        cancel_each(batch, cancels(&batch.view, &switch.agent, &pending))?;
        for (ordinal, instrument) in switch.instruments.iter().enumerate() {
            let working = batch.view.orders.values().any(|order| {
                &order.instrument == instrument
                    && (order.agent.as_ref() == Some(&switch.agent)
                        || reached(&batch.view, order, &switch.agent, &switch.instruments))
                    && (live(order) || unplaced(&batch.view, order))
            }) || batch.view.intents.iter().any(|(intent, record)| {
                record.outcome == IntentOutcome::Received
                    && is_flatten(&batch.view, intent)
                    && record.agent == switch.agent
                    && matches!(batch.view.bodies.get(intent),
                        Some(IntentBody::Order { instrument: of, .. }) if of == instrument)
            });
            let qty = sub_ledger(&batch.view, &switch.agent, instrument)?;
            let stop = batch
                .view
                .protection
                .get(instrument)
                .and_then(|protection| protection.prices)
                .map(|prices| prices.stop);
            let limit = match (lowest_sane(&batch.view, instrument).or(stop), switch.floor) {
                (Some(own), floor) => Some(floor.map_or(own, |floor| own.max(floor))),
                (None, floor) => floor,
            };
            let (true, false, true, Some(limit)) = (
                switch.pending.contains(instrument),
                working || deferred(batch, instrument, switch.purpose),
                qty > Qty::ZERO,
                limit,
            ) else {
                continue;
            };
            let tif = Some(order_tif(batch, instrument));
            received(
                batch,
                IntentHandoff {
                    intent_id: IntentId(EventId(format!("{FLATTEN}{}-{ordinal}", id.0))),
                    agent: switch.agent.clone(),
                    tif,
                    body: IntentBody::Order {
                        instrument: instrument.clone(),
                        side: Side::Sell,
                        qty,
                        limit,
                        purpose: switch.purpose,
                        protection: None,
                    },
                },
            )?;
        }
    }
    Ok(())
}

/// §5.5: exactly the agent's sub-ledger quantity, its attributed lots (§8.1: its own orders'
/// fills, buys net of sells), capped at the long position, so another agent's and the owner's
/// unattributed shares are untouched.
fn sub_ledger(
    view: &ExecutorState,
    agent: &AgentId,
    instrument: &InstrumentId,
) -> Result<Qty, ExecutorError> {
    let mut lots = SignedQty::ZERO;
    for order in view.orders.values() {
        if &order.instrument == instrument && order.agent.as_ref() == Some(agent) {
            let filled = SignedQty::from(order.filled_qty);
            lots = lots.checked_add(match order.side {
                Side::Buy => filled,
                Side::Sell => filled.negated(),
            })?;
        }
    }
    let open = view
        .positions
        .get(instrument)
        .copied()
        .unwrap_or(SignedQty::ZERO);
    Ok(lots.min(open).max(SignedQty::ZERO).abs())
}

/// The switch and close a flatten intent's id names, `k-<switch>-<n>`, or `None` for any other.
pub(crate) fn close_named(intent: &IntentId) -> Option<(EventId, usize)> {
    let (switch, ordinal) = intent.0.0.strip_prefix(FLATTEN)?.rsplit_once('-')?;
    Some((EventId(switch.to_owned()), ordinal.parse().ok()?))
}

/// Whether `switch`'s close `ordinal` is `agent`'s sell of `instrument`: what makes an intent
/// under a switch's id that close's flatten (#668 round 1, major 1; DEC-485 item 15).
pub(crate) fn closes_with(
    switch: &Switch,
    ordinal: usize,
    agent: &AgentId,
    instrument: &InstrumentId,
) -> bool {
    &switch.agent == agent && switch.instruments.get(ordinal) == Some(instrument)
}

/// The switch whose close `intent` is: its id names a journaled switch, and it is that close's
/// agent's sell of that close's instrument.
fn switch_of<'s>(view: &'s ExecutorState, intent: &IntentId) -> Option<&'s Switch> {
    let (id, ordinal) = close_named(intent)?;
    let switch = view.switches.get(&id)?;
    let record = view.intents.get(intent)?;
    let Some(IntentBody::Order { instrument, .. }) = view.bodies.get(intent) else {
        return None;
    };
    closes_with(switch, ordinal, &record.agent, instrument).then_some(switch)
}

/// The floor an exit's ladder never prices below (§5.5, §5.6): the owner's confirmed floor for a
/// flatten raised by a confirmed switch, nothing for any other exit.
pub(crate) fn floor_of(view: &ExecutorState, intent: &IntentId) -> Option<Price> {
    switch_of(view, intent)?.floor
}

/// Whether an intent is a journaled switch's own flatten, which the agent's mode never holds.
pub(crate) fn is_flatten(view: &ExecutorState, intent: &IntentId) -> bool {
    switch_of(view, intent).is_some()
}

/// Whether the agent's mode holds an exit (§7.4: `paused` or `stopped`). A switch's flatten is
/// exempt from the agent's mode (§5.5, DEC-260 (3)), so only the account's own state holds it,
/// rule 13's broker hold; every other exit is held by the agent's effective mode.
pub(crate) fn mode_holds(view: &ExecutorState, agent: &AgentId, intent: &IntentId) -> bool {
    let mode = if is_flatten(view, intent) {
        view.account_mode()
    } else {
        view.effective_mode(agent)
    };
    mode >= Mode::Paused
}

#[cfg(test)]
mod tests {
    use mandate_accounting::{AssetClass, InstrumentId, Side};
    use mandate_canon::Value;
    use mandate_num::{Price, Qty, ShareIncrement, Usd};
    use mandate_time::Date;

    use crate::error::ExecutorError;
    use crate::ids::{ClientOrderId, IntentId};
    use crate::payload::{clock, object, text};
    use crate::ports::{InstrumentSnapshot, Ports};
    use crate::reconcile::tests::{
        Everything, Executor, Ids, aapl, executor_config, fees, missing, protected_by_an_oco,
        reporting,
    };
    use crate::state::ExecutorState;
    use crate::types::{
        AccountRef, AccountScope, AccountState, AgentId, BrokerFill, BrokerOrder, BrokerOutcome,
        BrokerRequest, BrokerUpdate, Command, Effect, EventDraft, EventId, ExitTier, FillId,
        Initiator, Input, IntentBody, IntentHandoff, KillScope, MarketObservation, Mode,
        OwnerConfirmation, ProtectionPrices, Purpose, RiskClock, SubmitOrder, TimeInForce,
        WorkspaceId,
    };

    /// Saturday 2026-09-26, 12:00 New York: no v1 session is open.
    const SATURDAY: i64 = 1_790_438_400;
    /// Monday 2026-09-28, 10:00 New York: the regular session.
    const MONDAY: i64 = 1_790_604_000;
    const ENTRY: &str = "01JABCDEFGHJKMNPQRSTVWXYZ1";

    /// Every instrument as crypto, which no session holds.
    struct Coins;

    impl InstrumentSnapshot for Coins {
        fn asset_class(&self, _instrument: &InstrumentId) -> Option<AssetClass> {
            Some(AssetClass::Crypto)
        }

        fn increment(&self, _instrument: &InstrumentId) -> Option<ShareIncrement> {
            Some(ShareIncrement::Whole)
        }

        fn exit_tier(&self, _instrument: &InstrumentId) -> Option<ExitTier> {
            None
        }
    }

    fn switch(initiator: Initiator, confirmed: bool) -> Result<Input, ExecutorError> {
        let confirmation = confirmed
            .then(|| -> Result<OwnerConfirmation, ExecutorError> {
                Ok(OwnerConfirmation {
                    bid: Price::parse("150")?,
                    bid_size: Qty::parse("100")?,
                    floor: Price::parse("148")?,
                    user: "user-1".to_owned(),
                    step_up: "step-up-1".to_owned(),
                })
            })
            .transpose()?;
        Ok(Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(AgentId("agent-a".to_owned())),
            initiator,
            confirmation,
        }))
    }

    /// `protected_by_an_oco` with its buy reported filled, so nothing of the agent's but the
    /// protection is live in AAPL.
    fn protected(ports: &Ports<'_>) -> Result<Executor, ExecutorError> {
        let mut executor = protected_by_an_oco(ports)?;
        commit(
            &mut executor,
            "OrderStateChanged",
            vec![
                ("client_order_id", text("md-buy-a")),
                ("state", text("filled")),
            ],
        )?;
        Ok(executor)
    }

    fn commit(
        executor: &mut Executor,
        event_type: &str,
        mut pairs: Vec<(&str, Value)>,
    ) -> Result<(), ExecutorError> {
        pairs.push(("risk_clock", clock(executor.state.clock())?));
        executor.commit_one(event_type, object(pairs)?)
    }

    fn draft<'e>(effects: &'e [Effect], event_type: &str) -> Option<&'e EventDraft> {
        effects.iter().find_map(|effect| match effect {
            Effect::Journal(draft) if draft.event_type == event_type => Some(draft),
            _ => None,
        })
    }

    fn field<'d>(draft: &'d EventDraft, name: &str) -> Option<&'d Value> {
        draft.payload.get(name)
    }

    fn names(draft: &EventDraft, name: &str) -> Vec<String> {
        match field(draft, name) {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect(),
            _ => vec!["<absent>".to_owned()],
        }
    }

    fn cancels(effects: &[Effect]) -> Vec<&str> {
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

    fn sells(effects: &[Effect]) -> usize {
        effects
            .iter()
            .filter(|effect| {
                matches!(effect, Effect::Broker(BrokerRequest::Submit(order))
                    if order.side == Side::Sell && order.purpose != Purpose::Protective)
            })
            .count()
    }

    fn raised(effects: &[Effect]) -> Vec<String> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Journal(draft) if draft.event_type == "IntentReceived" => {
                    field(draft, "intent_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                }
                _ => None,
            })
            .collect()
    }

    fn the_switch(effects: &[Effect]) -> Result<&EventDraft, ExecutorError> {
        draft(effects, "KillSwitchActivated").ok_or_else(|| missing("the switch's record"))
    }

    /// §5.5, journal §9.5: the mode first, then the switch's record caused by it, naming who
    /// ordered it, what its sells are, what it closes and what the owner confirmed, and one alert
    /// that carries the record's id and nothing else. An instrument the agent traded out of is no
    /// close.
    #[test]
    fn the_switch_records_its_initiator_purpose_closes_and_confirmation()
    -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let cases = [
            (
                Initiator::Owner,
                true,
                "stopped",
                "owner",
                "owner_exit",
                Some(("148", "150", "100")),
            ),
            (
                Initiator::RiskLimit,
                false,
                "paused",
                "risk_limit",
                "risk_exit",
                None,
            ),
            (
                Initiator::PlatformOperator,
                false,
                "stopped",
                "platform_operator",
                "risk_exit",
                None,
            ),
        ];
        for (initiator, confirmed, mode, name, purpose, confirmation) in cases {
            let mut executor = protected(&ports)?;
            for (id, agent, instrument, side) in [
                ("md-msft-in", "agent-a", "MSFT", "buy"),
                ("md-msft-out", "agent-a", "MSFT", "sell"),
                ("md-aapl-b", "agent-b", "AAPL", "buy"),
            ] {
                commit(
                    &mut executor,
                    "OrderSubmitted",
                    vec![
                        ("client_order_id", text(id)),
                        ("agent", text(agent)),
                        ("instrument", text(instrument)),
                        ("side", text(side)),
                        ("qty", text("5")),
                        ("limit", text("100")),
                    ],
                )?;
                commit(
                    &mut executor,
                    "FillApplied",
                    vec![
                        ("fill_id", text(format!("f-{id}"))),
                        ("client_order_id", text(id)),
                        ("instrument", text(instrument)),
                        ("side", text(side)),
                        ("qty_gross", text("5")),
                        ("price", text("100")),
                    ],
                )?;
                commit(
                    &mut executor,
                    "OrderStateChanged",
                    vec![("client_order_id", text(id)), ("state", text("filled"))],
                )?;
            }
            let effects = executor.run(switch(initiator, confirmed)?, &ports)?;
            let applied = draft(&effects, "AgentModeApplied").ok_or_else(|| missing("the mode"))?;
            assert_eq!(
                (
                    field(applied, "agent").and_then(Value::as_str),
                    field(applied, "to").and_then(Value::as_str),
                    field(applied, "restriction").and_then(Value::as_str),
                    field(applied, "originated"),
                ),
                (
                    Some("agent-a"),
                    Some(mode),
                    Some("kill_switch"),
                    Some(&Value::Bool(true))
                ),
                "{name}"
            );
            let record = the_switch(&effects)?;
            assert_eq!(
                record.causation_id.as_ref(),
                Some(&applied.event_id),
                "{name}"
            );
            let said = |member: &str| field(record, member).and_then(Value::as_str);
            assert_eq!(
                (
                    said("scope"),
                    said("subject"),
                    said("initiator"),
                    said("purpose")
                ),
                (Some("agent"), Some("agent-a"), Some(name), Some(purpose))
            );
            assert_eq!(names(record, "closes"), vec!["AAPL".to_owned()], "{name}");
            assert_eq!(names(record, "deferred"), Vec::<String>::new(), "{name}");
            assert_eq!(names(record, "cancels"), Vec::<String>::new(), "{name}");
            assert_eq!(
                (said("floor"), said("bid"), said("bid_size")),
                confirmation.map_or((None, None, None), |(floor, bid, size)| {
                    (Some(floor), Some(bid), Some(size))
                }),
                "{name}"
            );
            let alerts: Vec<(&EventId, &str)> = effects
                .iter()
                .filter_map(|effect| match effect {
                    Effect::Notify(alert) => Some((&alert.subject_event, alert.message_key)),
                    _ => None,
                })
                .collect();
            assert_eq!(alerts, vec![(&record.event_id, "kill_switch")], "{name}");
            assert_eq!(
                raised(&effects),
                vec![format!("k-{}-0", record.event_id.0)],
                "{name}: the close is raised under the switch's own id, at once, since nothing \
                 of the agent's works in AAPL"
            );
            let close = draft(&effects, "IntentReceived").ok_or_else(|| missing("the close"))?;
            assert_eq!(
                field(close, "qty").and_then(Value::as_str),
                Some("10"),
                "{name}: agent-a's ten, not agent-b's five beside them"
            );
        }
        Ok(())
    }

    /// §7.3: the account's own state is the part of the effective mode a flatten is not exempt
    /// from, `blocked` holding every exit and `closing_only` every opening.
    #[test]
    fn the_account_state_is_the_mode_a_flatten_answers_to() -> Result<(), ExecutorError> {
        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        });
        let agent = AgentId("agent-a".to_owned());
        for (account, mode) in [
            (AccountState::Active, Mode::Normal),
            (AccountState::ClosingOnly, Mode::ExitsOnly),
            (AccountState::Blocked, Mode::Paused),
        ] {
            state.account_state = account;
            assert_eq!(
                (state.account_mode(), state.effective_mode(&agent)),
                (mode, mode)
            );
        }
        Ok(())
    }

    /// §5.5: outside the regular session an automated switch defers its equity close, leaving the
    /// protection resting, and raises it at the first step of the session; an owner's close and a
    /// crypto close are not deferred.
    #[test]
    fn an_automated_equity_close_waits_for_the_session() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let equities = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let coins = Ports {
            instruments: &Coins,
            ..equities
        };
        for (ports, initiator, deferred) in [
            (&equities, Initiator::RiskLimit, true),
            (&equities, Initiator::Owner, false),
            (&coins, Initiator::RiskLimit, false),
        ] {
            let mut executor = protected(ports)?;
            executor.run(Input::Tick(RiskClock::from_secs(SATURDAY)), ports)?;
            let effects = executor.run(switch(initiator, false)?, ports)?;
            let record = the_switch(&effects)?;
            let (closes, later) = if deferred {
                (Vec::new(), vec!["AAPL".to_owned()])
            } else {
                (vec!["AAPL".to_owned()], Vec::new())
            };
            assert_eq!(
                (names(record, "closes"), names(record, "deferred")),
                (closes, later),
                "{initiator:?}"
            );
            assert_eq!(raised(&effects).is_empty(), deferred, "{initiator:?}");
            if deferred {
                assert!(cancels(&effects).is_empty(), "the protection keeps resting");
                let opened = executor.run(Input::Tick(RiskClock::from_secs(MONDAY)), ports)?;
                assert_eq!(
                    raised(&opened),
                    vec![format!("k-{}-0", record.event_id.0)],
                    "the deferred close is raised at the session's first step"
                );
                assert_eq!(
                    cancels(&opened),
                    vec!["md-oco-1"],
                    "through §5.4's sequence"
                );
            }
        }
        Ok(())
    }

    /// DEC-485 item 7: lifting the `kill_switch` restriction drops the closes not yet raised, and
    /// lifting any other restriction leaves them.
    #[test]
    fn only_lifting_the_kill_switch_drops_its_pending_closes() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        for (restriction, kept) in [("kill_switch", false), ("cash_mismatch", true)] {
            let mut executor = protected(&ports)?;
            executor.run(Input::Tick(RiskClock::from_secs(SATURDAY)), &ports)?;
            executor.run(switch(Initiator::RiskLimit, false)?, &ports)?;
            commit(
                &mut executor,
                "AgentModeApplied",
                vec![
                    ("agent", text("agent-a")),
                    ("to", text("normal")),
                    ("restriction", text(restriction)),
                    ("originated", Value::Bool(true)),
                ],
            )?;
            let opened = executor.run(Input::Tick(RiskClock::from_secs(MONDAY)), &ports)?;
            assert_eq!(raised(&opened).len(), usize::from(kept), "{restriction}");
        }
        Ok(())
    }

    /// §5.5: the switch reaches the agent's own orders and a `*` exit only where it closes, and an
    /// order whose cancel is already outstanding is neither asked again nor named again. A close
    /// whose cancel confirms with nothing of the agent's left to sell raises nothing.
    #[test]
    fn the_switch_cancels_only_what_it_reaches_and_raises_no_empty_close()
    -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = protected(&ports)?;
        for (id, agent, instrument, side, purpose) in [
            ("md-msft-buy", "agent-a", "MSFT", "buy", "open"),
            ("md-w-elsewhere", "*", "NVDA", "sell", "risk_exit"),
        ] {
            commit(
                &mut executor,
                "OrderSubmitted",
                vec![
                    ("client_order_id", text(id)),
                    ("agent", text(agent)),
                    ("instrument", text(instrument)),
                    ("side", text(side)),
                    ("qty", text("5")),
                    ("limit", text("100")),
                    ("purpose", text(purpose)),
                ],
            )?;
            commit(
                &mut executor,
                "OrderStateChanged",
                vec![("client_order_id", text(id)), ("state", text("accepted"))],
            )?;
        }
        let first = executor.run(switch(Initiator::RiskLimit, false)?, &ports)?;
        assert!(
            cancels(&first).contains(&"md-msft-buy"),
            "{:?}",
            cancels(&first)
        );
        assert!(
            !cancels(&first).contains(&"md-w-elsewhere"),
            "a `*` exit where the switch closes nothing is not its to cancel"
        );
        assert_eq!(names(the_switch(&first)?, "cancels"), vec!["md-msft-buy"]);
        assert_eq!(
            names(the_switch(&first)?, "closes"),
            vec!["AAPL".to_owned(), "MSFT".to_owned()]
        );
        let again = executor.run(switch(Initiator::RiskLimit, false)?, &ports)?;
        assert!(
            !cancels(&again).contains(&"md-msft-buy"),
            "{:?}",
            cancels(&again)
        );
        assert_eq!(names(the_switch(&again)?, "cancels"), Vec::<String>::new());
        let at = Price::parse("100")?;
        executor.run(
            Input::Market(MarketObservation {
                instrument: InstrumentId::new("MSFT")?,
                bid: Some(at),
                bid_size: Some(Qty::parse("100")?),
                ask: Some(at),
                last_trade: Some(at),
                mark: Some(at),
                sane: true,
                observed_at: RiskClock::from_secs(0),
            }),
            &ports,
        )?;
        let confirmed = executor.run(
            Input::Broker(Ok(BrokerOutcome::CancelAccepted {
                client_order_id: "md-msft-buy".to_owned(),
            })),
            &ports,
        )?;
        assert!(
            raised(&confirmed).iter().all(|id| !id.ends_with("-1")),
            "nothing of the agent's is left in MSFT, so its close raises nothing: {:?}",
            raised(&confirmed)
        );
        Ok(())
    }

    fn bought(qty: &str, at: &str) -> Result<Input, ExecutorError> {
        Ok(Input::BrokerUpdate(BrokerUpdate::Fill(BrokerFill {
            fill_id: FillId(format!("f-{at}")),
            client_order_id: Some(format!("md-{ENTRY}")),
            instrument: aapl()?,
            side: Side::Buy,
            qty: Qty::parse(qty)?,
            price: Price::parse("150")?,
            fees: Usd::ZERO,
            trade_date: Date::parse("2026-09-28")?,
        })))
    }

    /// The broker's report of the bracket entry: `status`, with `filled` of its 10 filled.
    fn entry_report(status: &str, filled: &str) -> Result<Input, ExecutorError> {
        Ok(Input::BrokerUpdate(BrokerUpdate::Order(BrokerOrder {
            broker_order_id: "b-1".to_owned(),
            client_order_id: Some(format!("md-{ENTRY}")),
            instrument: aapl()?,
            side: Side::Buy,
            qty: Qty::parse("10")?,
            filled_qty: Qty::parse(filled)?,
            limit_price: Some(Price::parse("150")?),
            stop_price: None,
            status: status.to_owned(),
            reject_code: None,
            replaced_by_broker_order_id: None,
            legs: Vec::new(),
            created_on: None,
        })))
    }

    fn bracket_entry(ports: &Ports<'_>) -> Result<Executor, ExecutorError> {
        let mut executor = reporting(ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(MONDAY)), ports)?;
        let at = Price::parse("150")?;
        executor.run(
            Input::Market(MarketObservation {
                instrument: aapl()?,
                bid: Some(at),
                bid_size: Some(Qty::parse("100")?),
                ask: Some(at),
                last_trade: Some(at),
                mark: Some(at),
                sane: true,
                observed_at: RiskClock::from_secs(MONDAY),
            }),
            ports,
        )?;
        executor.run(
            Input::Intent(IntentHandoff {
                intent_id: IntentId(EventId(ENTRY.to_owned())),
                agent: AgentId("agent-a".to_owned()),
                tif: Some(TimeInForce::Day),
                body: IntentBody::Order {
                    instrument: aapl()?,
                    side: Side::Buy,
                    qty: Qty::parse("10")?,
                    limit: Price::parse("150")?,
                    purpose: Purpose::Open,
                    protection: Some(ProtectionPrices {
                        stop: Price::parse("140")?,
                        take_profit: Some(Price::parse("170")?),
                    }),
                },
            }),
            ports,
        )?;
        executor.run(entry_report("accepted", "0")?, ports)?;
        Ok(executor)
    }

    /// DEC-485 item 16, rule 12: a bracket entry the broker reports filled while the switch's
    /// cancel of it is outstanding activates its legs, and the flatten waits that step for them to
    /// be recorded, then cancels them before it sells. An entry cancelled with nothing filled holds nothing back.
    #[test]
    fn a_flatten_never_sells_beside_legs_it_has_not_cancelled() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = bracket_entry(&ports)?;
        executor.run(bought("4", "a")?, &ports)?;
        let switched = executor.run(switch(Initiator::RiskLimit, false)?, &ports)?;
        assert_eq!(cancels(&switched), vec![format!("md-{ENTRY}")]);
        let completed = executor.run(entry_report("filled", "10")?, &ports)?;
        assert_eq!(
            sells(&completed),
            0,
            "no sell beside the legs just activated"
        );
        assert!(raised(&completed).is_empty(), "{:?}", raised(&completed));
        let next = executor.run(Input::Tick(RiskClock::from_secs(MONDAY + 1)), &ports)?;
        assert_eq!(raised(&next).len(), 1, "the flatten goes at the next step");
        assert_eq!(
            sells(&next),
            0,
            "and waits for its sequence's cancel of the legs"
        );
        assert_eq!(cancels(&next).len(), 1, "{:?}", cancels(&next));

        let mut unfilled = bracket_entry(&ports)?;
        commit(
            &mut unfilled,
            "OrderSubmitted",
            vec![
                ("client_order_id", text("md-held")),
                ("agent", text("agent-a")),
                ("instrument", text("AAPL")),
                ("side", text("buy")),
                ("qty", text("10")),
                ("limit", text("150")),
                ("purpose", text("open")),
            ],
        )?;
        commit(
            &mut unfilled,
            "FillApplied",
            vec![
                ("fill_id", text("f-held")),
                ("client_order_id", text("md-held")),
                ("instrument", text("AAPL")),
                ("side", text("buy")),
                ("qty_gross", text("10")),
                ("price", text("150")),
            ],
        )?;
        commit(
            &mut unfilled,
            "OrderStateChanged",
            vec![
                ("client_order_id", text("md-held")),
                ("state", text("filled")),
            ],
        )?;
        unfilled.run(switch(Initiator::RiskLimit, false)?, &ports)?;
        let gone = unfilled.run(
            Input::Broker(Ok(BrokerOutcome::CancelAccepted {
                client_order_id: format!("md-{ENTRY}"),
            })),
            &ports,
        )?;
        assert_eq!(
            raised(&gone).len(),
            1,
            "an unfilled entry holds the flatten back no longer"
        );
        assert_eq!(sells(&gone), 1);
        Ok(())
    }

    /// DEC-485 item 2: the switch's own intent ids derive their order ids with the watchdog's
    /// grammar, and a bare prefix is refused.
    #[test]
    fn a_flatten_id_derives_its_order_id() -> Result<(), ExecutorError> {
        let derived = |raw: &str| ClientOrderId::for_intent(&IntentId(EventId(raw.to_owned())));
        assert_eq!(derived("k-e1h2o3-0")?.as_str(), "md-k-e1h2o3-0");
        assert!(derived("k-").is_err());
        assert!(derived("w-").is_err());
        assert_eq!(derived("w-e1h2o3")?.as_str(), "md-w-e1h2o3");
        Ok(())
    }

    /// Every instrument an equity but `BTCUSD`, which is crypto.
    struct Mixed;

    impl InstrumentSnapshot for Mixed {
        fn asset_class(&self, instrument: &InstrumentId) -> Option<AssetClass> {
            Some(if instrument.as_str() == "BTCUSD" {
                AssetClass::Crypto
            } else {
                AssetClass::UsEquity
            })
        }

        fn increment(&self, _instrument: &InstrumentId) -> Option<ShareIncrement> {
            Some(ShareIncrement::Whole)
        }

        fn exit_tier(&self, _instrument: &InstrumentId) -> Option<ExitTier> {
            None
        }
    }

    fn quote_of(name: &str, price: &str, at: i64) -> Result<Input, ExecutorError> {
        let price = Price::parse(price)?;
        Ok(Input::Market(MarketObservation {
            instrument: InstrumentId::new(name)?,
            bid: Some(price),
            bid_size: Some(Qty::parse("100")?),
            ask: Some(price),
            last_trade: Some(price),
            mark: Some(price),
            sane: true,
            observed_at: RiskClock::from_secs(at),
        }))
    }

    /// The non-protective sells `effects` submitted.
    fn sold(effects: &[Effect]) -> Vec<SubmitOrder> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Broker(BrokerRequest::Submit(order))
                    if order.side == Side::Sell && order.purpose != Purpose::Protective =>
                {
                    Some(order.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// The broker's acknowledgment of `order`, nothing filled.
    fn accepted(order: &SubmitOrder) -> Input {
        Input::Broker(Ok(BrokerOutcome::Submitted(BrokerOrder {
            broker_order_id: format!("b-{}", order.client_order_id.as_str()),
            client_order_id: Some(order.client_order_id.as_str().to_owned()),
            instrument: order.instrument.clone(),
            side: order.side,
            qty: order.qty,
            filled_qty: Qty::ZERO,
            limit_price: order.limit_price,
            stop_price: order.stop_price,
            status: "accepted".to_owned(),
            reject_code: None,
            replaced_by_broker_order_id: None,
            legs: Vec::new(),
            created_on: None,
        })))
    }

    fn cancel_accepted(id: &str) -> Input {
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: id.to_owned(),
        }))
    }

    /// One order of agent-a's, journaled `state`, with `filled` of it applied.
    fn order_of_a(
        executor: &mut Executor,
        id: &str,
        instrument: &str,
        side: &str,
        filled: Option<&str>,
        state: &str,
    ) -> Result<(), ExecutorError> {
        commit(
            executor,
            "OrderSubmitted",
            vec![
                ("client_order_id", text(id)),
                ("agent", text("agent-a")),
                ("instrument", text(instrument)),
                ("side", text(side)),
                ("qty", text("1")),
                ("limit", text("100")),
                ("purpose", text("open")),
            ],
        )?;
        if let Some(qty) = filled {
            commit(
                executor,
                "FillApplied",
                vec![
                    ("fill_id", text(format!("f-{id}"))),
                    ("client_order_id", text(id)),
                    ("instrument", text(instrument)),
                    ("side", text(side)),
                    ("qty_gross", text(qty)),
                    ("price", text("100")),
                ],
            )?;
        }
        commit(
            executor,
            "OrderStateChanged",
            vec![("client_order_id", text(id)), ("state", text(state))],
        )
    }

    /// #668 round 1, blocker 1: once the switch's own sell is acknowledged, no later step cancels
    /// it, while another close of the same switch still waits on an opening of the agent's.
    #[test]
    fn a_switch_never_cancels_its_own_flatten_beside_a_waiting_close() -> Result<(), ExecutorError>
    {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = protected(&ports)?;
        order_of_a(
            &mut executor,
            "md-msft-buy",
            "MSFT",
            "buy",
            None,
            "accepted",
        )?;
        executor.run(Input::Tick(RiskClock::from_secs(MONDAY)), &ports)?;
        executor.run(quote_of("AAPL", "150", MONDAY)?, &ports)?;
        executor.run(switch(Initiator::RiskLimit, false)?, &ports)?;
        let confirmed = executor.run(cancel_accepted("md-oco-1"), &ports)?;
        let [sell] = sold(&confirmed)
            .try_into()
            .map_err(|_| missing("one flatten sell"))?;
        let mut later = executor.run(accepted(&sell), &ports)?;
        later.extend(executor.run(Input::Tick(RiskClock::from_secs(MONDAY + 1)), &ports)?);
        later.extend(executor.run(Input::Tick(RiskClock::from_secs(MONDAY + 2)), &ports)?);
        assert!(
            !cancels(&later).contains(&sell.client_order_id.as_str()),
            "the flatten is never the switch's to cancel: {:?}",
            cancels(&later)
        );
        Ok(())
    }

    /// #668 round 1, blocker 1: a crypto close raised beside an equity close deferred to the
    /// session is not cancelled at the steps the deferred one waits through.
    #[test]
    fn a_crypto_flatten_survives_a_deferred_equity_close() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Mixed,
            config: &config,
            fees: &fees,
        };
        let mut executor = protected(&ports)?;
        order_of_a(
            &mut executor,
            "md-btc",
            "BTCUSD",
            "buy",
            Some("1"),
            "filled",
        )?;
        executor.run(Input::Tick(RiskClock::from_secs(SATURDAY)), &ports)?;
        executor.run(quote_of("BTCUSD", "100", SATURDAY)?, &ports)?;
        let switched = executor.run(switch(Initiator::RiskLimit, false)?, &ports)?;
        assert_eq!(names(the_switch(&switched)?, "deferred"), vec!["AAPL"]);
        let [sell] = sold(&switched)
            .try_into()
            .map_err(|_| missing("the crypto sell"))?;
        let mut later = executor.run(accepted(&sell), &ports)?;
        later.extend(executor.run(Input::Tick(RiskClock::from_secs(SATURDAY + 1)), &ports)?);
        assert!(
            !cancels(&later).contains(&sell.client_order_id.as_str()),
            "{:?}",
            cancels(&later)
        );
        Ok(())
    }

    /// #668 round 1, blocker 1 and minor 2: a second switch while the first's sell works cancels
    /// none of it and raises no second close beside it.
    #[test]
    fn a_second_switch_leaves_the_first_flatten_working() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = protected(&ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(MONDAY)), &ports)?;
        executor.run(quote_of("AAPL", "150", MONDAY)?, &ports)?;
        executor.run(switch(Initiator::RiskLimit, false)?, &ports)?;
        let confirmed = executor.run(cancel_accepted("md-oco-1"), &ports)?;
        let [sell] = sold(&confirmed)
            .try_into()
            .map_err(|_| missing("one flatten sell"))?;
        executor.run(accepted(&sell), &ports)?;
        let mut second = executor.run(switch(Initiator::Owner, false)?, &ports)?;
        assert_eq!(names(the_switch(&second)?, "cancels"), Vec::<String>::new());
        second.extend(executor.run(Input::Tick(RiskClock::from_secs(MONDAY + 1)), &ports)?);
        assert!(
            !cancels(&second).contains(&sell.client_order_id.as_str()),
            "{:?}",
            cancels(&second)
        );
        assert!(raised(&second).is_empty(), "{:?}", raised(&second));
        Ok(())
    }

    /// #668 round 1, major 1 (DEC-485 item 15): an intent handed over under a `k-` or `w-` id is
    /// refused before anything is journaled, so it neither takes a switch's mode exemption nor its
    /// pending close, which is still raised at the session.
    #[test]
    fn a_handed_over_intent_cannot_pose_as_the_executors_own() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = protected(&ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(SATURDAY)), &ports)?;
        let switched = executor.run(switch(Initiator::RiskLimit, false)?, &ports)?;
        let own = format!("k-{}-0", the_switch(&switched)?.event_id.0);
        for id in [own.as_str(), "w-e1h2o3"] {
            let before = executor.state.clone();
            let posed = executor.run(
                Input::Intent(IntentHandoff {
                    intent_id: IntentId(EventId(id.to_owned())),
                    agent: AgentId("agent-a".to_owned()),
                    tif: Some(TimeInForce::Day),
                    body: IntentBody::Order {
                        instrument: aapl()?,
                        side: Side::Sell,
                        qty: Qty::parse("1")?,
                        limit: Price::parse("150")?,
                        purpose: Purpose::DiscretionaryExit,
                        protection: None,
                    },
                }),
                &ports,
            );
            assert!(posed.is_err(), "{id}: {posed:?}");
            assert_eq!(
                executor.state, before,
                "{id}: a refused intent changes nothing"
            );
        }
        let opened = executor.run(Input::Tick(RiskClock::from_secs(MONDAY)), &ports)?;
        assert_eq!(raised(&opened), vec![own]);
        Ok(())
    }

    /// #668 round 1, major 1: a journaled `IntentReceived` under a switch's id whose agent or
    /// instrument is not that close's is no flatten, and leaves the close pending.
    #[test]
    fn only_the_close_the_id_names_is_a_flatten() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        for (agent, instrument) in [("agent-b", "AAPL"), ("agent-a", "MSFT")] {
            let mut executor = protected(&ports)?;
            executor.run(Input::Tick(RiskClock::from_secs(SATURDAY)), &ports)?;
            let switched = executor.run(switch(Initiator::RiskLimit, false)?, &ports)?;
            let switch_id = the_switch(&switched)?.event_id.clone();
            let held = aapl()?;
            let own = IntentId(EventId(format!("k-{}-0", switch_id.0)));
            commit(
                &mut executor,
                "IntentReceived",
                vec![
                    ("intent_id", text(own.0.0.clone())),
                    ("agent_id", text(agent)),
                    ("instrument_id", text(instrument)),
                    ("side", text("sell")),
                    ("type", text("limit")),
                    ("tif", text("day")),
                    ("qty", text("1")),
                    ("limit_price", text("150")),
                    ("purpose", text("discretionary_exit")),
                ],
            )?;
            assert!(
                !super::is_flatten(&executor.state, &own),
                "{agent} {instrument}"
            );
            assert!(
                executor
                    .state
                    .switches
                    .get(&switch_id)
                    .is_some_and(|switch| switch.pending.contains(&held)),
                "{agent} {instrument}"
            );
        }
        Ok(())
    }

    /// #668 round 1, minor 2: a second switch while the first's flatten is received but not yet
    /// sent, waiting on its protection's cancel, raises no second close for the same lots, and
    /// the confirmation sends one sell.
    #[test]
    fn a_second_switch_waits_on_the_first_flatten_still_unsent() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = protected(&ports)?;
        executor.run(Input::Tick(RiskClock::from_secs(MONDAY)), &ports)?;
        executor.run(quote_of("AAPL", "150", MONDAY)?, &ports)?;
        let first = executor.run(switch(Initiator::RiskLimit, false)?, &ports)?;
        assert_eq!(raised(&first).len(), 1, "{:?}", raised(&first));
        let second = executor.run(switch(Initiator::Owner, false)?, &ports)?;
        assert!(raised(&second).is_empty(), "{:?}", raised(&second));
        let confirmed = executor.run(cancel_accepted("md-oco-1"), &ports)?;
        assert_eq!(sold(&confirmed).len(), 1);
        assert!(raised(&confirmed).is_empty(), "{:?}", raised(&confirmed));
        Ok(())
    }

    /// Agent-b's buy in the instrument agent-a's switch closes.
    const OTHER_BUY: &str = "md-buy-of-b";
    /// Agent-a's buy in MSFT, where it holds no lots.
    const UNFILLED_BUY: &str = "md-msft-buy";
    /// A watchdog exit of no agent's in MSFT, and a later one.
    const WATCHDOG_EXITS: [&str; 2] = ["md-w-msft", "md-w-msft-later"];

    /// One order of `agent`'s for 5, journaled `accepted`, nothing filled.
    fn accepted_order(
        executor: &mut Executor,
        id: &str,
        agent: &str,
        instrument: &str,
        side: &str,
        purpose: &str,
    ) -> Result<(), ExecutorError> {
        commit(
            executor,
            "OrderSubmitted",
            vec![
                ("client_order_id", text(id)),
                ("agent", text(agent)),
                ("instrument", text(instrument)),
                ("side", text(side)),
                ("qty", text("5")),
                ("limit", text("100")),
                ("purpose", text(purpose)),
            ],
        )?;
        commit(
            executor,
            "OrderStateChanged",
            vec![("client_order_id", text(id)), ("state", text("accepted"))],
        )
    }

    /// #668 round 2, major 3: agent-b's accepted buy in AAPL, where agent-a's switch closes its 10
    /// protected lots, is never the switch's. Neither the switch nor any step after it cancels it,
    /// it holds no flatten back, and the flatten sells agent-a's 10, not a share of agent-b's.
    #[test]
    fn another_agents_accepted_order_is_never_reached() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = protected(&ports)?;
        accepted_order(&mut executor, OTHER_BUY, "agent-b", "AAPL", "buy", "open")?;
        executor.run(Input::Tick(RiskClock::from_secs(MONDAY)), &ports)?;
        executor.run(quote_of("AAPL", "150", MONDAY)?, &ports)?;
        let mut effects = executor.run(switch(Initiator::RiskLimit, false)?, &ports)?;
        assert!(
            !names(the_switch(&effects)?, "cancels").contains(&OTHER_BUY.to_owned()),
            "the switch records no cancel of agent-b's order"
        );
        let confirmed = executor.run(cancel_accepted("md-oco-1"), &ports)?;
        let [sell] = sold(&confirmed)
            .try_into()
            .map_err(|_| missing("one flatten sell"))?;
        assert_eq!(
            sell.qty,
            Qty::parse("10")?,
            "agent-a's 10 lots, the sub-ledger, and nothing of agent-b's (§5.5)"
        );
        effects.extend(confirmed);
        effects.extend(executor.run(accepted(&sell), &ports)?);
        for at in [MONDAY + 1, MONDAY + 2] {
            effects.extend(executor.run(Input::Tick(RiskClock::from_secs(at)), &ports)?);
        }
        assert!(
            !cancels(&effects).contains(&OTHER_BUY),
            "agent-b's order is never cancelled, at the switch or after it: {:?}",
            cancels(&effects)
        );
        Ok(())
    }

    /// #668 round 2, major 2 (rule 13): agent-a holds no lots in MSFT, only an accepted buy, so
    /// a `*` watchdog exit there is not its switch's to cancel. The switch cancels the buy and
    /// leaves the exit, and no later step cancels it or a new `*` exit, though the close stays
    /// pending with nothing to sell.
    #[test]
    fn a_switch_with_no_lots_leaves_a_watchdog_exit_alone() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = protected(&ports)?;
        accepted_order(
            &mut executor,
            UNFILLED_BUY,
            "agent-a",
            "MSFT",
            "buy",
            "open",
        )?;
        let [first_exit, later_exit] = WATCHDOG_EXITS;
        accepted_order(&mut executor, first_exit, "*", "MSFT", "sell", "risk_exit")?;
        executor.run(Input::Tick(RiskClock::from_secs(MONDAY)), &ports)?;
        executor.run(quote_of("MSFT", "100", MONDAY)?, &ports)?;
        let mut effects = executor.run(switch(Initiator::RiskLimit, false)?, &ports)?;
        assert!(
            cancels(&effects).contains(&UNFILLED_BUY),
            "the agent's own buy is cancelled: {:?}",
            cancels(&effects)
        );
        effects.extend(executor.run(cancel_accepted(UNFILLED_BUY), &ports)?);
        accepted_order(&mut executor, later_exit, "*", "MSFT", "sell", "risk_exit")?;
        for at in [MONDAY + 1, MONDAY + 2] {
            effects.extend(executor.run(Input::Tick(RiskClock::from_secs(at)), &ports)?);
        }
        assert!(
            WATCHDOG_EXITS
                .iter()
                .all(|exit| !cancels(&effects).contains(exit)),
            "a risk exit of no agent's is never held for a switch with nothing to sell: {:?}",
            cancels(&effects)
        );
        Ok(())
    }
}
