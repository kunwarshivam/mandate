//! A kill switch's cancel whose answer is lost within one run (DEC-2302 item 7; trading-domain
//! spec §5.5, §5.7; `AGENTS.md` rules 5, 6 and 13).
//!
//! Each order a switch named in `cancels` whose cancel is unconfirmed, `PendingCancel` or left
//! working by an ignored ask, has the cancel sent again every `unknown_absent_window_s` by the
//! risk clock, at most `unknown_absent_lookups` times in each run: §5.7's "N lookups over T
//! seconds" pair, which now carries both meanings. Each re-send is journaled first, and a run
//! counts the asks journaled since its start, so every run has its own budget. When the window
//! after the last re-send passes with the cancel still unconfirmed, one alert with an opaque key
//! sounds. After that nothing more is sent within the run, the order stays `PendingCancel` and is
//! never dropped, and the next start sends the cancel again (DEC-2302 item 3) with a fresh budget.
//! No other cancel is retried: protection keeps its own bound (§5.4), and an `Unknown` order is
//! queried, never cancelled blind.

mod common;

use common::{
    ACCOUNT_STREAM, ALLOWING_BINDING_GATE, FixedInstruments, FixedMandate, Ran, Shell, TestIds,
    agent, broker_order, clock, config, event, handoff, opening, ports, quote, risk_exit,
    stream_opened, text, with_clock,
};
use mandate_accounting::Side;
use mandate_canon::Value;
use mandate_executor::{
    BrokerOutcome, BrokerRequest, BrokerUnknown, ClientOrderId, Command, Effect, ExecutorConfig,
    Initiator, Input, KillScope, OrderState, Ports, handle,
};

const AAPL: &str = FixedInstruments::LIQUID_EQUITY;
const FRAC: &str = FixedInstruments::FRACTIONABLE;
const OCO: &str = "md-oco-1";
const INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ0";
const OTHER_INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ1";
const SWITCHED_AT: i64 = 100;
const RETRIES_EXHAUSTED: &str = "exit_stalled";

/// A stream opened and a process started on it, ready to open, with the risk clock at
/// [`SWITCHED_AT`].
fn fresh(ports: &Ports<'_>) -> Shell {
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(ports);
    shell.run(Input::Tick(clock(SWITCHED_AT)), ports);
    shell
}

/// Submits an opening of the agent's for ten under `intent`, answering its client order id.
fn submitted(shell: &mut Shell, ports: &Ports<'_>, intent: &str) -> String {
    let ran = shell.run(
        handoff(intent, common::AGENT, opening(AAPL, "10", "150")),
        ports,
    );
    ran.submissions()
        .first()
        .map(|order| order.client_order_id.as_str().to_owned())
        .expect("the opening is sent")
}

fn reported(id: &str, status: &str) -> Input {
    Input::Broker(Ok(BrokerOutcome::Order(broker_order(
        "b-1",
        Some(id),
        AAPL,
        Side::Buy,
        "10",
        "0",
        status,
    ))))
}

fn switch() -> Input {
    Input::Command(Command::KillSwitch {
        scope: KillScope::Agent(agent(common::AGENT)),
        initiator: Initiator::RiskLimit,
        confirmation: None,
        cause: None,
    })
}

/// A working opening, the agent's switch run on it, and the cancel's answer lost.
fn switched(ports: &Ports<'_>) -> (Shell, String) {
    let mut shell = fresh(ports);
    let id = submitted(&mut shell, ports, INTENT);
    shell.run(reported(&id, "new"), ports);
    let ran = shell.run(switch(), ports);
    assert_eq!(cancels(&ran, &id), 1, "{:?}", ran.requests);
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), ports);
    assert_eq!(state_of(&shell, &id), Some(OrderState::PendingCancel));
    (shell, id)
}

fn state_of(shell: &Shell, id: &str) -> Option<OrderState> {
    shell
        .state
        .order(&ClientOrderId::parse(id).expect("an id this platform derived"))
        .map(|order| order.state)
}

fn unconfirmed(shell: &Shell, id: &str) -> bool {
    shell
        .state
        .order(&ClientOrderId::parse(id).expect("an id this platform derived"))
        .is_some_and(|order| order.cancel_unconfirmed)
}

fn cancels(ran: &Ran, id: &str) -> usize {
    ran.requests
        .iter()
        .filter(|request| {
            matches!(request, BrokerRequest::Cancel { client_order_id } if client_order_id.as_str() == id)
        })
        .count()
}

fn queries(ran: &Ran, id: &str) -> usize {
    ran.requests
        .iter()
        .filter(|request| {
            matches!(request, BrokerRequest::GetOrderByClientId(asked) if asked.as_str() == id)
        })
        .count()
}

fn alerts(ran: &Ran) -> usize {
    ran.notifications
        .iter()
        .filter(|key| **key == RETRIES_EXHAUSTED)
        .count()
}

/// Ticks at `from`, `from + 1`, ..., `to`, answering the cancels and alerts they sent.
fn ticks(shell: &mut Shell, ports: &Ports<'_>, id: &str, from: i64, to: i64) -> (usize, usize) {
    let mut sent = (0, 0);
    for at in from..=to {
        let ran = shell.run(Input::Tick(clock(at)), ports);
        sent.0 += cancels(&ran, id);
        sent.1 += alerts(&ran);
    }
    sent
}

/// The first re-send goes at the window and not a second before it, and the
/// `OrderStateChanged` that asks it precedes the request in the same list (rule 5).
#[test]
fn a_lost_kill_switch_cancel_is_sent_again_once_the_window_passes() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let window = configuration.unknown_absent_window_s;
    let (mut shell, id) = switched(&ports);
    let (early, _) = ticks(
        &mut shell,
        &ports,
        &id,
        SWITCHED_AT + 1,
        SWITCHED_AT + window - 1,
    );
    assert_eq!(early, 0, "nothing before the window");
    let ran = shell.run(Input::Tick(clock(SWITCHED_AT + window)), &ports);
    assert_eq!(cancels(&ran, &id), 1, "{:?}", ran.requests);
    let asked = ran.effects.iter().position(|effect| {
        matches!(effect, Effect::Journal(draft)
            if draft.event_type == "OrderStateChanged"
                && draft.payload.get("client_order_id") == Some(&Value::Str(id.clone()))
                && draft.payload.get("state") == Some(&Value::Str("pending_cancel".to_owned()))
                && draft.payload.get("cancel_requested") == Some(&Value::Bool(true)))
    });
    let sent = ran.effects.iter().position(|effect| {
        matches!(effect, Effect::Broker(BrokerRequest::Cancel { client_order_id })
            if client_order_id.as_str() == id)
    });
    assert!(
        asked.is_some() && asked < sent,
        "journaled before it is sent: {:?}",
        ran.effects
    );
    assert_eq!(state_of(&shell, &id), Some(OrderState::PendingCancel));
}

/// The re-sends stop at the cap; the one alert sounds when the window after the cap-th passes with
/// the cancel still unconfirmed, never with the cap-th itself; the order stays `PendingCancel` with
/// its cancel unconfirmed after it, never dropped.
#[test]
fn the_retry_stops_at_the_cap_and_alerts_once_its_last_window_passes() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let window = configuration.unknown_absent_window_s;
    let cap = configuration.unknown_absent_lookups;
    let (mut shell, id) = switched(&ports);
    let mut sent = 0;
    for retry in 1..=i64::from(cap) {
        let ran = shell.run(Input::Tick(clock(SWITCHED_AT + retry * window)), &ports);
        sent += cancels(&ran, &id);
        assert_eq!(alerts(&ran), 0, "retry {retry}");
    }
    assert_eq!(sent, usize::try_from(cap).unwrap_or(usize::MAX));
    let last = SWITCHED_AT + i64::from(cap) * window;
    let (early, quiet) = ticks(&mut shell, &ports, &id, last + 1, last + window - 1);
    assert_eq!(
        (early, quiet),
        (0, 0),
        "no alert before the last window passes"
    );
    let expired = shell.run(Input::Tick(clock(last + window)), &ports);
    assert_eq!((cancels(&expired, &id), alerts(&expired)), (0, 1));
    let (more, alerted) = ticks(
        &mut shell,
        &ports,
        &id,
        last + window + 1,
        last + 5 * window,
    );
    assert_eq!((more, alerted), (0, 0), "nothing more within the run");
    assert_eq!(state_of(&shell, &id), Some(OrderState::PendingCancel));
    assert!(unconfirmed(&shell, &id));
}

/// A cancel the broker confirms after the cap-th re-send raises no alert: "exhausted" means the
/// last window passed with the cancel still unconfirmed.
#[test]
fn a_cancel_confirmed_after_the_last_re_send_raises_no_alert() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let window = configuration.unknown_absent_window_s;
    let cap = i64::from(configuration.unknown_absent_lookups);
    let (mut shell, id) = switched(&ports);
    let (sent, early) = ticks(
        &mut shell,
        &ports,
        &id,
        SWITCHED_AT + 1,
        SWITCHED_AT + cap * window,
    );
    assert_eq!(early, 0, "no alert with the cap-th re-send");
    assert_eq!(sent, usize::try_from(cap).unwrap_or(usize::MAX));
    shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: id.clone(),
        })),
        &ports,
    );
    let last = SWITCHED_AT + cap * window;
    let (more, alerted) = ticks(&mut shell, &ports, &id, last + 1, last + 3 * window);
    assert_eq!((more, alerted), (0, 0));
    assert_eq!(state_of(&shell, &id), Some(OrderState::Canceled));
}

/// The cadence and the cap are the configuration's, not literals: a window of 7 and two lookups
/// send at 7 and 14 and nothing after.
#[test]
fn the_cadence_and_the_cap_follow_the_configuration() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = ExecutorConfig {
        unknown_absent_window_s: 7,
        unknown_absent_lookups: 2,
        ..config()
    };
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let (mut shell, id) = switched(&ports);
    let mut at = Vec::new();
    for tick in SWITCHED_AT + 1..=SWITCHED_AT + 40 {
        let ran = shell.run(Input::Tick(clock(tick)), &ports);
        if cancels(&ran, &id) > 0 {
            at.push(tick - SWITCHED_AT);
        }
    }
    assert_eq!(at, vec![7, 14]);
}

/// Every run has its own budget: a run counts the asks journaled since its start. After one
/// re-send in the first run, a restart sends the cancel once at the start (DEC-2302 item 3) and the
/// run then retries the whole cap again, with its own alert; so does the run after that, so an
/// outage that spends one run's budget never leaves the order working (`AGENTS.md` rule 13).
#[test]
fn every_run_retries_with_its_own_budget() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let window = configuration.unknown_absent_window_s;
    let cap = usize::try_from(configuration.unknown_absent_lookups).unwrap_or(usize::MAX);
    let (mut shell, id) = switched(&ports);
    let first = shell.run(Input::Tick(clock(SWITCHED_AT + window)), &ports);
    assert_eq!(cancels(&first, &id), 1);

    let (mut restarted, started) = shell.restart_keeping_broker(&ports);
    assert_eq!(cancels(&started, &id), 1, "{:?}", started.requests);
    let (again, alerted) = ticks(
        &mut restarted,
        &ports,
        &id,
        SWITCHED_AT + window + 1,
        SWITCHED_AT + 10 * window,
    );
    assert_eq!(
        (again, alerted),
        (cap, 1),
        "a fresh budget in the second run"
    );

    let (mut third, started) = restarted.restart_keeping_broker(&ports);
    assert_eq!(cancels(&started, &id), 1, "{:?}", started.requests);
    let (again, alerted) = ticks(
        &mut third,
        &ports,
        &id,
        SWITCHED_AT + 10 * window + 1,
        SWITCHED_AT + 20 * window,
    );
    assert_eq!((again, alerted), (cap, 1), "and in the third");
    assert_eq!(state_of(&third, &id), Some(OrderState::PendingCancel));
}

/// A confirmation, or any terminal report, ends the retry.
#[test]
fn a_confirmed_cancel_is_not_sent_again() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let window = configuration.unknown_absent_window_s;
    let (mut shell, id) = switched(&ports);
    shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: id.clone(),
        })),
        &ports,
    );
    let (sent, _) = ticks(
        &mut shell,
        &ports,
        &id,
        SWITCHED_AT + 1,
        SWITCHED_AT + 5 * window,
    );
    assert_eq!(sent, 0);
    assert_eq!(state_of(&shell, &id), Some(OrderState::Canceled));
}

/// A cancel that is not a kill switch's, its answer lost, is not retried by this rule: the
/// sequence that asked it keeps its own bound (§5.4).
#[test]
fn a_cancel_no_kill_switch_asked_is_never_retried() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let window = configuration.unknown_absent_window_s;
    let mut shell = fresh(&ports);
    let id = submitted(&mut shell, &ports, INTENT);
    shell.run(reported(&id, "new"), &ports);
    let asked = shell.run(
        Input::Command(Command::CancelOpenings {
            agent: agent(common::AGENT),
            instrument: common::instrument(AAPL),
        }),
        &ports,
    );
    assert_eq!(cancels(&asked, &id), 1);
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);
    let (sent, alerted) = ticks(
        &mut shell,
        &ports,
        &id,
        SWITCHED_AT + 1,
        SWITCHED_AT + 5 * window,
    );
    assert_eq!((sent, alerted), (0, 0));
    assert_eq!(state_of(&shell, &id), Some(OrderState::PendingCancel));
}

/// An `Unknown` order of the switched agent is queried as §5.7 says, never cancelled blind, by
/// the retry or by the switch.
#[test]
fn an_unknown_order_of_the_switched_agent_is_queried_never_cancelled() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let window = configuration.unknown_absent_window_s;
    let mut shell = fresh(&ports);
    let id = submitted(&mut shell, &ports, OTHER_INTENT);
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);
    assert_eq!(state_of(&shell, &id), Some(OrderState::Unknown));
    let ran = shell.run(switch(), &ports);
    assert_eq!(cancels(&ran, &id), 0);
    let mut asked = 0;
    for at in SWITCHED_AT + 1..=SWITCHED_AT + 5 * window {
        let ran = shell.run(Input::Tick(clock(at)), &ports);
        assert_eq!(cancels(&ran, &id), 0, "tick {at}");
        asked += queries(&ran, &id);
    }
    assert!(asked > 0, "the Unknown order is looked up");
    assert_eq!(state_of(&shell, &id), Some(OrderState::Unknown));
}

/// The agent's buy of `quantity` in `name`, filled, as the fold reads it from `seq` on.
fn bought(shell: &mut Shell, seq: u64, id: &str, name: &str, quantity: &str) {
    let events = [
        event(
            ACCOUNT_STREAM,
            seq,
            "OrderSubmitted",
            with_clock(
                &[
                    ("client_order_id", text(id)),
                    ("agent", text(common::AGENT)),
                    ("instrument", text(name)),
                    ("side", text("buy")),
                    ("qty", text(quantity)),
                    ("limit", text("150")),
                ],
                10,
            ),
        ),
        event(
            ACCOUNT_STREAM,
            seq + 1,
            "FillApplied",
            with_clock(
                &[
                    ("fill_id", text(&format!("f-{id}"))),
                    ("client_order_id", text(id)),
                    ("instrument", text(name)),
                    ("side", text("buy")),
                    ("qty_gross", text(quantity)),
                    ("price", text("150")),
                ],
                10,
            ),
        ),
        event(
            ACCOUNT_STREAM,
            seq + 2,
            "OrderStateChanged",
            with_clock(
                &[("client_order_id", text(id)), ("state", text("filled"))],
                10,
            ),
        ),
    ];
    for folded in &events {
        shell.fold_one(folded).expect("the attributed buy folds");
    }
}

/// A protective order's cancel, asked by a risk exit's sequence and its answer lost, is never
/// touched by this retry: no cancel of it and no alert within the cap's span, which is shorter
/// than §5.4's bound, so protection's own re-ask rule alone governs it.
#[test]
fn a_protective_cancel_is_never_retried_by_this_rule() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let window = configuration.unknown_absent_window_s;
    let span = i64::from(configuration.unknown_absent_lookups) * window;
    assert!(
        span < configuration.max_unprotected_s,
        "the case needs the cap inside §5.4's bound"
    );
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("folds");
    bought(&mut shell, 2, "md-held-1", AAPL, "10");
    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            5,
            "ProtectionChanged",
            with_clock(
                &[
                    ("instrument", text(AAPL)),
                    ("action", text("placed")),
                    ("orders", text(OCO)),
                    ("qty", text("10")),
                    ("take_profit", text("170")),
                    ("stop", text("140")),
                    ("created_on", text("2026-09-22")),
                ],
                11,
            ),
        ))
        .expect("the resting OCO folds");
    let mut shell = shell.restart_ready(&ports);
    shell.run(Input::Tick(clock(SWITCHED_AT)), &ports);
    shell.run(
        Input::Market(quote(AAPL, "155", "155.1", SWITCHED_AT)),
        &ports,
    );
    let asked = shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "155")),
        &ports,
    );
    assert_eq!(cancels(&asked, OCO), 1, "{:?}", asked.requests);
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);
    let (sent, alerted) = ticks(&mut shell, &ports, OCO, SWITCHED_AT + 1, SWITCHED_AT + span);
    assert_eq!((sent, alerted), (0, 0));
}

/// DEC-2857, which ends DEC-485 item 2's wait on an `Unknown` order: while an opening of the
/// switched agent is `Unknown` in one instrument, the switch still sells the ten it holds there at
/// once, beside the sell in the other instrument, and nothing is denied. The opening is then found
/// cancelled with nothing bought, so its remainder finds nothing more to sell.
#[test]
#[ignore = "pending E7-4"]
fn an_unknown_opening_holds_no_flatten_and_leaves_its_remainder_nothing_more() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, FRAC]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("folds");
    bought(&mut shell, 2, "md-held-1", AAPL, "10");
    bought(&mut shell, 5, "md-held-2", FRAC, "5");
    let mut shell = shell.restart_ready(&ports);
    shell.run(Input::Tick(clock(SWITCHED_AT)), &ports);
    shell.run(
        Input::Market(quote(AAPL, "155", "155.1", SWITCHED_AT)),
        &ports,
    );
    shell.run(
        Input::Market(quote(FRAC, "155", "155.1", SWITCHED_AT)),
        &ports,
    );
    let unknown = submitted(&mut shell, &ports, OTHER_INTENT);
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);
    assert_eq!(state_of(&shell, &unknown), Some(OrderState::Unknown));
    let ran = shell.run(switch(), &ports);
    assert_eq!(
        sells(&ran),
        vec![AAPL.to_owned(), FRAC.to_owned()],
        "{:?}",
        ran.draft_types()
    );
    let record = ran
        .draft("KillSwitchActivated")
        .expect("the switch is journaled");
    let named = |member: &str| record.payload.get(member).cloned();
    assert_eq!(
        named("closes"),
        Some(Value::Array(vec![text(AAPL), text(FRAC)]))
    );
    assert_eq!(named("deferred"), Some(Value::Array(Vec::new())));
    assert!(
        !ran.drafts
            .iter()
            .any(|draft| draft.event_type == "GateDecided"
                && draft.payload.get("verdict") == Some(&Value::Str("deny".to_owned()))),
        "nothing is denied: {:?}",
        ran.draft_types()
    );
    let mut released = sells(&shell.run(reported(&unknown, "canceled"), &ports));
    for at in SWITCHED_AT + 1..=SWITCHED_AT + 3 {
        released.extend(sells(&shell.run(Input::Tick(clock(at)), &ports)));
    }
    assert_eq!(released, Vec::<String>::new());
}

fn sells(ran: &Ran) -> Vec<String> {
    ran.submissions()
        .into_iter()
        .filter(|order| order.side == Side::Sell)
        .map(|order| order.instrument.as_str().to_owned())
        .collect()
}

/// An order a switch named whose cancel the journal asked but could not record as `PendingCancel`
/// (an ignored attempt, which leaves it `Accepted` with the cancel unconfirmed, as when it was
/// `Unknown` at the switch) is cancelled again too: the re-send takes it to `PendingCancel`, within
/// the same budget, with the alert when its last window passes (`AGENTS.md` rule 13).
#[test]
fn a_switched_order_an_ignored_ask_left_working_is_cancelled_again() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let window = configuration.unknown_absent_window_s;
    let id = "md-switched-1";
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("folds");
    let events = [
        event(
            ACCOUNT_STREAM,
            2,
            "OrderSubmitted",
            with_clock(
                &[
                    ("client_order_id", text(id)),
                    ("agent", text(common::AGENT)),
                    ("instrument", text(AAPL)),
                    ("side", text("buy")),
                    ("qty", text("10")),
                    ("limit", text("150")),
                ],
                SWITCHED_AT,
            ),
        ),
        event(
            ACCOUNT_STREAM,
            3,
            "OrderStateChanged",
            with_clock(
                &[("client_order_id", text(id)), ("state", text("accepted"))],
                SWITCHED_AT,
            ),
        ),
        event(
            ACCOUNT_STREAM,
            4,
            "KillSwitchActivated",
            with_clock(
                &[
                    ("scope", text("agent")),
                    ("subject", text(common::AGENT)),
                    ("initiator", text("risk_limit")),
                    ("purpose", text("risk_exit")),
                    ("cancels", Value::Array(vec![text(id)])),
                    ("closes", Value::Array(Vec::new())),
                    ("deferred", Value::Array(Vec::new())),
                ],
                SWITCHED_AT,
            ),
        ),
        event(
            ACCOUNT_STREAM,
            5,
            "OrderStateChanged",
            with_clock(
                &[
                    ("client_order_id", text(id)),
                    ("state", text("accepted")),
                    ("attempted", text("pending_cancel")),
                    ("ignored", Value::Bool(true)),
                    ("cancel_requested", Value::Bool(true)),
                ],
                SWITCHED_AT,
            ),
        ),
    ];
    for folded in &events {
        shell.fold_one(folded).expect("the switched order folds");
    }
    let mut shell = shell.restart_ready(&ports);
    assert_eq!(state_of(&shell, id), Some(OrderState::Accepted));
    assert!(unconfirmed(&shell, id));
    let cap = usize::try_from(configuration.unknown_absent_lookups).unwrap_or(usize::MAX);
    let (sent, alerted) = ticks(
        &mut shell,
        &ports,
        id,
        SWITCHED_AT,
        SWITCHED_AT + 10 * window,
    );
    assert_eq!((sent, alerted), (cap, 1));
    assert_eq!(state_of(&shell, id), Some(OrderState::PendingCancel));
}

/// The order a switch named and cancelled at `at`, as the fold reads it from `seq` on: submitted,
/// accepted, and asked `PendingCancel`.
fn seeded(shell: &mut Shell, seq: u64, id: &str) {
    for (offset, event_type, fields) in [
        (
            0,
            "OrderSubmitted",
            vec![
                ("client_order_id", text(id)),
                ("agent", text(common::AGENT)),
                ("instrument", text(AAPL)),
                ("side", text("buy")),
                ("qty", text("10")),
                ("limit", text("150")),
            ],
        ),
        (
            1,
            "OrderStateChanged",
            vec![("client_order_id", text(id)), ("state", text("accepted"))],
        ),
    ] {
        shell
            .fold_one(&event(
                ACCOUNT_STREAM,
                seq + offset,
                event_type,
                with_clock(&fields, SWITCHED_AT),
            ))
            .expect("the seeded order folds");
    }
}

fn asked(shell: &mut Shell, seq: u64, id: &str, at: i64) {
    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            seq,
            "OrderStateChanged",
            with_clock(
                &[
                    ("client_order_id", text(id)),
                    ("state", text("pending_cancel")),
                    ("cancel_requested", Value::Bool(true)),
                ],
                at,
            ),
        ))
        .expect("the ask folds");
}

/// The alert survives a tick handed again after its append was left in doubt: two orders one
/// switch named, asked a window apart, put one order's last re-send on the tick where the other's
/// alert falls, so that tick's batch journals. Handed again unfolded, as the shell does with an
/// append in doubt, it still raises the alert, since it is judged against the same earlier tick.
#[test]
fn the_alert_survives_a_tick_handed_again_after_an_append_in_doubt() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let window = configuration.unknown_absent_window_s;
    let cap = i64::from(configuration.unknown_absent_lookups);
    let (first, second) = ("md-switched-a", "md-switched-b");
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("folds");
    seeded(&mut shell, 2, first);
    seeded(&mut shell, 4, second);
    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            6,
            "KillSwitchActivated",
            with_clock(
                &[
                    ("scope", text("agent")),
                    ("subject", text(common::AGENT)),
                    ("initiator", text("risk_limit")),
                    ("purpose", text("risk_exit")),
                    ("cancels", Value::Array(vec![text(first), text(second)])),
                    ("closes", Value::Array(Vec::new())),
                    ("deferred", Value::Array(Vec::new())),
                ],
                SWITCHED_AT,
            ),
        ))
        .expect("the switch folds");
    asked(&mut shell, 7, first, SWITCHED_AT);
    asked(&mut shell, 8, second, SWITCHED_AT + window);
    let mut shell = shell.restart_ready(&ports);
    let alerted = |effects: &[Effect]| {
        effects.iter().any(
            |effect| matches!(effect, Effect::Notify(notice) if notice.kind.key() == RETRIES_EXHAUSTED),
        )
    };
    let drafted = |effects: &[Effect]| {
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Journal(_)))
    };
    for at in SWITCHED_AT + window..=SWITCHED_AT + (cap + 3) * window {
        let mut probe = shell.state.clone();
        let handed = handle(
            &mut probe,
            Input::Tick(clock(at)),
            &ports,
            &ALLOWING_BINDING_GATE,
        )
        .expect("the tick is handed");
        if alerted(&handed) && drafted(&handed) {
            let again = handle(
                &mut probe,
                Input::Tick(clock(at)),
                &ports,
                &ALLOWING_BINDING_GATE,
            )
            .expect("the same tick is handed again while its append is in doubt");
            assert!(alerted(&again), "tick {at}: {again:?}");
            return;
        }
        shell.run(Input::Tick(clock(at)), &ports);
    }
    panic!("no tick raised the alert beside a journaled re-send");
}
