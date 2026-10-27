//! One working opening per instrument (trading-domain spec §5.3 rule 6, `working_order_limit`;
//! R0's second ask, DEC-1150 items 3 to 5).
//!
//! While an opening the runtime proposed is still working on the account stream, a new opening or
//! increase in that instrument is not evaluated at all: no decision, no alert, nothing asked or
//! proposed (DEC-2843, which amends DEC-1150 item 5's journaled `working_order_limit` deny). The opening is followed from the journal alone:
//! `OrderRequestRecorded` names its intent, `OrderSubmitted` (its `causation_id` that request)
//! names its order, and the opening ends on a `GateDecided` deny, an `OrderAbandoned`, or an
//! `OrderStateChanged` of its order to `filled`, `canceled`, `rejected` or `expired`; a `replaced`
//! order follows its successor. A record that does not say so in full ends nothing, so the hold
//! never lifts on a guess. An exit is never held (`AGENTS.md` rule 13). Every "held" case is paired
//! with the same step proceeding.

mod common;

use common::{
    ACCOUNT_STREAM, AllowGate, FixedPlan, Ran, Shell, TestIds, clock, copied, event, fresh_output,
    ports, text, universe, with_clock,
};
use mandate_canon::Value;
use mandate_runtime::{Autonomy, EventId, FoldedEvent, Input};

fn reconciliation(seq: u64, at: i64) -> FoldedEvent {
    event(
        ACCOUNT_STREAM,
        seq,
        "ReconciliationRun",
        with_clock(&[("result", text("clean"))], at),
    )
}

fn evaluated(shell: &mut Shell, ports: &mandate_runtime::Ports<'_>, at: i64) -> Ran {
    shell.run(Input::ModelOutput(fresh_output(at)), ports);
    shell.run(Input::Tick(clock(at)), ports)
}

fn proposed(ran: &Ran) -> Option<EventId> {
    ran.drafts
        .iter()
        .find(|d| d.event_type == "IntentProposed")
        .map(|d| d.event_id.clone())
}

/// Held: the evaluation journals nothing for the opening, no decision, no alert, no intent, so a
/// working opening's wait never floods the journal or the owner (DEC-2843, amending DEC-1150 item
/// 5's journaled deny).
fn held(ran: &Ran) -> bool {
    proposed(ran).is_none()
        && ran.handed.is_empty()
        && ran.drafts.iter().all(|d| d.event_type != "DecisionMade")
        && ran.notifications.is_empty()
}

/// The account stream as the executor writes it for one intent: what the order is named, and
/// the records that carry it there.
struct Account<'s> {
    shell: &'s mut Shell,
    at: i64,
}

impl Account<'_> {
    fn next(&self) -> u64 {
        self.shell
            .state
            .head(ACCOUNT_STREAM)
            .map_or(1, |seq| seq.0.saturating_add(1))
    }

    fn fold(&mut self, one: &FoldedEvent) {
        self.shell
            .fold_one(one)
            .unwrap_or_else(|e| panic!("{} folds: {e}", one.event_type));
    }

    /// `OrderRequestRecorded` for `intent`, then `OrderSubmitted` of `order` caused by it.
    fn submitted(&mut self, intent: &EventId, order: &str) {
        let request = event(
            ACCOUNT_STREAM,
            self.next(),
            "OrderRequestRecorded",
            with_clock(&[("intent_id", text(&intent.0))], self.at),
        );
        self.fold(&request);
        let submitted = copied(
            ACCOUNT_STREAM,
            self.next(),
            "OrderSubmitted",
            with_clock(
                &[
                    ("attempt", common::int(1)),
                    ("client_order_id", text(order)),
                ],
                self.at,
            ),
            &request.event_id,
        );
        self.fold(&submitted);
    }

    fn state(&mut self, order: &str, state: &str, replaced_by: Option<&str>) {
        let changed = event(
            ACCOUNT_STREAM,
            self.next(),
            "OrderStateChanged",
            with_clock(
                &[
                    ("client_order_id", text(order)),
                    ("state", text(state)),
                    ("replaced_by", replaced_by.map_or(Value::Null, text)),
                ],
                self.at,
            ),
        );
        self.fold(&changed);
    }

    fn intent_record(&mut self, event_type: &str, intent: &EventId, extra: (&str, Value)) {
        let record = event(
            ACCOUNT_STREAM,
            self.next(),
            event_type,
            with_clock(&[("intent_id", text(&intent.0)), extra], self.at),
        );
        self.fold(&record);
    }
}

/// A restart of `shell` whose startup reconciliation then comes back clean, so the hold it takes
/// lifts (trading spec §11) and the next evaluation decides.
fn restarted(shell: &Shell, ports: &mandate_runtime::Ports<'_>) -> Shell {
    let (mut next, _) = shell.restart(ports);
    let seq = next
        .state
        .head(ACCOUNT_STREAM)
        .map_or(1, |seq| seq.0.saturating_add(1));
    let clean = reconciliation(seq, 116);
    common::escalation::tail(&mut next, &clean, ports);
    next
}

/// A started shell that proposed one AUTO opening of AAPL at 100, and that opening's intent.
fn opened(ports: &mandate_runtime::Ports<'_>) -> (Shell, EventId) {
    let mut shell = Shell::new(1);
    shell
        .fold_one(&reconciliation(1, 0))
        .unwrap_or_else(|e| panic!("{e}"));
    let (mut shell, _) = shell.restart(ports);
    let first = evaluated(&mut shell, ports, 100);
    let intent =
        proposed(&first).unwrap_or_else(|| panic!("the fixture opens: {:?}", first.draft_types()));
    (shell, intent)
}

#[test]
fn a_second_opening_is_held_while_the_first_works() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, intent) = opened(&ports);
    let ran = evaluated(&mut shell, &ports, 110);
    assert!(
        held(&ran),
        "proposed but not yet sent: §5.3 rule 6 holds the next one: {:?}",
        ran.draft_types()
    );
    let mut account = Account {
        shell: &mut shell,
        at: 115,
    };
    account.submitted(&intent, "md-first");
    account.state("md-first", "accepted", None);
    account.state("md-first", "partially_filled", None);
    let ran = evaluated(&mut shell, &ports, 120);
    assert!(
        held(&ran),
        "accepted and partly filled, it still works: {:?}",
        ran.draft_types()
    );
}

#[test]
fn a_terminal_state_of_the_openings_order_releases_the_instrument() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    for ended in ["filled", "canceled", "rejected", "expired"] {
        let (mut shell, intent) = opened(&ports);
        let mut account = Account {
            shell: &mut shell,
            at: 115,
        };
        account.submitted(&intent, "md-first");
        account.state("md-first", ended, None);
        let ran = evaluated(&mut shell, &ports, 120);
        assert!(
            proposed(&ran).is_some(),
            "{ended} ends the opening, so the next is judged afresh: {:?}",
            ran.draft_types()
        );
    }
}

#[test]
fn a_denied_or_abandoned_intent_releases_the_instrument() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let cases: [(&str, (&str, Value)); 3] = [
        ("GateDecided", ("verdict", text("deny"))),
        ("OrderAbandoned", ("reason", text("gate_recheck"))),
        ("GateDecided", ("verdict", text("allow"))),
    ];
    for (event_type, extra) in cases {
        let releases = event_type == "OrderAbandoned" || extra.1 == text("deny");
        let (mut shell, intent) = opened(&ports);
        let mut account = Account {
            shell: &mut shell,
            at: 115,
        };
        account.intent_record(event_type, &intent, extra.clone());
        let ran = evaluated(&mut shell, &ports, 120);
        assert_eq!(
            proposed(&ran).is_some(),
            releases,
            "{event_type} {:?}: {:?}",
            extra.1,
            ran.draft_types()
        );
    }
}

#[test]
fn a_replaced_order_works_on_as_its_successor() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, intent) = opened(&ports);
    let mut account = Account {
        shell: &mut shell,
        at: 115,
    };
    account.submitted(&intent, "md-first");
    account.state("md-first", "replaced", Some("md-r-first"));
    let ran = evaluated(&mut shell, &ports, 120);
    assert!(
        held(&ran),
        "a replacement is the same opening still working: {:?}",
        ran.draft_types()
    );
    let mut account = Account {
        shell: &mut shell,
        at: 125,
    };
    account.state("md-r-first", "filled", None);
    let ran = evaluated(&mut shell, &ports, 130);
    assert!(
        proposed(&ran).is_some(),
        "its successor's fill ends it: {:?}",
        ran.draft_types()
    );
}

#[test]
fn another_orders_end_or_a_partial_record_releases_nothing() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, intent) = opened(&ports);
    let mut account = Account {
        shell: &mut shell,
        at: 115,
    };
    account.submitted(&intent, "md-first");
    account.state("md-someone-else", "filled", None);
    let unlinked = event(
        ACCOUNT_STREAM,
        account.next(),
        "OrderSubmitted",
        with_clock(&[("client_order_id", text("md-unlinked"))], 115),
    );
    account.fold(&unlinked);
    account.state("md-unlinked", "filled", None);
    let bare = event(
        ACCOUNT_STREAM,
        account.next(),
        "OrderStateChanged",
        with_clock(&[("state", text("filled"))], 115),
    );
    account.fold(&bare);
    let ran = evaluated(&mut shell, &ports, 120);
    assert!(
        held(&ran),
        "only a record naming this opening's own order ends it: {:?}",
        ran.draft_types()
    );
}

#[test]
fn an_exit_is_never_held_by_a_working_opening() {
    let (ids, gate, view) = (TestIds, AllowGate, universe(&["AAPL"]));
    let opening = FixedPlan::opening(Autonomy::Auto);
    let opening_ports = ports(&ids, &gate, &opening, &view);
    let (shell, _) = opened(&opening_ports);
    let exiting = FixedPlan::exiting(Autonomy::Auto);
    let exit_ports = ports(&ids, &gate, &exiting, &view);
    let mut shell = restarted(&shell, &exit_ports);
    let ran = evaluated(&mut shell, &exit_ports, 120);
    assert!(
        proposed(&ran).is_some() && ran.handed.len() == 1,
        "risk reduction is never denied by a working opening (rule 13): {:?}",
        ran.draft_types()
    );
}

#[test]
fn a_restart_holds_the_same_opening() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, intent) = opened(&ports);
    let mut account = Account {
        shell: &mut shell,
        at: 115,
    };
    account.submitted(&intent, "md-first");
    account.state("md-first", "accepted", None);
    let mut again = restarted(&shell, &ports);
    assert!(
        held(&evaluated(&mut again, &ports, 120)),
        "the hold is derived from the journal alone (ES-21)"
    );
    let mut account = Account {
        shell: &mut again,
        at: 125,
    };
    account.state("md-first", "filled", None);
    let mut last = restarted(&again, &ports);
    assert!(
        proposed(&evaluated(&mut last, &ports, 130)).is_some(),
        "and so is its end"
    );
}

#[test]
fn a_grant_read_while_an_opening_works_is_skipped() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, asked) = common::escalation::asking_shell(&ports, Some("155"));
    let grant = common::escalation::Answer::grant(
        common::escalation::next_control_seq(&shell),
        &asked,
        common::escalation::ASKED_AT + 10,
    )
    .event();
    let ran = common::escalation::tail(&mut shell, &grant, &ports);
    let intent = proposed(&ran).unwrap_or_else(|| panic!("granted: {:?}", ran.draft_types()));
    let next = evaluated(&mut shell, &ports, common::escalation::ASKED_AT + 20);
    assert!(
        !next.draft_types().contains(&"ApprovalRequested"),
        "no second ask while the granted opening works: {:?}",
        next.draft_types()
    );
    let mut account = Account {
        shell: &mut shell,
        at: common::escalation::ASKED_AT + 25,
    };
    account.submitted(&intent, "md-granted");
    account.state("md-granted", "filled", None);
    let after = evaluated(&mut shell, &ports, common::escalation::ASKED_AT + 30);
    assert!(
        after.draft_types().contains(&"ApprovalRequested"),
        "once it ends, the next opening is asked afresh: {:?}",
        after.draft_types()
    );
}

/// A version-1 `OrderSubmitted` names its intent as its cause directly, and is followed the same.
#[test]
fn an_order_caused_by_its_intent_is_followed_too() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, intent) = opened(&ports);
    let mut account = Account {
        shell: &mut shell,
        at: 115,
    };
    let submitted = copied(
        ACCOUNT_STREAM,
        account.next(),
        "OrderSubmitted",
        with_clock(&[("client_order_id", text("md-direct"))], 115),
        &intent,
    );
    account.fold(&submitted);
    let ran = evaluated(&mut shell, &ports, 118);
    assert!(held(&ran), "submitted, it works: {:?}", ran.draft_types());
    let mut account = Account {
        shell: &mut shell,
        at: 119,
    };
    account.state("md-direct", "filled", None);
    let ran = evaluated(&mut shell, &ports, 120);
    assert!(
        proposed(&ran).is_some(),
        "its fill ends it: {:?}",
        ran.draft_types()
    );
}
