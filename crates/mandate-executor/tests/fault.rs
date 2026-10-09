//! Fault injection at every submission step (E7-3's acceptance clause: "fault injection at every
//! submission step yields zero duplicates and full reconciliation; mismatches pause the agent and
//! alert").
//!
//! "Every step" is enumerated rather than asserted, so the claim is checkable. Each of the twelve
//! points is a named position in one submission's effect pipeline ([`common::CrashPoint`]). Each
//! case drives the pipeline until the list that contains that point, crashes there, **asserts that
//! the point was reached** (a crash point the run never reaches proves nothing), restarts with the
//! broker carried across, answers recovery with what the broker actually holds, and then asserts
//! what the script requires:
//!
//! 1. **Exactly one order at the broker**, read off the fake connector's own counter rather than
//!    the journal: the intent's client order id was accepted once — not zero times (the order
//!    lost) and not twice (the duplicate R-03 names).
//! 2. **Full reconciliation**: a reconciliation against the broker's own picture leaves nothing
//!    unexplained, adopts nothing twice, and pauses nobody.
//! 3. **Protection is accounted for**: every unprotected interval the journal opens it also
//!    closes.
//! 4. **The fold is faithful**: replaying the journal reproduces the state, and the replay reaches
//!    no broker.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{
    ACCOUNT_STREAM, CrashPoint, FixedInstruments, FixedMandate, Shell, TestIds, broker_account,
    broker_fill, broker_order, broker_position, clock, config, event, handoff, instrument, opening,
    ports, price, protected_opening, qty, quote, risk_exit, snapshot, stream_opened, text, usd,
    with_clock,
};
use mandate_accounting::Side;
use mandate_canon::Value;
use mandate_executor::{
    BrokerFill, BrokerOrder, BrokerOutcome, BrokerPosition, BrokerRequest, BrokerUpdate,
    ClientOrderId, EventId, Input, IntentId, Ports, ReconcileReason, ReconciliationVerdict,
};
use mandate_num::Usd;

const AAPL: &str = FixedInstruments::LIQUID_EQUITY;
const INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ0";

/// The client order id E7-2 derives for [`INTENT`].
fn intent_order_id() -> String {
    ClientOrderId::for_intent(&IntentId(EventId(INTENT.to_owned())))
        .unwrap_or_else(|e| panic!("the intent's id derives: {e}"))
        .as_str()
        .to_owned()
}

/// What the broker holds at the end of a case, which is what every answer after the restart
/// tells the executor: the recovery sees the broker's truth, never a guess.
#[derive(Default)]
struct Truth {
    open: Vec<BrokerOrder>,
    fills: Vec<BrokerFill>,
    positions: Vec<BrokerPosition>,
    /// The broker's cash once a fill since the last account snapshot has moved it; `None` keeps
    /// the default account's. A reconciliation compares cash within a band of the fills since its
    /// base (§11 step 4), so a truth that reports a fill must report the cash that fill moved.
    /// With no position left, the account's equity and buying power are that cash too.
    cash: Option<Usd>,
}

impl Truth {
    fn snapshot(&self, shell: &Shell, reason: ReconcileReason) -> mandate_executor::BrokerSnapshot {
        let mut taken = snapshot(shell.head().0, reason);
        taken.open_orders = self.open.clone();
        taken.fills = self.fills.clone();
        taken.positions = self.positions.clone();
        if let Some(cash) = self.cash {
            assert!(
                self.positions.is_empty(),
                "a truth that sets the cash holds no position, so its equity and buying power are \
                 that cash"
            );
            taken.account.cash = cash;
            taken.account.equity = cash;
            taken.account.buying_power = cash;
            taken.account.non_marginable_buying_power = cash;
        }
        taken
    }
}

/// The broker's answer to a lookup of `id`: the order if its counter accepted it, else absent.
fn lookup(shell: &Shell, id: &str, status: &str, filled: &str, quantity: &str) -> Input {
    if shell.connector.accepted_for(id) > 0 {
        Input::Broker(Ok(BrokerOutcome::Order(broker_order(
            "b-1",
            Some(id),
            AAPL,
            Side::Buy,
            quantity,
            filled,
            status,
        ))))
    } else {
        Input::Broker(Ok(BrokerOutcome::Absent {
            client_order_id: id.to_owned(),
        }))
    }
}

/// Answers absences, eight seconds apart, until the order is sent again: the broker never received
/// it, and `unknown_absent_lookups` absences spanning `unknown_absent_window_s` are what confirm
/// that (§5.7, interpretation 9). Once the order is resubmitted the broker has it, so no further
/// absence is answered.
fn confirm_absence(shell: &mut Shell, ports: &Ports<'_>, id: &str, from: i64) {
    for at in [
        from,
        from.saturating_add(8),
        from.saturating_add(16),
        from.saturating_add(24),
    ] {
        if shell.connector.accepted_for(id) > 0 {
            return;
        }
        shell.run(Input::Tick(clock(at)), ports);
        shell.run(
            Input::Broker(Ok(BrokerOutcome::Absent {
                client_order_id: id.to_owned(),
            })),
            ports,
        );
    }
}

/// Requirement 1: the broker accepted each client order id at most once, and `id` exactly once.
fn assert_exactly_one(shell: &Shell, id: &str, point: CrashPoint) {
    for (other, count) in &shell.connector.accepted {
        assert!(
            *count <= 1,
            "crash at {point:?}: the broker accepted {count} distinct submissions for {other}, \
             which is the duplicate window R-03 names"
        );
    }
    assert_eq!(
        shell.connector.accepted_for(id),
        1,
        "crash at {point:?}: the order for the intent reached the broker exactly once after \
         recovery — zero would be an order lost, two a duplicate: {:?}",
        shell.connector.requests
    );
    let mut submitted: BTreeMap<(String, u64), usize> = BTreeMap::new();
    for stored in &shell.account_journal {
        if stored.event_type != "OrderSubmitted" {
            continue;
        }
        let named = stored
            .payload
            .get("client_order_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let attempt = stored
            .payload
            .get("attempt")
            .and_then(Value::as_int)
            .unwrap_or(0);
        *submitted.entry((named, attempt)).or_insert(0) += 1;
    }
    assert!(
        submitted.keys().any(|(named, _)| named == id),
        "crash at {point:?}: the journal names the order before the broker has it"
    );
    for ((named, attempt), count) in submitted {
        assert_eq!(
            count, 1,
            "crash at {point:?}: {named} attempt {attempt} was journaled {count} times"
        );
    }
}

/// Requirements 2 to 4.
fn assert_recovered(shell: &Shell, ports: &Ports<'_>, truth: &Truth, point: CrashPoint) {
    let run = mandate_executor::reconcile(
        &shell.state,
        &truth.snapshot(shell, ReconcileReason::Scheduled),
        ports,
    )
    .unwrap_or_else(|e| panic!("crash at {point:?}: the reconciliation refused with {e}"));
    let unexplained: Vec<_> = run
        .differences
        .iter()
        .filter(|d| {
            d.kind != mandate_executor::DifferenceKind::MissingFill
                && d.kind != mandate_executor::DifferenceKind::OrderState
        })
        .collect();
    assert!(
        unexplained.is_empty() && run.verdict != ReconciliationVerdict::Mismatch,
        "crash at {point:?}: recovery left {unexplained:?} unexplained ({:?})",
        run.verdict
    );
    assert!(
        !run.effects.iter().any(|e| matches!(
            e,
            mandate_executor::Effect::Journal(d) if d.event_type == "AgentModeApplied"
        )),
        "crash at {point:?}: a recovered account pauses nobody"
    );

    let mut open: BTreeSet<String> = BTreeSet::new();
    for stored in &shell.account_journal {
        if stored.event_type != "ProtectionChanged" {
            continue;
        }
        let (Some(name), Some(action)) = (
            common::protected_instrument(&stored.payload),
            stored.payload.get("action").and_then(Value::as_str),
        ) else {
            continue;
        };
        match action {
            "unprotected_start" => {
                open.insert(name.to_owned());
            }
            "unprotected_end" => {
                open.remove(name);
            }
            _ => {}
        }
    }
    assert!(
        open.is_empty(),
        "crash at {point:?}: {open:?} were left unprotected with no journaled end"
    );

    let before = shell.connector.total_accepted();
    let replayed = shell
        .replay()
        .unwrap_or_else(|e| panic!("crash at {point:?}: the replay refused with {e}"));
    assert!(
        !replayed.orders().is_empty(),
        "crash at {point:?}: the replay holds the order"
    );
    assert_eq!(
        replayed.orders(),
        shell.state.orders(),
        "crash at {point:?}: the fold does not reproduce the orders the run ended with"
    );
    assert_eq!(
        replayed.reservations(),
        shell.state.reservations(),
        "crash at {point:?}: the fold does not reproduce the reservations"
    );
    assert_eq!(
        shell.connector.total_accepted(),
        before,
        "crash at {point:?}: a replay reaches no broker"
    );
}

/// The shell every plain-submission case starts from: the stream open, the process started, and a
/// fresh quote.
fn opened(ports: &Ports<'_>) -> Shell {
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("the stream opens");
    let mut shell = shell.restart_ready(ports);
    shell.run(Input::Market(quote(AAPL, "150", "150.2", 20)), ports);
    shell
}

/// One plain submission of ten `AAPL` at 150, crashed at `point` (one of the first eight),
/// restarted, and recovered against the broker's truth.
fn crash_one_submission(point: CrashPoint) {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let id = intent_order_id();
    let intent = || handoff(INTENT, common::AGENT, opening(AAPL, "10", "150"));
    let accepted = || {
        Input::Broker(Ok(BrokerOutcome::Submitted(broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "10",
            "0",
            "accepted",
        ))))
    };
    let mut shell = opened(&ports);
    let mut truth = Truth::default();

    match point {
        CrashPoint::BeforeIntentReceived
        | CrashPoint::IntentReceivedBeforeGate
        | CrashPoint::GateBeforeOrderSubmitted
        | CrashPoint::JournalBeforeRequest => {
            shell
                .step_crashing(intent(), &ports, point)
                .unwrap_or_else(|e| panic!("the step refused with {}: {e}", e.code()));
        }
        CrashPoint::RequestNoResponse | CrashPoint::RequestAmbiguousResponse => {
            shell.run(intent(), &ports);
            assert_eq!(
                shell.connector.accepted_for(&id),
                1,
                "crash at {point:?}: the request left the process before the crash"
            );
            if let Some(answer) = point.broker_answer() {
                shell.run(Input::Broker(answer), &ports);
            }
        }
        CrashPoint::ResponseBeforeStateChange => {
            shell.run(intent(), &ports);
            shell
                .step_crashing(accepted(), &ports, point)
                .unwrap_or_else(|e| panic!("the step refused with {}: {e}", e.code()));
        }
        CrashPoint::StateChangeBeforeFill => {
            shell.run(intent(), &ports);
            shell.run(accepted(), &ports);
            truth.fills = vec![broker_fill("f-1", Some(&id), "10", "150")];
            truth.positions = vec![broker_position(AAPL, "10")];
            let filled = truth.snapshot(&shell, ReconcileReason::Scheduled);
            shell
                .step_crashing(Input::BrokerSnapshot(filled), &ports, point)
                .unwrap_or_else(|e| panic!("the step refused with {}: {e}", e.code()));
        }
        CrashPoint::CancelBeforeConfirmation
        | CrashPoint::ConfirmationBeforeExitSubmit
        | CrashPoint::BetweenEntryFillAndOco
        | CrashPoint::MidReconciliationBeforeCompensatingEvent => {
            unreachable!("{point:?} has its own case")
        }
    }
    if point.broker_answer().is_none() {
        assert_eq!(
            shell.crashed_at,
            Some(point),
            "the run reached {point:?}: a crash point the list never contains proves nothing"
        );
    }

    let (mut shell, started) = shell.restart_keeping_broker(&ports);
    assert!(
        started.submissions().is_empty(),
        "crash at {point:?}: Started never resubmits; it queries (journal §5.2)"
    );
    let journaled = shell
        .account_journal
        .iter()
        .any(|e| e.event_type == "OrderSubmitted");
    if journaled {
        assert!(
            started.requests.iter().any(|r| matches!(
                r,
                BrokerRequest::GetOrderByClientId(asked) if asked.as_str() == id
            )),
            "crash at {point:?}: an OrderSubmitted with no settled answer is resolved by a query \
             on its client order id: {:?}",
            started.requests
        );
        let filled = if truth.fills.is_empty() { "0" } else { "10" };
        let status = if truth.fills.is_empty() {
            "accepted"
        } else {
            "filled"
        };
        let answer = lookup(&shell, &id, status, filled, "10");
        shell.run(answer, &ports);
    }
    if shell.connector.accepted_for(&id) > 0 && truth.fills.is_empty() {
        truth.open = vec![broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "10",
            "0",
            "accepted",
        )];
    }
    let startup = truth.snapshot(&shell, ReconcileReason::Startup);
    shell.run(Input::BrokerSnapshot(startup), &ports);
    let rehanded = shell.run(intent(), &ports);
    if point == CrashPoint::BeforeIntentReceived {
        assert_eq!(
            rehanded.submissions().len(),
            1,
            "crash at {point:?}: nothing was journaled, so the runtime's re-hand is the intent's \
             first arrival and is submitted once"
        );
    } else {
        assert!(
            rehanded.is_empty(),
            "crash at {point:?}: a re-hand of an intent the fold carries costs nothing: {:?}",
            rehanded.draft_types()
        );
    }
    if shell.connector.accepted_for(&id) == 0 {
        confirm_absence(&mut shell, &ports, &id, 30);
        truth.open = vec![broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "10",
            "0",
            "accepted",
        )];
    }

    assert_exactly_one(&shell, &id, point);
    if !truth.fills.is_empty() {
        let applied = shell
            .account_journal
            .iter()
            .filter(|e| {
                (e.event_type == "FillApplied" || e.event_type == "LateFillApplied")
                    && e.payload.get("fill_id").and_then(Value::as_str) == Some("f-1")
            })
            .count();
        assert_eq!(
            applied, 1,
            "crash at {point:?}: the fill is applied exactly once — neither lost nor doubled"
        );
        assert_eq!(
            shell.state.positions().get(&instrument(AAPL)).copied(),
            Some(common::signed_qty("10")),
            "crash at {point:?}: and the ledger holds the ten shares the broker holds"
        );
    }
    assert_recovered(&shell, &ports, &truth, point);
}

/// `RC-14`'s protected position: ten `AAPL` under one resting GTC OCO at 170 and 140.
fn protected(ports: &Ports<'_>) -> Shell {
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("the stream opens");
    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            2,
            "OrderSubmitted",
            with_clock(
                &[
                    ("client_order_id", text("md-held-1")),
                    ("agent", text(common::AGENT)),
                    ("instrument", text(AAPL)),
                    ("side", text("buy")),
                    ("qty", text("10")),
                    ("limit", text("150")),
                ],
                10,
            ),
        ))
        .expect("the attributed buy folds");
    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            3,
            "FillApplied",
            with_clock(
                &[
                    ("fill_id", text("f-0")),
                    ("client_order_id", text("md-held-1")),
                    ("instrument", text(AAPL)),
                    ("side", text("buy")),
                    ("qty_gross", text("10")),
                    ("price", text("150")),
                ],
                10,
            ),
        ))
        .expect("the opening fill folds");
    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            4,
            "OrderStateChanged",
            with_clock(
                &[
                    ("client_order_id", text("md-held-1")),
                    ("state", text("filled")),
                ],
                10,
            ),
        ))
        .expect("the buy's fill completes it");
    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            5,
            "ProtectionChanged",
            with_clock(
                &[
                    ("instrument", text(AAPL)),
                    ("action", text("placed")),
                    ("orders", text("md-oco-1")),
                    ("qty", text("10")),
                    ("take_profit", text("170")),
                    ("stop", text("140")),
                    ("created_on", text("2026-09-22")),
                ],
                11,
            ),
        ))
        .expect("the resting OCO folds");
    let mut shell = shell.restart_ready(ports);
    shell.run(Input::Market(quote(AAPL, "155", "155.1", 20)), ports);
    shell
}

/// The resting OCO as the broker describes it.
fn resting_oco() -> BrokerOrder {
    broker_order(
        "b-oco",
        Some("md-oco-1"),
        AAPL,
        Side::Sell,
        "10",
        "0",
        "new",
    )
}

/// The default account's cash plus the exit sell's proceeds, 10 × 154: what the broker holds once
/// the exit has filled, from a base the startup snapshot recorded before it.
fn cash_after_the_exit_sell() -> Usd {
    usd("21540")
}

/// The proceeds of every sell fill the journal applied, at its own quantity and price: the cash
/// the exit moved, computed from the executor's record rather than the fixture's figure.
fn journaled_sell_proceeds(shell: &Shell) -> Option<Usd> {
    shell
        .account_journal
        .iter()
        .filter(|e| {
            e.event_type == "FillApplied"
                && e.payload.get("side").and_then(Value::as_str) == Some("sell")
        })
        .try_fold(Usd::ZERO, |total, e| {
            let field = |name: &str| e.payload.get(name).and_then(Value::as_str);
            let notional = qty(field("qty_gross")?)
                .notional(price(field("price")?))
                .ok()?;
            total.checked_add(notional).ok()
        })
}

/// A risk exit of the protected position, crashed inside its cancel → confirm → submit sequence
/// (points 9 and 10), restarted, and driven to the exit's fill.
fn crash_one_protective_sequence(point: CrashPoint) {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let exit = || handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "155"));
    let confirmed = || {
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: "md-oco-1".to_owned(),
        }))
    };
    let mut shell = protected(&ports);
    let mut truth = Truth {
        open: vec![resting_oco()],
        fills: Vec::new(),
        positions: vec![broker_position(AAPL, "10")],
        cash: None,
    };

    match point {
        CrashPoint::CancelBeforeConfirmation => {
            shell
                .step_crashing(exit(), &ports, point)
                .unwrap_or_else(|e| panic!("the step refused with {}: {e}", e.code()));
        }
        CrashPoint::ConfirmationBeforeExitSubmit => {
            shell.run(exit(), &ports);
            truth.open.clear();
            shell
                .step_crashing(confirmed(), &ports, point)
                .unwrap_or_else(|e| panic!("the step refused with {}: {e}", e.code()));
        }
        _ => unreachable!("{point:?} is not a protective-sequence point"),
    }
    assert_eq!(
        shell.crashed_at,
        Some(point),
        "the run reached {point:?}: a crash point the list never contains proves nothing"
    );

    let (mut shell, started) = shell.restart_keeping_broker(&ports);
    assert!(
        started.submissions().is_empty(),
        "crash at {point:?}: Started never submits an exit on state it has not reconciled"
    );
    shell.run(
        Input::BrokerSnapshot(truth.snapshot(&shell, ReconcileReason::Startup)),
        &ports,
    );
    let exit_id = intent_order_id();
    if point == CrashPoint::CancelBeforeConfirmation {
        let asked = shell
            .connector
            .requests
            .iter()
            .filter(|r| r.as_str() == "cancel md-oco-1")
            .count();
        assert!(
            asked > 0,
            "crash at {point:?}: the unsent cancel is sent after the restart, because the \
             sequence resumes where the journal says it stopped (interpretation 20): {:?}",
            shell.connector.requests
        );
        assert_eq!(
            shell.connector.accepted_for(&exit_id),
            0,
            "crash at {point:?}: and nothing is submitted into a still-resting OCO (planted bug 5)"
        );
        let released = shell.run(confirmed(), &ports);
        assert_eq!(
            released.submissions().len(),
            1,
            "crash at {point:?}: the exit goes once the cancel is confirmed"
        );
    } else {
        let answer = lookup(&shell, &exit_id, "new", "0", "10");
        shell.run(answer, &ports);
        confirm_absence(&mut shell, &ports, &exit_id, 30);
    }
    assert_exactly_one(&shell, &exit_id, point);

    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(BrokerFill {
            side: Side::Sell,
            ..broker_fill("f-exit", Some(&exit_id), "10", "154")
        })),
        &ports,
    );
    truth.open.clear();
    truth.positions.clear();
    truth.fills = vec![BrokerFill {
        side: Side::Sell,
        ..broker_fill("f-exit", Some(&exit_id), "10", "154")
    }];
    truth.cash = Some(cash_after_the_exit_sell());
    assert_eq!(
        Some(cash_after_the_exit_sell()),
        journaled_sell_proceeds(&shell)
            .and_then(|proceeds| broker_account().cash.checked_add(proceeds).ok()),
        "crash at {point:?}: the broker's cash is exactly the default account's plus the journaled \
         exit proceeds, not merely within §11's band"
    );
    let started = shell
        .account_journal
        .iter()
        .filter(|e| {
            e.event_type == "ProtectionChanged"
                && e.payload.get("action").and_then(Value::as_str) == Some("unprotected_start")
        })
        .count();
    assert!(
        started > 0,
        "crash at {point:?}: the unprotected interval the exit opened is on the journal"
    );
    assert_recovered(&shell, &ports, &truth, point);
}

#[test]
fn crash_at_before_intent_received() {
    crash_one_submission(CrashPoint::BeforeIntentReceived);
}

#[test]
fn crash_at_intent_received_before_gate() {
    crash_one_submission(CrashPoint::IntentReceivedBeforeGate);
}

#[test]
fn crash_at_gate_before_order_submitted() {
    crash_one_submission(CrashPoint::GateBeforeOrderSubmitted);
}

#[test]
fn crash_at_journal_before_request() {
    crash_one_submission(CrashPoint::JournalBeforeRequest);
}

#[test]
fn crash_at_request_no_response() {
    crash_one_submission(CrashPoint::RequestNoResponse);
}

#[test]
fn crash_at_request_ambiguous_response() {
    crash_one_submission(CrashPoint::RequestAmbiguousResponse);
}

#[test]
fn crash_at_response_before_state_change() {
    crash_one_submission(CrashPoint::ResponseBeforeStateChange);
}

#[test]
fn crash_at_state_change_before_fill() {
    crash_one_submission(CrashPoint::StateChangeBeforeFill);
}

#[test]
fn crash_at_cancel_before_confirmation() {
    crash_one_protective_sequence(CrashPoint::CancelBeforeConfirmation);
}

#[test]
fn crash_at_confirmation_before_exit_submit() {
    crash_one_protective_sequence(CrashPoint::ConfirmationBeforeExitSubmit);
}

#[test]
fn crash_between_entry_fill_and_oco() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let point = CrashPoint::BetweenEntryFillAndOco;
    let id = intent_order_id();

    let mut shell = opened(&ports);
    let sent = shell.run(
        handoff(
            INTENT,
            common::AGENT,
            protected_opening(AAPL, "100", "150", "140", Some("170")),
        ),
        &ports,
    );
    assert_eq!(
        sent.submissions()
            .first()
            .map(|o| o.client_order_id.as_str().to_owned()),
        Some(id.clone()),
        "the bracket is sent"
    );
    let partial = || {
        broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "100",
            "60",
            "partially_filled",
        )
    };
    shell
        .step_crashing(
            Input::BrokerUpdate(BrokerUpdate::Order(partial())),
            &ports,
            point,
        )
        .unwrap_or_else(|e| panic!("the step refused with {}: {e}", e.code()));
    assert_eq!(
        shell.crashed_at,
        Some(point),
        "the run reached {point:?}: the partial fill's list protects the filled quantity"
    );

    let (mut shell, _) = shell.restart_keeping_broker(&ports);
    let mut truth = Truth {
        open: vec![partial()],
        fills: vec![broker_fill("f-1", Some(&id), "60", "150")],
        positions: vec![broker_position(AAPL, "60")],
        cash: None,
    };
    shell.run(
        Input::BrokerSnapshot(truth.snapshot(&shell, ReconcileReason::Startup)),
        &ports,
    );
    let timed_out = shell.run(Input::Tick(clock(400)), &ports);
    assert!(
        timed_out.requests.iter().any(|r| matches!(
            r,
            BrokerRequest::Cancel { client_order_id } if client_order_id.as_str() == id
        )),
        "crash at {point:?}: past bracket_partial_fill_timeout the entry remainder is cancelled"
    );
    let placed = shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: id.clone(),
        })),
        &ports,
    );
    let oco = placed
        .submissions()
        .into_iter()
        .find_map(|o| o.oco.clone())
        .expect("the filled 60 shares get their OCO after the restart (planted bug 4)");
    assert_eq!(
        (oco.qty, oco.take_profit, oco.stop),
        (qty("60"), price("170"), price("140")),
        "for exactly the filled quantity, at the bracket's prices"
    );
    assert!(
        shell
            .state
            .protection(&instrument(AAPL))
            .expect("the protection accessor answers")
            .is_some_and(|p| !p.resting.is_empty()),
        "and the fold carries it as resting protection"
    );
    let oco_id = placed
        .submissions()
        .into_iter()
        .find(|o| o.oco.is_some())
        .map(|o| o.client_order_id.as_str().to_owned())
        .unwrap_or_default();
    truth.open = vec![broker_order(
        "b-oco-2",
        Some(&oco_id),
        AAPL,
        Side::Sell,
        "60",
        "0",
        "new",
    )];
    let intervals = shell
        .account_journal
        .iter()
        .filter(|e| {
            e.event_type == "ProtectionChanged"
                && e.payload.get("action").and_then(Value::as_str) == Some("unprotected_start")
        })
        .count();
    assert!(
        intervals > 0,
        "crash at {point:?}: the interval the partial fill opened is journaled even though the \
         crash cut the list that would have journaled it"
    );
    assert_recovered(&shell, &ports, &truth, point);
}

#[test]
fn crash_mid_reconciliation_before_the_compensating_event() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let point = CrashPoint::MidReconciliationBeforeCompensatingEvent;
    let id = intent_order_id();

    let mut shell = opened(&ports);
    shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    shell.run(
        Input::Broker(Ok(BrokerOutcome::Submitted(broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "10",
            "0",
            "accepted",
        )))),
        &ports,
    );
    let truth = Truth {
        open: vec![broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "10",
            "4",
            "partially_filled",
        )],
        fills: vec![broker_fill("f-1", Some(&id), "4", "150")],
        positions: vec![broker_position(AAPL, "4")],
        cash: None,
    };
    shell
        .step_crashing(
            Input::BrokerSnapshot(truth.snapshot(&shell, ReconcileReason::Scheduled)),
            &ports,
            point,
        )
        .unwrap_or_else(|e| panic!("the step refused with {}: {e}", e.code()));
    assert_eq!(
        shell.crashed_at,
        Some(point),
        "the run reached {point:?}: the broker's partially filled order differs from our accepted \
         one, so the reconciliation adopts it with a compensating event"
    );

    let (mut shell, _) = shell.restart_keeping_broker(&ports);
    shell.run(
        Input::BrokerSnapshot(truth.snapshot(&shell, ReconcileReason::Startup)),
        &ports,
    );
    let adoptions = shell
        .account_journal
        .iter()
        .filter(|e| {
            e.event_type == "OrderStateChanged"
                && e.payload.get("client_order_id").and_then(Value::as_str) == Some(id.as_str())
                && e.payload.get("state").and_then(Value::as_str) == Some("partially_filled")
        })
        .count();
    let compensations = shell
        .account_journal
        .iter()
        .filter(|e| e.event_type == "CompensatingEvent")
        .count();
    assert_eq!(
        adoptions, 1,
        "crash at {point:?}: the broker's state is adopted once, never twice"
    );
    assert_eq!(
        compensations, adoptions,
        "crash at {point:?}: and every adoption on the journal carries its compensating event \
         after recovery, so the difference is never adopted silently (§11)"
    );
    assert_exactly_one(&shell, &id, point);
    assert_recovered(&shell, &ports, &truth, point);
}
