//! `mandate agent acknowledge --subject` and `request-ack --subject` (E7-4b; journal spec v0.104
//! §9.28, DEC-2824, DEC-2830): given a subject, the acknowledgment or its request is written as
//! version 2 with that `subject`, which a real journal accepts, and the executor reads the `event`
//! it names as that subject's record, a reconciliation hold's `AgentModeApplied` among them.
//! Without one, the record stays version 1, a limit's. The code binds the subject, and an
//! acknowledgment answers only a request of the same subject (rule 240).

mod common;

use std::collections::BTreeSet;

use common::{
    AGENT, ASKED_AT, CONTROL, FixedIds, Fixture, OWNER, WORKSPACE, at, body, member, member_text,
    owner,
};
use mandate_canon::{Int, Value};
use mandate_cli::agent::{Command, Subject, code, command};
use mandate_cli::control::{ControlError, Owner, Submitted};
use mandate_journal::{Draft, Environment};

const CHECKER: &str = "user-checker";

fn user(id: &str) -> Owner {
    Owner {
        workspace: WORKSPACE.to_owned(),
        user: id.to_owned(),
        environment: Environment::Paper,
    }
}

fn hold() -> String {
    common::runtime_id(42)
}

fn acknowledge(subject: Option<Subject>, request: Option<&str>) -> Command {
    Command::Acknowledge {
        event: hold(),
        request: request.map(str::to_owned),
        subject,
    }
}

fn request(subject: Option<Subject>) -> Command {
    Command::RequestAcknowledgment {
        event: hold(),
        subject,
    }
}

fn run(
    fx: &mut Fixture,
    by: &Owner,
    cmd: &Command,
    second: i64,
) -> Result<Submitted, ControlError> {
    let typed = code(&fx.journal, by, AGENT, cmd)?;
    command(
        &mut fx.journal,
        &mut FixedIds::default(),
        by,
        AGENT,
        cmd,
        typed.as_deref(),
        at(second),
    )
}

/// The last committed event, which a real journal accepts.
fn last(fx: &Fixture) -> Value {
    let rows = fx.rows(CONTROL);
    let Some(row) = rows.last() else {
        panic!("nothing committed");
    };
    if let Err(e) = Draft::parse(&row.body) {
        panic!("a real journal refuses what the CLI committed: {e:?}");
    }
    body(row)
}

fn members(event: &Value) -> BTreeSet<String> {
    member(event, "payload")
        .and_then(Value::as_object)
        .map(|o| o.keys().map(|k| k.as_str().to_owned()).collect())
        .unwrap_or_default()
}

fn version(n: u64) -> Option<Value> {
    Int::new(n).map(Value::Int)
}

#[test]
fn an_acknowledgment_of_a_reconciliation_hold_is_version_2_with_its_subject() {
    for subject in [
        Subject::Limit,
        Subject::Restriction,
        Subject::Reconciliation,
    ] {
        let mut fx = Fixture::new();
        if let Err(e) = run(
            &mut fx,
            &owner(),
            &acknowledge(Some(subject), None),
            ASKED_AT,
        ) {
            panic!("{subject:?}: {e}");
        }
        let event = last(&fx);
        assert_eq!(
            member(&event, "schema_version").cloned(),
            version(2),
            "{subject:?}"
        );
        assert_eq!(
            member_text(&event, "payload.subject"),
            Some(subject.as_str())
        );
        assert_eq!(member_text(&event, "payload.event"), Some(hold().as_str()));
        assert_eq!(members(&event).len(), 10, "§9.28's nine and `subject`");
    }
}

#[test]
fn without_a_subject_an_acknowledgment_and_a_request_stay_version_1() {
    let mut fx = Fixture::new();
    for (cmd, by) in [
        (request(None), user(CHECKER)),
        (acknowledge(None, None), owner()),
    ] {
        if let Err(e) = run(&mut fx, &by, &cmd, ASKED_AT) {
            panic!("{cmd:?}: {e}");
        }
        let event = last(&fx);
        assert_eq!(member(&event, "schema_version").cloned(), version(1));
        assert!(!members(&event).contains("subject"), "{cmd:?}");
    }
}

#[test]
fn a_request_with_a_subject_is_answered_by_an_acknowledgment_of_the_same_subject() {
    let mut fx = Fixture::new();
    let asked = run(
        &mut fx,
        &user(CHECKER),
        &request(Some(Subject::Reconciliation)),
        ASKED_AT,
    );
    let Ok(asked) = asked else {
        panic!("request: {asked:?}");
    };
    let event = last(&fx);
    assert_eq!(member(&event, "schema_version").cloned(), version(2));
    assert_eq!(
        member_text(&event, "payload.subject"),
        Some("reconciliation")
    );
    let before = fx.rows(CONTROL).len();
    for other in [None, Some(Subject::Limit), Some(Subject::Restriction)] {
        assert_eq!(
            run(
                &mut fx,
                &owner(),
                &acknowledge(other, Some(&asked.event_id)),
                ASKED_AT + 5
            ),
            Err(ControlError::Refused {
                reason: "request_invalid"
            }),
            "{other:?}"
        );
    }
    assert_eq!(fx.rows(CONTROL).len(), before, "nothing committed");
    let same = acknowledge(Some(Subject::Reconciliation), Some(&asked.event_id));
    if let Err(e) = run(&mut fx, &owner(), &same, ASKED_AT + 5) {
        panic!("the same subject: {e}");
    }
    let event = last(&fx);
    assert_eq!(member_text(&event, "actor.id"), Some(OWNER));
    assert_eq!(
        member_text(&event, "payload.request"),
        Some(asked.event_id.as_str())
    );
    assert_eq!(
        member_text(&event, "payload.subject"),
        Some("reconciliation")
    );
}

#[test]
fn a_limit_request_of_version_1_is_answered_by_a_limit_acknowledgment_of_either_version() {
    for subject in [None, Some(Subject::Limit)] {
        let mut fx = Fixture::new();
        let asked = run(&mut fx, &user(CHECKER), &request(None), ASKED_AT);
        let Ok(asked) = asked else {
            panic!("request: {asked:?}");
        };
        let ack = acknowledge(subject, Some(&asked.event_id));
        if let Err(e) = run(&mut fx, &owner(), &ack, ASKED_AT + 5) {
            panic!("{subject:?}: {e}");
        }
        assert_eq!(
            run(
                &mut fx,
                &owner(),
                &acknowledge(Some(Subject::Reconciliation), Some(&asked.event_id)),
                ASKED_AT + 6
            )
            .err(),
            Some(ControlError::Refused {
                reason: "request_invalid"
            })
        );
    }
}

#[test]
fn the_code_binds_the_subject() {
    let fx = Fixture::new();
    let of = |cmd: &Command| match code(&fx.journal, &owner(), AGENT, cmd) {
        Ok(Some(code)) => code,
        other => panic!("{cmd:?}: {other:?}"),
    };
    let subjects = [
        None,
        Some(Subject::Limit),
        Some(Subject::Restriction),
        Some(Subject::Reconciliation),
    ];
    let makers: [&dyn Fn(Option<Subject>) -> Command; 2] = [&|s| acknowledge(s, None), &request];
    for make in makers {
        let codes: BTreeSet<String> = subjects.iter().map(|s| of(&make(*s))).collect();
        assert_eq!(codes.len(), subjects.len(), "{:?}", make(None));
    }
    let mut fx = Fixture::new();
    let plain = of(&acknowledge(None, None));
    assert_eq!(
        command(
            &mut fx.journal,
            &mut FixedIds::default(),
            &owner(),
            AGENT,
            &acknowledge(Some(Subject::Reconciliation), None),
            Some(&plain),
            at(ASKED_AT),
        ),
        Err(ControlError::Refused {
            reason: "step_up_missing"
        })
    );
}
