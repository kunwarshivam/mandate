//! A passkey enrolment over HTTP, `POST /v1/workspaces/{ws}/passkeys` (workspace API spec §3.6,
//! [DEC-2108] items 4 and 5, [DEC-2118], [DEC-2122], [DEC-2124]; journal spec §9.31 rules 249 to
//! 251): a registration response from a software authenticator, over the challenge the
//! registration-challenge route issued, commits `CredentialEnrolled` version 2 and stores the key;
//! every refusal appends and stores nothing.
//!
//! Every expected record, digest, and id is written here by hand from the request the test sent,
//! and the registration responses are built by `support_passkey`'s own encoders.
//!
//! [DEC-2108]: ../../../docs/project/decisions/DEC-2108.md
//! [DEC-2118]: ../../../docs/project/decisions/DEC-2118.md
//! [DEC-2122]: ../../../docs/project/decisions/DEC-2122.md
//! [DEC-2124]: ../../../docs/project/decisions/DEC-2124.md

mod common;
mod support_passkey;

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use common::control::{Rows, hosted_stream};
use common::{MemJournal, Reply, WORKSPACE, b64url, hosted, send, serve};
use mandate_api::envelope::{StepUpAssertion, StepUpEvidence};
use mandate_api::problem::{Effect, Problem, ProblemCode};
use mandate_api::wire::decode;
use mandate_api_server::challenges::Challenges;
use mandate_api_server::config::{SESSION_KEY_FILE, read_session_key};
use mandate_api_server::http::{self, App, Settings};
use mandate_api_server::lifecycle::Drain;
use mandate_api_server::objects::{ObjectStore, Objects};
use mandate_api_server::session::{Answer, COOKIE_NAME, OsRandom, Sessions};
use mandate_api_server::stepup::{Action, Asserted, Judged, StepUp, Unverified};
use mandate_api_server::token::{FixedKeys, Issuer};
use mandate_authn::{Algorithm, IssuerConfig};
use mandate_canon::{Digest, Value, parse};
use mandate_identity::{MembershipEvent, MembershipRecord, PrincipalId, Role};
use mandate_identity_seal::Seal;
use mandate_journal::{ArtifactError, ArtifactRef, ArtifactSource, StoredEvent, get_artifact};
use mandate_time::UtcNanos;
use serde_json::{Value as Json, json};
use support_passkey::{
    CREDENTIAL, ORIGIN, OTHER_CREDENTIAL, RP_ID, Registered, Shape, Signer, enrol, enrolled,
    removed,
};

const T0: i64 = common::control::T0;
const KEY: &str = "k1Z-9_aaaaaaaaaa";
const KEY_2: &str = "k2Z-9_bbbbbbbbbb";
const KEY_3: &str = "k3Z-9_cccccccccc";
const SUBJECT: &str = "sha256:aaaa";
const ALICE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Z";
const BOB: &str = "01JB3K6F0C9R2V7N4M8Q1T5W41";
const DAVE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W43";
/// A third passkey id: the 16 bytes 64 to 79.
const THIRD_CREDENTIAL: &str = "QEFCQ0RFRkdISUpLTE1OTw";
/// A cool-off's length, in seconds (journal spec §9.31 rule 249).
const COOL_OFF_S: i64 = 86_400;

/// The in-memory object store the server reads and adds to, counting what it was handed.
struct MemObjects {
    inner: Objects<BTreeMap<Digest, Vec<u8>>>,
    puts: Mutex<usize>,
}

impl ArtifactSource for MemObjects {
    fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        self.inner.read_artifact(reference)
    }
}

impl ObjectStore for MemObjects {
    fn put(&self, bytes: &[u8]) -> Result<ArtifactRef, ArtifactError> {
        *self.puts.lock().expect("unpoisoned") += 1;
        self.inner.put(bytes)
    }
}

/// The step-up port as a test binds it: it answers every assertion `answer`, and keeps each
/// action it was asked about, with the evidence and assertion it judged.
struct Port {
    answer: Unverified,
    asked: Mutex<Vec<(Action, StepUpEvidence, Option<StepUpAssertion>)>>,
}

impl StepUp for Port {
    fn verify(
        &self,
        _evidence: &StepUpEvidence,
        _action: &Action,
        _judged: &Judged<'_>,
    ) -> Result<(), ProblemCode> {
        Err(ProblemCode::StepUpMissing)
    }

    fn asserted(
        &self,
        evidence: &StepUpEvidence,
        assertion: Option<&StepUpAssertion>,
        action: &Action,
        _judged: &Judged<'_>,
    ) -> Result<Option<Asserted>, Unverified> {
        self.asked.lock().expect("unpoisoned").push((
            *action,
            evidence.clone(),
            assertion.cloned(),
        ));
        Err(self.answer)
    }
}

struct Fixture {
    addr: std::net::SocketAddr,
    journal: Arc<MemJournal>,
    sessions: Arc<Sessions>,
    clock: Arc<NanoClock>,
    objects: Arc<MemObjects>,
    port: Arc<Port>,
}

/// The server's clock, settable to the nanosecond.
struct NanoClock(Mutex<UtcNanos>);

impl NanoClock {
    fn set(&self, secs: i64) {
        self.set_at(at(secs));
    }

    fn set_at(&self, now: UtcNanos) {
        *self.0.lock().expect("unpoisoned") = now;
    }

    fn now_at(&self) -> UtcNanos {
        *self.0.lock().expect("unpoisoned")
    }
}

impl mandate_api_server::clock::Clock for NanoClock {
    fn now(&self) -> Option<UtcNanos> {
        Some(self.now_at())
    }
}

static DIRS: Mutex<u32> = Mutex::new(0);

/// A server for the hosted workspace, its control stream `rows`, where ALICE (an operator and
/// approver) and BOB (a viewer) are members and DAVE is none, its step-up port answering every
/// assertion `answer`, and an empty object store.
fn fixture_answering(rows: &Rows, answer: Unverified) -> Fixture {
    fixture_with(rows, answer, true)
}

/// [`fixture_answering`] with an object store for the server only when `stores`.
fn fixture_with(rows: &Rows, answer: Unverified, stores: bool) -> Fixture {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = std::env::temp_dir().join(format!(
        "mandate-api-server-passkey-enrol-{}-{n}",
        std::process::id()
    ));
    drop(n);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let file = dir.join(SESSION_KEY_FILE);
    std::fs::write(&file, [0x48_u8; 32]).expect("the key");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("mode");
    let key = read_session_key(&dir).expect("the key reads");
    let journal = Arc::new(MemJournal::default());
    journal
        .streams
        .lock()
        .expect("unpoisoned")
        .insert(hosted_stream(), rows.rows.clone());
    let records = vec![
        activated(1, id(ALICE), &[Role::Operator, Role::Approver]),
        activated(2, id(BOB), &[Role::Viewer]),
    ];
    let sessions = Arc::new(Sessions::new(&key, Box::new(OsRandom::new())));
    let config = IssuerConfig::new(
        "https://issuer.test/auth/v1",
        &["authenticated"],
        &[Algorithm::Es256],
    )
    .expect("the issuer");
    let clock = Arc::new(NanoClock(Mutex::new(at(T0))));
    let objects = Arc::new(MemObjects {
        inner: Objects::new(BTreeMap::new()),
        puts: Mutex::new(0),
    });
    let port = Arc::new(Port {
        answer,
        asked: Mutex::new(Vec::new()),
    });
    let app = App {
        settings: Settings {
            app_origin: ORIGIN.to_owned(),
        },
        sessions: Arc::clone(&sessions),
        tokens: Arc::new(Issuer::new(config, Arc::new(FixedKeys(None)))),
        links: Arc::new(common::LinksAt(dir.join("absent"))),
        clock: Arc::clone(&clock) as Arc<dyn mandate_api_server::clock::Clock>,
        journal: Arc::clone(&journal) as Arc<dyn mandate_api_server::journal::Journal>,
        members: Arc::new(move |_: &[StoredEvent]| Ok(records.clone())),
        hosted: hosted(),
        artifacts: None,
        objects: stores.then(|| Arc::clone(&objects) as Arc<dyn ObjectStore>),
        step_up: Arc::clone(&port) as Arc<dyn StepUp>,
        challenges: Arc::new(Challenges::default()),
        connect: None,
        reduction: Arc::default(),
    };
    Fixture {
        addr: serve(http::api(app, Drain::new())),
        journal,
        sessions,
        clock,
        objects,
        port,
    }
}

/// [`fixture_answering`] whose port would refuse every assertion `step_up_missing`.
fn fixture(rows: &Rows) -> Fixture {
    fixture_answering(rows, Unverified::Code(ProblemCode::StepUpMissing))
}

fn id(text: &str) -> PrincipalId {
    PrincipalId::from_ulid_text(text).expect("a ULID")
}

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).expect("a time")
}

fn time(secs: i64) -> String {
    at(secs).to_string()
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

/// A paper control stream where ALICE holds `signer`'s key as [`OTHER_CREDENTIAL`], enrolled at
/// `T0 - 500` and past its cool-off.
fn holding(signer: &Signer) -> Rows {
    let mut rows = Rows::opened();
    enrol(
        &mut rows,
        T0 - 500,
        enrolled(id(ALICE), OTHER_CREDENTIAL, signer, 0, None),
    );
    rows
}

fn signed_in(f: &Fixture, who: &str) -> String {
    f.sessions
        .open(id(who), SUBJECT.to_owned(), None, f.clock.now_at())
        .expect("opened")
        .value()
        .to_owned()
}

/// `cookie`'s session refreshed at `secs`, with the clock set there: a refresh renews the tokens
/// and leaves the sign-in (DEC-2124 item 4).
fn refreshed(f: &Fixture, cookie: &str, who: &str, secs: i64) -> String {
    f.clock.set(secs);
    f.sessions
        .refresh(
            cookie,
            Answer::Granted {
                subject: SUBJECT.to_owned(),
                principal: id(who),
            },
            at(secs),
        )
        .expect("refreshed")
        .expect("a rotated cookie")
        .value()
        .to_owned()
}

fn passkeys_path() -> String {
    format!("/v1/workspaces/{WORKSPACE}/passkeys")
}

fn post(f: &Fixture, path: &str, cookie: &str, key: &str, body: &[u8]) -> Reply {
    let cookie = format!("{COOKIE_NAME}={cookie}");
    send(
        f.addr,
        "POST",
        path,
        &[
            ("Origin", ORIGIN),
            ("X-Mandate-Request", "1"),
            ("Cookie", cookie.as_str()),
            ("Idempotency-Key", key),
        ],
        body,
    )
}

/// A registration challenge the route issued: its id and its bytes.
struct Issued {
    id: String,
    bytes: Vec<u8>,
}

fn challenge(f: &Fixture, cookie: &str, key: &str) -> Issued {
    let reply = post(
        f,
        &format!("{}/registration-challenges", passkeys_path()),
        cookie,
        key,
        b"{}",
    );
    assert_eq!(
        reply.status,
        201,
        "the challenge route: {:?}",
        String::from_utf8_lossy(&reply.body)
    );
    Issued {
        id: text(&reply, "challenge_id"),
        bytes: support_passkey::unb64url(&text(&reply, "webauthn_challenge")),
    }
}

/// The enrolment request for `registered` as `passkey_id` over the challenge `challenge_id`,
/// with `step_up` and `step_up_assertion` as given.
fn request(
    challenge_id: &str,
    passkey_id: &str,
    registered: &Registered,
    step_up: Json,
    assertion: Json,
) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "challenge_id": challenge_id,
        "passkey_id": passkey_id,
        "attestation_object": b64url(&registered.attestation_object),
        "client_data_json": b64url(&registered.client_data_json),
        "step_up": step_up,
        "step_up_assertion": assertion,
    }))
    .expect("JSON")
}

/// A first enrolment's request: no step-up.
fn first(challenge_id: &str, passkey_id: &str, registered: &Registered) -> Vec<u8> {
    request(challenge_id, passkey_id, registered, Json::Null, Json::Null)
}

fn enrol_with(f: &Fixture, cookie: &str, key: &str, body: &[u8]) -> Reply {
    post(f, &passkeys_path(), cookie, key, body)
}

fn list(f: &Fixture, cookie: &str) -> Reply {
    let cookie = format!("{COOKIE_NAME}={cookie}");
    send(
        f.addr,
        "GET",
        &passkeys_path(),
        &[("Origin", ORIGIN), ("Cookie", cookie.as_str())],
        b"",
    )
}

/// §3.4's derivation written out from the spec, independent of `mandate_api::idempotency`: the
/// canonical object of four ASCII members in byte order, its SHA-256 by `ring`, and the first 128
/// bits as 26 Crockford base-32 digits after two zero bits.
fn derived_in(workspace: &str, principal: &str, operation: &str, key: &str) -> String {
    let object = format!(
        r#"{{"key":"{key}","operation":"{operation}","principal":"{principal}","workspace":"{workspace}"}}"#
    );
    let sha = ring::digest::digest(&ring::digest::SHA256, object.as_bytes());
    let (head, _) = sha
        .as_ref()
        .split_first_chunk::<16>()
        .expect("SHA-256 has 32 bytes");
    let value = u128::from_be_bytes(*head);
    let digits = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    (0..26_u32)
        .rev()
        .map(|place| {
            char::from(digits[usize::try_from((value >> (5 * place)) & 31).expect("a digit")])
        })
        .collect()
}

/// The oracle reproduces §3.4's worked vector before it is trusted.
#[test]
fn the_derivation_oracle_reproduces_the_specs_worked_vector() {
    assert_eq!(
        derived_in("ws_01", "user_7", "kill_switch", "k1Z-9_aaaaaaaaaa"),
        "7KXV6BGDW36KYC7RE5BPQG28Z0"
    );
}

/// The enrolment's event id for ALICE's `key` (§3.4, operation `passkey_enrol`).
fn enrolment_id(key: &str) -> String {
    derived_in(WORKSPACE, ALICE, "passkey_enrol", key)
}

fn body(reply: &Reply) -> Value {
    parse(&reply.body)
        .unwrap_or_else(|e| panic!("{e:?}: {:?}", String::from_utf8_lossy(&reply.body)))
}

fn member(reply: &Reply, name: &str) -> Value {
    match body(reply) {
        Value::Object(members) => members
            .into_iter()
            .find(|(key, _)| key.as_str() == name)
            .map(|(_, value)| value)
            .unwrap_or_else(|| panic!("no {name}")),
        other => panic!("not an object: {other:?}"),
    }
}

fn text(reply: &Reply, name: &str) -> String {
    match member(reply, name) {
        Value::Str(text) => text,
        other => panic!("{name} is not a string: {other:?}"),
    }
}

fn problem(reply: &Reply) -> Problem {
    decode::<Problem>(&reply.body)
        .unwrap_or_else(|e| panic!("{e:?}: {:?}", String::from_utf8_lossy(&reply.body)))
}

/// `reply` is the refusal `code` at `status`, changing nothing, with `Cache-Control: no-store`.
fn refused(reply: &Reply, status: u16, code: ProblemCode) -> Problem {
    assert_eq!(
        reply.status,
        status,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    let problem = problem(reply);
    assert_eq!(problem.code, code);
    assert_eq!(problem.effect, Effect::None);
    problem
}

/// The violations' paths of a refusal, as the tests compare them.
fn paths(problem: &Problem) -> Vec<String> {
    serde_json::to_value(&problem.violations)
        .expect("JSON")
        .as_array()
        .expect("an array")
        .iter()
        .map(|violation| violation["path"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// `invalid` at `/challenge_id` alone.
fn challenge_refused(reply: &Reply) {
    let problem = refused(reply, 422, ProblemCode::Invalid);
    assert_eq!(paths(&problem), ["/challenge_id"]);
}

/// `invalid` with no violation path, as a registration response that does not read is answered
/// (DEC-2118 item 5).
fn registration_refused(reply: &Reply) {
    let problem = refused(reply, 422, ProblemCode::Invalid);
    assert_eq!(
        paths(&problem),
        Vec::<String>::new(),
        "no path names what was wrong"
    );
}

/// `step_up_stale` with its one violation at `/session`.
fn stale(reply: &Reply) {
    let problem = refused(reply, 401, ProblemCode::StepUpStale);
    assert_eq!(paths(&problem), ["/session"]);
}

/// Nothing appended to the control stream since `rows`, and nothing stored.
fn nothing_changed(f: &Fixture, rows: &Rows) {
    assert_eq!(
        f.journal.rows_of(&hosted_stream()).len(),
        rows.rows.len(),
        "nothing appended"
    );
    assert_eq!(
        *f.objects.puts.lock().expect("unpoisoned"),
        0,
        "nothing stored"
    );
}

/// The payload of `row`, as JSON.
fn payload(row: &StoredEvent) -> Json {
    let envelope: Json = serde_json::from_slice(&row.body).expect("JSON");
    envelope["payload"].clone()
}

fn envelope(row: &StoredEvent) -> Json {
    serde_json::from_slice(&row.body).expect("JSON")
}

#[test]
fn a_first_passkey_enrols_within_300_s_of_the_sign_in_and_waits_its_cool_off() {
    for signer in [Signer::es256(), Signer::ed25519()] {
        let rows = Rows::opened();
        let f = fixture(&rows);
        let cookie = signed_in(&f, ALICE);
        f.clock.set(T0 + 10);
        let issued = challenge(&f, &cookie, KEY);
        let cookie = refreshed(&f, &cookie, ALICE, T0 + 100);
        f.clock.set(T0 + 300);
        let registered = signer.register(&issued.bytes, CREDENTIAL);
        let reply = enrol_with(
            &f,
            &cookie,
            KEY_2,
            &first(&issued.id, CREDENTIAL, &registered),
        );
        assert_eq!(
            reply.status,
            201,
            "{}, at the 300th second of the sign-in, a refresh moving nothing: {:?}",
            signer.alg(),
            String::from_utf8_lossy(&reply.body)
        );
        assert_eq!(reply.header("cache-control"), Some("no-store"));
        assert_eq!(text(&reply, "event_id"), enrolment_id(KEY_2));
        assert_eq!(text(&reply, "passkey_id"), CREDENTIAL);
        assert_eq!(text(&reply, "alg"), signer.alg());
        assert_eq!(
            text(&reply, "cool_off_ends"),
            time(T0 + 300 + COOL_OFF_S),
            "24 hours after the commit"
        );
        let all = f.journal.rows_of(&hosted_stream());
        assert_eq!(all.len(), rows.rows.len() + 1, "one record appended");
        let row = all.last().expect("the enrolment");
        assert_eq!(
            (row.event_type.as_str(), row.schema_version),
            ("CredentialEnrolled", 2)
        );
        assert_eq!(row.event_id, enrolment_id(KEY_2));
        let cose = signer.cose_key();
        assert_eq!(
            payload(row),
            json!({
                "member": ALICE, "kind": "passkey", "credential": CREDENTIAL,
                "public_key": format!("sha256:{}", Digest::of(&cose).to_hex()),
                "alg": signer.alg(), "sign_count": 0, "backup_eligible": false,
                "cool_off_ends": time(T0 + 300 + COOL_OFF_S), "step_up": null,
            }),
            "§9.31's version 2, every member written out"
        );
        let envelope = envelope(row);
        assert_eq!(
            (&envelope["actor"]["kind"], &envelope["actor"]["id"]),
            (&json!("user"), &json!(ALICE)),
            "the member enrols for themself (rule 249)"
        );
        assert_eq!(envelope["event_time"], json!(time(T0 + 300)));
        assert_eq!(
            envelope["artifact_refs"],
            json!([format!("sha256:{}", Digest::of(&cose).to_hex())]),
            "the envelope lists the payload's one `sha256:` reference, the key (journal spec §3)"
        );
        assert_eq!(
            get_artifact(f.objects.as_ref(), &ArtifactRef::of(&cose)),
            Ok(cose.clone()),
            "the COSE key is stored under its own digest"
        );
        let listed = list(&f, &cookie);
        assert_eq!(listed.status, 200);
        let passkeys =
            serde_json::from_slice::<Json>(&listed.body).expect("JSON")["passkeys"].clone();
        assert_eq!(
            passkeys,
            json!([{
                "passkey_id": CREDENTIAL, "alg": signer.alg(),
                "enrolled_at": row.recorded_at,
                "cool_off_ends": time(T0 + 300 + COOL_OFF_S),
            }]),
            "the list shows the new passkey"
        );
    }
}

#[test]
fn a_backup_eligible_registration_is_recorded_so() {
    let signer = Signer::es256();
    let rows = Rows::opened();
    let f = fixture(&rows);
    let cookie = signed_in(&f, ALICE);
    let issued = challenge(&f, &cookie, KEY);
    let registered = signer.register_shaped(&issued.bytes, CREDENTIAL, &Shape::backup_eligible());
    let reply = enrol_with(
        &f,
        &cookie,
        KEY_2,
        &first(&issued.id, CREDENTIAL, &registered),
    );
    assert_eq!(
        reply.status,
        201,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    let all = f.journal.rows_of(&hosted_stream());
    let row = all.last().expect("the enrolment");
    assert_eq!(payload(row)["backup_eligible"], json!(true));
}

#[test]
fn a_first_enrolment_past_300_s_of_the_sign_in_is_stale_and_spends_its_challenge() {
    let signer = Signer::es256();
    let rows = Rows::opened();
    let f = fixture(&rows);
    let cookie = signed_in(&f, ALICE);
    f.clock.set(T0 + 200);
    let issued = challenge(&f, &cookie, KEY);
    let cookie = refreshed(&f, &cookie, ALICE, T0 + 250);
    f.clock
        .set_at(UtcNanos::from_parts(T0 + 300, 1).expect("a time"));
    let registered = signer.register(&issued.bytes, CREDENTIAL);
    let body = first(&issued.id, CREDENTIAL, &registered);
    stale(&enrol_with(&f, &cookie, KEY_2, &body));
    nothing_changed(&f, &rows);
    let again = signed_in(&f, ALICE);
    challenge_refused(&enrol_with(&f, &again, KEY_3, &body));
    nothing_changed(&f, &rows);
}

#[test]
fn a_second_enrolment_needs_passkey_step_up_and_own_row_step_up_is_not_judged_yet() {
    let signer = Signer::es256();
    let rows = holding(&signer);
    let f = fixture(&rows);
    let cookie = signed_in(&f, ALICE);
    let cookie = refreshed(&f, &cookie, ALICE, T0 + 600);
    let fresh = Signer::es256();
    let issued = challenge(&f, &cookie, KEY);
    let registered = fresh.register(&issued.bytes, CREDENTIAL);
    refused(
        &enrol_with(
            &f,
            &cookie,
            KEY_2,
            &first(&issued.id, CREDENTIAL, &registered),
        ),
        401,
        ProblemCode::StepUpRequired,
    );
    nothing_changed(&f, &rows);
    let issued = challenge(&f, &cookie, KEY_3);
    let registered = fresh.register(&issued.bytes, CREDENTIAL);
    let evidence = json!({
        "assertion_id": "01JAA3P4A000000000000000S1",
        "authenticated_at": time(T0 + 590), "method": "passkey",
    });
    let body = request(
        &issued.id,
        CREDENTIAL,
        &registered,
        evidence,
        signer.assert(&[7; 32], 1).json(OTHER_CREDENTIAL),
    );
    refused(
        &enrol_with(&f, &cookie, "k4Z-9_dddddddddd", &body),
        401,
        ProblemCode::StepUpMissing,
    );
    nothing_changed(&f, &rows);
    assert!(
        f.port.asked.lock().expect("unpoisoned").is_empty(),
        "no own-row step-up is judged until E9-4 S4 binds one, so no evidence counts"
    );
    challenge_refused(&enrol_with(&f, &cookie, "k5Z-9_eeeeeeeeee", &body));
}

/// A member who holds a passkey needs step-up for another however fresh the sign-in: a session
/// taken over within 300 s of its sign-in cannot add its own passkey.
#[test]
fn a_second_enrolment_on_a_fresh_sign_in_without_step_up_is_step_up_required() {
    let signer = Signer::es256();
    let rows = holding(&signer);
    let f = fixture(&rows);
    let cookie = signed_in(&f, ALICE);
    f.clock.set(T0 + 10);
    let issued = challenge(&f, &cookie, KEY);
    let registered = Signer::es256().register(&issued.bytes, CREDENTIAL);
    refused(
        &enrol_with(
            &f,
            &cookie,
            KEY_2,
            &first(&issued.id, CREDENTIAL, &registered),
        ),
        401,
        ProblemCode::StepUpRequired,
    );
    nothing_changed(&f, &rows);
    assert!(
        f.port.asked.lock().expect("unpoisoned").is_empty(),
        "no evidence was sent, so none was judged"
    );
}

#[test]
fn a_first_enrolment_carrying_step_up_or_half_of_it_is_invalid() {
    let signer = Signer::es256();
    let rows = Rows::opened();
    let f = fixture(&rows);
    let cookie = signed_in(&f, ALICE);
    let evidence = json!({
        "assertion_id": "01JAA3P4A000000000000000S1",
        "authenticated_at": time(T0), "method": "passkey",
    });
    let assertion = signer.assert(&[7; 32], 1).json(OTHER_CREDENTIAL);
    for (n, (step_up, step_up_assertion)) in [
        (evidence.clone(), Json::Null),
        (Json::Null, assertion.clone()),
        (evidence.clone(), assertion.clone()),
    ]
    .into_iter()
    .enumerate()
    {
        let issued = challenge(&f, &cookie, &format!("k{n}Z-9_aaaaaaaaaa"));
        let registered = signer.register(&issued.bytes, CREDENTIAL);
        let body = request(
            &issued.id,
            CREDENTIAL,
            &registered,
            step_up,
            step_up_assertion,
        );
        refused(
            &enrol_with(&f, &cookie, &format!("k{n}Z-9_bbbbbbbbbb"), &body),
            422,
            ProblemCode::Invalid,
        );
        nothing_changed(&f, &rows);
    }
}

#[test]
fn a_body_with_another_member_or_one_missing_is_invalid() {
    let signer = Signer::es256();
    let rows = Rows::opened();
    let f = fixture(&rows);
    let cookie = signed_in(&f, ALICE);
    for (n, edit) in [
        (|body: &mut Json| body["roles"] = json!(["owner"])) as fn(&mut Json),
        |body: &mut Json| {
            body.as_object_mut()
                .map(|members| members.remove("step_up_assertion"));
        },
    ]
    .into_iter()
    .enumerate()
    {
        let issued = challenge(&f, &cookie, &format!("k{n}Z-9_aaaaaaaaaa"));
        let registered = signer.register(&issued.bytes, CREDENTIAL);
        let mut body: Json =
            serde_json::from_slice(&first(&issued.id, CREDENTIAL, &registered)).expect("JSON");
        edit(&mut body);
        refused(
            &enrol_with(
                &f,
                &cookie,
                &format!("k{n}Z-9_bbbbbbbbbb"),
                &serde_json::to_vec(&body).expect("JSON"),
            ),
            422,
            ProblemCode::Invalid,
        );
        nothing_changed(&f, &rows);
    }
}

#[test]
fn a_registration_challenge_serves_one_submit_whatever_follows_and_a_same_key_retry_finds_it_spent_dec_2118_item_1()
 {
    let signer = Signer::es256();
    let rows = Rows::opened();
    let f = fixture(&rows);
    let cookie = signed_in(&f, ALICE);
    let issued = challenge(&f, &cookie, KEY);
    let wrong = signer.register_shaped(
        &issued.bytes,
        CREDENTIAL,
        &Shape {
            origin: "https://evil.test".to_owned(),
            ..Shape::good()
        },
    );
    registration_refused(&enrol_with(
        &f,
        &cookie,
        KEY_2,
        &first(&issued.id, CREDENTIAL, &wrong),
    ));
    let right = signer.register(&issued.bytes, CREDENTIAL);
    challenge_refused(&enrol_with(
        &f,
        &cookie,
        KEY_3,
        &first(&issued.id, CREDENTIAL, &right),
    ));
    nothing_changed(&f, &rows);
    let issued = challenge(&f, &cookie, "k4Z-9_dddddddddd");
    let right = signer.register(&issued.bytes, CREDENTIAL);
    let body = first(&issued.id, CREDENTIAL, &right);
    let opened = enrol_with(&f, &cookie, "k5Z-9_eeeeeeeeee", &body);
    assert_eq!(
        opened.status,
        201,
        "{:?}",
        String::from_utf8_lossy(&opened.body)
    );
    challenge_refused(&enrol_with(&f, &cookie, "k5Z-9_eeeeeeeeee", &body));
    assert_eq!(
        f.journal.rows_of(&hosted_stream()).len(),
        rows.rows.len() + 1,
        "a retry under the same key, its challenge spent, appends nothing more"
    );
}

#[test]
fn an_unknown_expired_or_foreign_registration_challenge_is_invalid_at_its_id() {
    let signer = Signer::es256();
    let rows = Rows::opened();
    let f = fixture(&rows);
    let alice = signed_in(&f, ALICE);
    let bob = signed_in(&f, BOB);
    let registered = signer.register(&[9; 32], CREDENTIAL);
    challenge_refused(&enrol_with(
        &f,
        &alice,
        KEY,
        &first("01JB3K6F0C9R2V7N4M8Q1T5W44", CREDENTIAL, &registered),
    ));
    let bobs = challenge(&f, &bob, KEY);
    let registered = signer.register(&bobs.bytes, CREDENTIAL);
    challenge_refused(&enrol_with(
        &f,
        &alice,
        KEY_2,
        &first(&bobs.id, CREDENTIAL, &registered),
    ));
    let step_up = post(
        &f,
        &format!("/v1/workspaces/{WORKSPACE}/step-up/challenges"),
        &alice,
        KEY_3,
        br#"{"action":{"kind":"approve","digest":"sha256:5555555555555555555555555555555555555555555555555555555555555555"}}"#,
    );
    assert_eq!(
        step_up.status,
        201,
        "{:?}",
        String::from_utf8_lossy(&step_up.body)
    );
    challenge_refused(&enrol_with(
        &f,
        &alice,
        "k4Z-9_dddddddddd",
        &first(&text(&step_up, "challenge_id"), CREDENTIAL, &registered),
    ));
    let issued = challenge(&f, &alice, "k5Z-9_eeeeeeeeee");
    f.clock.set(T0 + 300);
    let registered = signer.register(&issued.bytes, CREDENTIAL);
    let fresh = signed_in(&f, ALICE);
    challenge_refused(&enrol_with(
        &f,
        &fresh,
        "k6Z-9_ffffffffff",
        &first(&issued.id, CREDENTIAL, &registered),
    ));
    nothing_changed(&f, &rows);
}

#[test]
fn a_registration_that_does_not_read_is_invalid_with_no_path_and_spends_its_challenge() {
    let signer = Signer::es256();
    let rows = Rows::opened();
    let f = fixture(&rows);
    let cookie = signed_in(&f, ALICE);
    let shapes: Vec<(&str, Shape)> = vec![
        (
            "an assertion's ceremony",
            Shape {
                ceremony: "webauthn.get",
                ..Shape::good()
            },
        ),
        (
            "another origin",
            Shape {
                origin: "https://evil.test".to_owned(),
                ..Shape::good()
            },
        ),
        (
            "another relying party",
            Shape {
                rp_id: "evil.test".to_owned(),
                ..Shape::good()
            },
        ),
        (
            "no user verification",
            Shape {
                flags: 0x01 | support_passkey::AT,
                ..Shape::good()
            },
        ),
        (
            "an attestation other than none",
            Shape {
                fmt: "packed",
                ..Shape::good()
            },
        ),
    ];
    for (n, (name, shape)) in shapes.into_iter().enumerate() {
        let issued = challenge(&f, &cookie, &format!("k{n}Z-9_aaaaaaaaaa"));
        let registered = signer.register_shaped(&issued.bytes, CREDENTIAL, &shape);
        let reply = enrol_with(
            &f,
            &cookie,
            &format!("k{n}Z-9_bbbbbbbbbb"),
            &first(&issued.id, CREDENTIAL, &registered),
        );
        assert_eq!(reply.status, 422, "{name}");
        registration_refused(&reply);
        let good = signer.register(&issued.bytes, CREDENTIAL);
        challenge_refused(&enrol_with(
            &f,
            &cookie,
            &format!("k{n}Z-9_cccccccccc"),
            &first(&issued.id, CREDENTIAL, &good),
        ));
    }
    let issued = challenge(&f, &cookie, "k8Z-9_aaaaaaaaaa");
    let other_challenge = signer.register(&[3; 32], CREDENTIAL);
    registration_refused(&enrol_with(
        &f,
        &cookie,
        "k8Z-9_bbbbbbbbbb",
        &first(&issued.id, CREDENTIAL, &other_challenge),
    ));
    let issued = challenge(&f, &cookie, "k9Z-9_aaaaaaaaaa");
    let registered = signer.register(&issued.bytes, CREDENTIAL);
    registration_refused(&enrol_with(
        &f,
        &cookie,
        "k9Z-9_bbbbbbbbbb",
        &first(&issued.id, THIRD_CREDENTIAL, &registered),
    ));
    nothing_changed(&f, &rows);
}

#[test]
fn the_challenge_is_judged_before_the_session_and_the_session_before_the_registration() {
    let signer = Signer::es256();
    let rows = Rows::opened();
    let f = fixture(&rows);
    let cookie = signed_in(&f, ALICE);
    f.clock.set(T0 + 100);
    let issued = challenge(&f, &cookie, KEY);
    let bad = signer.register_shaped(
        &issued.bytes,
        CREDENTIAL,
        &Shape {
            fmt: "packed",
            ..Shape::good()
        },
    );
    let cookie = refreshed(&f, &cookie, ALICE, T0 + 301);
    stale(&enrol_with(
        &f,
        &cookie,
        KEY_2,
        &first(&issued.id, CREDENTIAL, &bad),
    ));
    f.clock.set(T0 + 10_000);
    let fresh = signed_in(&f, ALICE);
    let issued = challenge(&f, &fresh, KEY_3);
    f.clock.set(T0 + 10_300);
    let later = signed_in(&f, ALICE);
    challenge_refused(&enrol_with(
        &f,
        &later,
        "k4Z-9_dddddddddd",
        &first(&issued.id, CREDENTIAL, &bad),
    ));
    nothing_changed(&f, &rows);
}

#[test]
fn a_passkey_id_the_stream_ever_enrolled_is_invalid_with_no_path() {
    let signer = Signer::es256();
    let mut rows = Rows::opened();
    enrol(
        &mut rows,
        T0 - 600,
        enrolled(id(BOB), THIRD_CREDENTIAL, &signer, 0, None),
    );
    enrol(
        &mut rows,
        T0 - 500,
        enrolled(
            id(ALICE),
            CREDENTIAL,
            &signer,
            0,
            Some(T0 - 500 + COOL_OFF_S),
        ),
    );
    rows.edited("CredentialRemoved", T0 - 400, |envelope| {
        envelope["payload"] = removed(id(ALICE), CREDENTIAL);
        envelope["actor"] = json!({"kind": "user", "id": ALICE});
    });
    let f = fixture(&rows);
    let cookie = signed_in(&f, ALICE);
    for (n, passkey_id) in [THIRD_CREDENTIAL, CREDENTIAL].into_iter().enumerate() {
        let issued = challenge(&f, &cookie, &format!("k{n}Z-9_aaaaaaaaaa"));
        let registered = Signer::es256().register(&issued.bytes, passkey_id);
        registration_refused(&enrol_with(
            &f,
            &cookie,
            &format!("k{n}Z-9_bbbbbbbbbb"),
            &first(&issued.id, passkey_id, &registered),
        ));
    }
    nothing_changed(&f, &rows);
}

#[test]
fn a_member_whose_records_break_their_rule_is_journal_unavailable() {
    let signer = Signer::es256();
    let rows = Rows::opened();
    let f = fixture(&rows);
    let cookie = signed_in(&f, ALICE);
    let issued = challenge(&f, &cookie, KEY);
    let mut broken = Rows::opened();
    for secs in [T0 - 300, T0 - 200] {
        enrol(
            &mut broken,
            secs,
            enrolled(
                id(ALICE),
                OTHER_CREDENTIAL,
                &signer,
                0,
                Some(secs + COOL_OFF_S),
            ),
        );
    }
    f.journal
        .streams
        .lock()
        .expect("unpoisoned")
        .insert(hosted_stream(), broken.rows.clone());
    let registered = signer.register(&issued.bytes, CREDENTIAL);
    let body = first(&issued.id, CREDENTIAL, &registered);
    refused(
        &enrol_with(&f, &cookie, KEY_2, &body),
        503,
        ProblemCode::JournalUnavailable,
    );
    nothing_changed(&f, &broken);
    challenge_refused(&enrol_with(&f, &cookie, KEY_3, &body));
}

#[test]
fn a_non_member_is_refused_and_a_live_workspace_is_live_unavailable() {
    let signer = Signer::es256();
    let rows = Rows::opened();
    let f = fixture(&rows);
    let dave = signed_in(&f, DAVE);
    let registered = signer.register(&[9; 32], CREDENTIAL);
    refused(
        &enrol_with(
            &f,
            &dave,
            KEY,
            &first("01JB3K6F0C9R2V7N4M8Q1T5W44", CREDENTIAL, &registered),
        ),
        404,
        ProblemCode::NotFound,
    );
    let mut live = Rows { rows: Vec::new() };
    live.push(
        &hosted_stream(),
        "live",
        "StreamOpened",
        T0 - 1_000,
        json!({"stream_type": "control"}),
    );
    let f = fixture(&live);
    let alice = signed_in(&f, ALICE);
    refused(
        &enrol_with(
            &f,
            &alice,
            KEY,
            &first("01JB3K6F0C9R2V7N4M8Q1T5W44", CREDENTIAL, &registered),
        ),
        409,
        ProblemCode::LiveUnavailable,
    );
    nothing_changed(&f, &live);
}

#[test]
fn the_registration_encoder_writes_the_shape_mandate_passkey_reads() {
    let signer = Signer::es256();
    let challenge = [0x2a; 32];
    let registered = signer.register(&challenge, CREDENTIAL);
    let credential = mandate_passkey::enrol(
        &support_passkey::rp(),
        &mandate_passkey::Challenge::new(&challenge).expect("32 bytes"),
        &mandate_passkey::Registration {
            attestation_object: &registered.attestation_object,
            client_data_json: &registered.client_data_json,
        },
    )
    .expect("the oracle's registration reads");
    assert_eq!(credential, signer.credential(CREDENTIAL));
    assert_eq!(RP_ID, "app.owlhead.test");
}

#[test]
fn without_an_object_store_an_enrolment_is_journal_unavailable_and_appends_nothing() {
    let signer = Signer::es256();
    let rows = Rows::opened();
    let f = fixture_with(&rows, Unverified::Code(ProblemCode::StepUpMissing), false);
    let cookie = signed_in(&f, ALICE);
    let issued = challenge(&f, &cookie, KEY);
    let registered = signer.register(&issued.bytes, CREDENTIAL);
    refused(
        &enrol_with(
            &f,
            &cookie,
            KEY_2,
            &first(&issued.id, CREDENTIAL, &registered),
        ),
        503,
        ProblemCode::JournalUnavailable,
    );
    nothing_changed(&f, &rows);
}
