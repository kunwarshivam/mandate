//! `mandate step-up confirm` (DEC-2104 item 2, DEC-2106; journal spec §9.29; E9-4 S2). The journal
//! is `tests/common`'s in-memory `Journal`, whose agent stream holds the approval as the runtime
//! writes it. Every expectation is computed here: the action digest from the request's content
//! object, the record from DEC-2106's items, and whether the journal accepts it from
//! `mandate_journal::Draft::parse`, which judges rule 242. Each refusal is paired with the commit
//! of [`a_confirmation_appends_one_record_the_journal_accepts`], so a command that does nothing
//! passes none of them. The flags' checks are `tests/step_up_check.rs`.

mod common;

use common::{
    AGENT, ASKED_AT, CONTROL, Fixture, TIMEOUT_S, WORKSPACE, agent_stream, at, body, code_of,
    content, hash_of, int, member, member_text, object, text,
};
use mandate_approval::content_object;
use mandate_canon::{Value, to_canonical};
use mandate_cli::control::{ControlError, Now};
use mandate_cli::step_up::{
    Checked, Confirmed, Confirming, EVENT_TYPE, check, confirm, confirmed_lines, run_confirm,
};
use mandate_journal::Draft;

const PRINCIPAL: &str = "01J9ZP000000000000000000PR";
const ANOTHER_PRINCIPAL: &str = "01J9ZP000000000000000000PS";
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

/// A `StepUpConfirmed` of [`CHALLENGE`] another writer stored, for `digest` with `code` by
/// `principal`.
fn landed(event_id: &str, digest: &str, principal: &str, code: &str) -> Vec<u8> {
    to_canonical(&object(&[
        ("event_id", text(event_id)),
        ("event_type", text(EVENT_TYPE)),
        ("environment", text("paper")),
        ("stream_id", text(CONTROL)),
        (
            "payload",
            object(&[
                ("challenge", text(CHALLENGE)),
                ("action_kind", text("approve")),
                ("action_digest", text(digest)),
                ("principal", text(principal)),
                ("code", text(code)),
            ]),
        ),
    ]))
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
fn a_confirmation_appends_one_record_the_journal_accepts() {
    let mut fx = Fixture::new();
    let asked = fx.ask(AGENT, "3", ASKED_AT);
    let secs = ASKED_AT + TIMEOUT_S - 1;
    let now = Now {
        at: mandate_time::UtcNanos::from_parts(secs, 123_456_789).unwrap(),
        secs,
    };
    let confirmed = confirm(&mut fx.journal, &checked(&asked.approval, "abcd-efgh"), now).unwrap();
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
        ("event_time", &now.at.to_string()),
    ] {
        assert_eq!(member_text(&event, path), Some(want), "{path}");
    }
    assert_eq!(member(&event, "schema_version"), Some(&int(1)));
    let payload = object(&[
        ("challenge", text(CHALLENGE)),
        ("action_kind", text("approve")),
        ("action_digest", text(&hash_of(&asked.content))),
        ("principal", text(PRINCIPAL)),
        ("code", text("ABCDEFGH")),
        ("confirmed_at", int(secs)),
    ]);
    assert_eq!(member(&event, "payload"), Some(&payload));
}

/// DEC-2106 items 2.5 to 2.9, in order, each with its code: a fixture holding any mix of them is
/// refused by the first.
const FAULTS: [(&str, &str); 6] = [
    ("live control stream", "workspace_environment"),
    ("unknown approval", "not_pending"),
    ("stated hash not its content", "content_mismatch"),
    ("live request", "paper_only"),
    ("at the deadline", "late"),
    ("confirmed with another code", "challenge_confirmed"),
];

/// A fixture with the faults `on` names, in [`FAULTS`]' order, and the approval and second the
/// command runs with.
fn arranged(on: [bool; 6]) -> (Fixture, String, i64) {
    let [live_control, unknown, mismatch, live_request, late, other] = on;
    let mut fx = Fixture::new();
    if live_control {
        fx.append_in("live", CONTROL, "StreamOpened", None, object(&[]));
    }
    let environment = if live_request { "live" } else { "paper" };
    let deadline = ASKED_AT + TIMEOUT_S;
    let request = content("3", deadline);
    let digest = hash_of(&content_object(&request).unwrap());
    let approval = if mismatch {
        let payload = object(&[
            ("deadline", int(deadline)),
            ("content", content_object(&request).unwrap()),
            (
                "content_hash",
                text(&hash_of(&text("not the request's content"))),
            ),
        ]);
        fx.append_in(
            environment,
            &agent_stream(AGENT),
            "ApprovalRequested",
            None,
            payload,
        )
    } else {
        fx.ask_in(environment, AGENT, &request).approval
    };
    if other {
        let payload = object(&[
            ("challenge", text(CHALLENGE)),
            ("action_kind", text("approve")),
            ("action_digest", text(&digest)),
            ("principal", text(PRINCIPAL)),
            ("code", text("ZZZZZZZZ")),
        ]);
        fx.append_in("paper", CONTROL, EVENT_TYPE, None, payload);
    }
    let approval = if unknown {
        "0FFFFFFFFFFFFFFFFFFFFFFFFF".to_owned()
    } else {
        approval
    };
    let secs = if late { deadline } else { ASKED_AT };
    (fx, approval, secs)
}

/// Every one of the 64 mixes of [`FAULTS`]: with none the command appends exactly one record;
/// with any it is refused by the first in DEC-2106 item 2's order, and appends and attempts
/// nothing. The oracle is [`FAULTS`]' own order, not the command's.
#[test]
fn every_mix_of_faults_is_refused_by_its_first_and_appends_nothing() {
    for mask in 0..64u8 {
        let on: [bool; 6] = std::array::from_fn(|i| mask >> i & 1 == 1);
        let (mut fx, approval, secs) = arranged(on);
        let before = (fx.rows(CONTROL).len(), fx.journal.attempts.len());
        let got = confirm_at(&mut fx, &approval, "ABCD-EFGH", secs);
        let first = FAULTS.iter().zip(on).find(|(_, on)| *on).map(|(f, _)| *f);
        let after = (fx.rows(CONTROL).len(), fx.journal.attempts.len());
        match first {
            Some((name, code)) => {
                assert_eq!(got.err(), Some(refused(code)), "{on:?}: first is {name}");
                assert_eq!(after, before, "{on:?} appended or tried to");
            }
            None => {
                assert!(got.is_ok(), "no fault: {got:?}");
                assert_eq!(after.0, before.0 + 1, "one record");
            }
        }
    }
}

/// DEC-2106 item 3 (DEC-290): a re-run with the same digest, principal and code finds the record,
/// whatever its second. DEC-2107: the exception reaches item 2.9 alone, so a re-run at the deadline
/// is `late`, and a re-run by another principal is `challenge_confirmed`.
#[test]
fn a_re_run_finds_its_confirmation_and_appends_nothing() {
    let mut fx = Fixture::new();
    let asked = fx.ask(AGENT, "3", ASKED_AT);
    let first = confirm_at(&mut fx, &asked.approval, "ABCD-EFGH", ASKED_AT).unwrap();
    let rerun = confirm_at(&mut fx, &asked.approval, "abcdefgh", ASKED_AT + 5).unwrap();
    assert!(rerun.earlier && !first.earlier);
    assert_eq!(rerun.submitted, first.submitted);
    assert_eq!(control(&fx).len(), 1);
    let late = confirm_at(&mut fx, &asked.approval, "ABCD-EFGH", asked.deadline);
    assert_eq!(
        late.err(),
        Some(refused("late")),
        "DEC-2107 item 1: late before item 3"
    );
    let another = Confirming {
        principal: ANOTHER_PRINCIPAL,
        ..confirming(&asked.approval, "ABCD-EFGH")
    };
    let theirs = confirm(&mut fx.journal, &check(&another).unwrap(), at(ASKED_AT + 6));
    assert_eq!(
        theirs.err(),
        Some(refused("challenge_confirmed")),
        "another principal"
    );
    assert_eq!(control(&fx).len(), 1);
}

/// DEC-2106 item 4: what lands after the rows were read and before the append is judged when the
/// command reads them again. An unrelated row: it decides again and appends once, at the new head,
/// after its first attempt was refused. Another confirmation of the challenge: refused. The same
/// confirmation, another writer's: found, and nothing appended (item 3).
#[test]
fn what_lands_before_the_append_is_judged_on_a_second_read() {
    let mut fx = Fixture::new();
    let asked = fx.ask(AGENT, "3", ASKED_AT);
    fx.journal.behind_next = 1;
    let done = confirm_at(&mut fx, &asked.approval, "ABCD-EFGH", ASKED_AT).unwrap();
    let rows = fx.rows(CONTROL);
    let types: Vec<_> = rows.iter().map(|r| r.event_type.as_str()).collect();
    assert_eq!(types, ["OwnerCommandIssued", EVENT_TYPE]);
    assert_eq!((done.submitted.seq, done.earlier), (2, false));
    assert_eq!(
        fx.journal.attempts.len(),
        2,
        "the first attempt was refused"
    );
    assert_eq!(
        fx.journal.attempts[1],
        std::slice::from_ref(&done.submitted.event_id)
    );

    let digest = hash_of(&asked.content);
    for (code, want) in [
        ("ZZZZZZZZ", Err(refused("challenge_confirmed"))),
        ("ABCDEFGH", Ok(true)),
    ] {
        let mut fx = Fixture::new();
        let asked = fx.ask(AGENT, "3", ASKED_AT);
        let id = "40000000000000000000000003";
        fx.journal
            .land_before_next
            .push(landed(id, &digest, PRINCIPAL, code));
        let got = confirm_at(&mut fx, &asked.approval, "ABCD-EFGH", ASKED_AT);
        assert_eq!(
            got.as_ref().map(|c| c.earlier).map_err(Clone::clone),
            want.clone(),
            "landed {code}"
        );
        let ids: Vec<_> = fx
            .rows(CONTROL)
            .iter()
            .map(|r| r.event_id.clone())
            .collect();
        assert_eq!(ids, [id], "only the landed record, for {code}");
        if want.is_ok() {
            assert_eq!(got.unwrap().submitted.event_id, id);
        }
    }
}

/// DEC-2106 item 5: the approval as `approvals show` prints it, then the outcome line; a refusal
/// prints nothing.
#[test]
fn the_command_prints_the_approval_then_its_confirmation() {
    let mut fx = Fixture::new();
    let asked = fx.ask(AGENT, "3", ASKED_AT);
    let ok = checked(&asked.approval, "ABCD-EFGH");
    for (word, secs) in [("confirmed", ASKED_AT), ("already confirmed", ASKED_AT + 1)] {
        let mut out = Vec::new();
        let done = run_confirm(&ok, &mut fx.journal, at(secs), &mut out).unwrap();
        let stored = &fx.rows(CONTROL)[0];
        let hash = hash_of(&asked.content);
        let want = vec![
            format!("approval {} deadline {}", asked.approval, asked.deadline),
            String::from_utf8(to_canonical(&asked.content)).unwrap(),
            format!("content_hash {hash}"),
            format!("code {}", code_of(&asked.content)),
            format!(
                "{word} {CHALLENGE} as event {} at seq {} for {hash}",
                stored.event_id, stored.seq
            ),
        ];
        assert_eq!(confirmed_lines(&ok, &done).unwrap(), want, "{word}");
        let printed = String::from_utf8(out).unwrap();
        assert_eq!(printed, want.join("\n") + "\n", "{word}");
    }
    let mut out = Vec::new();
    let late = run_confirm(&ok, &mut fx.journal, at(asked.deadline), &mut out);
    assert!(late.is_err() && out.is_empty(), "{late:?}");
}
