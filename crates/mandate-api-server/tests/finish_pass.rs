//! The connect's finish pass (DEC-2342 items 3 and 10): what each pass appends to the control
//! stream, at which head, and which connects it tears down, in which order, on the first pass after
//! a start and on every pass after it.

mod common;

use std::sync::{Arc, Mutex};

use common::{MemJournal, Scripted};
use mandate_api_server::finish::{CONNECT_DEADLINE_SECS, Fingerprints, Judged, Unbacked};
use mandate_api_server::finish_pass::{
    Alerts, DeleteFailed, Deleted, Finisher, TEARDOWN_RUNBOOK, TEARDOWN_STALLED, Teardown,
};
use mandate_api_server::journal::Journal;
use mandate_canon::Digest;
use mandate_connections::manager::RequestMembers;
use mandate_connections::record::{AccountRef, ConnectionId};
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;
use serde_json::{Value, json};

const WORKSPACE: &str = "ws_01JB3K6F0C9R2V7N4M8Q1T5W3Z";
const A: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W50";
const B: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W51";
const C: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W52";
const D: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W53";
const E: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W54";
const OPENED: i64 = 1_791_000_000;
const CHECK: &str = "01JB3K6F0C9R2V7N4M8Q1T5W99";

fn ctl() -> String {
    format!("ctl:{WORKSPACE}")
}

/// The account stream of the request for `connection`: each has its own account.
fn acct(connection: &str) -> String {
    format!("acct:{WORKSPACE}:{}", account(connection))
}

fn account(connection: &str) -> String {
    format!(
        "01JB3K6F0C9R2V7N4M8Q1T5W6{}",
        &connection[connection.len() - 1..]
    )
}

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

/// Seeds `stream` with its next row.
fn seed(journal: &MemJournal, stream: &str, kind: &str, version: u64, secs: i64, payload: Value) {
    let mut streams = journal.streams.lock().unwrap();
    let rows = streams.entry(stream.to_owned()).or_default();
    let seq = u64::try_from(rows.len()).unwrap() + 1;
    rows.push(StoredEvent {
        stream_id: stream.to_owned(),
        seq,
        event_id: format!("{stream}-{seq}"),
        event_type: kind.to_owned(),
        schema_version: version,
        environment: "paper".to_owned(),
        recorded_at: at(secs).to_string(),
        prev_hash: Digest::ZERO,
        hash: Digest::of(format!("{stream}-{seq}").as_bytes()),
        body: serde_json::to_vec(&json!({ "payload": payload })).unwrap(),
    });
}

fn request(journal: &MemJournal, connection: &str, version: u64, credential: &str) {
    request_as(
        journal,
        connection,
        &account(connection),
        version,
        credential,
        OPENED,
    );
}

/// A request for `connection` on `account_ref`, recorded at `secs`: DEC-699 lets a refused
/// connection id be requested again with a new `account_ref`.
fn request_as(
    journal: &MemJournal,
    connection: &str,
    account_ref: &str,
    version: u64,
    credential: &str,
    secs: i64,
) {
    let mut payload = json!({
        "connection_id": connection,
        "account_ref": account_ref,
        "broker": "alpaca",
        "environment": "paper",
        "user": "01JB3K6F0C9R2V7N4M8Q1T5W3Z",
        "step_up": {
            "assertion_id": "asrt-1",
            "authenticated_at": at(secs - 10).to_string(),
            "method": "passkey",
        },
    });
    if version == 2 {
        payload["credential"] = json!(credential);
        payload["breadth"] = Value::Null;
    }
    seed(
        journal,
        &ctl(),
        "ConnectionRequested",
        version,
        secs,
        payload,
    );
}

fn close(journal: &MemJournal, kind: &str, connection: &str) {
    seed(
        journal,
        &ctl(),
        kind,
        1,
        OPENED + 1,
        json!({ "connection_id": connection }),
    );
}

/// The executor's `connect` check of `connection`, listing `account`, `environment` and `scope`
/// (rule 57): every result passed, or `scope` failed.
fn check(journal: &MemJournal, connection: &str, passed: bool) {
    check_on(journal, connection, &account(connection), passed);
}

/// The executor's `connect` check of `connection` on `account_ref`'s stream, as [`check`].
fn check_on(journal: &MemJournal, connection: &str, account_ref: &str, passed: bool) {
    let stream = format!("acct:{WORKSPACE}:{account_ref}");
    let scope = if passed {
        json!({"check": "scope", "reason": null, "result": "passed"})
    } else {
        json!({"check": "scope", "reason": "scope_mismatch", "result": "failed"})
    };
    let results = json!([
        {"check": "account", "reason": null, "result": "passed"},
        {"check": "environment", "reason": null, "result": "passed"},
        scope,
    ]);
    let mut streams = journal.streams.lock().unwrap();
    streams
        .entry(stream.clone())
        .or_default()
        .push(StoredEvent {
            stream_id: stream,
            seq: 1,
            event_id: CHECK.to_owned(),
            event_type: "ConnectionChecked".to_owned(),
            schema_version: 1,
            environment: "paper".to_owned(),
            recorded_at: at(OPENED + 30).to_string(),
            prev_hash: Digest::ZERO,
            hash: Digest::of(CHECK.as_bytes()),
            body: serde_json::to_vec(&json!({ "payload": {
                "connection_id": connection,
                "occasion": "connect",
                "results": results,
                "account_pii_ref": "pii_01JB3K6F0C9R2V7N4M8Q1T5W70",
                "risk_clock": at(OPENED + 30).to_string(),
            }}))
            .unwrap(),
        });
}

/// A fingerprint port that answers `Unique`.
struct Unique;

impl Fingerprints for Unique {
    fn judge(&self, _request: &RequestMembers, _account_pii_ref: &str) -> Judged {
        Judged::Unique
    }

    fn release(&self, _connection: &ConnectionId, _account_ref: &AccountRef) {}
}

/// A fingerprint port that answers `Unique` and records each release with the teardown's deletes
/// so far.
#[derive(Clone)]
struct Releases {
    deletes: Deletes,
    calls: Arc<Mutex<Vec<(String, String, usize)>>>,
}

impl Releases {
    fn take(&self) -> Vec<(String, String, usize)> {
        std::mem::take(&mut *self.calls.lock().unwrap())
    }
}

impl Fingerprints for Releases {
    fn judge(&self, _request: &RequestMembers, _account_pii_ref: &str) -> Judged {
        Judged::Unique
    }

    fn release(&self, connection: &ConnectionId, account_ref: &AccountRef) {
        let deletes = self.deletes.calls.lock().unwrap().len();
        self.calls.lock().unwrap().push((
            connection.as_str().to_owned(),
            account_ref.as_str().to_owned(),
            deletes,
        ));
    }
}

/// The alerts raised, each a code and a runbook.
#[derive(Clone, Default)]
struct Raised(Arc<Mutex<Vec<(&'static str, &'static str)>>>);

impl Raised {
    fn take(&self) -> Vec<(&'static str, &'static str)> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

impl Alerts for Raised {
    fn raise(&self, code: &'static str, runbook: &'static str) {
        self.0.lock().unwrap().push((code, runbook));
    }
}

/// A teardown that records each delete with the control stream's length when it ran, and fails
/// for the connections in `failing`, and for every connection once the control stream holds
/// `failing_from` rows.
#[derive(Clone)]
struct Deletes {
    journal: Arc<MemJournal>,
    calls: Arc<Mutex<Vec<(String, usize)>>>,
    failing: Arc<Mutex<Vec<String>>>,
    failing_from: Arc<Mutex<Option<usize>>>,
}

impl Deletes {
    fn new(journal: &Arc<MemJournal>) -> Self {
        Self {
            journal: Arc::clone(journal),
            calls: Arc::default(),
            failing: Arc::default(),
            failing_from: Arc::default(),
        }
    }

    fn take(&self) -> Vec<(String, usize)> {
        std::mem::take(&mut *self.calls.lock().unwrap())
    }

    fn of(&self) -> Vec<String> {
        self.take().into_iter().map(|(id, _)| id).collect()
    }
}

impl Teardown for Deletes {
    fn delete<'a>(&'a self, connection: &'a ConnectionId) -> Deleted<'a> {
        let length = self.journal.rows_of(&ctl()).len();
        self.calls
            .lock()
            .unwrap()
            .push((connection.as_str().to_owned(), length));
        let fails = self
            .failing
            .lock()
            .unwrap()
            .iter()
            .any(|id| id == connection.as_str())
            || self
                .failing_from
                .lock()
                .unwrap()
                .is_some_and(|rows| length >= rows);
        Box::pin(async move { if fails { Err(DeleteFailed) } else { Ok(()) } })
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
}

type Pass<F> = Finisher<F, Deletes, Raised>;

/// A journal, its teardown, and a finisher over them with `fingerprints`.
fn finisher<F: Fingerprints>(fingerprints: F) -> (Arc<MemJournal>, Deletes, Pass<F>) {
    let journal = Arc::new(MemJournal::default());
    let deletes = Deletes::new(&journal);
    let finisher = restarted(&journal, fingerprints, &deletes, &Raised::default());
    (journal, deletes, finisher)
}

/// A finisher over `journal` as a newly started process sees it.
fn restarted<F: Fingerprints>(
    journal: &Arc<MemJournal>,
    fingerprints: F,
    deletes: &Deletes,
    raised: &Raised,
) -> Pass<F> {
    let shared: Arc<dyn Journal> = Arc::clone(journal) as Arc<dyn Journal>;
    Finisher::new(
        shared,
        fingerprints,
        deletes.clone(),
        raised.clone(),
        WORKSPACE.to_owned(),
    )
}

/// A journal, its teardown, its alerts, and a finisher over them that records its releases.
fn recorded() -> (Arc<MemJournal>, Deletes, Raised, Releases, Pass<Releases>) {
    let journal = Arc::new(MemJournal::default());
    let deletes = Deletes::new(&journal);
    let releases = Releases {
        deletes: deletes.clone(),
        calls: Arc::default(),
    };
    let raised = Raised::default();
    let finisher = restarted(&journal, releases.clone(), &deletes, &raised);
    (journal, deletes, raised, releases, finisher)
}

fn pass<F: Fingerprints>(finisher: &mut Pass<F>, secs: i64) {
    runtime().block_on(finisher.pass(at(secs))).unwrap();
}

/// The payload of the control stream's last row, its type, and its `causation_id`.
fn last(journal: &MemJournal) -> (String, Value, Value) {
    let row = journal.rows_of(&ctl()).pop().unwrap();
    let body: Value = serde_json::from_slice(&row.body).unwrap();
    (
        row.event_type,
        body["payload"].clone(),
        body["causation_id"].clone(),
    )
}

#[test]
fn a_passed_check_judged_unique_is_established_at_the_head_read() {
    let (journal, deletes, mut finisher) = finisher(Unique);
    request(&journal, A, 2, "oauth");
    check(&journal, A, true);
    pass(&mut finisher, OPENED + 31);
    let (kind, payload, causation) = last(&journal);
    assert_eq!(kind, "ConnectionEstablished");
    assert_eq!(payload["connection_id"], A);
    assert_eq!(causation, CHECK);
    assert_eq!(
        journal.appends.lock().unwrap()[0].1,
        1,
        "the head the pass read"
    );
    assert_eq!(deletes.take(), [], "an establishment deletes nothing");
}

/// Item 3's order: deleted, then the refusal appended, then deleted again.
#[test]
fn a_refusal_is_deleted_before_and_after_it_is_appended() {
    let (journal, deletes, mut finisher) = finisher(Unique);
    request(&journal, A, 2, "oauth");
    check(&journal, A, false);
    pass(&mut finisher, OPENED + 31);
    let (kind, payload, causation) = last(&journal);
    assert_eq!(kind, "ConnectionRefused");
    assert_eq!(payload["reason"], "scope_mismatch");
    assert_eq!(causation, CHECK);
    assert_eq!(deletes.take(), [(A.to_owned(), 1), (A.to_owned(), 2)]);
    pass(&mut finisher, OPENED + 36);
    assert_eq!(deletes.take(), [], "deleted once the deletes succeed");
    assert_eq!(journal.rows_of(&ctl()).len(), 2, "nothing more appended");
}

#[test]
fn the_deadline_tears_the_connect_down_as_timeout() {
    let (journal, deletes, mut finisher) = finisher(Unbacked);
    request(&journal, A, 2, "oauth");
    check(&journal, A, true);
    pass(&mut finisher, OPENED + CONNECT_DEADLINE_SECS - 1);
    assert_eq!(journal.rows_of(&ctl()).len(), 1, "an unbacked port waits");
    pass(&mut finisher, OPENED + CONNECT_DEADLINE_SECS);
    let (kind, payload, causation) = last(&journal);
    assert_eq!(kind, "ConnectionRefused");
    assert_eq!(payload["reason"], "timeout");
    assert_eq!(causation, Value::Null);
    assert_eq!(deletes.take(), [(A.to_owned(), 1), (A.to_owned(), 2)]);
}

#[test]
fn the_first_pass_past_the_deadline_is_restart_past_deadline() {
    let (journal, _, mut finisher) = finisher(Unique);
    request(&journal, A, 2, "oauth");
    pass(&mut finisher, OPENED + CONNECT_DEADLINE_SECS);
    assert_eq!(last(&journal).1["reason"], "restart_past_deadline");
}

#[test]
fn a_control_stream_that_cannot_be_read_leaves_the_restart_pass_to_come() {
    let (journal, _, mut finisher) = finisher(Unique);
    request(&journal, A, 2, "oauth");
    journal.failing.lock().unwrap().insert(ctl());
    assert!(runtime().block_on(finisher.pass(at(OPENED + 1))).is_err());
    journal.failing.lock().unwrap().clear();
    pass(&mut finisher, OPENED + CONNECT_DEADLINE_SECS);
    assert_eq!(last(&journal).1["reason"], "restart_past_deadline");
}

/// Item 6: an account stream that cannot be read leaves the fold unanswered, past the deadline too.
#[test]
fn an_account_stream_that_cannot_be_read_appends_nothing() {
    let (journal, deletes, mut finisher) = finisher(Unique);
    request(&journal, A, 2, "oauth");
    journal.failing.lock().unwrap().insert(acct(A));
    pass(&mut finisher, OPENED + CONNECT_DEADLINE_SECS + 1);
    assert_eq!(journal.rows_of(&ctl()).len(), 1);
    assert_eq!(deletes.take(), []);
}

/// Item 5: a request that is not an `oauth` one is not this writer's.
#[test]
fn a_request_that_is_not_oauth_waits_past_its_deadline() {
    let (journal, _, mut finisher) = finisher(Unique);
    request(&journal, A, 1, "");
    request(&journal, B, 2, "api_key");
    pass(&mut finisher, OPENED + CONNECT_DEADLINE_SECS + 1);
    assert_eq!(journal.rows_of(&ctl()).len(), 2);
    assert!(journal.appends.lock().unwrap().is_empty());
}

#[test]
fn a_moved_head_appends_nothing_and_the_next_pass_retries() {
    let (journal, _, mut finisher) = finisher(Unique);
    request(&journal, A, 2, "oauth");
    check(&journal, A, true);
    journal.script(&[Scripted::HeadMismatch]);
    pass(&mut finisher, OPENED + 31);
    assert_eq!(journal.rows_of(&ctl()).len(), 1);
    pass(&mut finisher, OPENED + 36);
    assert_eq!(last(&journal).0, "ConnectionEstablished");
}

#[test]
fn two_finishes_in_one_pass_append_in_turn() {
    let (journal, _, mut finisher) = finisher(Unique);
    request(&journal, A, 2, "oauth");
    request(&journal, B, 2, "oauth");
    check(&journal, A, true);
    check(&journal, B, false);
    pass(&mut finisher, OPENED + 31);
    let heads: Vec<u64> = journal
        .appends
        .lock()
        .unwrap()
        .iter()
        .map(|(_, head, _)| *head)
        .collect();
    assert_eq!(heads, [2, 3]);
    let kinds: Vec<String> = journal
        .rows_of(&ctl())
        .into_iter()
        .map(|row| row.event_type)
        .collect();
    assert_eq!(
        kinds,
        [
            "ConnectionRequested",
            "ConnectionRequested",
            "ConnectionEstablished",
            "ConnectionRefused"
        ]
    );
}

/// A refusal that another writer appended first still has its deletes after it.
#[test]
fn a_refusal_already_committed_is_deleted_again() {
    let (journal, deletes, mut finisher) = finisher(Unique);
    request(&journal, A, 2, "oauth");
    check(&journal, A, false);
    journal.script(&[Scripted::RaceIdentical]);
    pass(&mut finisher, OPENED + 31);
    assert_eq!(deletes.take(), [(A.to_owned(), 1), (A.to_owned(), 2)]);
    pass(&mut finisher, OPENED + 36);
    assert_eq!(deletes.take(), []);
}

/// Item 3: with the vault down nothing is appended; once it recovers, the delete runs, then the
/// refusal is appended, then the delete runs again.
#[test]
fn a_vault_that_is_down_holds_the_refusal_until_its_first_delete_succeeds() {
    let (journal, deletes, mut finisher) = finisher(Unique);
    request(&journal, A, 2, "oauth");
    check(&journal, A, false);
    deletes.failing.lock().unwrap().push(A.to_owned());
    pass(&mut finisher, OPENED + 31);
    assert_eq!(deletes.take(), [(A.to_owned(), 1)]);
    pass(&mut finisher, OPENED + 36);
    assert_eq!(deletes.take(), [(A.to_owned(), 1)]);
    assert_eq!(journal.rows_of(&ctl()).len(), 1, "nothing appended");
    assert!(journal.appends.lock().unwrap().is_empty());
    deletes.failing.lock().unwrap().clear();
    pass(&mut finisher, OPENED + 41);
    assert_eq!(deletes.take(), [(A.to_owned(), 1), (A.to_owned(), 2)]);
    assert_eq!(last(&journal).0, "ConnectionRefused");
    pass(&mut finisher, OPENED + 46);
    assert_eq!(deletes.take(), []);
}

#[test]
fn a_failed_second_delete_is_retried_on_every_pass_until_it_succeeds() {
    let (journal, deletes, mut finisher) = finisher(Unique);
    request(&journal, A, 2, "oauth");
    check(&journal, A, false);
    *deletes.failing_from.lock().unwrap() = Some(2);
    pass(&mut finisher, OPENED + 31);
    assert_eq!(deletes.take(), [(A.to_owned(), 1), (A.to_owned(), 2)]);
    pass(&mut finisher, OPENED + 36);
    assert_eq!(deletes.of(), [A]);
    *deletes.failing_from.lock().unwrap() = None;
    pass(&mut finisher, OPENED + 41);
    assert_eq!(deletes.of(), [A]);
    pass(&mut finisher, OPENED + 46);
    assert_eq!(deletes.of(), Vec::<String>::new());
    assert_eq!(journal.rows_of(&ctl()).len(), 2, "nothing more appended");
}

/// DEC-699: a connection id refused and torn down, then requested again with a new `account_ref`,
/// has its second request torn down too, before and after its own refusal.
#[test]
fn a_second_request_on_a_torn_down_connection_is_torn_down_too() {
    let second = "01JB3K6F0C9R2V7N4M8Q1T5W6Z";
    let (journal, deletes, mut finisher) = finisher(Unique);
    request(&journal, A, 2, "oauth");
    check(&journal, A, false);
    pass(&mut finisher, OPENED + 31);
    assert_eq!(deletes.take(), [(A.to_owned(), 1), (A.to_owned(), 2)]);
    request_as(&journal, A, second, 2, "oauth", OPENED + 40);
    check_on(&journal, A, second, false);
    pass(&mut finisher, OPENED + 71);
    assert_eq!(last(&journal).0, "ConnectionRefused");
    assert_eq!(deletes.take(), [(A.to_owned(), 3), (A.to_owned(), 4)]);
    pass(&mut finisher, OPENED + 76);
    assert_eq!(deletes.take(), []);
}

/// The restart pass deletes each closed request's entries, two requests on one connection id
/// included.
#[test]
fn the_restart_pass_deletes_each_closed_request_on_one_connection() {
    let second = "01JB3K6F0C9R2V7N4M8Q1T5W6Z";
    let (journal, deletes, mut finisher) = finisher(Unique);
    request(&journal, A, 2, "oauth");
    close(&journal, "ConnectionRefused", A);
    request_as(&journal, A, second, 2, "oauth", OPENED + 40);
    close(&journal, "ConnectionRefused", A);
    pass(&mut finisher, OPENED + 71);
    assert_eq!(deletes.of(), [A, A]);
    pass(&mut finisher, OPENED + 76);
    assert_eq!(deletes.of(), Vec::<String>::new());
    assert_eq!(journal.rows_of(&ctl()).len(), 4, "nothing appended");
}

/// The restart pass deletes, once, every `oauth` connect a refusal or a revoke closed, and no
/// established connection's entries, nor another writer's request's.
#[test]
fn the_restart_pass_deletes_every_closed_connect_once() {
    let (journal, deletes, mut finisher) = finisher(Unique);
    for (connection, version, credential) in [
        (A, 2, "oauth"),
        (B, 2, "oauth"),
        (C, 2, "oauth"),
        (D, 2, "api_key"),
        (E, 2, "oauth"),
    ] {
        request(&journal, connection, version, credential);
    }
    close(&journal, "ConnectionRefused", A);
    close(&journal, "ConnectionRevoked", B);
    close(&journal, "ConnectionEstablished", C);
    close(&journal, "ConnectionRefused", D);
    close(&journal, "ConnectionEstablished", E);
    close(&journal, "ConnectionRevoked", E);
    pass(&mut finisher, OPENED + 31);
    assert_eq!(deletes.of(), [A, B]);
    pass(&mut finisher, OPENED + 36);
    assert_eq!(deletes.of(), Vec::<String>::new());
    assert_eq!(journal.rows_of(&ctl()).len(), 11, "nothing appended");
}

const STALLED: (&str, &str) = (TEARDOWN_STALLED, TEARDOWN_RUNBOOK);

/// DEC-2348 item 1: a refusal held by a vault that stays down raises the alert once, at the
/// deadline and not before, and appends nothing; once the vault recovers, delete, append, delete,
/// and no second alert.
#[test]
fn a_held_refusal_past_its_deadline_raises_one_alert() {
    let (journal, deletes, raised, _, mut finisher) = recorded();
    request(&journal, A, 2, "oauth");
    check(&journal, A, false);
    deletes.failing.lock().unwrap().push(A.to_owned());
    pass(&mut finisher, OPENED + 31);
    pass(&mut finisher, OPENED + CONNECT_DEADLINE_SECS - 1);
    assert_eq!(raised.take(), [], "within its deadline");
    pass(&mut finisher, OPENED + CONNECT_DEADLINE_SECS);
    assert_eq!(raised.take(), [STALLED]);
    pass(&mut finisher, OPENED + CONNECT_DEADLINE_SECS + 5);
    assert_eq!(raised.take(), [], "once per request");
    assert_eq!(journal.rows_of(&ctl()).len(), 1, "nothing appended");
    deletes.failing.lock().unwrap().clear();
    deletes.take();
    pass(&mut finisher, OPENED + CONNECT_DEADLINE_SECS + 10);
    assert_eq!(deletes.take(), [(A.to_owned(), 1), (A.to_owned(), 2)]);
    assert_eq!(last(&journal).0, "ConnectionRefused");
    assert_eq!(raised.take(), []);
}

/// A closed request whose deletes keep failing raises the alert once past its deadline, and a
/// restarted process raises it again.
#[test]
fn a_closed_request_whose_delete_keeps_failing_raises_one_alert_per_process() {
    let (journal, deletes, raised, releases, mut finisher) = recorded();
    request(&journal, A, 2, "oauth");
    close(&journal, "ConnectionRevoked", A);
    deletes.failing.lock().unwrap().push(A.to_owned());
    pass(&mut finisher, OPENED + 31);
    assert_eq!(raised.take(), []);
    pass(&mut finisher, OPENED + CONNECT_DEADLINE_SECS);
    assert_eq!(raised.take(), [STALLED]);
    pass(&mut finisher, OPENED + CONNECT_DEADLINE_SECS + 5);
    assert_eq!(raised.take(), []);
    let mut again = restarted(&journal, releases.clone(), &deletes, &raised);
    pass(&mut again, OPENED + CONNECT_DEADLINE_SECS + 10);
    assert_eq!(raised.take(), [STALLED], "a restart raises it again");
    assert_eq!(
        releases.take(),
        [],
        "nothing released while the delete fails"
    );
}

/// A delete that succeeds raises nothing, past the deadline too.
#[test]
fn a_teardown_that_succeeds_raises_nothing() {
    let (journal, _, raised, _, mut finisher) = recorded();
    request(&journal, A, 2, "oauth");
    request(&journal, B, 2, "oauth");
    close(&journal, "ConnectionRefused", B);
    pass(&mut finisher, OPENED + CONNECT_DEADLINE_SECS + 1);
    assert_eq!(last(&journal).1["reason"], "restart_past_deadline");
    assert_eq!(raised.take(), []);
}

/// DEC-2348 item 2: each closed request's fingerprint is released once its deletes succeeded,
/// once per process, and an establishment releases nothing.
#[test]
fn a_closed_request_is_released_after_its_deletes() {
    let (journal, deletes, _, releases, mut finisher) = recorded();
    request(&journal, A, 2, "oauth");
    request(&journal, B, 2, "oauth");
    check(&journal, A, false);
    check(&journal, B, true);
    pass(&mut finisher, OPENED + 31);
    assert_eq!(deletes.of(), [A, A]);
    assert_eq!(releases.take(), [(A.to_owned(), account(A), 2)]);
    pass(&mut finisher, OPENED + 36);
    assert_eq!(releases.take(), [], "once per process");
    let mut again = restarted(&journal, releases.clone(), &deletes, &Raised::default());
    pass(&mut again, OPENED + 41);
    assert_eq!(
        releases.take(),
        [(A.to_owned(), account(A), 1)],
        "a restart releases again, after its delete"
    );
}

/// The release follows a failed delete only once the delete succeeds, and each request on one
/// connection id is released on its own.
#[test]
fn each_request_on_one_connection_is_released_once_its_delete_succeeds() {
    let second = "01JB3K6F0C9R2V7N4M8Q1T5W6Z";
    let (journal, deletes, _, releases, mut finisher) = recorded();
    request(&journal, A, 2, "oauth");
    close(&journal, "ConnectionRefused", A);
    request_as(&journal, A, second, 2, "oauth", OPENED + 40);
    close(&journal, "ConnectionRevoked", A);
    deletes.failing.lock().unwrap().push(A.to_owned());
    pass(&mut finisher, OPENED + 71);
    assert_eq!(releases.take(), []);
    deletes.failing.lock().unwrap().clear();
    pass(&mut finisher, OPENED + 76);
    assert_eq!(
        releases.take(),
        [
            (A.to_owned(), account(A), 3),
            (A.to_owned(), second.to_owned(), 4)
        ]
    );
}
