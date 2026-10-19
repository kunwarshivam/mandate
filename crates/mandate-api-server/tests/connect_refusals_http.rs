//! The connect start's refusals over HTTP (§3.4, §3.5, §4.5, DEC-2623 item 6, DEC-883): one code
//! per response, in order, and a refused start appends nothing and spends no step-up.

mod common;
mod support_connect;

use mandate_api::problem::ProblemCode;
use mandate_api_server::connect::{ACCOUNT_OPERATION, CONNECTION_OPERATION};
use mandate_canon::to_canonical;
use support_connect::{
    ALICE, BOB, CAROL, Fixture, KEY, appended, base, code, ctl, derived, fixture, fixture_drawing,
    seed, signed_in, start, with,
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
#[ignore = "pending E10-13"]
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
#[ignore = "pending E10-13"]
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
#[ignore = "pending E10-13"]
fn a_live_workspace_is_refused_before_any_step_up() {
    let f = fixture("live", true, true);
    assert_eq!(
        admin(&f, &to_canonical(&base())),
        (409, ProblemCode::LiveUnavailable)
    );
    untouched(&f);
}

#[test]
#[ignore = "pending E10-13"]
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
#[ignore = "pending E10-13"]
fn a_start_without_step_up_is_step_up_required_and_never_reaches_the_port() {
    let f = fixture("paper", true, true);
    assert_eq!(
        admin(&f, &with(&["step_up"], "null")),
        (401, ProblemCode::StepUpRequired)
    );
    untouched(&f);
}

#[test]
#[ignore = "pending E10-13"]
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
#[ignore = "pending E10-13"]
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
#[ignore = "pending E10-13"]
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
#[ignore = "pending E10-13"]
fn a_deployment_that_offers_no_connect_answers_not_found() {
    let f = fixture("paper", false, true);
    assert_eq!(
        admin(&f, &to_canonical(&base())),
        (404, ProblemCode::NotFound)
    );
    untouched(&f);
}

/// The rows a start appends after `seeded` rows were put on the control stream.
fn after(f: &Fixture, seeded: usize) -> usize {
    f.journal.rows_of(&ctl()).len() - 1 - seeded
}

fn requested(id: &str, account: &str) -> String {
    format!(
        r#"{{"connection_id":"{id}","account_ref":"{account}","broker":"alpaca","environment":"paper"}}"#
    )
}

/// The derived ids already named on the control stream: an establishment or an open request of
/// the connection, or a request or establishment of another connection with the derived account,
/// is `idempotency_conflict` after step-up, and nothing is recorded (DEC-2623 item 6).
#[test]
#[ignore = "pending E10-13"]
fn a_derived_id_the_control_stream_already_names_conflicts_after_step_up() {
    let connection = format!("conn_{}", derived(CONNECTION_OPERATION));
    let account = derived(ACCOUNT_OPERATION);
    let other = "01JB3K6F0C9R2V7N4M8Q1T5W70";
    let cases: [Vec<(&str, String)>; 5] = [
        vec![("ConnectionEstablished", requested(&connection, other))],
        vec![("ConnectionRequested", requested(&connection, other))],
        vec![("ConnectionRequested", requested("conn_other", &account))],
        vec![("ConnectionEstablished", requested("conn_other", &account))],
        vec![
            ("ConnectionEstablished", requested(&connection, other)),
            (
                "ConnectionRefused",
                format!(r#"{{"connection_id":"{connection}","occasion":"reconnect"}}"#),
            ),
        ],
    ];
    for rows in cases {
        let f = fixture("paper", true, true);
        for (event_type, payload) in &rows {
            seed(&f, event_type, 1, payload);
        }
        assert_eq!(
            admin(&f, &to_canonical(&base())),
            (409, ProblemCode::IdempotencyConflict),
            "{rows:?}"
        );
        assert_eq!(after(&f, rows.len()), 0, "{rows:?}");
        assert_eq!(
            f.port.asked.lock().expect("unpoisoned").len(),
            1,
            "step-up first"
        );
    }
}

/// A request of the derived connection closed by its refusal or revocation frees the id, and a
/// refusal of another connection naming the derived account does not use it: the start commits.
#[test]
#[ignore = "pending E10-13"]
fn a_closed_request_frees_the_id_and_a_refusal_does_not_use_the_account() {
    let connection = format!("conn_{}", derived(CONNECTION_OPERATION));
    let account = derived(ACCOUNT_OPERATION);
    let cases: [Vec<(&str, String)>; 3] = [
        vec![
            (
                "ConnectionRequested",
                requested(&connection, "01JB3K6F0C9R2V7N4M8Q1T5W70"),
            ),
            (
                "ConnectionRefused",
                format!(r#"{{"connection_id":"{connection}","occasion":"connect"}}"#),
            ),
        ],
        vec![
            (
                "ConnectionRequested",
                requested(&connection, "01JB3K6F0C9R2V7N4M8Q1T5W70"),
            ),
            (
                "ConnectionRevoked",
                format!(r#"{{"connection_id":"{connection}"}}"#),
            ),
        ],
        vec![("ConnectionRefused", requested("conn_other", &account))],
    ];
    for rows in cases {
        let f = fixture("paper", true, true);
        for (event_type, payload) in &rows {
            seed(&f, event_type, 1, payload);
        }
        let cookie = signed_in(&f, ALICE);
        let reply = start(&f, Some(&cookie), Some(KEY), &to_canonical(&base()));
        assert_eq!(
            reply.status,
            201,
            "{rows:?}: {}",
            String::from_utf8_lossy(&reply.body)
        );
        assert_eq!(after(&f, rows.len()), 1, "{rows:?}");
    }
}

/// A random source that fails after the commit answers `journal_unavailable`; the request stays
/// recorded, open until its deadline's teardown closes it (connections spec §5.2 step 6).
#[test]
#[ignore = "pending E10-13"]
fn a_random_source_that_fails_after_the_commit_answers_journal_unavailable() {
    let f = fixture_drawing("paper", true, true, 2);
    assert_eq!(
        admin(&f, &to_canonical(&base())),
        (503, ProblemCode::JournalUnavailable)
    );
    assert_eq!(appended(&f).len(), 1, "the request is recorded");
    let held = f
        .connect
        .as_ref()
        .expect("offered")
        .states
        .lock()
        .expect("unpoisoned")
        .len();
    assert_eq!(held, 0, "no state is issued");
}
