//! The connect start's refusals over HTTP (§3.4, §3.5, §4.5, DEC-2623 item 6, DEC-883): one code
//! per response, in order, and a refused start appends nothing and spends no step-up.

mod common;
mod support_connect;

use mandate_api::problem::ProblemCode;
use mandate_canon::to_canonical;
use support_connect::{
    ALICE, BOB, CAROL, Fixture, KEY, appended, base, code, fixture, signed_in, start, with,
};

/// Nothing was appended and the step-up port was never asked.
fn untouched(f: &Fixture) {
    assert!(appended(f).is_empty(), "nothing recorded");
    assert!(
        f.port.asked.lock().expect("unpoisoned").is_empty(),
        "no step-up judged"
    );
}

fn admin(f: &Fixture, body: &[u8]) -> (u16, ProblemCode) {
    let cookie = signed_in(f, ALICE);
    code(&start(f, Some(&cookie), Some(KEY), body))
}

#[test]
fn the_session_then_the_key_then_the_body_are_checked_first() {
    let f = fixture("paper", true, true);
    let body = to_canonical(&base());
    assert_eq!(code(&start(&f, None, Some(KEY), &body)).0, 401);
    let cookie = signed_in(&f, ALICE);
    assert_eq!(
        code(&start(&f, Some(&cookie), None, &body)).1,
        ProblemCode::Invalid
    );
    assert_eq!(admin(&f, b"not json").1, ProblemCode::Invalid);
    assert_eq!(
        admin(&f, &with(&["broker"], r#""robinhood""#)).1,
        ProblemCode::Invalid
    );
    untouched(&f);
}

#[test]
fn a_member_without_the_permission_is_forbidden_and_a_stranger_finds_nothing() {
    let f = fixture("paper", true, true);
    let body = to_canonical(&base());
    let viewer = signed_in(&f, BOB);
    assert_eq!(
        code(&start(&f, Some(&viewer), Some(KEY), &body)),
        (403, ProblemCode::Forbidden)
    );
    let stranger = signed_in(&f, CAROL);
    assert_eq!(
        code(&start(&f, Some(&stranger), Some(KEY), &body)),
        (404, ProblemCode::NotFound)
    );
    untouched(&f);
}

#[test]
fn a_live_workspace_is_refused_before_any_step_up() {
    let f = fixture("live", true, true);
    assert_eq!(
        admin(&f, &to_canonical(&base())),
        (409, ProblemCode::LiveUnavailable)
    );
    untouched(&f);
}

#[test]
fn another_disclosure_version_is_invalid_before_any_step_up() {
    let f = fixture("paper", true, true);
    let body = String::from_utf8(to_canonical(&base()))
        .expect("UTF-8")
        .replace(r#""version":"1""#, r#""version":"2""#);
    assert_eq!(admin(&f, body.as_bytes()), (422, ProblemCode::Invalid));
    let reply = start(&f, Some(&signed_in(&f, ALICE)), Some(KEY), body.as_bytes());
    let problem = String::from_utf8_lossy(&reply.body).into_owned();
    assert!(problem.contains("/disclosure"), "{problem}");
    untouched(&f);
}

#[test]
fn a_start_without_step_up_is_step_up_required_and_never_reaches_the_port() {
    let f = fixture("paper", true, true);
    assert_eq!(
        admin(&f, &with(&["step_up"], "null")),
        (401, ProblemCode::StepUpRequired)
    );
    untouched(&f);
}

#[test]
fn evidence_the_port_refuses_answers_its_code_and_records_nothing() {
    let f = fixture("paper", true, false);
    assert_eq!(
        admin(&f, &to_canonical(&base())),
        (401, ProblemCode::StepUpMismatch)
    );
    assert!(appended(&f).is_empty());
    assert_eq!(f.port.asked.lock().expect("unpoisoned").len(), 1);
}

#[test]
fn a_live_connection_is_refused_after_step_up_and_records_nothing() {
    let f = fixture("paper", true, true);
    assert_eq!(
        admin(&f, &with(&["environment"], r#""live""#)),
        (409, ProblemCode::LiveUnavailable)
    );
    assert!(appended(&f).is_empty());
    assert_eq!(
        f.port.asked.lock().expect("unpoisoned").len(),
        1,
        "step-up first, so a caller learns nothing before it"
    );
}

#[test]
fn a_control_stream_that_cannot_be_read_records_nothing() {
    let f = fixture("paper", true, true);
    f.journal
        .failing
        .lock()
        .expect("unpoisoned")
        .insert(support_connect::ctl());
    assert_eq!(
        admin(&f, &to_canonical(&base())).1,
        ProblemCode::JournalUnavailable
    );
    f.journal.failing.lock().expect("unpoisoned").clear();
    untouched(&f);
}

#[test]
fn a_deployment_that_offers_no_connect_answers_not_found() {
    let f = fixture("paper", false, true);
    assert_eq!(
        admin(&f, &to_canonical(&base())),
        (404, ProblemCode::NotFound)
    );
    untouched(&f);
}
