//! The connect start's fixture (§4.5, DEC-2623): a paper workspace whose admin is Alice and
//! viewer Bob, an OAuth client, a step-up port that counts only the connect digest computed here,
//! a random source whose draws the tests know, and a vault port that keeps each code written.

#![allow(dead_code, reason = "each test file uses part of the shared helpers")]

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use mandate_api::envelope::StepUpEvidence;
use mandate_api::idempotency::{Derivation, IdempotencyKey, event_id};
use mandate_api::problem::{Problem, ProblemCode};
use mandate_api::wire::{Id, decode};
use mandate_api_server::challenges::Challenges;
use mandate_api_server::config::{SESSION_KEY_FILE, read_session_key};
use mandate_api_server::connect::{CodeVault, Connect};
use mandate_api_server::http::{self, App, Settings};
use mandate_api_server::lifecycle::Drain;
use mandate_api_server::objects::{ObjectStore, Objects};
use mandate_api_server::session::{COOKIE_NAME, OsRandom, Random, Sessions, Unauthenticated};
use mandate_api_server::stepup::{Action, Judged, StepUp};
use mandate_api_server::token::{FixedKeys, Issuer};
use mandate_authn::{Algorithm, IssuerConfig};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_connections::ConnectError;
use mandate_connections::start::{
    AuthorizePage, Authorizer, Binding, ClientId, Environment, OAuthClient, PkceVerifier,
    RedirectUri,
};
use mandate_connections::vault::AuthorizationCode;
use mandate_identity::{MembershipEvent, MembershipRecord, PrincipalId, Role, StepUpActionKind};
use mandate_identity_seal::Seal;
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;
use secrecy::ExposeSecret;

use crate::common::{self, MemJournal, Reply, WORKSPACE, hosted, send, serve};

pub const APP: &str = "https://app.owlhead.test";
pub const T0: i64 = 1_791_000_000;
pub const KEY: &str = "k1Z-9_connect001";
pub const ALICE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Z";
pub const BOB: &str = "01JB3K6F0C9R2V7N4M8Q1T5W41";
pub const CAROL: &str = "01JB3K6F0C9R2V7N4M8Q1T5W42";
pub const PATH: &str = "/connections/oauth/start";

/// The two draws the fixture's random source answers, in order.
pub const DRAWS: [[u8; 32]; 2] = [[0x11; 32], [0x22; 32]];

/// The step-up port: it records each action it is asked about, and counts evidence only for the
/// connect digest computed by [`digest`], when `accepting`.
pub struct Port {
    pub asked: Mutex<Vec<Action>>,
    pub accepting: bool,
}

impl StepUp for Port {
    fn verify(
        &self,
        _evidence: &StepUpEvidence,
        action: &Action,
        _judged: &Judged<'_>,
    ) -> Result<(), ProblemCode> {
        self.asked.lock().expect("unpoisoned").push(*action);
        let bound = action.kind == StepUpActionKind::Connection && action.digest == digest("1");
        if self.accepting && bound {
            Ok(())
        } else {
            Err(ProblemCode::StepUpMismatch)
        }
    }
}

/// Answers [`DRAWS`] in turn, then fails.
pub struct Draws(pub Mutex<usize>);

impl Random for Draws {
    fn fill(&self, out: &mut [u8]) -> Result<(), Unauthenticated> {
        let mut next = self.0.lock().expect("unpoisoned");
        let draw = DRAWS.get(*next).ok_or(Unauthenticated)?;
        out.copy_from_slice(draw.get(..out.len()).ok_or(Unauthenticated)?);
        *next += 1;
        Ok(())
    }
}

/// Draws that never run out: each fill is the next byte value, repeated.
pub struct Counting(pub Mutex<u8>);

impl Random for Counting {
    fn fill(&self, out: &mut [u8]) -> Result<(), Unauthenticated> {
        let mut next = self.0.lock().expect("unpoisoned");
        *next = next.wrapping_add(1);
        out.fill(*next);
        Ok(())
    }
}

/// One code the callback wrote: the binding's members and the two secrets' text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Put {
    pub workspace_id: String,
    pub user_id: String,
    pub connection_id: String,
    pub environment: Environment,
    pub code: String,
    pub verifier: String,
}

/// The callback's vault port: every code written, and whether the next write fails.
#[derive(Default)]
pub struct Codes {
    pub puts: Mutex<Vec<Put>>,
    pub failing: AtomicBool,
}

impl CodeVault for Codes {
    fn put_code(
        &self,
        binding: &Binding,
        code: AuthorizationCode,
        verifier: PkceVerifier,
    ) -> Result<(), ConnectError> {
        if self.failing.load(Ordering::SeqCst) {
            return Err(ConnectError::VaultUnavailable);
        }
        self.puts.lock().expect("unpoisoned").push(Put {
            workspace_id: binding.workspace_id.clone(),
            user_id: binding.user_id.clone(),
            connection_id: binding.connection_id.as_str().to_owned(),
            environment: binding.environment,
            code: code.0.expose_secret().to_owned(),
            verifier: verifier.0.expose_secret().to_owned(),
        });
        Ok(())
    }
}

pub struct Fixture {
    pub addr: std::net::SocketAddr,
    pub journal: Arc<MemJournal>,
    pub sessions: Arc<Sessions>,
    pub clock: Arc<common::ManualClock>,
    pub connect: Option<Arc<Connect>>,
    pub port: Arc<Port>,
    pub codes: Arc<Codes>,
    /// The membership records the server reads, which a test may change mid-flight.
    pub records: Arc<Mutex<Vec<MembershipRecord>>>,
    /// The challenges the server holds.
    pub challenges: Arc<Challenges>,
    /// The object store, when the fixture binds one.
    pub objects: Option<Arc<dyn ObjectStore>>,
}

static DIRS: Mutex<u32> = Mutex::new(0);

/// A paper workspace (or `environment`'s), offering a connect when `offered`.
pub fn fixture(environment: &str, offered: bool, accepting: bool) -> Fixture {
    fixture_drawing(environment, offered, accepting, 0)
}

/// [`fixture`], with the random source starting at [`DRAWS`]'s `first`: past both, it fails.
pub fn fixture_drawing(environment: &str, offered: bool, accepting: bool, first: usize) -> Fixture {
    fixture_random(
        environment,
        offered,
        accepting,
        Box::new(Draws(Mutex::new(first))),
    )
}

/// [`fixture`] with `random` as the start's random source.
pub fn fixture_random(
    environment: &str,
    offered: bool,
    accepting: bool,
    random: Box<dyn Random>,
) -> Fixture {
    fixture_port(environment, offered, accepting, random, None)
}

/// [`fixture`] whose callback writes through `vault` when given, rather than [`Codes`].
pub fn fixture_port(
    environment: &str,
    offered: bool,
    accepting: bool,
    random: Box<dyn Random>,
    vault: Option<Arc<dyn CodeVault>>,
) -> Fixture {
    fixture_bound(environment, offered, accepting, random, vault, None)
}

/// An object store in memory.
pub type MemObjects = Objects<BTreeMap<Digest, Vec<u8>>>;

/// What a fixture binds at the step-up port instead of its [`Port`]: the binding, given the
/// fixture's challenge store, and the object store the server reads and adds to.
pub struct StepUpBinding {
    pub bind: Box<dyn FnOnce(Arc<Challenges>) -> Arc<dyn StepUp>>,
    pub objects: Arc<dyn ObjectStore>,
}

/// A paper workspace offering a connect, whose step-up port and object store are `binding`'s.
pub fn fixture_binding(binding: StepUpBinding) -> Fixture {
    fixture_bound(
        "paper",
        true,
        true,
        Box::new(Draws(Mutex::new(0))),
        None,
        Some(binding),
    )
}

fn fixture_bound(
    environment: &str,
    offered: bool,
    accepting: bool,
    random: Box<dyn Random>,
    vault: Option<Arc<dyn CodeVault>>,
    binding: Option<StepUpBinding>,
) -> Fixture {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mandate-api-server-connect-{}-{n}",
        std::process::id()
    ));
    drop(n);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let file = dir.join(SESSION_KEY_FILE);
    std::fs::write(&file, [0x46_u8; 32]).expect("the key");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("mode");
    let key = read_session_key(&dir).expect("the key reads");
    let journal = Arc::new(MemJournal::default());
    let records = Arc::new(Mutex::new(vec![
        activated(1, principal(ALICE), &[Role::WorkspaceAdmin]),
        activated(2, principal(BOB), &[Role::Viewer]),
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
    let port = Arc::new(Port {
        asked: Mutex::default(),
        accepting,
    });
    let codes = Arc::new(Codes::default());
    let connect = offered.then(|| {
        Arc::new(Connect::new(
            Authorizer {
                page: AuthorizePage::alpaca(),
                client: client(),
            },
            random,
            vault.unwrap_or_else(|| Arc::clone(&codes) as Arc<dyn CodeVault>),
        ))
    });
    let challenges = Arc::new(Challenges::default());
    let (step_up, objects) = match binding {
        Some(binding) => (
            (binding.bind)(Arc::clone(&challenges)),
            Some(binding.objects),
        ),
        None => (Arc::clone(&port) as Arc<dyn StepUp>, None),
    };
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
        artifacts: objects
            .clone()
            .map(|store| store as Arc<dyn mandate_journal::ArtifactSource + Send + Sync>),
        objects: objects.clone(),
        step_up,
        challenges: Arc::clone(&challenges),
        connect: connect.clone(),
        reduction: Arc::default(),
        folds: Arc::default(),
        bars: None,
    };
    journal
        .streams
        .lock()
        .expect("unpoisoned")
        .insert(ctl(), vec![opened(environment)]);
    Fixture {
        addr: serve(http::api(app, Drain::new())),
        journal,
        sessions,
        clock,
        connect,
        port,
        codes,
        records,
        challenges,
        objects,
    }
}

/// Makes Alice a viewer only, as a demotion while she is at Alpaca would.
pub fn demote_alice(f: &Fixture) {
    f.records.lock().expect("unpoisoned")[0] = activated(1, principal(ALICE), &[Role::Viewer]);
}

pub fn client() -> OAuthClient {
    OAuthClient {
        client_id: ClientId("client-1".to_owned()),
        redirect_uri: RedirectUri::production(),
    }
}

pub fn principal(id: &str) -> PrincipalId {
    PrincipalId::from_ulid_text(id).expect("a ULID")
}

pub fn at(secs: i64) -> UtcNanos {
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

pub fn ctl() -> String {
    format!("ctl:{WORKSPACE}")
}

fn opened(environment: &str) -> StoredEvent {
    let text = format!(
        r#"{{"environment":"{environment}","event_id":"01JBH3BTZ8K2M4N6P8R0S2T001","event_type":"StreamOpened","payload":{{}},"stream_id":"{}"}}"#,
        ctl()
    );
    let body = to_canonical(&parse(text.as_bytes()).expect("JSON"));
    StoredEvent {
        stream_id: ctl(),
        seq: 1,
        event_id: "01JBH3BTZ8K2M4N6P8R0S2T001".to_owned(),
        event_type: "StreamOpened".to_owned(),
        schema_version: 1,
        environment: environment.to_owned(),
        recorded_at: "2026-10-17T10:00:01.000000000Z".to_owned(),
        prev_hash: Digest::ZERO,
        hash: Digest::of(&body),
        body,
    }
}

pub fn signed_in(f: &Fixture, who: &str) -> String {
    f.sessions
        .open(
            principal(who),
            "sha256:aaaa".to_owned(),
            None,
            at(f.clock.secs()),
        )
        .expect("opened")
        .value()
        .to_owned()
}

/// The schema's example body, which carries a step-up and the current disclosure.
pub fn base() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../schemas/workspace-api/examples/commands.oauth-start-request.json"
    );
    parse(&std::fs::read(path).expect("the example")).expect("JSON")
}

/// `base` with `pointer`'s member set to `value` (JSON text).
pub fn with(pointer: &[&str], value: &str) -> Vec<u8> {
    let mut text = String::from_utf8(to_canonical(&base())).expect("UTF-8");
    let old = pointer
        .iter()
        .try_fold(&base(), |v, k| v.get(k))
        .cloned()
        .expect("a member");
    let old_text = String::from_utf8(to_canonical(&old)).expect("UTF-8");
    let named = format!(r#""{}":{old_text}"#, pointer.last().expect("a name"));
    let set = format!(r#""{}":{value}"#, pointer.last().expect("a name"));
    assert!(text.contains(&named), "{named}");
    text = text.replacen(&named, &set, 1);
    text.into_bytes()
}

pub fn start(f: &Fixture, cookie: Option<&str>, key: Option<&str>, body: &[u8]) -> Reply {
    let cookie = cookie.map(|c| format!("{COOKIE_NAME}={c}"));
    let mut headers = vec![
        ("Origin", APP),
        ("X-Mandate-Request", "1"),
        ("Content-Type", "application/json"),
    ];
    if let Some(cookie) = cookie.as_deref() {
        headers.push(("Cookie", cookie));
    }
    if let Some(key) = key {
        headers.push(("Idempotency-Key", key));
    }
    send(
        f.addr,
        "POST",
        &format!("/v1/workspaces/{WORKSPACE}{PATH}"),
        &headers,
        body,
    )
}

/// §3.4's id for `operation` under [`KEY`] for Alice, computed by `mandate-api`.
pub fn derived(operation: &str) -> String {
    let workspace = Id::try_from(WORKSPACE.to_owned()).expect("an id");
    let principal = Id::try_from(ALICE.to_owned()).expect("an id");
    let key = IdempotencyKey::parse(KEY).expect("a key");
    let derivation = Derivation {
        workspace: &workspace,
        principal: &principal,
        operation,
        key: &key,
        position: None,
    };
    let id = event_id(&derivation).expect("an id");
    parse(&mandate_api::wire::encode(&id).expect("JSON"))
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .expect("a JSON string")
}

/// DEC-2621 item 4's connect digest, computed here: SHA-256 of the canonical `{action, broker,
/// disclosure: {document, version}, environment, workspace_id}`.
pub fn digest(version: &str) -> Digest {
    let text = format!(
        r#"{{"action":"connect","broker":"alpaca","disclosure":{{"document":"alpaca_oauth_token_breadth","version":"{version}"}},"environment":"paper","workspace_id":"{WORKSPACE}"}}"#
    );
    Digest::of(&to_canonical(&parse(text.as_bytes()).expect("JSON")))
}

pub fn b64(bytes: &[u8]) -> String {
    common::b64url(bytes)
}

/// The S256 challenge of a verifier's text (RFC 7636 §4.2).
pub fn challenge(verifier: &str) -> String {
    b64(Digest::of(verifier.as_bytes()).as_bytes())
}

pub fn json(reply: &Reply) -> Value {
    parse(&reply.body).expect("a JSON body")
}

pub fn code(reply: &Reply) -> (u16, ProblemCode) {
    assert_eq!(
        reply.header("Content-Type"),
        Some(http::PROBLEM_JSON),
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    let problem = decode::<Problem>(&reply.body).expect("a problem");
    assert_eq!(problem.status, reply.status);
    (reply.status, problem.code)
}

/// The text a start could derive from [`DRAWS`], in either order: each draw's text and its
/// challenge.
pub fn drawn_texts() -> [String; 4] {
    let [first, second] = DRAWS.map(|draw| b64(&draw));
    [challenge(&first), challenge(&second), first, second]
}

/// A refused start leaves nothing redeemable: no `state` is held, and neither `body` nor any
/// journal row carries a text derived from the draws.
pub fn unissued(f: &Fixture, body: &[u8]) {
    if let Some(connect) = &f.connect {
        assert_eq!(
            connect.states.lock().expect("unpoisoned").len(),
            0,
            "no state is held"
        );
    }
    let rows: Vec<String> = f
        .journal
        .streams
        .lock()
        .expect("unpoisoned")
        .values()
        .flatten()
        .map(|row| String::from_utf8_lossy(&row.body).into_owned())
        .collect();
    let body = String::from_utf8_lossy(body);
    for drawn in drawn_texts() {
        assert!(!body.contains(&drawn), "the answer carries no drawn text");
        assert!(
            rows.iter().all(|row| !row.contains(&drawn)),
            "no row carries a drawn text"
        );
    }
}

/// The control stream's rows after the seeded `StreamOpened`.
pub fn appended(f: &Fixture) -> Vec<StoredEvent> {
    f.journal.rows_of(&ctl()).into_iter().skip(1).collect()
}

pub fn text<'v>(value: &'v Value, path: &[&str]) -> &'v str {
    path.iter()
        .try_fold(value, |v, k| v.get(k))
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{path:?}"))
}

/// Appends a row of `event_type` at `version` with `payload` (JSON text) to the control stream,
/// under an event id no start derives.
pub fn seed(f: &Fixture, event_type: &str, version: u64, payload: &str) {
    let mut streams = f.journal.streams.lock().expect("unpoisoned");
    let rows = streams.entry(ctl()).or_default();
    let seq = u64::try_from(rows.len()).expect("small") + 1;
    let event_id = format!("01JBH3BTZ8K2M4N6P8R0S2T{seq:03}");
    let text = format!(
        r#"{{"environment":"paper","event_id":"{event_id}","event_type":"{event_type}","payload":{payload},"stream_id":"{}"}}"#,
        ctl()
    );
    let body = to_canonical(&parse(text.as_bytes()).expect("JSON"));
    rows.push(StoredEvent {
        stream_id: ctl(),
        seq,
        event_id,
        event_type: event_type.to_owned(),
        schema_version: version,
        environment: "paper".to_owned(),
        recorded_at: format!("2026-10-17T10:00:{seq:02}.000000000Z"),
        prev_hash: Digest::ZERO,
        hash: Digest::of(&body),
        body,
    });
}
