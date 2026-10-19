//! `mandate step-up confirm` (DEC-2104 item 2, DEC-2106; journal spec §9.29; E9-4 S2). The journal
//! is `tests/common`'s in-memory `Journal`, whose agent stream holds the approval as the runtime
//! writes it. Every expectation is computed here: the action digest from the request's content
//! object, the record from DEC-2106's items, and whether the journal accepts it from
//! `mandate_journal::Draft::parse`, which judges rule 242. Each refusal is paired with the commit
//! of [`a_confirmation_appends_one_record_the_journal_accepts`], so a command that does nothing
//! passes none of them. The flags' checks are `tests/step_up_check.rs`. The tests are pending E9-4
//! and fail on the stubs' `ControlError::Unimplemented` (DEC-77, DEC-110).

mod common;

use common::{
    AGENT, ASKED_AT, CONTROL, Fixture, TIMEOUT_S, WORKSPACE, at, body, content, hash_of, member,
    member_text, object, text,
};
use mandate_canon::{Value, to_canonical};
use mandate_cli::control::ControlError;
use mandate_cli::inbox::show_lines;
use mandate_cli::step_up::{
    Checked, Confirmed, Confirming, EVENT_TYPE, check, confirm, confirmed_lines, run_confirm,
};
use mandate_journal::Draft;

const PRINCIPAL: &str = "01J9ZP000000000000000000PR";
const CHALLENGE: &str = "01J9ZC000000000000000000CH";

fn confirming<'a>(approval: &'a str, code: &'a str) -> Confirming<'a> {
    Confirming {
        workspace: WORKSPACE,
        principal: PRINCIPAL,
        agent: AGENT,
        approval,
        challenge: CHALLENGE,
        code,
    }
}

fn checked(approval: &str, code: &str) -> Checked {
    check(&confirming(approval, code)).unwrap_or_else(|e| panic!("checked, not: {e}"))
}

fn refused(reason: &'static str) -> ControlError {
    ControlError::Refused { reason }
}

/// [`confirm`] of `approval` with `code` at the second `secs`.
fn confirm_at(
    fx: &mut Fixture,
    approval: &str,
    code: &str,
    secs: i64,
) -> Result<Confirmed, ControlError> {
    confirm(&mut fx.journal, &checked(approval, code), at(secs))
}

/// The control stream's stored envelopes.
fn control(fx: &Fixture) -> Vec<Value> {
    fx.rows(CONTROL).iter().map(body).collect()
}

/// DEC-2104 items 2 and 3: exactly one `StepUpConfirmed` on the control stream, its members
/// closed and computed here, which the journal accepts under rule 242.
#[test]
#[ignore = "pending E9-4"]
fn a_confirmation_appends_one_record_the_journal_accepts() {
    let mut fx = Fixture::new();
    let asked = fx.ask(AGENT, "3", ASKED_AT);
    let now = at(ASKED_AT + TIMEOUT_S - 1);
    let confirmed = confirm_at(&mut fx, &asked.approval, "abcd-efgh", now.secs).unwrap();
    assert!(!confirmed.earlier);
    assert_eq!(confirmed.shown.content_hash, hash_of(&asked.content));
    let rows = fx.rows(CONTROL);
    assert_eq!(rows.len(), 1, "one record");
    assert_eq!(rows[0].event_id, confirmed.submitted.event_id);
    let parsed = Draft::parse(&rows[0].body).map(|d| d.event_type().to_owned());
    assert_eq!(parsed, Ok(EVENT_TYPE.to_owned()));
    let event = body(&rows[0]);
    for (path, want) in [
        ("environment", "paper"),
        ("stream_id", CONTROL),
        ("actor.kind", "user"),
        ("actor.id", PRINCIPAL),
    ] {
        assert_eq!(member_text(&event, path), Some(want), "{path}");
    }
    let payload = object(&[
        ("challenge", text(CHALLENGE)),
        ("action_kind", text("approve")),
        ("action_digest", text(&hash_of(&asked.content))),
        ("principal", text(PRINCIPAL)),
        ("code", text("ABCDEFGH")),
        ("confirmed_at", common::int(now.secs)),
    ]);
    assert_eq!(member(&event, "payload"), Some(&payload));
}

/// DEC-2106 items 2.5 to 2.9: each refusal appends nothing; with two faults the first answers.
#[test]
#[ignore = "pending E9-4"]
fn each_journal_check_refuses_appending_nothing() {
    let cases: [(&str, &str); 6] = [
        ("live control stream", "workspace_environment"),
        ("unknown approval", "not_pending"),
        ("live request", "paper_only"),
        ("at the deadline", "late"),
        ("live request, at the deadline", "paper_only"),
        ("confirmed with another code", "challenge_confirmed"),
    ];
    for (name, reason) in cases {
        let mut fx = Fixture::new();
        let asked = match name {
            "live control stream" => {
                fx.append_in("live", CONTROL, "StreamOpened", None, object(&[]));
                fx.ask(AGENT, "3", ASKED_AT)
            }
            "live request" | "live request, at the deadline" => {
                fx.ask_in("live", AGENT, &content("3", ASKED_AT + TIMEOUT_S))
            }
            _ => fx.ask(AGENT, "3", ASKED_AT),
        };
        let approval = match name {
            "unknown approval" => "0FFFFFFFFFFFFFFFFFFFFFFFFF".to_owned(),
            _ => asked.approval.clone(),
        };
        let secs = match name {
            "at the deadline" | "live request, at the deadline" => asked.deadline,
            _ => ASKED_AT,
        };
        if name == "confirmed with another code" {
            confirm_at(&mut fx, &approval, "ZZZZ-ZZZZ", ASKED_AT).unwrap();
        }
        let rows = fx.rows(CONTROL).len();
        let attempts = fx.journal.attempts.len();
        let got = confirm_at(&mut fx, &approval, "ABCD-EFGH", secs);
        assert_eq!(got.err(), Some(refused(reason)), "{name}");
        let after = (fx.rows(CONTROL).len(), fx.journal.attempts.len());
        assert_eq!(after, (rows, attempts), "{name} appended or tried to");
    }
}

/// DEC-2106 item 3 (DEC-290): a re-run with the same code finds the record, whatever its second.
#[test]
#[ignore = "pending E9-4"]
fn a_re_run_finds_its_confirmation_and_appends_nothing() {
    let mut fx = Fixture::new();
    let asked = fx.ask(AGENT, "3", ASKED_AT);
    let first = confirm_at(&mut fx, &asked.approval, "ABCD-EFGH", ASKED_AT).unwrap();
    let rerun = confirm_at(&mut fx, &asked.approval, "abcdefgh", ASKED_AT + 5).unwrap();
    assert!(rerun.earlier && !first.earlier);
    assert_eq!(rerun.submitted, first.submitted);
    assert_eq!(control(&fx).len(), 1);
}

/// DEC-2106 item 4: a confirmation of the challenge landing after the rows were read and before
/// the append is judged, so rule 243 holds against every row the record would follow.
#[test]
#[ignore = "pending E9-4"]
fn a_confirmation_landing_first_is_judged_before_the_append() {
    let mut fx = Fixture::new();
    let asked = fx.ask(AGENT, "3", ASKED_AT);
    let other = object(&[
        ("event_id", text("40000000000000000000000001")),
        ("event_type", text(EVENT_TYPE)),
        ("environment", text("paper")),
        (
            "payload",
            object(&[
                ("challenge", text(CHALLENGE)),
                ("action_digest", text(&hash_of(&asked.content))),
                ("principal", text(PRINCIPAL)),
                ("code", text("ZZZZZZZZ")),
            ]),
        ),
    ]);
    fx.journal.land_before_next.push(to_canonical(&other));
    let got = confirm_at(&mut fx, &asked.approval, "ABCD-EFGH", ASKED_AT);
    assert_eq!(got.err(), Some(refused("challenge_confirmed")));
    let codes: Vec<_> = control(&fx)
        .iter()
        .map(|e| member(e, "payload.code").cloned())
        .collect();
    assert_eq!(codes, [Some(text("ZZZZZZZZ"))], "only the landed record");
}

/// DEC-2106 item 5: the approval as `approvals show` prints it, then the outcome line; a refusal
/// prints nothing.
#[test]
#[ignore = "pending E9-4"]
fn the_command_prints_the_approval_then_its_confirmation() {
    let mut fx = Fixture::new();
    let asked = fx.ask(AGENT, "3", ASKED_AT);
    let ok = checked(&asked.approval, "ABCD-EFGH");
    for (word, secs) in [("confirmed", ASKED_AT), ("already confirmed", ASKED_AT + 1)] {
        let mut out = Vec::new();
        let done = run_confirm(&ok, &mut fx.journal, at(secs), &mut out).unwrap();
        let mut want = show_lines(&done.shown).unwrap();
        want.push(format!(
            "{word} {CHALLENGE} as event {} at seq {} for {}",
            done.submitted.event_id,
            done.submitted.seq,
            hash_of(&asked.content)
        ));
        assert_eq!(confirmed_lines(&ok, &done).unwrap(), want, "{word}");
        let printed = String::from_utf8(out).unwrap();
        assert_eq!(printed, want.join("\n") + "\n", "{word}");
    }
    let mut out = Vec::new();
    let late = run_confirm(&ok, &mut fx.journal, at(asked.deadline), &mut out);
    assert!(late.is_err() && out.is_empty(), "{late:?}");
}
