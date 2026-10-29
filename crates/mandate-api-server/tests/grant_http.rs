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

use std::collections::BTreeMap;
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
use mandate_api_server::members::control_parser;
use mandate_api_server::methods::{Methods, Passkey};
use mandate_api_server::objects::{ObjectStore, Objects};
use mandate_api_server::session::{COOKIE_NAME, OsRandom, Sessions};
use mandate_api_server::stepup::{Action, Asserted, Judged, StepUp, Unverified};
use mandate_api_server::subject::{LinkRefusal, NamesPrincipal};
use mandate_api_server::token::{FixedKeys, Issuer};
use mandate_authn::{Algorithm, IssuerConfig};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_identity::{OWNER_BUNDLE, PrincipalId, Role, StepUpActionKind, WorkspaceId};
use mandate_time::UtcNanos;
use serde_json::json;

const APP: &str = "https://app.owlhead.test";
const T0: i64 = 1_791_000_000;
const KEY: &str = "k1Z-9_grant00001";
const CHALLENGE_KEY: &str = "k1Z-9_challenge1";
const ALICE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Z";
const BOB: &str = "01JB3K6F0C9R2V7N4M8Q1T5W41";
const CAROL: &str = "01JB3K6F0C9R2V7N4M8Q1T5W42";
const BOB_INVITATION: &str = "01JB3K6F0C9R2V7N4M8Q1T5W51";
const CAROL_INVITATION: &str = "01JB3K6F0C9R2V7N4M8Q1T5W52";
/// An approver invited and activated whose enrolment cool-off has not ended: `cooling_off`.
const DAVE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W43";
const DAVE_INVITATION: &str = "01JB3K6F0C9R2V7N4M8Q1T5W53";
const APPROVAL: &str = "01JBH3BTZ8K2M4N6P8R0S2T7AA";
/// The `DecisionMade` the approval names as its cause, as the runtime writes the two (DEC-2519).
const DECISION: &str = "01JBH3BTZ8K2M4N6P8R0S2T7A9";
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

/// How a world is set up: its environment, the mandate's approvers, the roles of Alice's founding
/// grant (Bob is an invited approver, Carol an invited viewer), and what the bound version is to
/// the store.
struct Setup<'a> {
    environment: &'a str,
    approvers: &'a [&'a str],
    alice: &'a [Role],
    bound: Bound,
}

const PAPER: Setup<'static> = Setup {
    environment: "paper",
    approvers: &["role:approver"],
    alice: &OWNER_BUNDLE,
    bound: Bound::Stored,
};

static DIRS: Mutex<u32> = Mutex::new(0);

/// A workspace whose control stream is opened in `setup.environment`, with `agt_a` deployed and
/// one pending approval on its stream bound to a mandate listing `setup.approvers`; the members are
/// [`World::seed`]'s, and Alice and Bob are linked.
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
                cool_off_waiver: None,
                cool_off_waiver_until: None,
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
        members: Arc::new(control_parser(
            WorkspaceId::from_ulid_text(WORKSPACE).expect("a ULID"),
        )),
        hosted: hosted(),
        artifacts: (setup.bound != Bound::NoStore).then(|| {
            Arc::clone(&objects) as Arc<dyn mandate_journal::ArtifactSource + Send + Sync>
        }),
        objects: None,
        step_up,
        challenges: Arc::clone(&challenges),
        connect: None,
        reduction: Arc::default(),
        folds: Arc::default(),
        bars: None,
        chat: Default::default(),
    };
    let w = World {
        addr: serve(http::api(app, Drain::new())),
        journal,
        sessions,
        clock,
        challenges,
        asked,
        linked,
        hash,
    };
    w.seed(setup, &content);
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
    /// The control stream as `mandate workspace found` and two invitations leave it: Alice's
    /// founding `MemberActivated` with `setup.alice`'s roles, then Bob invited and activated as an
    /// approver, Carol as a viewer, and Dave as an approver still cooling off (journal spec §9.12), then `agt_a` deployed; and the approval
    /// on `agt_a`'s stream.
    fn seed(&self, setup: &Setup<'_>, content: &serde_json::Value) {
        let codes = |roles: &[Role]| -> Vec<&'static str> {
            let mut codes: Vec<&str> = roles.iter().filter_map(|role| role.code()).collect();
            codes.sort_unstable();
            codes
        };
        let since = at(T0 - 1_000).to_string();
        let invited = |invitation: &str, roles: &[Role]| {
            json!({"invitation": invitation, "roles": codes(roles),
                   "expires_at": at(T0 + 86_400).to_string(), "invited_by": ALICE})
        };
        let accepted = |member: &str, invitation: &str, roles: &[Role]| {
            json!({"member": member, "invitation": invitation, "reason": "invitation_accepted",
                   "roles": codes(roles), "independent_approval_required": false,
                   "cool_off_ends_at": since, "session_ref": null})
        };
        let mut cooling = accepted(DAVE, DAVE_INVITATION, &[Role::Approver]);
        cooling["cool_off_ends_at"] = json!(at(T0 + 86_400).to_string());
        let control = [
            ("StreamOpened", json!({"stream_type": "control"})),
            (
                "MemberActivated",
                json!({"member": ALICE, "invitation": null, "reason": "founding",
                       "roles": codes(setup.alice), "independent_approval_required": false,
                       "cool_off_ends_at": since, "session_ref": null}),
            ),
            ("MemberInvited", invited(BOB_INVITATION, &[Role::Approver])),
            (
                "MemberActivated",
                accepted(BOB, BOB_INVITATION, &[Role::Approver]),
            ),
            ("MemberInvited", invited(CAROL_INVITATION, &[Role::Viewer])),
            (
                "MemberActivated",
                accepted(CAROL, CAROL_INVITATION, &[Role::Viewer]),
            ),
            ("MemberInvited", invited(DAVE_INVITATION, &[Role::Approver])),
            ("MemberActivated", cooling),
            (
                "AgentDeployed",
                json!({"agent_id": AGENT, "mandate_version": "sha256:01"}),
            ),
        ];
        let environment = setup.environment;
        let rows = control
            .into_iter()
            .enumerate()
            .map(|(i, (kind, payload))| {
                let seq = u64::try_from(i).expect("small") + 1;
                stored(
                    seq,
                    &envelope(&ctl(), seq, environment, kind, T0 - 1_000, payload),
                )
            })
            .collect();
        let mut streams = self.journal.streams.lock().expect("unpoisoned");
        streams.insert(ctl(), rows);
        drop(streams);
        self.decided(environment, content, Some((1, json!({"dry_run": "allow"}))));
    }

    /// `agt_a`'s stream: the decision, at `version` with `payload`, or none when `None`, then the
    /// approval naming it as its cause, at version 1 with `content`.
    fn decided(
        &self,
        environment: &str,
        content: &serde_json::Value,
        decision: Option<(u64, serde_json::Value)>,
    ) {
        self.requested(environment, content, (1, Some(DECISION)), decision);
    }

    /// `agt_a`'s stream: the decision, at `version` with `payload`, or none when `None`, then the
    /// approval at `request.0` with `content`, naming `request.1` as its cause.
    fn requested(
        &self,
        environment: &str,
        content: &serde_json::Value,
        request: (u64, Option<&str>),
        decision: Option<(u64, serde_json::Value)>,
    ) {
        let mut rows = Vec::new();
        if let Some((version, payload)) = decision {
            let mut decided = envelope(
                &agent_stream(),
                1,
                environment,
                "DecisionMade",
                T0 - 61,
                payload,
            );
            decided["event_id"] = json!(DECISION);
            decided["schema_version"] = json!(version);
            rows.push(stored(1, &decided));
        }
        let mut requested = envelope(
            &agent_stream(),
            2,
            environment,
            "ApprovalRequested",
            T0 - 60,
            json!({"content": content, "content_hash": self.hash, "deadline": DEADLINE}),
        );
        requested["event_id"] = json!(APPROVAL);
        requested["schema_version"] = json!(request.0);
        if let Some(cause) = request.1 {
            requested["causation_id"] = json!(cause);
        }
        rows.push(stored(2, &requested));
        let mut streams = self.journal.streams.lock().expect("unpoisoned");
        streams.insert(agent_stream(), rows);
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

/// A reply's status and problem code, compared with `(status, code)`; its `Debug` carries the
/// whole body, so a failed comparison shows what the server answered.
struct Coded(u16, Option<ProblemCode>, String);

impl std::fmt::Debug for Coded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {:?}: {}", self.0, self.1, self.2)
    }
}

impl PartialEq<(u16, ProblemCode)> for Coded {
    fn eq(&self, other: &(u16, ProblemCode)) -> bool {
        self.0 == other.0 && self.1 == Some(other.1)
    }
}

fn code(reply: &Reply) -> Coded {
    let problem = decode::<Problem>(&reply.body).ok();
    if let Some(problem) = &problem {
        assert_eq!(problem.status, reply.status);
    }
    Coded(reply.status, problem.map(|p| p.code), lossy(reply))
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

/// A challenge issued part-way through a second (DEC-2129 item 3): the grant journals its
/// `authenticated_at`, and its `submitted_at`, as the whole second, truncated and never later than
/// the instant verified, so the payload is still the CLI's for that second.
#[test]
fn a_grant_authenticated_part_way_through_a_second_journals_the_second_it_began() {
    let w = world();
    w.clock.set_nanos(999_999_999);
    let issued = w.challenge();
    assert!(
        issued.issued_at.ends_with(".999999999Z"),
        "the challenge carries the nanoseconds: {}",
        issued.issued_at
    );
    w.confirm(&issued);
    recorded(&w, &w.respond(ALICE, KEY, &grant(&w, &issued)));
    let answers = w.answers();
    assert_eq!(answers.len(), 1, "exactly one response");
    assert_eq!(
        answers[0].get("payload"),
        Some(&clis_grant(&w, &issued.id, T0)),
        "the whole second, T0, never T0 + 1"
    );
}

#[test]
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
    let viewer = w.respond(CAROL, "k1Z-9_grant00002", &grant(&w, &evidence));
    assert_eq!(
        code(&viewer),
        (403, ProblemCode::Forbidden),
        "the row first"
    );
}

#[test]
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

/// A grant of a request whose deferred checks are not exactly its decision's, or whose decision is
/// not on the stream, is refused `deferral_mismatch` before the port is asked: nothing is
/// committed and the challenge stays unspent. The role is judged first, and a Skip of the same
/// request is recorded, since skipping adds no risk (DEC-2521).
#[test]
fn a_grant_of_a_request_that_disagrees_with_its_decision_is_refused_and_a_skip_is_not() {
    let content = content(Some(&reference(&document(&["role:approver"]))));
    let cases: [(&str, Option<(u64, serde_json::Value)>); 4] = [
        (
            "its decision deferred checks it does not name",
            Some((3, json!({"deferred_to_binding": ["halt"]}))),
        ),
        ("its decision is not on the stream", None),
        (
            "its decision is version 3 without the list",
            Some((3, json!({"dry_run": "allow"}))),
        ),
        (
            "its decision deferred the liquidity checks it does not name",
            Some((
                3,
                json!({"deferred_to_binding": ["liquidity_floor", "participation"]}),
            )),
        ),
    ];
    for (case, decision) in cases {
        let w = world();
        w.decided("paper", &content, decision);
        let issued = w.challenge();
        w.confirm(&issued);
        let viewer = w.respond(CAROL, "k1Z-9_grant00009", &grant(&w, &issued));
        assert_eq!(
            code(&viewer),
            (403, ProblemCode::Forbidden),
            "{case}: the row first"
        );
        let reply = w.respond(ALICE, KEY, &grant(&w, &issued));
        assert_eq!(
            code(&reply),
            (409, ProblemCode::DeferralMismatch),
            "{case}: {}",
            w.said()
        );
        assert!(w.answers().is_empty(), "{case}: nothing is committed");
        assert!(w.asked().is_empty(), "{case}: the port is never asked");
        assert!(w.held(&issued), "{case}: the challenge stays unspent");
        let skip = w.respond(ALICE, "k1Z-9_grant00002", &skipped(&w));
        recorded(&w, &skip);
        assert_eq!(w.answers().len(), 1, "{case}: the Skip is recorded");
    }
}

/// One request-side case: its name, the request's list (absent when `None`), its version and
/// cause, and the decision on the stream, if any.
type RequestCase<'a> = (
    &'a str,
    Option<serde_json::Value>,
    (u64, Option<&'a str>),
    Option<(u64, serde_json::Value)>,
);

/// The request's own side: a version 3 request whose list is exactly its deferring decision's is
/// granted, as the runtime writes every deferring ask; a list on version 1 or 2, an empty or absent
/// one on version 3, another order, a version past 3, a name the journal never writes, a repeated
/// name, an item that is not a name, and a cause that names no decision are each refused `deferral_mismatch` before the port (DEC-2521).
#[test]
fn a_grant_is_judged_by_the_requests_own_list_against_its_decisions() {
    let deferred = json!(["halt", "liquidity_floor", "participation"]);
    let with = |list: Option<serde_json::Value>| {
        let mut content = content(Some(&reference(&document(&["role:approver"]))));
        if let Some(list) = list {
            content["deferred_to_binding"] = list;
        }
        content
    };
    let deferring = Some((3, json!({"deferred_to_binding": deferred})));
    let w = world();
    w.requested(
        "paper",
        &with(Some(deferred.clone())),
        (3, Some(DECISION)),
        deferring.clone(),
    );
    let issued = w.challenge();
    w.confirm(&issued);
    recorded(&w, &w.respond(ALICE, KEY, &grant(&w, &issued)));
    assert_eq!(
        w.answers().len(),
        1,
        "an agreeing version 3 request is granted"
    );
    let numbered = json!(["check_2"]);
    let w = world();
    w.requested(
        "paper",
        &with(Some(numbered.clone())),
        (3, Some(DECISION)),
        Some((3, json!({"deferred_to_binding": numbered}))),
    );
    let issued = w.challenge();
    w.confirm(&issued);
    recorded(&w, &w.respond(ALICE, KEY, &grant(&w, &issued)));
    assert_eq!(w.answers().len(), 1, "a name with a digit is a name");
    let plain = Some((1, json!({"dry_run": "allow"})));
    let cases: [RequestCase<'_>; 10] = [
        (
            "a list on version 2",
            Some(json!(["halt"])),
            (2, Some(DECISION)),
            Some((3, json!({"deferred_to_binding": ["halt"]}))),
        ),
        (
            "an empty list on version 3",
            Some(json!([])),
            (3, Some(DECISION)),
            plain.clone(),
        ),
        (
            "no list on version 3",
            None,
            (3, Some(DECISION)),
            deferring.clone(),
        ),
        (
            "the decision's list in another order",
            Some(json!(["participation", "liquidity_floor", "halt"])),
            (3, Some(DECISION)),
            deferring.clone(),
        ),
        (
            "a version past 3",
            Some(deferred.clone()),
            (4, Some(DECISION)),
            deferring.clone(),
        ),
        (
            "a name the journal never writes, on both",
            Some(json!(["Halt", "Halt"])),
            (3, Some(DECISION)),
            Some((3, json!({"deferred_to_binding": ["Halt", "Halt"]}))),
        ),
        (
            "a name repeated, on both",
            Some(json!(["halt", "halt"])),
            (3, Some(DECISION)),
            Some((3, json!({"deferred_to_binding": ["halt", "halt"]}))),
        ),
        (
            "an item that is not a name, on both",
            Some(json!([1])),
            (3, Some(DECISION)),
            Some((3, json!({"deferred_to_binding": [1]}))),
        ),
        ("no cause", None, (1, None), plain.clone()),
        (
            "a cause that is not a decision",
            None,
            (1, Some(APPROVAL)),
            plain.clone(),
        ),
    ];
    for (case, list, request, decision) in cases {
        let w = world();
        w.requested("paper", &with(list), request, decision);
        let issued = w.challenge();
        w.confirm(&issued);
        let reply = w.respond(ALICE, KEY, &grant(&w, &issued));
        assert_eq!(
            code(&reply),
            (409, ProblemCode::DeferralMismatch),
            "{case}: {}",
            w.said()
        );
        assert!(w.answers().is_empty(), "{case}: nothing is committed");
        assert!(w.asked().is_empty(), "{case}: the port is never asked");
        assert!(w.held(&issued), "{case}: the challenge stays unspent");
    }
}

/// The world's own request names its decision, which deferred nothing, as the runtime writes a
/// version 1 request: a grant of it is not refused for its deferrals (DEC-2521).
#[test]
fn a_grant_of_a_request_that_agrees_with_its_decision_is_judged_as_before() {
    let w = world();
    let issued = w.challenge();
    w.confirm(&issued);
    recorded(&w, &w.respond(ALICE, KEY, &grant(&w, &issued)));
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
    let at: Vec<String> = decode::<Problem>(&reply.body)
        .expect("a problem")
        .violations
        .iter()
        .map(|violation| format!("{violation:?}"))
        .collect();
    assert!(
        at.len() == 1 && at[0].contains("\"/delegation\""),
        "one violation, at /delegation: {}",
        lossy(&reply)
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

    let listed = format!("user:{DAVE}");
    let w = world_with(&Setup {
        approvers: &[listed.as_str()],
        ..PAPER
    });
    let body = grant(&w, &placeholder());
    let cooling = w.respond(DAVE, KEY, &body);
    assert_eq!(
        cooling.status,
        403,
        "a member still cooling off is not an active approver: {} {}",
        lossy(&cooling),
        w.said()
    );
    assert!(w.answers().is_empty());
}

#[test]
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
fn an_approver_listed_by_role_is_recorded_and_a_member_without_the_role_is_not() {
    for listed in ["role:approver", "role:workspace_admin", "role:owner"] {
        let w = world_with(&Setup {
            approvers: &[listed],
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
    let unknown = w.post(
        &responses("01JBH3BTZ8K2M4N6P8R0S2T7AB"),
        ALICE,
        KEY,
        &delegated(&body),
    );
    assert_eq!(
        code(&unknown),
        (404, ProblemCode::NotFound),
        "404 before /delegation: {}",
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

#[test]
fn a_user_reference_matches_its_ulid_in_either_case_and_nothing_else() {
    let lower = format!("user:{}", ALICE.to_ascii_lowercase());
    let w = world_with(&Setup {
        approvers: &[lower.as_str()],
        ..PAPER
    });
    let issued = w.challenge();
    w.confirm(&issued);
    recorded(&w, &w.respond(ALICE, KEY, &grant(&w, &issued)));

    let aliased = format!("user:{}", ALICE.replacen('1', "I", 1));
    let unknown = format!("user:{}", ALICE.replacen('Z', "Y", 1));
    for listed in ["user:not-a-ulid", aliased.as_str(), unknown.as_str()] {
        let w = world_with(&Setup {
            approvers: &[listed],
            ..PAPER
        });
        let reply = w.respond(ALICE, KEY, &grant(&w, &placeholder()));
        assert_eq!(
            code(&reply),
            (403, ProblemCode::Forbidden),
            "{listed:?} lists no one: {}",
            w.said()
        );
        assert!(w.answers().is_empty(), "{listed:?}");
    }
}

/// A member deactivated after confirming is no member at the grant: the matrix row's one 404
/// (§3.3), before the approvers are read, and nothing is committed.
#[test]
fn an_approver_deactivated_since_is_refused_at_once() {
    let w = world();
    let (bobs, body) = bobs_grant(&w);
    w.append(
        "MemberDeactivated",
        T0,
        json!({"member": BOB, "session_ref": null}),
    );
    let reply = w.respond(BOB, KEY, &body);
    assert_eq!(code(&reply), (404, ProblemCode::NotFound));
    assert!(w.answers().is_empty());
    assert!(w.asked().is_empty(), "no evidence is read");
    assert!(w.held(&bobs));
}
