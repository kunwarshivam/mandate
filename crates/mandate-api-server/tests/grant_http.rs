//! A paper grant made in the web app with `cli_confirm` step-up over HTTP (E9-4 S6,
//! [DEC-2104], DEC-2103 item 7, DEC-2605 row 5, workspace API spec §5.2): the challenge comes from
//! the challenge route, the owner's `StepUpConfirmed` is on the control stream as `mandate step-up
//! confirm` appends it, the server's real binding judges the evidence, and the grant is committed
//! as the one `ApprovalResponseSubmitted` `mandate approvals approve` writes (journal spec §9.7).
//! Each refusal of DEC-2104 item 6 commits nothing and keeps the challenge; a principal the bound
//! mandate does not list in `autonomy.approval.approvers` is `forbidden`, and a mandate or
//! membership that cannot be read fails closed ([DEC-2129] item 7); the checks answer in item 1's
//! order; a `live` workspace is `live_unavailable`; a repeat under the key answers the same; and a
//! Skip is unchanged.
//!
//! The binding is wrapped in a recorder, so a failure shows what the port was asked and answered.
//! The oracles are the test's own: the code is encoded here from the challenge's bytes, the
//! expected payload is the CLI's member list written out here by hand (`approvals::response`, then
//! `control::commit`'s `submitted_at` and `step_up`), and each expected refusal is the one the case
//! arranged.
//!
//! [DEC-2104]: ../../../docs/project/decisions/DEC-2104.md
//! [DEC-2129]: ../../../docs/project/decisions/DEC-2129.md

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::control::{envelope, stored};
use common::{MemJournal, Reply, WORKSPACE, hosted, send, serve};
use mandate_api::envelope::{StepUpAssertion, StepUpEvidence};
use mandate_api::idempotency::{Derivation, IdempotencyKey, event_id};
use mandate_api::problem::{Problem, ProblemCode};
use mandate_api::wire::{Id, decode};
use mandate_api_server::challenges::Challenges;
use mandate_api_server::config::{SESSION_KEY_FILE, read_session_key};
use mandate_api_server::confirm::CliConfirm;
use mandate_api_server::http::{self, App, Settings};
use mandate_api_server::lifecycle::Drain;
use mandate_api_server::methods::{Methods, Passkey};
use mandate_api_server::objects::{ObjectStore, Objects};
use mandate_api_server::session::{COOKIE_NAME, OsRandom, Sessions};
use mandate_api_server::stepup::{Action, Asserted, Judged, StepUp, Unverified};
use mandate_api_server::subject::{LinkRefusal, NamesPrincipal};
use mandate_api_server::token::{FixedKeys, Issuer};
use mandate_authn::{Algorithm, IssuerConfig};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_identity::{MembershipEvent, MembershipRecord, PrincipalId, Role, StepUpActionKind};
use mandate_identity_seal::Seal;
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;
use serde_json::json;

const APP: &str = "https://app.owlhead.test";
const T0: i64 = 1_791_000_000;
const KEY: &str = "k1Z-9_grant00001";
const CHALLENGE_KEY: &str = "k1Z-9_challenge1";
const ALICE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Z";
const BOB: &str = "01JB3K6F0C9R2V7N4M8Q1T5W41";
const APPROVAL: &str = "01JBH3BTZ8K2M4N6P8R0S2T7AA";
const AGENT: &str = "agt_a";
/// A challenge id the server never issued.
const UNISSUED: &str = "01J9ZQ4B7Y8K3M5N6P7Q8R9S0V";
/// The approval's deadline, as its risk-clock second.
const DEADLINE: i64 = T0 + 600;
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The binding as the server holds it, with every question and answer the port gave kept.
struct Recorded {
    inner: Methods,
    asked: Arc<Mutex<Vec<(StepUpActionKind, Digest, String)>>>,
}

impl Recorded {
    fn keep(&self, action: &Action, answer: String) {
        self.asked
            .lock()
            .expect("unpoisoned")
            .push((action.kind, action.digest, answer));
    }
}

impl StepUp for Recorded {
    fn verify(
        &self,
        evidence: &StepUpEvidence,
        action: &Action,
        judged: &Judged<'_>,
    ) -> Result<(), ProblemCode> {
        let answer = self.inner.verify(evidence, action, judged);
        self.keep(action, format!("verify: {answer:?}"));
        answer
    }

    fn asserted(
        &self,
        evidence: &StepUpEvidence,
        assertion: Option<&StepUpAssertion>,
        action: &Action,
        judged: &Judged<'_>,
    ) -> Result<Option<Asserted>, Unverified> {
        let answer = self.inner.asserted(evidence, assertion, action, judged);
        self.keep(action, format!("asserted: {answer:?}"));
        answer
    }
}

/// Subject links that name Alice and Bob while `.0` holds, and nobody once it is cleared.
struct Linked(Arc<AtomicBool>);

impl NamesPrincipal for Linked {
    fn names(&self, who: PrincipalId) -> Result<bool, LinkRefusal> {
        Ok(self.0.load(Ordering::SeqCst) && (who == principal(ALICE) || who == principal(BOB)))
    }
}

type Asked = Arc<Mutex<Vec<(StepUpActionKind, Digest, String)>>>;

struct World {
    addr: std::net::SocketAddr,
    journal: Arc<MemJournal>,
    sessions: Arc<Sessions>,
    clock: Arc<common::ManualClock>,
    challenges: Arc<Challenges>,
    asked: Asked,
    linked: Arc<AtomicBool>,
    records: Arc<Mutex<Vec<MembershipRecord>>>,
    /// The approval's content hash.
    hash: String,
}

/// What the bound mandate version is to the store, for [DEC-2129] item 7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bound {
    /// Stored, and a mandate.
    Stored,
    /// Named, and not in the store.
    Absent,
    /// Stored, and not a mandate.
    NotAMandate,
    /// The content names no version.
    Unnamed,
    /// The deployment has no artifact store.
    NoStore,
}

/// How a world is set up: its environment, the mandate's approvers, Alice's roles (Bob is an
/// approver alone), and what the bound version is to the store.
struct Setup<'a> {
    environment: &'a str,
    approvers: &'a [&'a str],
    alice: &'a [Role],
    bound: Bound,
}

const PAPER: Setup<'static> = Setup {
    environment: "paper",
    approvers: &["role:approver"],
    alice: &[Role::Approver],
    bound: Bound::Stored,
};

static DIRS: Mutex<u32> = Mutex::new(0);

/// A workspace whose control stream is opened in `setup.environment`, with `agt_a` deployed and
/// one pending approval on its stream bound to a mandate listing `setup.approvers`; Alice and Bob
/// both hold the approver role, and both are linked.
fn world_with(setup: &Setup<'_>) -> World {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mandate-api-server-grant-{}-{n}",
        std::process::id()
    ));
    drop(n);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let file = dir.join(SESSION_KEY_FILE);
    std::fs::write(&file, [0x47_u8; 32]).expect("the key");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("mode");
    let key = read_session_key(&dir).expect("the key reads");
    let journal = Arc::new(MemJournal::default());
    let records = Arc::new(Mutex::new(vec![
        activated(1, principal(ALICE), setup.alice),
        activated(2, principal(BOB), &[Role::Approver]),
    ]));
    let read_records = Arc::clone(&records);
    let sessions = Arc::new(Sessions::new(&key, Box::new(OsRandom::new())));
    let clock = common::ManualClock::at(T0);
    let config = IssuerConfig::new(
        "https://issuer.test/auth/v1",
        &["authenticated"],
        &[Algorithm::Es256],
    )
    .expect("the issuer");
    let objects = Arc::new(Objects::new(BTreeMap::<Digest, Vec<u8>>::new()));
    let document = document(setup.approvers);
    let version = match setup.bound {
        Bound::Stored | Bound::NoStore => Some(reference(&document)),
        Bound::Absent => Some(reference(b"{}")),
        Bound::NotAMandate => Some(reference(&canonical(&json!({"name": "not a mandate"})))),
        Bound::Unnamed => None,
    };
    let held = match setup.bound {
        Bound::NotAMandate => canonical(&json!({"name": "not a mandate"})),
        _ => document,
    };
    objects.put(&held).expect("stored");
    let content = content(version.as_deref());
    let hash = reference(&canonical(&content));
    let challenges = Arc::new(Challenges::default());
    let asked: Asked = Arc::default();
    let linked = Arc::new(AtomicBool::new(true));
    let step_up = Arc::new(Recorded {
        inner: Methods {
            passkey: Passkey {
                challenges: Arc::clone(&challenges),
                rp: mandate_passkey::RelyingParty {
                    rp_id: "app.owlhead.test".to_owned(),
                    origin: APP.to_owned(),
                },
                keys: None,
            },
            cli_confirm: CliConfirm {
                challenges: Arc::clone(&challenges),
                links: Arc::new(Linked(Arc::clone(&linked))),
            },
        },
        asked: Arc::clone(&asked),
    });
    let app = App {
        settings: Settings {
            app_origin: APP.to_owned(),
        },
        sessions: Arc::clone(&sessions),
        tokens: Arc::new(Issuer::new(config, Arc::new(FixedKeys(None)))),
        links: Arc::new(common::LinksAt(dir.join("absent"))),
        clock: Arc::clone(&clock) as Arc<dyn mandate_api_server::clock::Clock>,
        journal: Arc::clone(&journal) as Arc<dyn mandate_api_server::journal::Journal>,
        members: Arc::new(move |_: &[StoredEvent]| {
            Ok(read_records.lock().expect("unpoisoned").clone())
        }),
        hosted: hosted(),
        artifacts: (setup.bound != Bound::NoStore).then(|| {
            Arc::clone(&objects) as Arc<dyn mandate_journal::ArtifactSource + Send + Sync>
        }),
        objects: None,
        step_up,
        challenges: Arc::clone(&challenges),
        connect: None,
        reduction: Arc::default(),
    };
    let w = World {
        addr: serve(http::api(app, Drain::new())),
        journal,
        sessions,
        clock,
        challenges,
        asked,
        linked,
        records,
        hash,
    };
    w.seed(setup.environment, &content);
    w
}

fn world() -> World {
    world_with(&PAPER)
}

fn world_in(environment: &str) -> World {
    world_with(&Setup {
        environment,
        ..PAPER
    })
}

fn principal(id: &str) -> PrincipalId {
    PrincipalId::from_ulid_text(id).expect("a ULID")
}

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).expect("a time")
}

fn activated(seq: u64, member: PrincipalId, roles: &[Role]) -> MembershipRecord {
    MembershipRecord::new(
        Seal::grant(),
        seq,
        at(T0 - 1_000),
        MembershipEvent::Activated {
            member,
            invitation: None,
            roles: roles.iter().copied().collect::<BTreeSet<_>>(),
            independent_approval_required: false,
            cool_off_ends_at: at(T0 - 1_000),
        },
    )
}

fn ctl() -> String {
    format!("ctl:{WORKSPACE}")
}

fn agent_stream() -> String {
    format!("agent:{WORKSPACE}:{AGENT}")
}

/// A mandate document listing `approvers`, from the fixture every read test stores.
fn document(approvers: &[&str]) -> Vec<u8> {
    let mut document: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/mandate.json")).expect("JSON");
    document["autonomy"]["approval"]["approvers"] = json!(approvers);
    canonical(&document)
}

fn canonical(value: &serde_json::Value) -> Vec<u8> {
    to_canonical(&parse(&serde_json::to_vec(value).expect("JSON")).expect("canonical"))
}

fn reference(bytes: &[u8]) -> String {
    format!("sha256:{}", Digest::of(bytes).to_hex())
}

/// The approval's content object, as the runtime binds it to the mandate version `version`.
fn content(version: Option<&str>) -> serde_json::Value {
    let mut content = json!({
        "action": {"instrument": "AAPL", "side": "buy", "qty": "3", "limit": "190.00"},
        "trigger": {"mandate_version": version, "decided_by": "rule"},
    });
    if version.is_none() {
        content["trigger"] = json!({"decided_by": "rule"});
    }
    content
}

/// A hash no request on the stream has.
fn other_hash() -> String {
    format!("sha256:{}", "cd".repeat(32))
}

/// The request's members a grant names: the challenge's id, bytes, and `issued_at`.
struct Issued {
    id: String,
    bytes: Vec<u8>,
    issued_at: String,
}

impl World {
    fn seed(&self, environment: &str, content: &serde_json::Value) {
        let control = [
            ("StreamOpened", json!({"stream_type": "control"})),
            (
                "AgentDeployed",
                json!({"agent_id": AGENT, "mandate_version": "sha256:01"}),
            ),
        ];
        let rows = control
            .into_iter()
            .enumerate()
            .map(|(i, (kind, payload))| {
                let seq = u64::try_from(i).expect("small") + 1;
                stored(
                    seq,
                    &envelope(&ctl(), seq, environment, kind, T0 - 900, payload),
                )
            })
            .collect();
        let mut requested = envelope(
            &agent_stream(),
            1,
            environment,
            "ApprovalRequested",
            T0 - 60,
            json!({"content": content, "content_hash": self.hash, "deadline": DEADLINE}),
        );
        requested["event_id"] = json!(APPROVAL);
        let mut streams = self.journal.streams.lock().expect("unpoisoned");
        streams.insert(ctl(), rows);
        streams.insert(agent_stream(), vec![stored(1, &requested)]);
    }

    fn cookie(&self, who: &str) -> String {
        let value = self
            .sessions
            .open(
                principal(who),
                "sha256:aaaa".to_owned(),
                None,
                at(self.clock.secs()),
            )
            .expect("opened")
            .value()
            .to_owned();
        format!("{COOKIE_NAME}={value}")
    }

    fn post(&self, path: &str, who: &str, key: &str, body: &[u8]) -> Reply {
        let cookie = self.cookie(who);
        send(
            self.addr,
            "POST",
            path,
            &[
                ("Origin", APP),
                ("X-Mandate-Request", "1"),
                ("Content-Type", "application/json"),
                ("Cookie", &cookie),
                ("Idempotency-Key", key),
            ],
            body,
        )
    }

    /// `who`'s `approve` challenge bound to `hash`, from the challenge route.
    fn challenge_by(&self, who: &str, hash: &str) -> Issued {
        let body = format!(r#"{{"action":{{"kind":"approve","digest":"{hash}"}}}}"#);
        let reply = self.post(
            &format!("/v1/workspaces/{WORKSPACE}/step-up/challenges"),
            who,
            CHALLENGE_KEY,
            body.as_bytes(),
        );
        assert_eq!(reply.status, 201, "{}", lossy(&reply));
        let answer = parse(&reply.body).expect("JSON");
        let text = |name: &str| {
            answer
                .get(name)
                .and_then(Value::as_str)
                .expect("a text member")
                .to_owned()
        };
        Issued {
            id: text("challenge_id"),
            bytes: URL_SAFE_NO_PAD
                .decode(text("webauthn_challenge"))
                .expect("base64url"),
            issued_at: text("issued_at"),
        }
    }

    /// Alice's challenge for the approval's own content hash.
    fn challenge(&self) -> Issued {
        self.challenge_by(ALICE, &self.hash)
    }

    /// Appends one control-stream record of `kind` at `secs`.
    fn append(&self, kind: &str, secs: i64, payload: serde_json::Value) {
        let mut streams = self.journal.streams.lock().expect("unpoisoned");
        let rows = streams.get_mut(&ctl()).expect("the control stream");
        let seq = u64::try_from(rows.len()).expect("small") + 1;
        rows.push(stored(
            seq,
            &envelope(&ctl(), seq, "paper", kind, secs, payload),
        ));
    }

    /// The `StepUpConfirmed` `mandate step-up confirm` appends for `issued` (DEC-2104 items 2
    /// and 3), with `edit` applied to its payload.
    fn confirm_with(&self, issued: &Issued, edit: impl FnOnce(&mut serde_json::Value)) {
        let mut payload = json!({
            "challenge": issued.id, "action_kind": "approve", "action_digest": self.hash,
            "principal": ALICE, "code": crockford40(&issued.bytes), "confirmed_at": T0 + 2,
        });
        edit(&mut payload);
        self.append("StepUpConfirmed", T0 + 2, payload);
    }

    fn confirm(&self, issued: &Issued) {
        self.confirm_with(issued, |_| {});
    }

    fn respond(&self, who: &str, key: &str, body: &[u8]) -> Reply {
        self.post(&responses(APPROVAL), who, key, body)
    }

    /// The control stream's approval responses, as their parsed bodies.
    fn answers(&self) -> Vec<Value> {
        self.journal
            .rows_of(&ctl())
            .iter()
            .filter(|row| row.event_type == "ApprovalResponseSubmitted")
            .map(|row| parse(&row.body).expect("JSON"))
            .collect()
    }

    fn held(&self, issued: &Issued) -> bool {
        self.challenges.held(&issued.id).is_some()
    }

    fn asked(&self) -> Vec<(StepUpActionKind, Digest, String)> {
        self.asked.lock().expect("unpoisoned").clone()
    }

    /// The approval's content hash as the digest a challenge binds.
    fn digest(&self) -> Digest {
        Digest::from_hex(self.hash.strip_prefix("sha256:").expect("a reference")).expect("hex")
    }

    /// What the port answered, for a failure message.
    fn said(&self) -> String {
        format!("{:?}", self.asked())
    }
}

fn responses(approval: &str) -> String {
    format!("/v1/workspaces/{WORKSPACE}/approvals/{approval}/responses")
}

fn lossy(reply: &Reply) -> String {
    String::from_utf8_lossy(&reply.body).into_owned()
}

/// The test's own Crockford base-32 of the first 40 bits of `bytes` (DEC-2104 item 1).
fn crockford40(bytes: &[u8]) -> String {
    let bits = bytes
        .iter()
        .take(5)
        .fold(0u64, |acc, byte| (acc << 8) | u64::from(*byte));
    (0..8)
        .rev()
        .map(|i| char::from(CROCKFORD[usize::try_from((bits >> (i * 5)) & 31).expect("5 bits")]))
        .collect()
}

/// A grant of `hash` with `cli_confirm` evidence for `issued`, authenticated at `authenticated_at`
/// (DEC-2104 item 5).
fn grant_at(issued: &Issued, hash: &str, authenticated_at: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "verdict": "approved",
        "content_hash": hash,
        "record": {"artifact": format!("sha256:{}", "ef".repeat(32)), "ui_build": format!("sha256:{}", "12".repeat(32))},
        "step_up": {"assertion_id": issued.id, "authenticated_at": authenticated_at, "method": "cli_confirm"},
        "delegation": null,
    }))
    .expect("JSON")
}

fn grant(w: &World, issued: &Issued) -> Vec<u8> {
    grant_at(issued, &w.hash, &issued.issued_at)
}

fn skipped(w: &World) -> Vec<u8> {
    format!(
        r#"{{"verdict":"skipped","content_hash":"{}","step_up":null,"delegation":null}}"#,
        w.hash
    )
    .into_bytes()
}

/// §3.4's id for `operation` under `key`, computed by `mandate-api`.
fn derived(operation: &str, principal: &str, key: &str) -> String {
    let workspace = Id::try_from(WORKSPACE.to_owned()).expect("an id");
    let principal = Id::try_from(principal.to_owned()).expect("an id");
    let key = IdempotencyKey::parse(key).expect("a key");
    let id = event_id(&Derivation {
        workspace: &workspace,
        principal: &principal,
        operation,
        key: &key,
        position: None,
    })
    .expect("an id");
    parse(&mandate_api::wire::encode(&id).expect("JSON"))
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .expect("a JSON string")
}

fn code(reply: &Reply) -> (u16, ProblemCode) {
    let problem = decode::<Problem>(&reply.body)
        .unwrap_or_else(|_| panic!("a problem, not {} {}", reply.status, lossy(reply)));
    assert_eq!(problem.status, reply.status);
    (reply.status, problem.code)
}

/// The payload `mandate approvals approve` commits for Alice's grant of the approval with
/// `cli_confirm` evidence naming `challenge` at second `second`, answered at `second` too: the
/// members of `approvals::response`, then `control::commit`'s `submitted_at` and `step_up`, whose
/// `authenticated_at` is an integer second (journal spec §9.7).
fn clis_grant(w: &World, challenge: &str, second: i64) -> Value {
    let text = json!({
        "agent": AGENT,
        "approval": APPROVAL,
        "verdict": "approved",
        "content_hash": w.hash,
        "responder": ALICE,
        "role": "approver",
        "submitted_at": second,
        "step_up": {"assertion_id": challenge, "authenticated_at": second, "method": "cli_confirm"},
    });
    parse(&serde_json::to_vec(&text).expect("JSON")).expect("canonical")
}

/// The reply is the `202` of a recorded response, else the failure names what the port said.
fn recorded(w: &World, reply: &Reply) -> Value {
    assert_eq!(
        reply.status,
        202,
        "{} after the port answered {}",
        lossy(reply),
        w.said()
    );
    decode::<mandate_api::responses::ApprovalResponseAccepted>(&reply.body)
        .expect("an ApprovalResponseAccepted");
    parse(&reply.body).expect("JSON")
}

/// The reply is the `401` refusal `expected`, nothing was committed, and the challenge is still
/// held, else the failure names what the port said.
fn refused(w: &World, reply: &Reply, expected: ProblemCode, case: &str) {
    assert_eq!(
        (
            reply.status,
            decode::<Problem>(&reply.body).ok().map(|p| p.code)
        ),
        (401, Some(expected)),
        "{case}: {} after the port answered {}",
        lossy(reply),
        w.said()
    );
    assert!(w.answers().is_empty(), "{case}: nothing is committed");
}

#[test]
#[ignore = "pending E9-4"]
fn a_confirmed_grant_commits_the_clis_answer_once_before_its_202() {
    let w = world();
    let issued = w.challenge();
    w.confirm(&issued);
    let reply = w.respond(ALICE, KEY, &grant(&w, &issued));
    let answer = recorded(&w, &reply);
    let id = derived("approval_response", ALICE, KEY);
    assert_eq!(
        answer.get("response_id").and_then(Value::as_str),
        Some(id.as_str())
    );
    assert_eq!(
        answer.get("phase").and_then(Value::as_str),
        Some("recorded")
    );
    let answers = w.answers();
    assert_eq!(answers.len(), 1, "exactly one response");
    let row = &answers[0];
    assert_eq!(
        row.get("payload"),
        Some(&clis_grant(&w, &issued.id, T0)),
        "the payload is the CLI's, member for member"
    );
    assert_eq!(
        to_canonical(row.get("payload").expect("a payload")),
        to_canonical(&clis_grant(&w, &issued.id, T0)),
        "and byte for byte"
    );
    let member = |path: &[&str]| {
        path.iter()
            .try_fold(row, |v, k| v.get(k))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    assert_eq!(member(&["event_id"]), Some(id));
    assert_eq!(
        member(&["event_type"]).as_deref(),
        Some("ApprovalResponseSubmitted")
    );
    assert_eq!(member(&["stream_id"]), Some(ctl()));
    assert_eq!(member(&["environment"]).as_deref(), Some("paper"));
    assert_eq!(member(&["actor", "kind"]).as_deref(), Some("user"));
    assert_eq!(member(&["actor", "id"]).as_deref(), Some(ALICE));
    assert_eq!(
        row.get("artifact_refs"),
        Some(&Value::Array(vec![Value::Str(w.hash.clone())])),
        "the content hash is the payload's one digest reference"
    );
    assert!(!w.held(&issued), "the challenge is spent with the grant");
}

#[test]
#[ignore = "pending E9-4"]
fn the_grant_is_judged_as_an_approve_bound_to_the_requests_content_hash() {
    let w = world();
    let issued = w.challenge();
    w.confirm(&issued);
    recorded(&w, &w.respond(ALICE, KEY, &grant(&w, &issued)));
    let asked: Vec<(StepUpActionKind, Digest)> = w
        .asked()
        .into_iter()
        .map(|(kind, digest, _)| (kind, digest))
        .collect();
    assert_eq!(asked, [(StepUpActionKind::Approve, w.digest())]);

    let w = world();
    let elsewhere = w.challenge_by(ALICE, &other_hash());
    w.confirm_with(&elsewhere, |p| p["action_digest"] = json!(other_hash()));
    let reply = w.respond(
        ALICE,
        KEY,
        &grant_at(&elsewhere, &w.hash, &elsewhere.issued_at),
    );
    refused(
        &w,
        &reply,
        ProblemCode::StepUpMismatch,
        "a challenge for another hash",
    );
    let digests: Vec<Digest> = w.asked().into_iter().map(|(_, digest, _)| digest).collect();
    assert_eq!(digests, [w.digest()], "the port judges the request's hash");
    assert!(w.held(&elsewhere));
}

/// One DEC-2104 item 6 refusal: how the stream and the grant are arranged, and its code.
struct Case {
    name: &'static str,
    code: ProblemCode,
    arrange: fn(&World, &Issued) -> Vec<u8>,
}

const CASES: [Case; 11] = [
    Case {
        name: "check 2: a challenge the server never issued",
        code: ProblemCode::StepUpMissing,
        arrange: |w, issued| {
            w.confirm(issued);
            let unissued = Issued {
                id: UNISSUED.to_owned(),
                bytes: issued.bytes.clone(),
                issued_at: issued.issued_at.clone(),
            };
            grant(w, &unissued)
        },
    },
    Case {
        name: "check 3: Bob's challenge presented by Alice",
        code: ProblemCode::StepUpMismatch,
        arrange: |w, _| {
            let bobs = w.challenge_by(BOB, &w.hash);
            w.confirm_with(&bobs, |p| p["principal"] = json!(BOB));
            grant(w, &bobs)
        },
    },
    Case {
        name: "check 4: authenticated at another second than the challenge's issue",
        code: ProblemCode::StepUpStale,
        arrange: |w, issued| {
            w.confirm(issued);
            grant_at(issued, &w.hash, "2026-10-17T10:00:00.000000000Z")
        },
    },
    Case {
        name: "check 5: an earlier event already carries the challenge",
        code: ProblemCode::StepUpReused,
        arrange: |w, issued| {
            w.confirm(issued);
            w.append(
                "OwnerCommandIssued",
                T0 + 3,
                json!({"agent": AGENT, "command": "resume", "step_up": {
                    "assertion_id": issued.id, "authenticated_at": T0, "method": "cli_confirm"}}),
            );
            grant(w, issued)
        },
    },
    Case {
        name: "check 6: no confirmation",
        code: ProblemCode::StepUpMissing,
        arrange: |w, issued| grant(w, issued),
    },
    Case {
        name: "check 6: two confirmations",
        code: ProblemCode::StepUpMissing,
        arrange: |w, issued| {
            w.confirm(issued);
            w.confirm(issued);
            grant(w, issued)
        },
    },
    Case {
        name: "check 7: confirmed for another digest",
        code: ProblemCode::StepUpMismatch,
        arrange: |w, issued| {
            w.confirm_with(issued, |p| p["action_digest"] = json!(other_hash()));
            grant(w, issued)
        },
    },
    Case {
        name: "check 7: confirmed by another principal",
        code: ProblemCode::StepUpMismatch,
        arrange: |w, issued| {
            w.confirm_with(issued, |p| p["principal"] = json!(BOB));
            grant(w, issued)
        },
    },
    Case {
        name: "check 7: a principal the subject link no longer names",
        code: ProblemCode::StepUpMissing,
        arrange: |w, issued| {
            w.confirm(issued);
            w.linked.store(false, Ordering::SeqCst);
            grant(w, issued)
        },
    },
    Case {
        name: "check 8: confirmed before the challenge was issued",
        code: ProblemCode::StepUpStale,
        arrange: |w, issued| {
            w.confirm_with(issued, |p| p["confirmed_at"] = json!(T0 - 1));
            grant(w, issued)
        },
    },
    Case {
        name: "check 9: another code",
        code: ProblemCode::StepUpMissing,
        arrange: |w, issued| {
            let shown = crockford40(&issued.bytes);
            let wrong = if shown.starts_with('0') { "1" } else { "0" };
            w.confirm_with(issued, |p| {
                p["code"] = json!(format!("{wrong}{}", &shown[1..]));
            });
            grant(w, issued)
        },
    },
];

#[test]
#[ignore = "pending E9-4"]
fn each_refusal_of_dec_2104_item_6_commits_nothing_and_keeps_the_challenge() {
    for case in &CASES {
        let w = world();
        let issued = w.challenge();
        let body = (case.arrange)(&w, &issued);
        let reply = w.respond(ALICE, KEY, &body);
        refused(&w, &reply, case.code, case.name);
        assert!(w.held(&issued), "{}: the challenge is kept", case.name);
        assert_eq!(w.asked().len(), 1, "{}: the port is asked once", case.name);
    }
}

#[test]
#[ignore = "pending E9-4"]
fn a_hash_other_than_the_journals_is_a_mismatch_since_the_cli_confirms_the_journal() {
    let w = world();
    let issued = w.challenge_by(ALICE, &other_hash());
    w.confirm(&issued);
    let reply = w.respond(
        ALICE,
        KEY,
        &grant_at(&issued, &other_hash(), &issued.issued_at),
    );
    refused(&w, &reply, ProblemCode::StepUpMismatch, "content mismatch");
    assert!(w.held(&issued));
}

#[test]
#[ignore = "pending E9-4"]
fn an_answered_or_late_approval_is_recorded_for_the_runtime_to_refuse() {
    let w = world();
    w.append(
        "ApprovalResponseSubmitted",
        T0 - 30,
        json!({"agent": AGENT, "approval": APPROVAL, "verdict": "skipped",
               "content_hash": w.hash, "submitted_at": T0 - 30, "step_up": null,
               "responder": BOB, "role": "approver"}),
    );
    let issued = w.challenge();
    w.confirm(&issued);
    let answered = recorded(&w, &w.respond(ALICE, KEY, &grant(&w, &issued)));
    assert!(answered.get("response_id").is_some());
    assert_eq!(w.answers().len(), 2, "the runtime judges it pending or not");

    let w = world();
    w.clock.set(DEADLINE - 10);
    let issued = w.challenge();
    w.confirm_with(&issued, |p| p["confirmed_at"] = json!(DEADLINE - 9));
    w.clock.set(DEADLINE + 5);
    let reply = w.respond(ALICE, KEY, &grant(&w, &issued));
    recorded(&w, &reply);
    let answers = w.answers();
    assert_eq!(
        answers[0]
            .get("payload")
            .and_then(|p| p.get("submitted_at")),
        clis_grant(&w, &issued.id, DEADLINE + 5).get("submitted_at"),
        "recorded at receipt, past the deadline, for the runtime to judge"
    );
}

#[test]
#[ignore = "pending E9-4"]
fn an_unknown_approval_is_404_before_the_port_is_asked() {
    let w = world();
    let issued = w.challenge();
    w.confirm(&issued);
    let reply = w.post(
        &responses("01JBH3BTZ8K2M4N6P8R0S2T7AB"),
        ALICE,
        KEY,
        &grant(&w, &issued),
    );
    assert_eq!(code(&reply), (404, ProblemCode::NotFound), "{}", w.said());
    assert!(w.asked().is_empty(), "the port is never asked");
    assert!(w.held(&issued));
    let granted = w.respond(ALICE, "k1Z-9_grant00002", &grant(&w, &issued));
    recorded(&w, &granted);
}

#[test]
#[ignore = "pending E9-4"]
fn a_live_workspace_is_live_unavailable_after_the_row_and_reads_no_evidence() {
    let w = world_in("live");
    let evidence = Issued {
        id: UNISSUED.to_owned(),
        bytes: vec![0; 32],
        issued_at: "2026-09-21T10:40:00.000000000Z".to_owned(),
    };
    let reply = w.respond(ALICE, KEY, &grant(&w, &evidence));
    assert_eq!(
        code(&reply),
        (409, ProblemCode::LiveUnavailable),
        "after the port answered {}",
        w.said()
    );
    assert!(w.asked().is_empty(), "no evidence is read");
    assert!(w.answers().is_empty());
    w.records.lock().expect("unpoisoned")[0] = activated(1, principal(ALICE), &[Role::Viewer]);
    let viewer = w.respond(ALICE, "k1Z-9_grant00002", &grant(&w, &evidence));
    assert_eq!(
        code(&viewer),
        (403, ProblemCode::Forbidden),
        "the row first"
    );
}

#[test]
#[ignore = "pending E9-4"]
fn a_repeat_under_the_key_answers_the_same_though_the_challenge_is_spent() {
    let w = world();
    let issued = w.challenge();
    w.confirm(&issued);
    let first = recorded(&w, &w.respond(ALICE, KEY, &grant(&w, &issued)));
    w.clock.set(T0 + 1);
    let again = recorded(&w, &w.respond(ALICE, KEY, &grant(&w, &issued)));
    assert_eq!(first.get("response_id"), again.get("response_id"));
    assert_eq!(w.answers().len(), 1, "a repeat commits nothing");
    assert_eq!(
        w.asked().len(),
        1,
        "a repeat replays before the port is asked"
    );
    let skip = w.respond(ALICE, KEY, &skipped(&w));
    assert_eq!(code(&skip), (409, ProblemCode::IdempotencyConflict));
    assert_eq!(w.answers().len(), 1);
}

#[test]
fn a_skip_is_unchanged_beside_the_grant_and_asks_no_port() {
    for environment in ["paper", "live"] {
        let w = world_in(environment);
        let reply = w.respond(ALICE, KEY, &skipped(&w));
        recorded(&w, &reply);
        let answers = w.answers();
        assert_eq!(answers.len(), 1, "{environment}");
        let payload = answers[0].get("payload").expect("a payload");
        assert_eq!(
            payload.get("verdict").and_then(Value::as_str),
            Some("skipped")
        );
        assert_eq!(payload.get("step_up"), Some(&Value::Null));
        assert!(w.asked().is_empty(), "{environment}: a Skip asks no port");
    }
}

#[test]
fn a_grant_without_evidence_is_still_step_up_required() {
    let w = world();
    let body = serde_json::to_vec(&json!({
        "verdict": "approved", "content_hash": w.hash,
        "record": {"artifact": format!("sha256:{}", "ef".repeat(32)), "ui_build": format!("sha256:{}", "12".repeat(32))},
        "step_up": null, "delegation": null,
    }))
    .expect("JSON");
    let reply = w.respond(ALICE, KEY, &body);
    assert_eq!(code(&reply), (401, ProblemCode::StepUpRequired));
    assert!(w.asked().is_empty());
    assert!(w.answers().is_empty());
}

#[test]
#[ignore = "pending E9-4"]
fn a_grant_naming_a_delegation_is_invalid_and_asks_no_port() {
    let w = world();
    let issued = w.challenge();
    w.confirm(&issued);
    let mut body: serde_json::Value = serde_json::from_slice(&grant(&w, &issued)).expect("JSON");
    body["delegation"] = json!({"preview_id": UNISSUED, "mandate_version": other_hash()});
    let reply = w.respond(ALICE, KEY, &serde_json::to_vec(&body).expect("JSON"));
    assert_eq!(
        code(&reply),
        (422, ProblemCode::Invalid),
        "§5.3's batch is not served: {}",
        w.said()
    );
    assert!(w.asked().is_empty(), "the port is never asked");
    assert!(w.answers().is_empty());
    assert!(w.held(&issued));
}

/// Bob's grant, confirmed and otherwise valid, in `w`.
fn bobs_grant(w: &World) -> (Issued, Vec<u8>) {
    let bobs = w.challenge_by(BOB, &w.hash);
    w.confirm_with(&bobs, |p| p["principal"] = json!(BOB));
    let body = grant(w, &bobs);
    (bobs, body)
}

#[test]
#[ignore = "pending E9-4"]
fn a_principal_the_mandate_does_not_list_is_forbidden_and_appends_nothing() {
    let listed = format!("user:{ALICE}");
    let w = world_with(&Setup {
        approvers: &[listed.as_str()],
        ..PAPER
    });
    let (bobs, body) = bobs_grant(&w);
    let reply = w.respond(BOB, KEY, &body);
    assert_eq!(
        code(&reply),
        (403, ProblemCode::Forbidden),
        "after the port answered {}",
        w.said()
    );
    assert!(w.asked().is_empty(), "no evidence is read");
    assert!(w.answers().is_empty());
    assert!(w.held(&bobs));
}

#[test]
#[ignore = "pending E9-4"]
fn an_approver_listed_by_user_id_is_recorded() {
    let listed = format!("user:{ALICE}");
    let w = world_with(&Setup {
        approvers: &[listed.as_str()],
        ..PAPER
    });
    let issued = w.challenge();
    w.confirm(&issued);
    recorded(&w, &w.respond(ALICE, KEY, &grant(&w, &issued)));
    assert_eq!(w.answers().len(), 1);
}

#[test]
#[ignore = "pending E9-4"]
fn an_approver_listed_by_role_is_recorded_and_a_member_without_the_role_is_not() {
    let roles: [(&str, &[Role]); 3] = [
        ("role:approver", &[Role::Approver]),
        (
            "role:workspace_admin",
            &[Role::Approver, Role::WorkspaceAdmin],
        ),
        ("role:owner", &[Role::Approver, Role::OrgOwner]),
    ];
    for (listed, alice) in roles {
        let w = world_with(&Setup {
            approvers: &[listed],
            alice,
            ..PAPER
        });
        let issued = w.challenge();
        w.confirm(&issued);
        recorded(&w, &w.respond(ALICE, KEY, &grant(&w, &issued)));
        assert_eq!(w.answers().len(), 1, "{listed}");
        let (_, body) = bobs_grant(&w);
        let bob = w.respond(BOB, "k1Z-9_grant00002", &body);
        let expected = if listed == "role:approver" { 202 } else { 403 };
        assert_eq!(
            bob.status,
            expected,
            "{listed}: {} {}",
            lossy(&bob),
            w.said()
        );
    }
}

#[test]
#[ignore = "pending E9-4"]
fn a_bound_mandate_that_cannot_be_read_fails_closed_and_reads_no_evidence() {
    for bound in [
        Bound::Absent,
        Bound::NotAMandate,
        Bound::Unnamed,
        Bound::NoStore,
    ] {
        let w = world_with(&Setup { bound, ..PAPER });
        let issued = w.challenge();
        w.confirm(&issued);
        let reply = w.respond(ALICE, KEY, &grant(&w, &issued));
        assert_eq!(
            code(&reply),
            (503, ProblemCode::JournalUnavailable),
            "{bound:?}: {}",
            w.said()
        );
        assert!(
            w.asked().is_empty(),
            "{bound:?}: no evidence is read: {}",
            w.said()
        );
        assert!(w.answers().is_empty(), "{bound:?}");
        assert!(w.held(&issued), "{bound:?}");
    }
}

#[test]
#[ignore = "pending E9-4"]
fn the_first_failing_check_in_dec_2129_item_1s_order_answers() {
    let listed = format!("user:{ALICE}");
    let only_alice = Setup {
        approvers: &[listed.as_str()],
        ..PAPER
    };
    let delegated = |body: &[u8]| {
        let mut body: serde_json::Value = serde_json::from_slice(body).expect("JSON");
        body["delegation"] = json!({"preview_id": UNISSUED, "mandate_version": other_hash()});
        serde_json::to_vec(&body).expect("JSON")
    };
    let bare = |w: &World| {
        let mut body: serde_json::Value =
            serde_json::from_slice(&grant_at(&placeholder(), &w.hash, "2026-10-17T10:00:00Z"))
                .expect("JSON");
        body["step_up"] = serde_json::Value::Null;
        serde_json::to_vec(&body).expect("JSON")
    };

    let w = world_with(&only_alice);
    let (_, body) = bobs_grant(&w);
    let unknown = w.post(&responses("01JBH3BTZ8K2M4N6P8R0S2T7AB"), BOB, KEY, &body);
    assert_eq!(
        code(&unknown),
        (404, ProblemCode::NotFound),
        "404 before the approvers: {}",
        w.said()
    );
    let both = w.respond(BOB, KEY, &delegated(&body));
    assert_eq!(
        code(&both),
        (403, ProblemCode::Forbidden),
        "approvers before /delegation: {}",
        w.said()
    );

    let w = world_with(&Setup {
        environment: "live",
        ..only_alice
    });
    let body = grant(&w, &placeholder());
    let both = w.respond(BOB, KEY, &body);
    assert_eq!(
        code(&both),
        (403, ProblemCode::Forbidden),
        "approvers before live: {}",
        w.said()
    );
    let both = w.respond(ALICE, KEY, &delegated(&body));
    assert_eq!(
        code(&both),
        (422, ProblemCode::Invalid),
        "/delegation before live: {}",
        w.said()
    );
    let both = w.respond(ALICE, "k1Z-9_grant00002", &bare(&w));
    assert_eq!(
        code(&both),
        (409, ProblemCode::LiveUnavailable),
        "live before step-up: {}",
        w.said()
    );
    assert!(w.asked().is_empty());

    let w = world();
    let both = w.respond(ALICE, KEY, &delegated(&bare(&w)));
    assert_eq!(
        code(&both),
        (422, ProblemCode::Invalid),
        "/delegation before step-up: {}",
        w.said()
    );
    let w = world_with(&Setup {
        bound: Bound::Absent,
        ..PAPER
    });
    let both = w.respond(ALICE, KEY, &delegated(&bare(&w)));
    assert_eq!(
        code(&both),
        (503, ProblemCode::JournalUnavailable),
        "the approvers' read before /delegation: {}",
        w.said()
    );
    assert!(w.answers().is_empty());
}

#[test]
fn a_failed_membership_read_refuses_a_grant_at_the_row() {
    let w = world();
    let issued = w.challenge();
    w.confirm(&issued);
    w.journal.failing.lock().expect("unpoisoned").insert(ctl());
    let reply = w.respond(ALICE, KEY, &grant(&w, &issued));
    assert_eq!(code(&reply).0, 503);
    w.journal.failing.lock().expect("unpoisoned").remove(&ctl());
    assert!(w.answers().is_empty());
    assert!(w.asked().is_empty());
}

/// Evidence naming a challenge the server never issued.
fn placeholder() -> Issued {
    Issued {
        id: UNISSUED.to_owned(),
        bytes: vec![0; 32],
        issued_at: "2026-09-21T10:40:00.000000000Z".to_owned(),
    }
}
