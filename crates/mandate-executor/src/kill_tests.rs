//! The kill switch's own paths, pinned where the hand tests' oracles cannot see them: the close
//! flow after the account-wide cancel-all, the restart resume, the session deferral, and the
//! second switch's plan. Each corresponds to a `cargo mutants` verdict on this module whose
//! missed mutant was invisible to the integration oracles, which read only intent-placed orders
//! (§5.5; DEC-451 item 1).

use mandate_accounting::InstrumentId;
use mandate_canon::Value;
use mandate_num::Qty;

use crate::error::ExecutorError;
use crate::payload::{clock, text};
use crate::ports::Ports;
use crate::reconcile::tests::{Everything, Executor, Ids, executor_config, fees};
use crate::types::{
    AccountRef, BrokerOutcome, BrokerRequest, Command, Effect, Initiator, Input, KillScope,
    RiskClock, WriterEpoch,
};

fn ports() -> Result<Ports<'static>, ExecutorError> {
    let fees = fees()?;
    let config = executor_config();
    Ok(Ports {
        ids: &Ids,
        mandates: &Everything,
        instruments: &Everything,
        config: Box::leak(Box::new(config)),
        fees: Box::leak(Box::new(fees)),
    })
}

fn position_held(executor: &mut Executor, name: &str, qty: &str) -> Result<(), ExecutorError> {
    executor.commit_one(
        "OrderSubmitted",
        crate::payload::object(vec![
            ("client_order_id", text("md-seed-1")),
            ("agent", text("agent-a")),
            ("instrument", text(name)),
            ("side", text("buy")),
            ("qty", text(qty)),
            ("limit", text("150")),
            ("risk_clock", clock(RiskClock::from_secs(1))?),
        ])?,
    )?;
    executor.commit_one(
        "FillApplied",
        crate::payload::object(vec![
            ("fill_id", text("f-seed")),
            ("client_order_id", text("md-seed-1")),
            ("instrument", text(name)),
            ("side", text("buy")),
            ("qty_gross", text(qty)),
            ("price", text("150")),
            ("risk_clock", clock(RiskClock::from_secs(2))?),
        ])?,
    )?;
    executor.commit_one(
        "OrderStateChanged",
        crate::payload::object(vec![
            ("client_order_id", text("md-seed-1")),
            ("state", text("filled")),
            ("risk_clock", clock(RiskClock::from_secs(3))?),
        ])?,
    )
}

/// The account scope's close flow: the cancel-all's acceptance opens the close step, which
/// journals the close record and requests the broker's close-position for the account-wide scope.
/// A stub here leaves a switched account closed on the journal and never at the broker.
#[test]
fn the_account_wide_acceptance_opens_the_close_step() -> Result<(), ExecutorError> {
    let ports = ports()?;
    let mut executor = Executor::opened(&ports)?;
    position_held(&mut executor, "AAPL", "10")?;
    let ran = executor.run_keeping(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Account(AccountRef("acct-1".to_owned())),
            initiator: Initiator::Owner,
            confirmation: None,
        }),
        &ports,
        usize::MAX,
    )?;
    assert!(
        ran.iter()
            .any(|effect| matches!(effect, Effect::Broker(BrokerRequest::CancelAll(_)))),
        "the account switch asks the broker's cancel-all"
    );
    let closed = executor.run_keeping(
        Input::Broker(Ok(BrokerOutcome::AccountWideAccepted)),
        &ports,
        usize::MAX,
    )?;
    assert!(
        closed.iter().any(|effect| matches!(
            effect,
            Effect::Journal(draft)
                if draft.event_type == "KillSwitchActivated"
                    && draft.payload.get("step") == Some(&Value::Str("close".to_owned()))
        )),
        "the close step is journaled: {closed:?}"
    );
    assert!(
        closed.iter().any(|effect| matches!(
            effect,
            Effect::Broker(BrokerRequest::ClosePosition(_, instrument))
                if instrument.as_str() == "AAPL"
        )),
        "and the broker's close-position is requested: {closed:?}"
    );
    Ok(())
}

/// A restart under an unanswered account-wide switch re-asks the idempotent cancel-all, whose
/// answer reconfirms the scope, rather than closing on a memory (§5.5's cancel → confirm →
/// close). Deleting the resume's account arm leaves the switched account never closed.
#[test]
fn a_restart_reasks_the_account_wide_cancel() -> Result<(), ExecutorError> {
    let ports = ports()?;
    let mut executor = Executor::opened(&ports)?;
    position_held(&mut executor, "AAPL", "10")?;
    executor.run_keeping(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Account(AccountRef("acct-1".to_owned())),
            initiator: Initiator::RiskLimit,
            confirmation: None,
        }),
        &ports,
        usize::MAX,
    )?;
    let mut state = Executor::refold(&executor)?;
    state.epoch = None;
    state.started = false;
    let mut next = Executor::from_state(state, 2);
    let started = next.run_keeping(
        Input::Started(WriterEpoch(
            u64::try_from(executor.journal.len())
                .map_err(|_| ExecutorError::NotInterpreted {
                    what: "journal len".to_owned(),
                    story: "E7-4",
                })?
                .saturating_add(5),
        )),
        &ports,
        usize::MAX,
    )?;
    assert!(
        started
            .iter()
            .any(|effect| matches!(effect, Effect::Broker(BrokerRequest::CancelAll(_)))),
        "the resume re-asks the cancel-all: {started:?}"
    );
    Ok(())
}

/// An automated equity switch made while no v1 session is open defers its sell to the session,
/// and the deferral holds while the market stays closed (§5.5): the plan's `deferred` was
/// journaled, the venue read fresh at every release. Flipping either comparison sends the sell
/// overnight or never.
#[test]
fn the_deferred_sell_waits_for_the_session() -> Result<(), ExecutorError> {
    let ports = ports()?;
    let mut executor = Executor::opened(&ports)?;
    let saturday = 1_790_438_400_i64;
    executor.run_keeping(
        Input::Tick(RiskClock::from_secs(saturday)),
        &ports,
        usize::MAX,
    )?;
    position_held(&mut executor, "AAPL", "10")?;
    let ran = executor.run_keeping(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(crate::types::AgentId("agent-a".to_owned())),
            initiator: Initiator::RiskLimit,
            confirmation: None,
        }),
        &ports,
        usize::MAX,
    )?;
    assert!(
        !ran.iter().any(|effect| matches!(
            effect,
            Effect::Broker(BrokerRequest::Submit(order)) if order.side == mandate_accounting::Side::Sell
        )),
        "the deferred sell waits for the session: no sell is submitted: {ran:?}"
    );
    let deferred = executor
        .state
        .flattens
        .get(&InstrumentId::new("AAPL")?)
        .is_some_and(|plan| plan.deferred);
    assert!(deferred, "and the plan is folded as deferred");
    Ok(())
}

/// The plan's quantity is what the sub-ledger has left: a second switch plans nothing when the
/// first one's sell still speaks for the lots, so two switches never sell the same shares twice
/// (§5.5 sells exactly the sub-ledger quantity).
#[test]
fn a_second_switch_plans_only_what_the_first_left() -> Result<(), ExecutorError> {
    let ports = ports()?;
    let mut executor = Executor::opened(&ports)?;
    position_held(&mut executor, "BTCUSD", "0.5")?;
    let whole = Qty::parse("0.5")?;
    let first = executor.run_keeping(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(crate::types::AgentId("agent-a".to_owned())),
            initiator: Initiator::RiskLimit,
            confirmation: None,
        }),
        &ports,
        usize::MAX,
    )?;
    assert!(
        first.iter().any(|effect| matches!(
            effect,
            Effect::Broker(BrokerRequest::Submit(order)) if order.qty == whole
        )),
        "the first switch sells the whole sub-ledger at once (crypto)"
    );
    let second = executor.run_keeping(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(crate::types::AgentId("agent-a".to_owned())),
            initiator: Initiator::RiskLimit,
            confirmation: None,
        }),
        &ports,
        usize::MAX,
    )?;
    assert!(
        !second
            .iter()
            .any(|effect| matches!(effect, Effect::Broker(BrokerRequest::Submit(_)))),
        "and the second plans nothing: {second:?}"
    );
    Ok(())
}

/// The kill switch's own orders go out under the switch's derived ids, and the fold ends the plan
/// on the sell's own submission (`kill_sell`), so a replay ends it too.
#[test]
fn the_kills_own_sell_id_ends_its_plan_on_replay() -> Result<(), ExecutorError> {
    let ports = ports()?;
    let mut executor = Executor::opened(&ports)?;
    position_held(&mut executor, "BTCUSD", "0.5")?;
    executor.run_keeping(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(crate::types::AgentId("agent-a".to_owned())),
            initiator: Initiator::RiskLimit,
            confirmation: None,
        }),
        &ports,
        usize::MAX,
    )?;
    assert!(
        executor.state.flattens.is_empty(),
        "the plan ended on the sell's own submission"
    );
    Ok(())
}
