//! `Command::CancelOpenings`: one agent's working openings in one instrument, cancelled through
//! the executor's own cancel path ([DEC-853](../../../docs/project/decisions/DEC-853.md); first
//! paper trade brief, slice E1b-0, FT-11; story E7-19 slice 5).
//!
//! Every expectation is computed here from the book the test folds, by the brief's rule written
//! out in [`cancelled_by_the_rule`]: an `open` or `increase` order of that agent in that
//! instrument that the broker holds as `accepted` or `partially_filled`. That is the set
//! `cancel_openings` already cancels for an exit and a stricter mode (§5.3 rule 5, mandate spec
//! §5.9); an `Unknown` order is queried, never cancelled blind (§5.7), so it is not in it.

mod common;

use std::cell::Cell;
use std::collections::BTreeSet;

use common::{
    ACCOUNT_STREAM, AGENT, AppendOutcome, FixedInstruments, FixedMandate, MISSING_BINDING_GATE,
    OTHER_AGENT, Ran, Shell, TestIds, agent, config, copied, derived_id, event, instrument, ports,
    stream_opened, text, with_clock,
};
use mandate_canon::Value;
use mandate_executor::{BrokerRequest, Command, Effect, EventId, ExecutorConfig, Input, Ports};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};

const AAPL: &str = FixedInstruments::LIQUID_EQUITY;
const CPHC: &str = FixedInstruments::THIN_EQUITY;

/// One order of a book: its id, then agent, instrument, purpose, and the state it is folded in.
type Order = (String, [&'static str; 4]);

/// The book every hand case folds: two openings the command must cancel, and one order of every
/// kind it must leave alone.
fn book() -> Vec<Order> {
    [
        ("md-open-a", AGENT, AAPL, "open", "accepted"),
        ("md-add-a", AGENT, AAPL, "increase", "partially_filled"),
        ("md-exit-a", AGENT, AAPL, "risk_exit", "accepted"),
        ("md-dexit-a", AGENT, AAPL, "discretionary_exit", "accepted"),
        ("md-stop-a", AGENT, AAPL, "protective", "accepted"),
        ("md-open-b", OTHER_AGENT, AAPL, "open", "accepted"),
        ("md-open-c", AGENT, CPHC, "open", "accepted"),
        ("md-done-a", AGENT, AAPL, "open", "canceled"),
    ]
    .map(|(id, who, name, purpose, state)| (id.to_owned(), [who, name, purpose, state]))
    .to_vec()
}

/// The brief's rule, written out here rather than read from the crate.
fn cancelled_by_the_rule(book: &[Order], who: &str, name: &str) -> BTreeSet<String> {
    book.iter()
        .filter(|(_, [by, at, purpose, state])| {
            *by == who
                && *at == name
                && ["open", "increase"].contains(purpose)
                && ["accepted", "partially_filled"].contains(state)
        })
        .map(|(id, ..)| id.clone())
        .collect()
}

/// A started, reconciled shell with `book` folded on the account stream as journaled facts.
fn shell_with(book: &[Order], ports: &Ports<'_>) -> Shell {
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(ports);
    fold_book(&mut shell, book);
    shell
}

fn fixtures() -> (TestIds, FixedMandate, FixedInstruments, ExecutorConfig) {
    let mandates = FixedMandate::covering(&[AAPL, CPHC]);
    (TestIds, mandates, FixedInstruments, config())
}

/// Folds `book` onto `shell`'s account stream as journaled facts.
fn fold_book(shell: &mut Shell, book: &[Order]) {
    for (id, [who, name, purpose, state]) in book {
        let side = match *purpose {
            "open" | "increase" => "buy",
            _ => "sell",
        };
        let submitted = with_clock(
            &[
                ("client_order_id", text(id)),
                ("agent", text(who)),
                ("instrument", text(name)),
                ("side", text(side)),
                ("qty", text("5")),
                ("limit", text("150")),
                ("purpose", text(purpose)),
            ],
            10,
        );
        let changed = with_clock(&[("client_order_id", text(id)), ("state", text(state))], 10);
        for (kind, payload) in [
            ("OrderSubmitted", submitted),
            ("OrderStateChanged", changed),
        ] {
            let seq = shell.head().0.saturating_add(1);
            shell
                .fold_one(&event(ACCOUNT_STREAM, seq, kind, payload))
                .unwrap_or_else(|e| panic!("{id} folds as {kind}: {e}"));
        }
    }
}

/// The two openings [`book`] holds for [`AGENT`] in [`AAPL`], named by hand.
fn the_two() -> BTreeSet<String> {
    ["md-add-a", "md-open-a"].map(str::to_owned).into()
}

fn command(who: &str, name: &str) -> Input {
    Input::Command(Command::CancelOpenings {
        agent: agent(who),
        instrument: instrument(name),
    })
}

fn cancels(ran: &Ran) -> Vec<String> {
    ran.requests
        .iter()
        .filter_map(|request| match request {
            BrokerRequest::Cancel { client_order_id } => Some(client_order_id.as_str().to_owned()),
            _ => None,
        })
        .collect()
}

/// Every broker request in `ran` is a cancel named in `expected`, each sent once, and each follows
/// a committed `OrderStateChanged` for that order to `pending_cancel` with `cancel_requested`;
/// nothing else is drafted. Answers the problem, or `None`.
fn exactly(ran: &Ran, expected: &BTreeSet<String>) -> Option<String> {
    let sent = cancels(ran);
    if sent.len() != ran.requests.len() {
        return Some(format!("only cancels leave: {:?}", ran.requests));
    }
    if sent.iter().cloned().collect::<BTreeSet<_>>() != *expected || sent.len() != expected.len() {
        return Some(format!("cancelled {sent:?}, the rule says {expected:?}"));
    }
    if ran.drafts.len() != expected.len() {
        return Some(format!("one draft per cancel: {:?}", ran.draft_types()));
    }
    for id in &sent {
        let at = ran.effects.iter().position(|effect| {
            matches!(effect, Effect::Broker(BrokerRequest::Cancel { client_order_id })
                if client_order_id.as_str() == id)
        });
        let journaled = ran.effects.iter().position(|effect| {
            matches!(effect, Effect::Journal(draft)
                if draft.event_type == "OrderStateChanged"
                    && draft.payload.get("client_order_id") == Some(&text(id))
                    && draft.payload.get("state") == Some(&text("pending_cancel"))
                    && draft.payload.get("cancel_requested") == Some(&Value::Bool(true)))
        });
        if !matches!((journaled, at), (Some(j), Some(c)) if j < c) {
            return Some(format!("{id} leaves before it is journaled (rule 5)"));
        }
    }
    None
}

/// DEC-853 item 2: the agent's working openings in the instrument are cancelled, and only those.
#[test]
fn only_that_agents_working_openings_in_that_instrument_are_cancelled() {
    let (ids, mandates, instruments, config) = fixtures();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = shell_with(&book(), &ports);
    let ran = shell.run(command(AGENT, AAPL), &ports);
    assert_eq!(cancelled_by_the_rule(&book(), AGENT, AAPL), the_two());
    assert_eq!(exactly(&ran, &the_two()), None);
}

/// DEC-853 item 2: exits, protective orders, another agent's opening in the instrument and the
/// agent's own opening in another instrument are neither cancelled nor journaled, and the
/// account-wide endpoints are never named (rule 13, the agent scope).
#[test]
fn exits_protection_other_agents_and_other_instruments_are_untouched() {
    let (ids, mandates, instruments, config) = fixtures();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = shell_with(&book(), &ports);
    let ran = shell.run(command(AGENT, AAPL), &ports);
    assert!(!cancels(&ran).is_empty(), "the openings are cancelled");
    for kept in "md-exit-a md-dexit-a md-stop-a md-open-b md-open-c".split(' ') {
        let named = |payload: &Value| payload.get("client_order_id") == Some(&text(kept));
        assert!(!cancels(&ran).iter().any(|id| id == kept), "{kept}");
        assert!(!ran.drafts.iter().any(|d| named(&d.payload)), "{kept}");
    }
    assert!(!ran.requests.iter().any(common::is_account_wide));
}

/// `AGENTS.md` rule 5: each cancel's `OrderStateChanged` is committed before the request leaves,
/// and its event id is the (epoch, head, ordinal) derivation, so a retried append re-derives it
/// and the journal answers `AlreadyCommitted` rather than appending twice (journal spec §5.1).
#[test]
fn each_cancel_is_journaled_before_it_leaves_under_a_derived_id() {
    let (ids, mandates, instruments, config) = fixtures();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = shell_with(&book(), &ports);
    let head = shell.head();
    shell.next_append = AppendOutcome::Unresolved;
    let first = shell.run(command(AGENT, AAPL), &ports);
    assert!(first.requests.is_empty(), "nothing leaves uncommitted");
    shell.next_append = AppendOutcome::Committed;
    let retried = shell.run(command(AGENT, AAPL), &ports);
    let ids_of =
        |ran: &Ran| -> Vec<EventId> { ran.drafts.iter().map(|d| d.event_id.clone()).collect() };
    let derived = |ordinal| derived_id(shell.epoch, head, ordinal);
    assert_eq!(
        ids_of(&first),
        [derived(0)],
        "the shell stops at the unresolved append"
    );
    assert_eq!(
        ids_of(&retried),
        [derived(0), derived(1)],
        "the same ids at the same head"
    );
    assert_eq!(exactly(&retried, &the_two()), None);
}

/// The idempotency key is the order's own client order id: a cancel already asked and not yet
/// confirmed is not asked again, by a second command or by one after a restart.
#[test]
fn a_repeated_command_asks_no_second_cancel() {
    let (ids, mandates, instruments, config) = fixtures();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = shell_with(&book(), &ports);
    let first = shell.run(command(AGENT, AAPL), &ports);
    assert_eq!(
        exactly(&first, &the_two()),
        None,
        "named by their own client order ids"
    );
    assert!(
        shell.run(command(AGENT, AAPL), &ports).is_empty(),
        "a second asks nothing"
    );
    let (mut restarted, _) = shell.restart(&ports);
    let again = restarted.run(command(AGENT, AAPL), &ports);
    assert!(
        cancels(&again).is_empty(),
        "nor after a restart: {:?}",
        again.requests
    );
}

/// DEC-853 item 3: with nothing working, the command answers `Ok` and changes nothing; once an
/// opening is working, the same command cancels it.
#[test]
fn nothing_working_is_a_no_op_not_an_error() {
    let (ids, mandates, instruments, config) = fixtures();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let others: Vec<Order> = book()
        .into_iter()
        .filter(|(id, ..)| !["md-open-a", "md-add-a"].contains(&id.as_str()))
        .collect();
    assert!(cancelled_by_the_rule(&others, AGENT, AAPL).is_empty());
    let mut shell = shell_with(&others, &ports);
    let head = shell.head();
    let ran = shell
        .step(command(AGENT, AAPL), &ports)
        .expect("a no-op, not an error");
    assert!(ran.is_empty(), "{:?}", ran.effects);
    assert_eq!(shell.head(), head, "and journals nothing");
    let opening: Order = ("md-open-a".to_owned(), [AGENT, AAPL, "open", "accepted"]);
    fold_book(&mut shell, &[opening]);
    let later = shell.run(command(AGENT, AAPL), &ports);
    let expected: BTreeSet<String> = ["md-open-a".to_owned()].into();
    assert_eq!(
        exactly(&later, &expected),
        None,
        "an opening folded since is cancelled"
    );
}

/// DEC-853 item 4, §5.7: an `Unknown` opening is left to its query and never cancelled blind;
/// the accepted opening beside it is still cancelled.
#[test]
fn an_unknown_opening_is_left_to_its_query() {
    let (ids, mandates, instruments, config) = fixtures();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let book: Vec<Order> = [("md-open-a", "accepted"), ("md-lost-a", "unknown")]
        .map(|(id, state)| (id.to_owned(), [AGENT, AAPL, "open", state]))
        .to_vec();
    let mut shell = shell_with(&book, &ports);
    let ran = shell.run(command(AGENT, AAPL), &ports);
    let expected: BTreeSet<String> = ["md-open-a".to_owned()].into();
    assert_eq!(exactly(&ran, &expected), None);
}

/// #1007 review, minor 2: only an opening the broker holds as `accepted` or `partially_filled` is
/// cancelled. One still `submitting`, one whose cancel is already `pending_cancel`, and one
/// `unknown` are left alone: none is cancelled, journaled, or queried by the command.
#[test]
fn only_resting_openings_are_cancelled() {
    let (ids, mandates, instruments, config) = fixtures();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let states = [
        "submitting",
        "pending_cancel",
        "unknown",
        "accepted",
        "partially_filled",
    ];
    let book: Vec<Order> = states
        .map(|state| {
            (
                format!("md-{}", state.replace('_', "-")),
                [AGENT, AAPL, "open", state],
            )
        })
        .to_vec();
    let mut shell = shell_with(&book, &ports);
    let ran = shell.run(command(AGENT, AAPL), &ports);
    let expected: BTreeSet<String> = ["md-accepted", "md-partially-filled"]
        .map(str::to_owned)
        .into();
    assert_eq!(exactly(&ran, &expected), None);
}

/// Mandate spec §5.9: "On entering `exits_only` or stricter, the executor cancels the agent's
/// working opening orders", so no mode blocks this command, and nor does a missing binding gate
/// (rule 13: a risk reduction needs no external snapshot).
#[test]
fn no_mode_and_no_missing_gate_blocks_it() {
    let (ids, mandates, instruments, config) = fixtures();
    let ports = ports(&ids, &mandates, &instruments, &config);
    for mode in ["exits_only", "paused", "stopped"] {
        let mut shell = shell_with(&book(), &ports);
        let seq = shell.head().0.saturating_add(1);
        let origin = EventId(format!("agent:ws1:{AGENT}-{mode}"));
        let applied = with_clock(&[("agent", text(AGENT)), ("to", text(mode))], 11);
        let copy = copied(ACCOUNT_STREAM, seq, "AgentModeApplied", applied, &origin);
        shell.fold_one(&copy).expect("the mode folds");
        let ran = shell
            .step_with_binding(command(AGENT, AAPL), &ports, &MISSING_BINDING_GATE)
            .unwrap_or_else(|e| panic!("{mode}: step refused with {}: {e}", e.code()));
        assert_eq!(exactly(&ran, &the_two()), None, "{mode}");
    }
}

/// Every purpose and every state an order can be folded in.
const PURPOSES: &str = "open increase risk_exit owner_exit discretionary_exit protective flatten";
const STATES: &str =
    "submitting accepted partially_filled pending_cancel unknown filled canceled rejected expired";

fn arbitrary_book() -> impl Strategy<Value = (Vec<Order>, bool, bool)> {
    let one = (
        prop::sample::select(vec![AGENT, OTHER_AGENT]),
        prop::sample::select(vec![AAPL, CPHC]),
        prop::sample::select(PURPOSES.split(' ').collect::<Vec<_>>()),
        prop::sample::select(STATES.split(' ').collect::<Vec<_>>()),
    );
    let named =
        |n: usize, (who, name, purpose, state)| (format!("md-r{n}"), [who, name, purpose, state]);
    let book = prop::collection::vec(one, 0..10).prop_map(move |orders| {
        orders
            .into_iter()
            .enumerate()
            .map(|(n, o)| named(n, o))
            .collect()
    });
    (book, any::<bool>(), any::<bool>())
}

/// Over random books and a random target, the cancels are exactly the rule's set, each journaled
/// before it leaves, and nothing else is sent or drafted; an empty set is an empty step.
#[test]
fn over_random_books_exactly_the_rules_openings_are_cancelled() {
    let (ids, mandates, instruments, config) = fixtures();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut runner = TestRunner::new(Config {
        cases: 128,
        failure_persistence: None,
        ..Config::default()
    });
    let reached = Cell::new(0_u32);
    let outcome = runner.run(&arbitrary_book(), |(book, first, aapl)| {
        let who = if first { AGENT } else { OTHER_AGENT };
        let name = if aapl { AAPL } else { CPHC };
        let expected = cancelled_by_the_rule(&book, who, name);
        let mut shell = shell_with(&book, &ports);
        let ran = shell
            .step(command(who, name), &ports)
            .map_err(|e| TestCaseError::fail(format!("step refused with {}: {e}", e.code())))?;
        if !expected.is_empty() {
            reached.set(reached.get().saturating_add(1));
        }
        prop_assert_eq!(exactly(&ran, &expected), None);
        Ok(())
    });
    if let Err(failure) = outcome {
        panic!("{failure}");
    }
    assert!(reached.get() > 0, "some book has an opening to cancel");
}
