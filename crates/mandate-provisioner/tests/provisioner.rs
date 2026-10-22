//! The role provisioner over a scripted control stream, vault and alerts (DEC-2350 items 1 and 3;
//! journal spec v0.77 §9.33). The stream's other records are the `connection_roles` vectors'; the
//! provisioner writes its own, and the journal's own fold, [`verify_connection_roles`], accepts
//! every stream it leaves. Before E21-33's S4b the plan and the pass are stubs, the answer these
//! tests fail on.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_connections::record::ConnectionId;
use mandate_deploy_manager::{Alerts, ControlJournal, Identity, OperatorAlert, Unavailable};
use mandate_journal::{AppendOutcome, Draft, StoredEvent, verify_connection_roles};
use mandate_provisioner::driver::{Halt, Provisioner, ROLES_REFUSED};
use mandate_provisioner::plan::{Occasion, Step, plan};
use mandate_provisioner::record::{ACTOR_ID, Outcome, draft, event_id};
use mandate_time::UtcNanos;
use mandate_vault::provisioner::{RoleProvisioner, RoleRefusal};

const CONNECTION: &str = "conn_alpaca_paper";
const NAMESPACE: &str = "ws-ws1";

fn identity() -> Identity {
    Identity {
        workspace: "ws_01J8Z2".to_owned(),
        actor_id: ACTOR_ID.to_owned(),
        version: "0.1.0".to_owned(),
        build: Digest::of(b"provisioner"),
    }
}

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(1_792_000_000 + secs, 0).unwrap()
}

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    parse(&std::fs::read(path).unwrap())
        .unwrap()
        .get("connection_roles")
        .cloned()
        .unwrap()
}

/// The records of the vectors' stream case `name` that are not the provisioner's own, in order.
fn world(name: &str) -> Vec<Value> {
    let section = section();
    let case = section
        .get("streams")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .find(|case| case.get("name").and_then(Value::as_str) == Some(name))
        .cloned()
        .unwrap();
    case.get("records")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .filter(|r| {
            !r.get("event_type")
                .and_then(Value::as_str)
                .unwrap()
                .starts_with("ConnectionRoles")
        })
        .cloned()
        .collect()
}

fn text<'a>(value: &'a Value, name: &str) -> &'a str {
    value.get(name).and_then(Value::as_str).unwrap_or_default()
}

/// The control stream in memory, shared by every writer that holds it: a row per record, the
/// journal's own parse on each append, and an `event_id` committed once. `moved` puts another
/// writer's record at the head just before the next append, which that append then misses.
#[derive(Clone, Default)]
struct Stream {
    rows: Rc<RefCell<Vec<StoredEvent>>>,
    moved: Rc<RefCell<Option<Vec<u8>>>>,
    down: Rc<RefCell<usize>>,
}

impl Stream {
    fn opened() -> Self {
        let stream = Self::default();
        stream.rows.borrow_mut().push(StoredEvent {
            stream_id: identity().stream(),
            seq: 1,
            event_id: "01J8Z4CS0000000000000OPEN0".to_owned(),
            event_type: "StreamOpened".to_owned(),
            schema_version: 1,
            environment: "paper".to_owned(),
            recorded_at: at(0).to_string(),
            prev_hash: Digest::of(b""),
            hash: Digest::of(b""),
            body: b"{}".to_vec(),
        });
        stream
    }

    fn push(&self, record: &Value) {
        let mut rows = self.rows.borrow_mut();
        let seq = u64::try_from(rows.len()).unwrap() + 1;
        rows.push(StoredEvent {
            stream_id: text(record, "stream_id").to_owned(),
            seq,
            event_id: text(record, "event_id").to_owned(),
            event_type: text(record, "event_type").to_owned(),
            schema_version: record
                .get("schema_version")
                .and_then(Value::as_int)
                .unwrap(),
            environment: text(record, "environment").to_owned(),
            recorded_at: at(0).to_string(),
            prev_hash: Digest::of(b""),
            hash: Digest::of(b""),
            body: to_canonical(record),
        });
    }

    fn rows(&self) -> Vec<StoredEvent> {
        self.rows.borrow().clone()
    }

    fn of(&self, event_type: &str) -> Vec<Value> {
        self.rows()
            .iter()
            .filter(|r| r.event_type == event_type)
            .map(|r| parse(&r.body).unwrap())
            .collect()
    }
}

impl ControlJournal for Stream {
    async fn rows_after(&mut self, seq: u64) -> Result<Vec<StoredEvent>, Unavailable> {
        Ok(self.rows().into_iter().filter(|r| r.seq > seq).collect())
    }

    async fn append(
        &mut self,
        draft: &[u8],
        _recorded_at: UtcNanos,
    ) -> Result<AppendOutcome, Unavailable> {
        if *self.down.borrow() > 0 {
            *self.down.borrow_mut() -= 1;
            return Err(Unavailable);
        }
        if let Some(other) = self.moved.borrow_mut().take() {
            self.push(&parse(&other).unwrap());
            return Ok(AppendOutcome::HeadMismatch {
                actual_seq: u64::try_from(self.rows.borrow().len()).unwrap(),
                actual_hash: Digest::of(b""),
            });
        }
        if let Err(error) = Draft::parse(draft) {
            return Ok(AppendOutcome::Invalid { draft: 0, error });
        }
        let record = parse(draft).unwrap();
        let id = text(&record, "event_id").to_owned();
        if let Some(stored) = self.rows().iter().find(|r| r.event_id == id) {
            return Ok(if stored.body == draft {
                AppendOutcome::AlreadyCommitted(vec![stored.clone()])
            } else {
                AppendOutcome::IdempotencyConflict {
                    stored_seq: stored.seq,
                }
            });
        }
        self.push(&record);
        Ok(AppendOutcome::Committed(Vec::new()))
    }
}

/// The vault as the provisioner's session sees it: what it was asked, and its answers.
#[derive(Default)]
struct Vault {
    made: RefCell<Vec<String>>,
    removed: RefCell<Vec<String>>,
    refusal: Option<RoleRefusal>,
    slow: bool,
}

impl RoleProvisioner for Vault {
    async fn make_roles(
        &self,
        connection: &ConnectionId,
        namespace: &str,
    ) -> Result<(), RoleRefusal> {
        assert_eq!(namespace, NAMESPACE);
        if self.slow {
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        self.made.borrow_mut().push(connection.as_str().to_owned());
        self.refusal.map_or(Ok(()), Err)
    }

    async fn remove_exchange_role(
        &self,
        connection: &ConnectionId,
        namespace: &str,
    ) -> Result<(), RoleRefusal> {
        assert_eq!(namespace, NAMESPACE);
        self.removed
            .borrow_mut()
            .push(connection.as_str().to_owned());
        Ok(())
    }
}

#[derive(Default)]
struct Raised(Vec<OperatorAlert>);

impl Alerts for Raised {
    fn raise(&mut self, alert: OperatorAlert) {
        self.0.push(alert);
    }
}

fn provisioner(stream: &Stream, vault: Vault) -> Provisioner<Stream, Vault, Raised> {
    Provisioner::new(
        identity(),
        NAMESPACE,
        stream.clone(),
        vault,
        Raised::default(),
    )
}

/// Each record the provisioner drafts is one the journal takes as §9.33's, and its `event_id` is
/// the step's own: the same for the same step, another for another.
#[test]
fn every_draft_parses_as_its_record_with_the_steps_own_event_id() {
    let make = Step::Make {
        connection: CONNECTION.to_owned(),
        request: "01J8Z4CS000000000000000000".to_owned(),
    };
    let remove = Step::Remove {
        connection: CONNECTION.to_owned(),
        cause: "01J8Z4CS020000000000000000".to_owned(),
        occasion: Occasion::RequestClosed,
    };
    let cases = [
        (&make, Outcome::Made),
        (&make, Outcome::Refused(RoleRefusal::VaultUnavailable)),
        (&make, Outcome::Refused(RoleRefusal::RoleRefused)),
        (&remove, Outcome::Removed),
    ];
    let mut ids = BTreeSet::new();
    for (step, outcome) in cases {
        let bytes = draft(&identity(), step, outcome, "paper", at(1));
        let parsed = Draft::parse(&bytes).unwrap();
        assert_eq!(parsed.event_type(), outcome.event_type());
        assert_eq!(
            parsed.event_id(),
            event_id(&identity(), step, outcome),
            "{outcome:?}"
        );
        assert_ne!(
            draft(&identity(), step, outcome, "paper", at(2)),
            bytes,
            "the time moves, the id stays"
        );
        ids.insert(event_id(&identity(), step, outcome));
    }
    assert_eq!(ids.len(), 3, "a refusal's id is the same for either reason");
    assert_eq!(ROLES_REFUSED.code, "connection_roles_refused");
    assert_eq!(ROLES_REFUSED.runbook, "RB-12");
}

/// A version-2 OAuth request gets both its roles made and one `ConnectionRolesMade` naming it, and
/// a second pass does nothing more.
#[tokio::test(flavor = "current_thread")]
async fn a_request_gets_its_roles_once() {
    let stream = Stream::opened();
    for record in world("made") {
        stream.push(&record);
    }
    let mut provisioner = provisioner(&stream, Vault::default());
    assert_eq!(provisioner.tick(at(1)).await, Ok(()));
    assert_eq!(provisioner.tick(at(2)).await, Ok(()));
    assert_eq!(provisioner.vault().made.borrow().as_slice(), [CONNECTION]);
    let made = stream.of("ConnectionRolesMade");
    assert_eq!(made.len(), 1);
    let request = &world("made")[0];
    assert_eq!(text(&made[0], "causation_id"), text(request, "event_id"));
    assert_eq!(verify_connection_roles(&stream.rows()), Ok(()));
}

/// A refused or slow vault refuses the request with its reason and raises the opaque alert, and no
/// later pass makes the roles for it again.
#[tokio::test(flavor = "current_thread")]
async fn a_refusal_or_a_slow_vault_is_journaled_once_and_alerted() {
    for (vault, reason) in [
        (
            Vault {
                refusal: Some(RoleRefusal::RoleRefused),
                ..Vault::default()
            },
            "role_refused",
        ),
        (
            Vault {
                slow: true,
                ..Vault::default()
            },
            "vault_unavailable",
        ),
    ] {
        let stream = Stream::opened();
        for record in world("made") {
            stream.push(&record);
        }
        let mut provisioner = provisioner(&stream, vault).with_deadline(Duration::from_millis(50));
        assert_eq!(provisioner.tick(at(1)).await, Ok(()), "{reason}");
        assert_eq!(provisioner.tick(at(2)).await, Ok(()), "{reason}");
        let refused = stream.of("ConnectionRolesRefused");
        assert_eq!(refused.len(), 1, "{reason}");
        assert_eq!(
            refused[0]
                .get("payload")
                .and_then(|p| p.get("reason"))
                .and_then(Value::as_str),
            Some(reason)
        );
        assert_eq!(stream.of("ConnectionRolesMade").len(), 0, "{reason}");
        assert_eq!(provisioner.alerts().0, [ROLES_REFUSED], "{reason}");
        assert_eq!(verify_connection_roles(&stream.rows()), Ok(()), "{reason}");
    }
}

/// Once a request whose roles were made closes without an establishment, or its connection is
/// revoked, the exchange's role is removed once, for that closer, with its occasion; the executor's
/// is never removed: the vault is asked to remove the exchange's alone.
#[tokio::test(flavor = "current_thread")]
async fn a_close_or_a_revocation_removes_only_the_exchanges_role_once() {
    for (case, occasion) in [
        ("removed_on_close", "request_closed"),
        ("revoked_exchange_at_once", "revoked"),
    ] {
        let stream = Stream::opened();
        let mut provisioner = provisioner(&stream, Vault::default());
        let records = world(case);
        stream.push(&records[0]);
        assert_eq!(provisioner.tick(at(1)).await, Ok(()), "{case}");
        for record in &records[1..] {
            stream.push(record);
        }
        for second in 2..4 {
            assert_eq!(provisioner.tick(at(second)).await, Ok(()), "{case}");
        }
        assert_eq!(
            provisioner.vault().removed.borrow().as_slice(),
            [CONNECTION],
            "{case}"
        );
        assert_eq!(
            provisioner.vault().made.borrow().as_slice(),
            [CONNECTION],
            "{case}"
        );
        let removed = stream.of("ConnectionRolesRemoved");
        assert_eq!(removed.len(), 1, "{case}");
        let payload = removed[0].get("payload").unwrap();
        assert_eq!(text(payload, "role"), "token_exchange");
        assert_eq!(text(payload, "occasion"), occasion, "{case}");
        let closer = records.last().unwrap();
        assert_eq!(
            text(&removed[0], "causation_id"),
            text(closer, "event_id"),
            "{case}"
        );
        assert_eq!(verify_connection_roles(&stream.rows()), Ok(()), "{case}");
    }
}

/// A journal that is down after the vault made the roles leaves the step to the next pass, which
/// makes them again, with nothing changed, and records them once.
#[tokio::test(flavor = "current_thread")]
async fn a_step_the_journal_missed_is_done_again_and_recorded_once() {
    let stream = Stream::opened();
    for record in world("made") {
        stream.push(&record);
    }
    *stream.down.borrow_mut() = 1;
    let mut provisioner = provisioner(&stream, Vault::default());
    assert_eq!(provisioner.tick(at(1)).await, Err(Halt::Journal));
    assert_eq!(stream.of("ConnectionRolesMade").len(), 0);
    assert_eq!(provisioner.tick(at(2)).await, Ok(()));
    assert_eq!(
        provisioner.vault().made.borrow().as_slice(),
        [CONNECTION, CONNECTION]
    );
    assert_eq!(stream.of("ConnectionRolesMade").len(), 1);
    assert_eq!(verify_connection_roles(&stream.rows()), Ok(()));
}

/// Two provisioners race on one stream: another writer's record lands between a provisioner's read
/// and its append, the same step's or another's. The provisioner reads the stream again, records
/// what is still owed, and no record is lost or written twice.
#[tokio::test(flavor = "current_thread")]
async fn two_writers_racing_lose_no_record_and_write_none_twice() {
    let records = world("two_requests");
    let (first, second) = (&records[0], &records[1]);
    let step = |request: &Value| Step::Make {
        connection: text(request.get("payload").unwrap(), "connection_id").to_owned(),
        request: text(request, "event_id").to_owned(),
    };
    for (raced, name) in [(first, "the same step"), (second, "another step")] {
        let stream = Stream::opened();
        stream.push(first);
        stream.push(second);
        *stream.moved.borrow_mut() = Some(draft(
            &identity(),
            &step(raced),
            Outcome::Made,
            "paper",
            at(0),
        ));
        let mut provisioner = provisioner(&stream, Vault::default());
        assert_eq!(provisioner.tick(at(1)).await, Ok(()), "{name}");
        let made = stream.of("ConnectionRolesMade");
        let causes: BTreeSet<&str> = made.iter().map(|m| text(m, "causation_id")).collect();
        assert_eq!(made.len(), 2, "{name}");
        assert_eq!(
            causes,
            [text(first, "event_id"), text(second, "event_id")]
                .into_iter()
                .collect(),
            "{name}"
        );
        assert_eq!(verify_connection_roles(&stream.rows()), Ok(()), "{name}");
    }
}

/// A stream with no `StreamOpened` gives no record its environment, so nothing is done.
#[tokio::test(flavor = "current_thread")]
async fn an_unopened_stream_does_nothing() {
    let stream = Stream::default();
    for record in world("made") {
        stream.push(&record);
    }
    let mut provisioner = provisioner(&stream, Vault::default());
    assert_eq!(provisioner.tick(at(1)).await, Err(Halt::NotOpened));
    assert!(provisioner.vault().made.borrow().is_empty());
}

/// The plan reads §9.33 as the journal's fold does: on every valid stream case of the vectors,
/// with the provisioner's records in it, nothing is owed; without them, exactly the steps those
/// records answer are, in the stream's order, and never an executor's removal, which no step is.
#[test]
fn the_plan_owes_exactly_what_the_valid_streams_record() {
    let section = section();
    for case in section.get("streams").and_then(Value::as_array).unwrap() {
        if text(case.get("expect").unwrap(), "outcome") != "Valid" {
            continue;
        }
        let name = text(case, "name");
        let full = Stream::opened();
        let bare = Stream::opened();
        let mut owed = Vec::new();
        for record in case.get("records").and_then(Value::as_array).unwrap() {
            full.push(record);
            let payload = record.get("payload").unwrap();
            let connection = text(payload, "connection_id").to_owned();
            let cause = text(record, "causation_id").to_owned();
            match text(record, "event_type") {
                "ConnectionRolesMade" | "ConnectionRolesRefused" => owed.push(Step::Make {
                    connection,
                    request: cause,
                }),
                "ConnectionRolesRemoved" => owed.push(Step::Remove {
                    connection,
                    cause,
                    occasion: match text(payload, "occasion") {
                        "revoked" => Occasion::Revoked,
                        _ => Occasion::RequestClosed,
                    },
                }),
                _ => bare.push(record),
            }
        }
        let done = plan(&full.rows()).unwrap();
        assert_eq!(done.environment.as_deref(), Some("paper"), "{name}");
        assert!(done.steps.is_empty(), "{name}: {:?}", done.steps);
        let removals = owed
            .iter()
            .filter(|step| matches!(step, Step::Remove { .. }))
            .count();
        if removals == 0 {
            assert_eq!(plan(&bare.rows()).unwrap().steps, owed, "{name}");
        }
    }
}
