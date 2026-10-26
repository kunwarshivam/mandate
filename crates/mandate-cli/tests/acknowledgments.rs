//! `mandate agent acknowledge` and `mandate agent request-ack`, the CLI's writers of journal spec
//! §9.28 (DEC-2509, DEC-2512; E8-3's CLI follow-up, M7 S2).
//!
//! An acknowledgment commits one `OwnerAcknowledged` with §9.28's nine members and a request one
//! `OwnerAcknowledgmentRequested` with its seven, each needing the code, each with `risk_clock` a
//! timestamp, and each accepted by `mandate_journal::Draft::parse` as a real journal registers it.
//! The CLI checks rules 239 to 241 against the control stream before appending and commits nothing
//! when one fails, refusing with the executor's own reason: an acknowledgment naming a request that
//! is not an earlier request of the same agent and event, or that an earlier acknowledgment already
//! named, is `request_invalid`; one of the owner's own request is `not_independent`. The users are
//! the authenticated owners, never a value the caller types. The tests of what is not implemented
//! yet are pending E8-3 and fail on the command's `ControlError::Unimplemented`, but for the two
//! plain acknowledgment tests, which fail on the members DEC-279 item 7 wrote (DEC-77).

mod common;

use std::collections::BTreeSet;
use std::path::Path;

use common::{
    AGENT, ASKED_AT, CONTROL, FixedIds, Fixture, OTHER_AGENT, OWNER, WORKSPACE, at, body, member,
    member_text, object, owner, text,
};
use mandate_canon::{Value, parse};
use mandate_cli::agent::{Command, code, command};
use mandate_cli::control::{ControlError, Owner, Submitted};
use mandate_journal::{Draft, Environment, StoredEvent};
use mandate_time::UtcNanos;

/// The user who asks for a lift where an independent user must acknowledge it.
const CHECKER: &str = "user-checker";

/// A third user, who is neither the requester nor the first acknowledger.
const THIRD: &str = "user-third";

/// `OwnerAcknowledgmentRequested`'s members (§9.28).
const REQUESTED: [&str; 7] = [
    "agent",
    "event",
    "independent_required",
    "requested_by",
    "risk_clock",
    "step_up",
    "submitted_at",
];

/// `OwnerAcknowledged`'s members (§9.28, DEC-2509 item 1).
const ACKNOWLEDGED: [&str; 9] = [
    "agent",
    "event",
    "independent_required",
    "request",
    "requested_by",
    "risk_clock",
    "step_up",
    "submitted_at",
    "user",
];

fn answer<T>(what: &str, result: Result<T, ControlError>) -> T {
    result.unwrap_or_else(|e| panic!("{what} answers, not: {e}"))
}

fn user(id: &str) -> Owner {
    Owner {
        workspace: WORKSPACE.to_owned(),
        user: id.to_owned(),
        environment: Environment::Paper,
    }
}

fn trigger(n: u64) -> String {
    common::runtime_id(n)
}

fn acknowledge(event: u64, request: Option<&str>) -> Command {
    Command::Acknowledge {
        event: trigger(event),
        request: request.map(str::to_owned),
        subject: None,
    }
}

fn request(event: u64) -> Command {
    Command::RequestAcknowledgment {
        event: trigger(event),
        subject: None,
    }
}

fn rows(fx: &Fixture) -> Vec<StoredEvent> {
    fx.rows(CONTROL)
}

/// `by` runs `cmd` for `agent` at `second` with the code it needs now.
fn run(
    fx: &mut Fixture,
    by: &Owner,
    agent: &str,
    cmd: &Command,
    second: i64,
) -> Result<Submitted, ControlError> {
    let typed = code(&fx.journal, by, agent, cmd)?;
    command(
        &mut fx.journal,
        &mut FixedIds::default(),
        by,
        agent,
        cmd,
        typed.as_deref(),
        at(second),
    )
}

/// The one event the last command committed, checked as `by`'s, as a real journal would accept it
/// (`Draft::parse`, the §9.28 registration), with exactly `expected` members and `risk_clock` the
/// whole second it ran at.
fn committed(fx: &Fixture, before: usize, by: &str, expected: &[&str], second: i64) -> Value {
    let rows = rows(fx);
    assert_eq!(rows.len(), before + 1, "one event per command");
    let row = &rows[before];
    if let Err(e) = Draft::parse(&row.body) {
        panic!("a real journal refuses what the CLI committed: {e:?}");
    }
    let event = body(row);
    assert_eq!(member_text(&event, "actor.id"), Some(by));
    let names: BTreeSet<&str> = member(&event, "payload")
        .and_then(Value::as_object)
        .map(|o| o.keys().map(|k| k.as_str()).collect())
        .unwrap_or_default();
    assert_eq!(
        names,
        expected.iter().copied().collect(),
        "the payload's members"
    );
    let clock = UtcNanos::from_parts(second, 0).map(|t| t.to_string()).ok();
    assert_eq!(member_text(&event, "payload.risk_clock"), clock.as_deref());
    assert!(
        member(&event, "payload.step_up").is_some_and(|s| *s != Value::Null),
        "step-up evidence"
    );
    event
}

/// §9.28, rule 239: an acknowledgment needing no independent user has §9.28's members, names no
/// request, and names its own user as the requester, so a real journal accepts it.
#[test]
fn a_plain_acknowledgment_has_section_9_28s_members_and_a_journal_accepts_it() {
    let mut fx = Fixture::new();
    answer(
        "acknowledge",
        run(&mut fx, &owner(), AGENT, &acknowledge(42, None), ASKED_AT),
    );
    let event = committed(&fx, 0, OWNER, &ACKNOWLEDGED, ASKED_AT);
    assert_eq!(member_text(&event, "event_type"), Some("OwnerAcknowledged"));
    assert_eq!(member_text(&event, "payload.agent"), Some(AGENT));
    assert_eq!(
        member_text(&event, "payload.event"),
        Some(trigger(42).as_str())
    );
    assert_eq!(member_text(&event, "payload.user"), Some(OWNER));
    assert_eq!(member_text(&event, "payload.requested_by"), Some(OWNER));
    assert_eq!(
        member(&event, "payload.independent_required"),
        Some(&Value::Bool(false))
    );
    assert_eq!(member(&event, "payload.request"), Some(&Value::Null));
}

/// The members a plain acknowledgment's independence rides on, which §9.28's vectors judge.
const INDEPENDENCE: [&str; 3] = ["independent_required", "request", "requested_by"];

/// The vectors' executor case `name` in §9.28's section.
fn executor_case(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    let cases = member(&fixture, "acknowledgments.executor")
        .and_then(Value::as_array)
        .map(<[Value]>::to_vec)
        .unwrap_or_default();
    cases
        .into_iter()
        .find(|c| member_text(c, "name") == Some(name))
        .unwrap_or_else(|| panic!("§9.28's vectors have no executor case {name}"))
}

/// DEC-2512 item 3, the coordinator's ruling (a): a plain acknowledgment writes
/// `independent_required: false` whatever the policy, which adds no risk because the executor
/// judges under the stricter of the copy's requirement and the overlay's. The CLI's payload, with
/// its own agent, event and user written as the vectors', is exactly the control-stream
/// acknowledgment of `reference/journal`'s `plain_under_overlay` case, whose oracle refuses its
/// copy `not_independent` under a requiring overlay and lifts nothing.
#[test]
fn a_plain_acknowledgment_under_a_requiring_policy_is_the_one_the_executor_refuses() {
    let case = executor_case("plain_under_overlay");
    assert_eq!(
        member(&case, "overlay_independent"),
        Some(&Value::Bool(true))
    );
    let outcomes: Vec<&str> = member(&case, "expect")
        .and_then(Value::as_array)
        .map(|e| e.iter().filter_map(|x| member_text(x, "outcome")).collect())
        .unwrap_or_default();
    assert_eq!(outcomes, ["not_independent"], "the executor refuses it");
    let vector = member(&case, "control")
        .and_then(Value::as_array)
        .and_then(|records| records.first())
        .cloned()
        .unwrap();
    assert_eq!(
        member_text(&vector, "event_type"),
        Some("OwnerAcknowledged")
    );
    let named = |path: &str| member_text(&vector, path).unwrap_or_default().to_owned();
    let (agent, event, by) = (
        named("payload.agent"),
        named("payload.event"),
        named("payload.user"),
    );
    let mut fx = Fixture::new();
    let ack = Command::Acknowledge {
        event: event.clone(),
        request: None,
        subject: None,
    };
    answer(
        "acknowledge",
        run(&mut fx, &user(&by), &agent, &ack, ASKED_AT),
    );
    let committed = committed(&fx, 0, &by, &ACKNOWLEDGED, ASKED_AT);
    for name in INDEPENDENCE {
        let path = format!("payload.{name}");
        assert_eq!(member(&committed, &path), member(&vector, &path), "{name}");
    }
}

/// DEC-2512 item 1, rule 239: a request needs the code, and without it is refused `step_up_missing`
/// with nothing committed; with it, it commits one `OwnerAcknowledgmentRequested` that names the
/// owner as the requester, requires an independent user, and carries step-up evidence.
#[test]
fn a_request_needs_the_code_and_names_its_owner_as_the_requester() {
    let mut fx = Fixture::new();
    let checker = user(CHECKER);
    for typed in [None, Some("00000000")] {
        assert_eq!(
            command(
                &mut fx.journal,
                &mut FixedIds::default(),
                &checker,
                AGENT,
                &request(42),
                typed,
                at(ASKED_AT),
            ),
            Err(ControlError::Refused {
                reason: "step_up_missing"
            }),
            "{typed:?}"
        );
        assert!(rows(&fx).is_empty(), "nothing committed");
    }
    answer(
        "request",
        run(&mut fx, &checker, AGENT, &request(42), ASKED_AT),
    );
    let event = committed(&fx, 0, CHECKER, &REQUESTED, ASKED_AT);
    assert_eq!(
        member_text(&event, "event_type"),
        Some("OwnerAcknowledgmentRequested")
    );
    assert_eq!(member_text(&event, "payload.agent"), Some(AGENT));
    assert_eq!(
        member_text(&event, "payload.event"),
        Some(trigger(42).as_str())
    );
    assert_eq!(member_text(&event, "payload.requested_by"), Some(CHECKER));
    assert_eq!(
        member(&event, "payload.independent_required"),
        Some(&Value::Bool(true))
    );
}

/// DEC-2512 item 2, rules 239 and 240: an acknowledgment of another user's request names it, copies
/// its requester, requires an independent user, and names the acknowledging owner as `user`.
#[test]
fn an_acknowledgment_of_another_users_request_names_it_and_copies_its_requester() {
    let mut fx = Fixture::new();
    let asked = answer(
        "request",
        run(&mut fx, &user(CHECKER), AGENT, &request(42), ASKED_AT),
    );
    let ack = acknowledge(42, Some(&asked.event_id));
    answer(
        "acknowledge",
        run(&mut fx, &owner(), AGENT, &ack, ASKED_AT + 5),
    );
    let event = committed(&fx, 1, OWNER, &ACKNOWLEDGED, ASKED_AT + 5);
    assert_eq!(
        member_text(&event, "payload.request"),
        Some(asked.event_id.as_str())
    );
    assert_eq!(member_text(&event, "payload.user"), Some(OWNER));
    assert_eq!(member_text(&event, "payload.requested_by"), Some(CHECKER));
    assert_eq!(
        member(&event, "payload.independent_required"),
        Some(&Value::Bool(true))
    );
}

/// Rule 239, DEC-2512 items 2 and 4: a user never acknowledges their own request. The CLI refuses it
/// `not_independent` and commits nothing.
#[test]
fn acknowledging_your_own_request_is_refused_not_independent() {
    let mut fx = Fixture::new();
    let asked = answer(
        "request",
        run(&mut fx, &owner(), AGENT, &request(42), ASKED_AT),
    );
    let ack = acknowledge(42, Some(&asked.event_id));
    assert_eq!(
        run(&mut fx, &owner(), AGENT, &ack, ASKED_AT + 5),
        Err(ControlError::Refused {
            reason: "not_independent"
        })
    );
    assert_eq!(rows(&fx).len(), 1, "nothing committed");
}

/// Rules 240 and 241, DEC-2512 items 2 and 4: an acknowledgment naming an unknown id, an event that is no
/// request, a request of another agent or another event, or a request an earlier acknowledgment
/// already answered, is refused `request_invalid` and commits nothing.
#[test]
fn an_acknowledgment_naming_no_matching_unspent_request_is_refused_request_invalid() {
    let mut fx = Fixture::new();
    let checker = user(CHECKER);
    let other_agent = answer(
        "request",
        run(&mut fx, &checker, OTHER_AGENT, &request(42), ASKED_AT),
    );
    let other_event = answer(
        "request",
        run(&mut fx, &checker, AGENT, &request(43), ASKED_AT + 1),
    );
    let spent = answer(
        "request",
        run(&mut fx, &checker, AGENT, &request(42), ASKED_AT + 2),
    );
    answer(
        "the first acknowledgment",
        run(
            &mut fx,
            &owner(),
            AGENT,
            &acknowledge(42, Some(&spent.event_id)),
            ASKED_AT + 3,
        ),
    );
    let not_a_request = fx.append_in(
        "paper",
        CONTROL,
        "OwnerCommandIssued",
        None,
        object(&[("command", text("pause")), ("subject", text(AGENT))]),
    );
    let before = rows(&fx).len();
    for (what, named) in [
        ("an unknown id", trigger(999)),
        ("an event that is no request", not_a_request),
        ("another agent's request", other_agent.event_id),
        ("another event's request", other_event.event_id),
        ("a request already acknowledged", spent.event_id),
    ] {
        assert_eq!(
            run(
                &mut fx,
                &user(THIRD),
                AGENT,
                &acknowledge(42, Some(&named)),
                ASKED_AT + 10
            ),
            Err(ControlError::Refused {
                reason: "request_invalid"
            }),
            "{what}"
        );
        assert_eq!(rows(&fx).len(), before, "{what}: nothing committed");
    }
}

/// Rule 241 spends a request only by an acknowledgment that names it: an earlier plain
/// acknowledgment, and an earlier acknowledgment of another request, leave a request unspent, so
/// its acknowledgment commits.
#[test]
fn an_earlier_acknowledgment_does_not_spend_another_request() {
    let mut fx = Fixture::new();
    let checker = user(CHECKER);
    let first = answer(
        "request",
        run(&mut fx, &checker, AGENT, &request(42), ASKED_AT),
    );
    let second = answer(
        "request",
        run(&mut fx, &checker, AGENT, &request(42), ASKED_AT + 1),
    );
    answer(
        "a plain acknowledgment",
        run(
            &mut fx,
            &owner(),
            AGENT,
            &acknowledge(43, None),
            ASKED_AT + 2,
        ),
    );
    answer(
        "the first request's acknowledgment",
        run(
            &mut fx,
            &owner(),
            AGENT,
            &acknowledge(42, Some(&first.event_id)),
            ASKED_AT + 3,
        ),
    );
    let before = rows(&fx).len();
    let ack = acknowledge(42, Some(&second.event_id));
    answer(
        "the second request's",
        run(&mut fx, &user(THIRD), AGENT, &ack, ASKED_AT + 4),
    );
    let event = committed(&fx, before, THIRD, &ACKNOWLEDGED, ASKED_AT + 4);
    assert_eq!(
        member_text(&event, "payload.request"),
        Some(second.event_id.as_str())
    );
}

/// The #1822 review: a control-stream row the CLI cannot parse fails the acknowledgment closed,
/// committing nothing, rather than being skipped, so an acknowledgment the CLI could not read never
/// leaves its request looking unspent (rule 241).
#[test]
fn an_unreadable_control_row_refuses_an_acknowledgment_of_a_request() {
    let mut fx = Fixture::new();
    let asked = answer(
        "request",
        run(&mut fx, &user(CHECKER), AGENT, &request(42), ASKED_AT),
    );
    fx.journal.push_raw(
        CONTROL,
        "OwnerAcknowledged",
        b"{\"payload\": {\"request\": ",
    );
    let before = rows(&fx).len();
    let ack = acknowledge(42, Some(&asked.event_id));
    let got = run(&mut fx, &owner(), AGENT, &ack, ASKED_AT + 5);
    assert!(matches!(got, Err(ControlError::Journal(_))), "{got:?}");
    assert_eq!(rows(&fx).len(), before, "nothing committed");
}

/// The #1822 review, nit 2: rule 239 is judged before rule 240, as §9.28 check 4 orders
/// `not_independent` before `request_invalid` (the vectors' `independence_before_the_request`). The
/// owner's own request of another agent or another event is refused `not_independent`.
#[test]
fn your_own_request_is_not_independent_before_it_is_another_agents_or_events() {
    let mut fx = Fixture::new();
    let other_agent = answer(
        "request",
        run(&mut fx, &owner(), OTHER_AGENT, &request(42), ASKED_AT),
    );
    let other_event = answer(
        "request",
        run(&mut fx, &owner(), AGENT, &request(43), ASKED_AT + 1),
    );
    let before = rows(&fx).len();
    for (what, named) in [
        ("another agent's", other_agent),
        ("another event's", other_event),
    ] {
        assert_eq!(
            run(
                &mut fx,
                &owner(),
                AGENT,
                &acknowledge(42, Some(&named.event_id)),
                ASKED_AT + 5
            ),
            Err(ControlError::Refused {
                reason: "not_independent"
            }),
            "{what}"
        );
        assert_eq!(rows(&fx).len(), before, "{what}: nothing committed");
    }
}

/// DEC-2512 items 1 and 2: the code binds what the owner confirms. A request's code differs from an
/// acknowledgment's of the same event, and an acknowledgment's code differs with the request it
/// names, or none.
#[test]
fn the_code_binds_the_request_an_acknowledgment_names() {
    let fx = Fixture::new();
    let of = |cmd: &Command| {
        answer("code", code(&fx.journal, &owner(), AGENT, cmd))
            .unwrap_or_else(|| panic!("{cmd:?} has a code"))
    };
    let codes = [
        ("a request", of(&request(42))),
        ("a plain acknowledgment", of(&acknowledge(42, None))),
        (
            "one naming a request",
            of(&acknowledge(42, Some(&trigger(7)))),
        ),
        (
            "one naming another",
            of(&acknowledge(42, Some(&trigger(8)))),
        ),
    ];
    for (i, (what, one)) in codes.iter().enumerate() {
        for (other, two) in &codes[i + 1..] {
            assert_ne!(one, two, "{what} and {other}");
        }
    }
}
