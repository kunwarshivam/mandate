//! The catalogue reads over HTTP (§4.1, §3.11 item 11, API-2, API-9, API-14; DEC-2136):
//! `GET /v1/workspaces/{ws}/instruments` and `…/signal-models`, users only, under the "Draft a
//! mandate version" row, read from the control stream and the store the version routes check
//! against.
//!
//! The oracle for each body's shape is the read model's own published example and its valid cases
//! (which `check_examples.py` validates against the schema): the test stores the objects and seeds
//! the rows that reproduce each, and the served body equals it apart from the members a test cannot
//! fix. Every body is also held to the members each schema requires and allows, read from the
//! schema files at every level. Elsewhere, expected bodies are computed here from the spec's own
//! words: the instrument oracle folds the test's own registrations with its own map of the bytes it
//! stored, never through the server's fold, and the properties below hold each read to exactly
//! what the create's checks (`versions::instruments`, V-007) admit.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use common::{MemJournal, Reply, WORKSPACE, hosted, send, serve};
use mandate_api::problem::{Effect, Problem, ProblemCode};
use mandate_api::wire::decode;
use mandate_api_server::challenges::Challenges;
use mandate_api_server::config::{SESSION_KEY_FILE, read_session_key};
use mandate_api_server::http::{self, App, Settings};
use mandate_api_server::lifecycle::Drain;
use mandate_api_server::objects::{ObjectStore, Objects};
use mandate_api_server::session::{COOKIE_NAME, OsRandom, Sessions};
use mandate_api_server::stepup::Unbound;
use mandate_api_server::token::{FixedKeys, Issuer};
use mandate_api_server::versions::{Pending, Refusal, check, instruments};
use mandate_artifacts_fs::FsArtifactStore;
use mandate_authn::{Algorithm, IssuerConfig};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_identity::{MembershipEvent, MembershipRecord, PrincipalId, Role};
use mandate_identity_seal::Seal;
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;
use proptest::prelude::*;

const APP: &str = "https://app.owlhead.test";
const T0: i64 = 1_791_000_000;
const ALICE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Z";
const BOB: &str = "01JB3K6F0C9R2V7N4M8Q1T5W41";
const CAROL: &str = "01JB3K6F0C9R2V7N4M8Q1T5W42";
const OTHER_WORKSPACE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W40";
const SPY_ID: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415";
const QQQ_ID: &str = "eb82ffb0-efd6-4f4e-9307-8fead28e04f6";
const INSTRUMENTS_SCHEMA: &str =
    include_str!("../../../schemas/workspace-api/read-models/instruments.schema.json");
const ENVELOPE_SCHEMA: &str = include_str!("../../../schemas/workspace-api/envelope.schema.json");
const INSTRUMENTS_EXAMPLE: &str =
    include_str!("../../../schemas/workspace-api/examples/read-models.instruments.json");
const INSTRUMENTS_VALID: &str =
    include_str!("../../../schemas/workspace-api/examples/read-models.instruments.valid.json");
const PRICE_SCHEMA: &str =
    include_str!("../../../schemas/workspace-api/read-models/instrument-price.schema.json");
const PRICE_EXAMPLE: &str =
    include_str!("../../../schemas/workspace-api/examples/read-models.instrument-price.json");
const MODELS_EXAMPLE: &str =
    include_str!("../../../schemas/workspace-api/examples/read-models.signal-models.json");
const MODELS_VALID: &str =
    include_str!("../../../schemas/workspace-api/examples/read-models.signal-models.valid.json");

type Json = serde_json::Value;

struct Fixture {
    addr: std::net::SocketAddr,
    journal: Arc<MemJournal>,
    store: Arc<Objects<FsArtifactStore>>,
    sessions: Arc<Sessions>,
}

static DIRS: Mutex<u32> = Mutex::new(0);

fn fixture() -> Fixture {
    built(true)
}

fn built(with_store: bool) -> Fixture {
    built_with(with_store, None)
}

/// [`built`], the price read given `bars` as its `MANDATE_BARS` root.
fn built_with(with_store: bool, bars: Option<std::path::PathBuf>) -> Fixture {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = std::env::temp_dir().join(format!(
        "mandate-api-server-catalogue-{}-{n}",
        std::process::id()
    ));
    drop(n);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let file = dir.join(SESSION_KEY_FILE);
    std::fs::write(&file, [0x46_u8; 32]).expect("the key");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("mode");
    let key = read_session_key(&dir).expect("the key reads");
    let store = Arc::new(Objects::new(
        FsArtifactStore::open(dir.join("store")).expect("a store"),
    ));
    let journal = Arc::new(MemJournal::default());
    let records = vec![
        activated(1, principal(ALICE), &[Role::Operator]),
        activated(2, principal(BOB), &[Role::Viewer]),
    ];
    let sessions = Arc::new(Sessions::new(&key, Box::new(OsRandom::new())));
    let config = IssuerConfig::new(
        "https://issuer.test/auth/v1",
        &["authenticated"],
        &[Algorithm::Es256],
    )
    .expect("the issuer");
    let app = App {
        settings: Settings {
            app_origin: APP.to_owned(),
        },
        sessions: Arc::clone(&sessions),
        tokens: Arc::new(Issuer::new(config, Arc::new(FixedKeys(None)))),
        links: Arc::new(common::LinksAt(dir.join("absent"))),
        clock: common::ManualClock::at(T0) as Arc<dyn mandate_api_server::clock::Clock>,
        journal: Arc::clone(&journal) as Arc<dyn mandate_api_server::journal::Journal>,
        members: Arc::new(move |_: &[StoredEvent]| Ok(records.clone())),
        hosted: hosted(),
        artifacts: None,
        objects: with_store.then(|| Arc::clone(&store) as Arc<dyn ObjectStore>),
        step_up: Arc::new(Unbound),
        challenges: Arc::new(Challenges::default()),
        connect: None,
        reduction: Arc::default(),
        folds: Arc::default(),
        bars,
    };
    Fixture {
        addr: serve(http::api(app, Drain::new())),
        journal,
        store,
        sessions,
    }
}

fn principal(text: &str) -> PrincipalId {
    PrincipalId::from_ulid_text(text).expect("a ULID")
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

fn signed_in(f: &Fixture, who: &str) -> String {
    f.sessions
        .open(principal(who), "sha256:aaaa".to_owned(), None, at(T0))
        .expect("opened")
        .value()
        .to_owned()
}

fn ctl() -> String {
    format!("ctl:{WORKSPACE}")
}

fn json(text: &str) -> Value {
    parse(text.as_bytes()).expect("JSON")
}

/// `sha256:` and the hex digest of `bytes`, as the spec's references name an object.
fn reference(bytes: &[u8]) -> String {
    format!("sha256:{}", Digest::of(bytes).to_hex())
}

/// The `recorded_at` the test gives the control stream's row `seq`.
fn recorded_at(seq: u64) -> String {
    format!("2026-10-17T10:{:02}:{:02}.000000000Z", seq / 60, seq % 60)
}

fn row(seq: u64, event_type: &str, payload: &str) -> StoredEvent {
    let stream = ctl();
    let event_id = format!("01JBH3BTZ8K2M4N6P8R0S2T{seq:03}");
    let text = format!(
        r#"{{"environment":"paper","event_id":"{event_id}","event_type":"{event_type}","payload":{payload},"stream_id":"{stream}"}}"#
    );
    let body = to_canonical(&json(&text));
    StoredEvent {
        stream_id: stream,
        seq,
        event_id,
        event_type: event_type.to_owned(),
        schema_version: 1,
        environment: "paper".to_owned(),
        recorded_at: recorded_at(seq),
        prev_hash: Digest::ZERO,
        hash: Digest::of(&body),
        body,
    }
}

/// An `instrument_snapshot` registration of the object under `hash`.
fn snapshot_payload(hash: &str) -> String {
    format!(
        r#"{{"admits_instruments":null,"content_hash":"{hash}","kind":"instrument_snapshot","model_id":null,"model_version":null,"params":[]}}"#
    )
}

/// An instrument snapshot object as DEC-523 pins one.
fn snapshot_object(id: &str, symbol: &str, asset_class: &str) -> String {
    format!(
        r#"{{"asset_class":"{asset_class}","etp":"plain","etp_classified_at":"2026-10-05T13:30:00Z","etp_source":"nasdaq_trader_symbol_directory","exchange":"arca","increment":"whole","instrument_id":"{id}","symbol":"{symbol}"}}"#
    )
}

/// Stores `object` and returns its reference.
fn stored(f: &Fixture, object: &str) -> String {
    f.store.put(object.as_bytes()).expect("stored");
    reference(object.as_bytes())
}

/// The control stream: opened, then each of `records`, numbered from 2.
fn seed(f: &Fixture, records: &[(&str, String)]) -> Vec<StoredEvent> {
    let mut rows = vec![row(1, "StreamOpened", "{}")];
    for (event_type, payload) in records {
        let seq = u64::try_from(rows.len()).expect("a count") + 1;
        rows.push(row(seq, event_type, payload));
    }
    f.journal
        .streams
        .lock()
        .expect("unpoisoned")
        .insert(ctl(), rows.clone());
    rows
}

fn ws(path: &str) -> String {
    format!("/v1/workspaces/{WORKSPACE}{path}")
}

fn get(f: &Fixture, path: &str, cookie: Option<&str>) -> Reply {
    match cookie {
        Some(cookie) => {
            let cookie = format!("{COOKIE_NAME}={cookie}");
            send(f.addr, "GET", path, &[("Cookie", cookie.as_str())], b"")
        }
        None => send(f.addr, "GET", path, &[], b""),
    }
}

fn ok(reply: &Reply) -> Json {
    assert_eq!(
        reply.status,
        200,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    assert_eq!(reply.header("content-type"), Some("application/json"));
    serde_json::from_slice(&reply.body).expect("a JSON body")
}

fn refused(reply: &Reply, status: u16, code: ProblemCode) {
    assert_eq!(
        reply.status,
        status,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    let problem = decode::<Problem>(&reply.body).expect("a problem");
    assert_eq!(problem.code, code);
    assert_eq!(problem.effect, Effect::None);
}

fn names(value: &Json) -> BTreeSet<String> {
    value
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect()
}

fn listed(value: &Json) -> BTreeSet<String> {
    value
        .as_array()
        .expect("a list")
        .iter()
        .map(|name| name.as_str().expect("a name").to_owned())
        .collect()
}

/// The body against `schema`: the envelope's and the read model's required members and nothing
/// either does not list, each item's members exactly its closed `$defs` entry's required list and,
/// at every level below, within what that entry allows ([`closed`]), and `as_of` naming the control
/// stream's last row only (API-14).
fn conforms(body: &Json, schema: &str, item: &str, def: &str, control: &[StoredEvent]) {
    let schema: Json = serde_json::from_str(schema).expect("the schema");
    let envelope: Json = serde_json::from_str(ENVELOPE_SCHEMA).expect("the envelope");
    let mut required = listed(&envelope["$defs"]["Envelope"]["required"]);
    required.extend(listed(&schema["required"]));
    let mut allowed = names(&envelope["$defs"]["Envelope"]["properties"]);
    allowed.extend(names(&schema["properties"]));
    let served = names(body);
    assert!(required.is_subset(&served), "{required:?} in {served:?}");
    assert!(served.is_subset(&allowed), "{served:?} within {allowed:?}");
    let shape = &schema["$defs"][def];
    assert_eq!(shape["additionalProperties"], Json::Bool(false));
    for entry in body[item].as_array().expect("a list") {
        assert_eq!(names(entry), listed(&shape["required"]), "{entry}");
        closed(entry, shape);
    }
    assert_eq!(body["api_version"], "v1");
    assert_eq!(body["served_at"], "2026-10-03T04:00:00.000000000Z");
    let last = control.last().expect("a row");
    let mark = serde_json::json!([{
        "stream_id": last.stream_id,
        "seq": last.seq,
        "hash": format!("sha256:{}", last.hash),
        "recorded_at": last.recorded_at,
    }]);
    assert_eq!(body["as_of"], mark);
}

/// `value` against `shape`, a part of the read model's schema file: an object's members within its
/// `properties` when it is closed, its `required` all present, and each member against its own
/// shape; each list item against `items`; a `oneOf` against the branch naming the value's type.
fn closed(value: &Json, shape: &Json) {
    if let Some(branches) = shape["oneOf"].as_array() {
        let kind = match value {
            Json::Null => "null",
            Json::Object(_) => "object",
            _ => panic!("{value} is neither branch of {shape}"),
        };
        let branch = branches
            .iter()
            .find(|branch| branch["type"] == kind)
            .unwrap_or_else(|| panic!("{value} in {shape}"));
        closed(value, branch);
        return;
    }
    if let Some(items) = value.as_array() {
        if let Some(each) = shape.get("items") {
            for item in items {
                closed(item, each);
            }
        }
        return;
    }
    let (Some(members), Some(properties)) = (value.as_object(), shape["properties"].as_object())
    else {
        return;
    };
    if shape["additionalProperties"] == Json::Bool(false) {
        for name in members.keys() {
            assert!(properties.contains_key(name), "{name} in {value}");
        }
    }
    for name in shape["required"].as_array().into_iter().flatten() {
        let name = name.as_str().expect("a name");
        assert!(members.contains_key(name), "{name} in {value}");
    }
    for (name, member) in members {
        if let Some(each) = properties.get(name) {
            closed(member, each);
        }
    }
}

/// One instrument as the read states it.
fn instrument(id: &str, symbol: &str, class: &str, snapshot: &str, seq: u64) -> Json {
    serde_json::json!({
        "instrument_id": id,
        "symbol": symbol,
        "asset_class": class,
        "snapshot": snapshot,
        "registered_at": recorded_at(seq),
    })
}

mod instruments_read {
    use super::*;

    #[test]
    fn no_session_is_the_one_401() {
        let f = fixture();
        seed(&f, &[]);
        let reply = get(&f, &ws("/instruments"), None);
        refused(&reply, 401, ProblemCode::Unauthenticated);
    }

    #[test]
    fn a_non_member_and_another_workspace_are_the_one_404() {
        let f = fixture();
        seed(&f, &[]);
        let carol = signed_in(&f, CAROL);
        refused(
            &get(&f, &ws("/instruments"), Some(&carol)),
            404,
            ProblemCode::NotFound,
        );
        let alice = signed_in(&f, ALICE);
        let other = format!("/v1/workspaces/{OTHER_WORKSPACE}/instruments");
        refused(&get(&f, &other, Some(&alice)), 404, ProblemCode::NotFound);
    }

    #[test]
    fn a_viewer_lacks_the_draft_row_and_is_forbidden() {
        let f = fixture();
        seed(&f, &[]);
        let bob = signed_in(&f, BOB);
        refused(
            &get(&f, &ws("/instruments"), Some(&bob)),
            403,
            ProblemCode::Forbidden,
        );
    }

    #[test]
    fn an_unreadable_control_stream_is_membership_unavailable() {
        let f = fixture();
        seed(&f, &[]);
        f.journal.failing.lock().expect("unpoisoned").insert(ctl());
        let alice = signed_in(&f, ALICE);
        refused(
            &get(&f, &ws("/instruments"), Some(&alice)),
            503,
            ProblemCode::MembershipUnavailable,
        );
    }

    #[test]
    fn no_registration_is_an_empty_list_as_of_the_control_stream() {
        let f = fixture();
        let control = seed(&f, &[]);
        let alice = signed_in(&f, ALICE);
        let body = ok(&get(&f, &ws("/instruments"), Some(&alice)));
        assert_eq!(body["instruments"], serde_json::json!([]));
        conforms(
            &body,
            INSTRUMENTS_SCHEMA,
            "instruments",
            "Instrument",
            &control,
        );
    }

    #[test]
    fn the_latest_registration_per_id_wins_sorted_by_symbol_then_id() {
        let f = fixture();
        let spy_old = stored(&f, &snapshot_object(SPY_ID, "SPYX", "us_equity"));
        let qqq = stored(&f, &snapshot_object(QQQ_ID, "QQQ", "us_equity"));
        let spy = stored(&f, &snapshot_object(SPY_ID, "SPY", "us_equity"));
        let btc_id = "00000000-0000-4000-8000-000000000001";
        let btc = stored(&f, &snapshot_object(btc_id, "BTCUSD", "crypto"));
        let control = seed(
            &f,
            &[
                ("ConfigSnapshotRegistered", snapshot_payload(&spy_old)),
                ("ConfigSnapshotRegistered", snapshot_payload(&qqq)),
                ("ConfigSnapshotRegistered", snapshot_payload(&spy)),
                ("ConfigSnapshotRegistered", snapshot_payload(&btc)),
                ("StreamNote", "{}".to_owned()),
            ],
        );
        let alice = signed_in(&f, ALICE);
        let body = ok(&get(&f, &ws("/instruments"), Some(&alice)));
        assert_eq!(
            body["instruments"],
            serde_json::json!([
                instrument(btc_id, "BTCUSD", "crypto", &btc, 5),
                instrument(QQQ_ID, "QQQ", "us_equity", &qqq, 3),
                instrument(SPY_ID, "SPY", "us_equity", &spy, 4),
            ])
        );
        conforms(
            &body,
            INSTRUMENTS_SCHEMA,
            "instruments",
            "Instrument",
            &control,
        );
    }

    #[test]
    fn one_symbol_on_two_ids_sorts_by_id() {
        let f = fixture();
        let high = stored(&f, &snapshot_object(QQQ_ID, "SAME", "us_equity"));
        let low = stored(&f, &snapshot_object(SPY_ID, "SAME", "us_equity"));
        seed(
            &f,
            &[
                ("ConfigSnapshotRegistered", snapshot_payload(&high)),
                ("ConfigSnapshotRegistered", snapshot_payload(&low)),
            ],
        );
        let alice = signed_in(&f, ALICE);
        let body = ok(&get(&f, &ws("/instruments"), Some(&alice)));
        assert_eq!(
            body["instruments"],
            serde_json::json!([
                instrument(SPY_ID, "SAME", "us_equity", &low, 3),
                instrument(QQQ_ID, "SAME", "us_equity", &high, 2),
            ])
        );
    }

    #[test]
    fn other_kinds_and_other_events_are_not_instruments() {
        let f = fixture();
        let spy = stored(&f, &snapshot_object(SPY_ID, "SPY", "us_equity"));
        let fee = format!(
            r#"{{"admits_instruments":null,"content_hash":"{spy}","kind":"fee_config","model_id":null,"model_version":null,"params":[]}}"#
        );
        seed(
            &f,
            &[
                ("ConfigSnapshotRegistered", fee),
                ("ConfigSnapshotNoted", snapshot_payload(&spy)),
            ],
        );
        let alice = signed_in(&f, ALICE);
        let body = ok(&get(&f, &ws("/instruments"), Some(&alice)));
        assert_eq!(body["instruments"], serde_json::json!([]));
    }

    #[test]
    fn a_snapshot_that_is_not_pinnable_is_omitted() {
        let f = fixture();
        let upper = stored(
            &f,
            &snapshot_object("B0B6DD9D-8B9B-48A9-BA46-B9D54906E415", "SPY", "us_equity"),
        );
        let short = stored(
            &f,
            &snapshot_object("b0b6dd9d-8b9b-48a9-bb46", "SHORT", "us_equity"),
        );
        let extra = stored(
            &f,
            &snapshot_object(&format!("{SPY_ID}-0"), "EXTRA", "us_equity"),
        );
        let long = stored(&f, &snapshot_object(QQQ_ID, &"Q".repeat(33), "us_equity"));
        let empty = stored(
            &f,
            &snapshot_object("00000000-0000-4000-8000-000000000002", "", "us_equity"),
        );
        let option = stored(
            &f,
            &snapshot_object("00000000-0000-4000-8000-000000000003", "OPT", "us_option"),
        );
        let classless = stored(
            &f,
            r#"{"instrument_id":"00000000-0000-4000-8000-000000000004","symbol":"NOCLS"}"#,
        );
        let edge = stored(&f, &snapshot_object(SPY_ID, &"S".repeat(32), "us_equity"));
        seed(
            &f,
            &[
                ("ConfigSnapshotRegistered", snapshot_payload(&upper)),
                ("ConfigSnapshotRegistered", snapshot_payload(&short)),
                ("ConfigSnapshotRegistered", snapshot_payload(&extra)),
                ("ConfigSnapshotRegistered", snapshot_payload(&long)),
                ("ConfigSnapshotRegistered", snapshot_payload(&empty)),
                ("ConfigSnapshotRegistered", snapshot_payload(&option)),
                ("ConfigSnapshotRegistered", snapshot_payload(&classless)),
                ("ConfigSnapshotRegistered", snapshot_payload(&edge)),
            ],
        );
        let alice = signed_in(&f, ALICE);
        let body = ok(&get(&f, &ws("/instruments"), Some(&alice)));
        assert_eq!(
            body["instruments"],
            serde_json::json!([instrument(SPY_ID, &"S".repeat(32), "us_equity", &edge, 9)])
        );
    }

    #[test]
    fn a_later_registration_that_is_not_pinnable_hides_the_id() {
        let f = fixture();
        let spy = stored(&f, &snapshot_object(SPY_ID, "SPY", "us_equity"));
        let later = stored(&f, &snapshot_object(SPY_ID, "SPY", "us_option"));
        seed(
            &f,
            &[
                ("ConfigSnapshotRegistered", snapshot_payload(&spy)),
                ("ConfigSnapshotRegistered", snapshot_payload(&later)),
            ],
        );
        let alice = signed_in(&f, ALICE);
        let body = ok(&get(&f, &ws("/instruments"), Some(&alice)));
        assert_eq!(body["instruments"], serde_json::json!([]));
    }

    #[test]
    fn a_snapshot_naming_no_symbol_leaves_the_earlier_one() {
        let f = fixture();
        let spy = stored(&f, &snapshot_object(SPY_ID, "SPY", "us_equity"));
        let nameless = stored(&f, &format!(r#"{{"instrument_id":"{SPY_ID}"}}"#));
        let listless = stored(&f, "[1]");
        seed(
            &f,
            &[
                ("ConfigSnapshotRegistered", snapshot_payload(&spy)),
                ("ConfigSnapshotRegistered", snapshot_payload(&nameless)),
                ("ConfigSnapshotRegistered", snapshot_payload(&listless)),
            ],
        );
        let alice = signed_in(&f, ALICE);
        let body = ok(&get(&f, &ws("/instruments"), Some(&alice)));
        assert_eq!(
            body["instruments"],
            serde_json::json!([instrument(SPY_ID, "SPY", "us_equity", &spy, 2)])
        );
    }

    #[test]
    fn any_unreadable_snapshot_is_journal_unavailable_never_a_shorter_list() {
        let missing = reference(b"never stored");
        let cases = [
            snapshot_payload(&missing),
            snapshot_payload("not a reference"),
            r#"{"kind":"instrument_snapshot"}"#.to_owned(),
        ];
        for broken in cases {
            let f = fixture();
            let spy = stored(&f, &snapshot_object(SPY_ID, "SPY", "us_equity"));
            seed(
                &f,
                &[
                    ("ConfigSnapshotRegistered", broken.clone()),
                    ("ConfigSnapshotRegistered", snapshot_payload(&spy)),
                ],
            );
            let alice = signed_in(&f, ALICE);
            refused(
                &get(&f, &ws("/instruments"), Some(&alice)),
                503,
                ProblemCode::JournalUnavailable,
            );
        }
    }

    #[test]
    fn a_stored_object_that_does_not_parse_is_journal_unavailable() {
        let f = fixture();
        let garbled = stored(&f, "{not json");
        seed(
            &f,
            &[("ConfigSnapshotRegistered", snapshot_payload(&garbled))],
        );
        let alice = signed_in(&f, ALICE);
        refused(
            &get(&f, &ws("/instruments"), Some(&alice)),
            503,
            ProblemCode::JournalUnavailable,
        );
    }

    #[test]
    fn no_store_is_journal_unavailable() {
        let f = built(false);
        seed(&f, &[]);
        let alice = signed_in(&f, ALICE);
        refused(
            &get(&f, &ws("/instruments"), Some(&alice)),
            503,
            ProblemCode::JournalUnavailable,
        );
    }

    #[test]
    fn a_write_method_is_the_one_401() {
        let f = fixture();
        seed(&f, &[]);
        let alice = signed_in(&f, ALICE);
        let cookie = format!("{COOKIE_NAME}={alice}");
        let reply = send(
            f.addr,
            "DELETE",
            &ws("/instruments"),
            &[("Cookie", cookie.as_str())],
            b"",
        );
        refused(&reply, 401, ProblemCode::Unauthenticated);
    }
}

/// One registration a random sequence holds: its object, or a reference the store does not hold.
#[derive(Debug, Clone)]
enum Registered {
    Object {
        id: usize,
        symbol: usize,
        class: usize,
    },
    Absent,
}

const IDS: [&str; 4] = [
    SPY_ID,
    QQQ_ID,
    "00000000-0000-4000-8000-00000000000a",
    "NOT-A-UUID",
];
const SYMBOLS: [&str; 4] = ["SPY", "QQQ", "", "SYMBOL_THAT_IS_LONGER_THAN_32_CHAR"];
const CLASSES: [&str; 3] = ["us_equity", "crypto", "us_option"];

fn registered() -> impl Strategy<Value = Registered> {
    prop_oneof![
        8 => (0..IDS.len(), 0..SYMBOLS.len(), 0..CLASSES.len())
            .prop_map(|(id, symbol, class)| Registered::Object { id, symbol, class }),
        1 => Just(Registered::Absent),
    ]
}

/// The oracle: the latest registration per id, from the test's own record of what it stored,
/// then only the pinnable ones (DEC-2136): `None` when any registration's object is absent.
fn expected(sequence: &[Registered]) -> Option<BTreeMap<String, (String, String, u64)>> {
    let mut latest: BTreeMap<String, (String, String, u64)> = BTreeMap::new();
    for (i, registration) in sequence.iter().enumerate() {
        let Registered::Object { id, symbol, class } = registration else {
            return None;
        };
        let seq = u64::try_from(i).expect("a count") + 2;
        latest.insert(
            IDS[*id].to_owned(),
            (SYMBOLS[*symbol].to_owned(), CLASSES[*class].to_owned(), seq),
        );
    }
    let uuid = |id: &str| {
        id.len() == 36
            && id.char_indices().all(|(i, c)| match i {
                8 | 13 | 18 | 23 => c == '-',
                _ => c.is_ascii_digit() || ('a'..='f').contains(&c),
            })
    };
    latest.retain(|id, (symbol, class, _)| {
        uuid(id)
            && (1..=32).contains(&symbol.chars().count())
            && (class == "us_equity" || class == "crypto")
    });
    Some(latest)
}

/// The document the create checks, pinning `id` with `symbol`.
fn pinning(id: &str, symbol: &str) -> Value {
    let text = format!(
        r#"{{"universe":{{"pinned_instruments":[{{"asset_class":"us_equity","asset_id":"{id}","symbol":"{symbol}"}}]}}}}"#
    );
    json(&text)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    /// The read lists exactly the oracle's instruments, each a pair the create's instrument check
    /// admits, and every pinnable pair the check admits is listed; one unreadable snapshot makes
    /// both the read and the check refuse.
    #[test]
    fn the_read_is_what_the_create_admits(sequence in prop::collection::vec(registered(), 0..8)) {
        let f = fixture();
        let mut records = Vec::new();
        for registration in &sequence {
            let hash = match registration {
                Registered::Object { id, symbol, class } => {
                    stored(&f, &snapshot_object(IDS[*id], SYMBOLS[*symbol], CLASSES[*class]))
                }
                Registered::Absent => reference(b"absent"),
            };
            records.push(("ConfigSnapshotRegistered", snapshot_payload(&hash)));
        }
        let control = seed(&f, &records);
        let alice = signed_in(&f, ALICE);
        let reply = get(&f, &ws("/instruments"), Some(&alice));
        let store = f.store.as_ref();
        let Some(oracle) = expected(&sequence) else {
            refused(&reply, 503, ProblemCode::JournalUnavailable);
            prop_assert_eq!(
                instruments(&control, store, &pinning(SPY_ID, "SPY")),
                Err(Refusal::Unreadable)
            );
            return Ok(());
        };
        let body = ok(&reply);
        let mut wanted: Vec<(&String, &(String, String, u64))> = oracle.iter().collect();
        wanted.sort_by(|a, b| (&a.1.0, a.0).cmp(&(&b.1.0, b.0)));
        let wanted: Vec<Json> = wanted
            .iter()
            .map(|(id, (symbol, class, seq))| {
                let hash = reference(snapshot_object(id, symbol, class).as_bytes());
                instrument(id, symbol, class, &hash, *seq)
            })
            .collect();
        prop_assert_eq!(&body["instruments"], &Json::Array(wanted));
        let listed: BTreeSet<(String, String)> = oracle
            .iter()
            .map(|(id, (symbol, _, _))| (id.clone(), symbol.clone()))
            .collect();
        conforms(&body, INSTRUMENTS_SCHEMA, "instruments", "Instrument", &control);
        for id in IDS {
            for symbol in SYMBOLS {
                let admitted = instruments(&control, store, &pinning(id, symbol)).is_ok();
                let pair = (id.to_owned(), symbol.to_owned());
                if listed.contains(&pair) {
                    prop_assert!(admitted, "{pair:?} is listed but refused");
                }
                let pinnable = oracle.contains_key(id);
                if admitted && pinnable {
                    prop_assert!(listed.contains(&pair), "{pair:?} is admitted but not listed");
                }
            }
        }
    }
}

const MODELS_SCHEMA: &str =
    include_str!("../../../schemas/workspace-api/read-models/signal-models.schema.json");
const MANDATE: &str = include_str!("../../mandate-cli/tests/fixtures/spy_mandate.json");
const MA: &str = "quant.ma_crossover";

/// A `model_version` registration naming `content_hash`.
fn model_payload(id: &str, version: &str, hash: &str, params: &[&str], admits: bool) -> String {
    let params: Vec<String> = params.iter().map(|p| format!("\"{p}\"")).collect();
    format!(
        r#"{{"admits_instruments":{admits},"content_hash":"{hash}","kind":"model_version","model_id":"{id}","model_version":"{version}","params":[{}]}}"#,
        params.join(",")
    )
}

/// A quant model's content object as DEC-518 item 3 states one, with members the projection
/// must never serve.
fn content_object(id: &str, version: &str) -> String {
    format!(
        r#"{{"authorship":"platform","code":[{{"path":"crates/mandate-modelhost/src/ma_crossover.rs","sha256":"3434343434343434343434343434343434343434343434343434343434343434"}}],"content_version":1,"inputs":{{"bars":"daily_close"}},"kind":"quant_model_content","methodology":"Long when the fast mean is above the slow mean.","model_id":"{id}","model_version":"{version}","output":{{"flat":"none"}},"params_schema":{{"constraints":["fast_periods < slow_periods"],"extra":true,"params":[{{"default":5,"max":1000,"min":1,"name":"fast_periods","type":"integer"}},{{"max":1000,"min":2,"name":"slow_periods","type":"integer"}}]}},"prompt":"never served"}}"#
    )
}

/// [`content_object`]'s projection.
fn projected() -> Json {
    serde_json::json!({
        "authorship": "platform",
        "methodology": "Long when the fast mean is above the slow mean.",
        "params_schema": {
            "constraints": ["fast_periods < slow_periods"],
            "params": [
                {"max": 1000, "min": 1, "name": "fast_periods", "type": "integer"},
                {"max": 1000, "min": 2, "name": "slow_periods", "type": "integer"},
            ],
        },
    })
}

fn model(id: &str, version: &str, hash: &str, params: &[&str], content: Json, seq: u64) -> Json {
    serde_json::json!({
        "model_id": id,
        "model_version": version,
        "content_hash": hash,
        "params": params,
        "admits_instruments": false,
        "content": content,
        "registered_at": recorded_at(seq),
    })
}

mod signal_models_read {
    use super::*;

    #[test]
    fn no_session_a_non_member_and_a_viewer_are_refused_by_the_draft_row() {
        let f = fixture();
        seed(&f, &[]);
        refused(
            &get(&f, &ws("/signal-models"), None),
            401,
            ProblemCode::Unauthenticated,
        );
        let carol = signed_in(&f, CAROL);
        refused(
            &get(&f, &ws("/signal-models"), Some(&carol)),
            404,
            ProblemCode::NotFound,
        );
        let bob = signed_in(&f, BOB);
        refused(
            &get(&f, &ws("/signal-models"), Some(&bob)),
            403,
            ProblemCode::Forbidden,
        );
        let alice = signed_in(&f, ALICE);
        let other = format!("/v1/workspaces/{OTHER_WORKSPACE}/signal-models");
        refused(&get(&f, &other, Some(&alice)), 404, ProblemCode::NotFound);
    }

    #[test]
    fn no_registration_is_an_empty_list_as_of_the_control_stream() {
        let f = fixture();
        let control = seed(&f, &[]);
        let alice = signed_in(&f, ALICE);
        let body = ok(&get(&f, &ws("/signal-models"), Some(&alice)));
        assert_eq!(body["models"], serde_json::json!([]));
        conforms(&body, MODELS_SCHEMA, "models", "Model", &control);
    }

    #[test]
    fn the_latest_registration_per_id_wins_whatever_its_version_sorted_by_id() {
        let f = fixture();
        let old = stored(&f, &content_object(MA, "1.0.0"));
        let new = stored(&f, &content_object(MA, "2.0.0"));
        let alpha = stored(&f, &content_object("fast.alpha", "1.0.0"));
        let params = ["fast_periods", "slow_periods"];
        let control = seed(
            &f,
            &[
                (
                    "ConfigSnapshotRegistered",
                    model_payload(MA, "2.0.0", &new, &params, false),
                ),
                (
                    "ConfigSnapshotRegistered",
                    model_payload("fast.alpha", "1.0.0", &alpha, &["window"], false),
                ),
                (
                    "ConfigSnapshotRegistered",
                    model_payload(MA, "1.0.0", &old, &params, false),
                ),
            ],
        );
        let alice = signed_in(&f, ALICE);
        let body = ok(&get(&f, &ws("/signal-models"), Some(&alice)));
        assert_eq!(
            body["models"],
            serde_json::json!([
                model("fast.alpha", "1.0.0", &alpha, &["window"], projected(), 3),
                model(MA, "1.0.0", &old, &params, projected(), 4),
            ])
        );
        conforms(&body, MODELS_SCHEMA, "models", "Model", &control);
    }

    #[test]
    fn params_are_ascending_and_admits_instruments_is_the_registrations() {
        let f = fixture();
        let hash = stored(&f, &content_object(MA, "1.0.0"));
        seed(
            &f,
            &[(
                "ConfigSnapshotRegistered",
                model_payload(MA, "1.0.0", &hash, &["slow_periods", "fast_periods"], true),
            )],
        );
        let alice = signed_in(&f, ALICE);
        let body = ok(&get(&f, &ws("/signal-models"), Some(&alice)));
        assert_eq!(
            body["models"][0]["params"],
            serde_json::json!(["fast_periods", "slow_periods"])
        );
        assert_eq!(body["models"][0]["admits_instruments"], Json::Bool(true));
    }

    #[test]
    fn content_the_store_cannot_give_or_parse_is_null_and_the_list_stands() {
        let f = fixture();
        let missing = reference(b"no content stored");
        let garbled = stored(&f, "{not json");
        let listed = stored(&f, "[1,2]");
        seed(
            &f,
            &[
                (
                    "ConfigSnapshotRegistered",
                    model_payload("fast.a", "1.0.0", &missing, &[], false),
                ),
                (
                    "ConfigSnapshotRegistered",
                    model_payload("fast.b", "1.0.0", &garbled, &[], false),
                ),
                (
                    "ConfigSnapshotRegistered",
                    model_payload("fast.c", "1.0.0", &listed, &[], false),
                ),
            ],
        );
        let alice = signed_in(&f, ALICE);
        let body = ok(&get(&f, &ws("/signal-models"), Some(&alice)));
        assert_eq!(
            body["models"],
            serde_json::json!([
                model("fast.a", "1.0.0", &missing, &[], Json::Null, 2),
                model("fast.b", "1.0.0", &garbled, &[], Json::Null, 3),
                model("fast.c", "1.0.0", &listed, &[], Json::Null, 4),
            ])
        );
    }

    #[test]
    fn the_projection_keeps_only_typed_members_member_by_member() {
        let f = fixture();
        let odd = stored(
            &f,
            r#"{"authorship":7,"methodology":"m","params_schema":{"constraints":["a",1],"params":[{"name":"x","type":3,"min":"1","max":4}]}}"#,
        );
        let nameless = stored(
            &f,
            r#"{"authorship":"platform","methodology":["m"],"params_schema":{"constraints":["c"],"params":[{"type":"integer"}]}}"#,
        );
        let bare = stored(&f, r#"{"params_schema":[1]}"#);
        seed(
            &f,
            &[
                (
                    "ConfigSnapshotRegistered",
                    model_payload("fast.a", "1.0.0", &odd, &[], false),
                ),
                (
                    "ConfigSnapshotRegistered",
                    model_payload("fast.b", "1.0.0", &nameless, &[], false),
                ),
                (
                    "ConfigSnapshotRegistered",
                    model_payload("fast.c", "1.0.0", &bare, &[], false),
                ),
            ],
        );
        let alice = signed_in(&f, ALICE);
        let body = ok(&get(&f, &ws("/signal-models"), Some(&alice)));
        assert_eq!(
            body["models"][0]["content"],
            serde_json::json!({"methodology": "m", "params_schema": {"params": [{"name": "x", "max": 4}]}})
        );
        assert_eq!(
            body["models"][1]["content"],
            serde_json::json!({"authorship": "platform", "params_schema": {"constraints": ["c"], "params": []}})
        );
        assert_eq!(body["models"][2]["content"], serde_json::json!({}));
    }

    #[test]
    fn a_parameter_without_a_valid_name_drops_only_itself() {
        let f = fixture();
        let hash = stored(
            &f,
            r#"{"params_schema":{"params":[{"name":"fast_periods","type":"integer"},{"type":"integer"},{"name":"Bad","min":1},7,{"max":3,"min":2,"name":"slow_periods"}]}}"#,
        );
        seed(
            &f,
            &[(
                "ConfigSnapshotRegistered",
                model_payload("fast.a", "1.0.0", &hash, &[], false),
            )],
        );
        let alice = signed_in(&f, ALICE);
        let body = ok(&get(&f, &ws("/signal-models"), Some(&alice)));
        assert_eq!(
            body["models"][0]["content"],
            serde_json::json!({"params_schema": {"params": [
                {"name": "fast_periods", "type": "integer"},
                {"name": "slow_periods", "min": 2, "max": 3},
            ]}})
        );
    }

    #[test]
    fn no_store_is_journal_unavailable() {
        let f = built(false);
        seed(&f, &[]);
        let alice = signed_in(&f, ALICE);
        refused(
            &get(&f, &ws("/signal-models"), Some(&alice)),
            503,
            ProblemCode::JournalUnavailable,
        );
    }
}

/// A control row the create's fold cannot read makes both reads `journal_unavailable`.
mod unreadable_control_rows {
    use super::*;

    fn both_refused(f: &Fixture) {
        let alice = signed_in(f, ALICE);
        for path in ["/instruments", "/signal-models"] {
            refused(
                &get(f, &ws(path), Some(&alice)),
                503,
                ProblemCode::JournalUnavailable,
            );
        }
    }

    #[test]
    fn a_malformed_model_registration() {
        let f = fixture();
        let spy = stored(&f, &snapshot_object(SPY_ID, "SPY", "us_equity"));
        seed(
            &f,
            &[
                ("ConfigSnapshotRegistered", snapshot_payload(&spy)),
                (
                    "ConfigSnapshotRegistered",
                    r#"{"content_hash":"sha256:1111111111111111111111111111111111111111111111111111111111111111","kind":"model_version","model_id":"quant.x","model_version":"1.0.0","params":[]}"#.to_owned(),
                ),
            ],
        );
        both_refused(&f);
    }

    #[test]
    fn a_deployment_whose_document_the_store_cannot_give() {
        let f = fixture();
        let version = reference(b"no document stored");
        seed(
            &f,
            &[(
                "AgentDeployed",
                format!(r#"{{"agent_id":"agent_1","mandate_version":"{version}"}}"#),
            )],
        );
        both_refused(&f);
    }

    #[test]
    fn a_deployment_whose_document_is_stored_is_read_and_both_reads_answer() {
        let f = fixture();
        let document = to_canonical(&json(MANDATE));
        f.store.put(&document).expect("stored");
        let version = reference(&document);
        let spy = stored(&f, &snapshot_object(SPY_ID, "SPY", "us_equity"));
        seed(
            &f,
            &[
                ("ConfigSnapshotRegistered", snapshot_payload(&spy)),
                (
                    "AgentDeployed",
                    format!(r#"{{"agent_id":"agt_a","mandate_version":"{version}"}}"#),
                ),
            ],
        );
        let alice = signed_in(&f, ALICE);
        let instruments = ok(&get(&f, &ws("/instruments"), Some(&alice)));
        assert_eq!(
            instruments["instruments"],
            serde_json::json!([instrument(SPY_ID, "SPY", "us_equity", &spy, 2)])
        );
        let models = ok(&get(&f, &ws("/signal-models"), Some(&alice)));
        assert_eq!(models["models"], serde_json::json!([]));
    }

    #[test]
    fn a_registration_with_no_kind() {
        let f = fixture();
        seed(&f, &[("ConfigSnapshotRegistered", "{}".to_owned())]);
        both_refused(&f);
    }
}

/// One model registration a random sequence holds.
#[derive(Debug, Clone)]
struct ModelRegistration {
    id: usize,
    version: usize,
    params: usize,
}

const MODEL_IDS: [&str; 3] = [MA, "fast.alpha", "llm.beta"];
const VERSIONS: [&str; 2] = ["1.0.0", "2.0.0"];
const PARAM_SETS: [&[&str]; 2] = [&["fast_periods", "slow_periods"], &["window"]];

fn model_registration() -> impl Strategy<Value = ModelRegistration> {
    (0..MODEL_IDS.len(), 0..VERSIONS.len(), 0..PARAM_SETS.len()).prop_map(
        |(id, version, params)| ModelRegistration {
            id,
            version,
            params,
        },
    )
}

/// A registration's content hash: the digest of its own content object.
fn model_hash(r: &ModelRegistration) -> String {
    reference(content_object(MODEL_IDS[r.id], VERSIONS[r.version]).as_bytes())
}

/// The fixture mandate naming `id`, `version`, `hash`, and one parameter per name of `params`.
fn naming(id: &str, version: &str, hash: &str, params: &[&str]) -> mandate_spec::Mandate {
    let mut document: Json = serde_json::from_str(MANDATE).expect("the fixture");
    let values: Vec<Json> = params
        .iter()
        .zip(["5", "20"])
        .map(|(key, value)| serde_json::json!({"key": key, "value": value}))
        .collect();
    let model = &mut document["behavior"]["signal_models"][0];
    model["id"] = Json::from(id);
    model["version"] = Json::from(version);
    model["content_hash"] = Json::from(hash);
    model["params"] = Json::Array(values);
    let value = json(&document.to_string());
    mandate_spec::Mandate::parse(&value).expect("a mandate")
}

/// Whether the create's V-rule check refuses `mandate` under V-007 on `control`.
fn v007(control: &[StoredEvent], f: &Fixture, mandate: &mandate_spec::Mandate) -> bool {
    let date = mandate_time::Date::parse("2026-10-17").expect("a date");
    match check(
        (control, &[]),
        f.store.as_ref(),
        (mandate, ""),
        Pending::default(),
        date,
    ) {
        Ok(_) => false,
        Err(Refusal::Rules(codes)) => codes.contains(&"V-007"),
        Err(other) => panic!("{other:?}"),
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, ..ProptestConfig::default() })]

    /// The read lists the latest registration per id, by the test's own fold, and a document naming
    /// any registered (id, version, hash, params) passes V-007 exactly when the read lists it.
    #[test]
    fn the_models_listed_are_what_v007_admits(
        sequence in prop::collection::vec(model_registration(), 0..6)
    ) {
        let f = fixture();
        let mut records = Vec::new();
        for r in &sequence {
            stored(&f, &content_object(MODEL_IDS[r.id], VERSIONS[r.version]));
            let payload = model_payload(
                MODEL_IDS[r.id],
                VERSIONS[r.version],
                &model_hash(r),
                PARAM_SETS[r.params],
                false,
            );
            records.push(("ConfigSnapshotRegistered", payload));
        }
        let control = seed(&f, &records);
        let alice = signed_in(&f, ALICE);
        let body = ok(&get(&f, &ws("/signal-models"), Some(&alice)));
        let mut latest: BTreeMap<&str, (&ModelRegistration, u64)> = BTreeMap::new();
        for (i, r) in sequence.iter().enumerate() {
            latest.insert(MODEL_IDS[r.id], (r, u64::try_from(i).expect("a count") + 2));
        }
        let wanted: Vec<Json> = latest
            .iter()
            .map(|(id, (r, seq))| {
                model(id, VERSIONS[r.version], &model_hash(r), PARAM_SETS[r.params], projected(), *seq)
            })
            .collect();
        prop_assert_eq!(&body["models"], &Json::Array(wanted));
        conforms(&body, MODELS_SCHEMA, "models", "Model", &control);
        let listed: BTreeSet<(String, String, String, Vec<String>)> = body["models"]
            .as_array()
            .expect("a list")
            .iter()
            .map(|m| {
                let text = |k: &str| m[k].as_str().expect(k).to_owned();
                let params = m["params"].as_array().expect("params").iter()
                    .map(|p| p.as_str().expect("a name").to_owned()).collect();
                (text("model_id"), text("model_version"), text("content_hash"), params)
            })
            .collect();
        for r in &sequence {
            let params: Vec<String> = PARAM_SETS[r.params].iter().map(|p| (*p).to_owned()).collect();
            let candidate = (
                MODEL_IDS[r.id].to_owned(),
                VERSIONS[r.version].to_owned(),
                model_hash(r),
                params,
            );
            let mandate = naming(MODEL_IDS[r.id], VERSIONS[r.version], &model_hash(r), PARAM_SETS[r.params]);
            prop_assert_eq!(
                !v007(&control, &f, &mandate),
                listed.contains(&candidate),
                "{:?}", candidate
            );
        }
    }
}

/// The content object with `members` as its top-level members.
fn content_with(members: &Json) -> String {
    members.to_string()
}

mod bounds {
    use super::*;

    fn one_param(name: &str, kind: &str) -> Json {
        serde_json::json!({"name": name, "type": kind, "min": 1, "max": 2})
    }

    /// Registers one model per object of `objects`, ids `fast.m0`, `fast.m1`, …, and reads them.
    fn contents(objects: &[Json]) -> Json {
        let f = fixture();
        let mut records = Vec::new();
        for (i, object) in objects.iter().enumerate() {
            let hash = stored(&f, &content_with(object));
            let id = format!("fast.m{i}");
            records.push((
                "ConfigSnapshotRegistered",
                model_payload(&id, "1.0.0", &hash, &[], false),
            ));
        }
        let control = seed(&f, &records);
        let alice = signed_in(&f, ALICE);
        let body = ok(&get(&f, &ws("/signal-models"), Some(&alice)));
        conforms(&body, MODELS_SCHEMA, "models", "Model", &control);
        body
    }

    #[test]
    fn content_at_every_bound_is_kept() {
        let params: Vec<Json> = (0..32)
            .map(|i| one_param(&format!("p{i:02}{}", "x".repeat(61)), &"t".repeat(32)))
            .collect();
        let object = serde_json::json!({
            "authorship": "a".repeat(64),
            "methodology": "m".repeat(4000),
            "params_schema": {"constraints": vec!["c".repeat(200); 32], "params": params},
        });
        let body = contents(std::slice::from_ref(&object));
        assert_eq!(body["models"][0]["content"], object);
    }

    #[test]
    fn content_past_any_bound_is_absent_member_by_member() {
        let many: Vec<Json> = (0..33)
            .map(|i| one_param(&format!("p{i}"), "integer"))
            .collect();
        let objects = [
            serde_json::json!({
                "authorship": "a".repeat(65),
                "methodology": "m".repeat(4001),
                "params_schema": {
                    "constraints": vec!["c"; 33],
                    "params": [one_param("fast_periods", &"t".repeat(33))],
                },
            }),
            serde_json::json!({
                "params_schema": {"constraints": ["c".repeat(201)], "params": many},
            }),
            serde_json::json!({
                "params_schema": {"params": [one_param("Bad", "integer")]},
            }),
            serde_json::json!({
                "params_schema": {"params": [one_param(&format!("p{}", "x".repeat(64)), "integer")]},
            }),
            serde_json::json!({
                "params_schema": {"params": [one_param("9lives", "integer")]},
            }),
        ];
        let body = contents(&objects);
        assert_eq!(
            body["models"][0]["content"],
            serde_json::json!({"params_schema": {"params": [
                {"name": "fast_periods", "min": 1, "max": 2}
            ]}})
        );
        assert_eq!(
            body["models"][1]["content"],
            serde_json::json!({"params_schema": {}})
        );
        for i in 2..5 {
            assert_eq!(
                body["models"][i]["content"],
                serde_json::json!({"params_schema": {"params": []}}),
                "{i}"
            );
        }
    }

    #[test]
    fn a_model_no_document_can_name_is_omitted() {
        let f = fixture();
        let hash = stored(&f, &content_object(MA, "1.0.0"));
        let long_name = format!("p{}", "x".repeat(63));
        let at_bound: Vec<String> = (0..32).map(|i| format!("p{i:02}")).collect();
        let at_bound: Vec<&str> = at_bound.iter().map(String::as_str).collect();
        let past: Vec<String> = (0..33).map(|i| format!("p{i:02}")).collect();
        let past: Vec<&str> = past.iter().map(String::as_str).collect();
        let too_long = format!("{long_name}x");
        let cases: Vec<(&str, &str, Vec<&str>, bool)> = vec![
            ("fast.v_short", "1.0", vec![], false),
            ("fast.v_padded", "01.0.0", vec![], false),
            ("fast.v_wide", "1.0.1234567", vec![], false),
            ("fast.v_four", "1.0.0.0", vec![], false),
            ("fast.v_letters", "a.b.c", vec![], false),
            ("fast.v_empty", "1..0", vec![], false),
            ("fast.v_widest", "999999.0.0", vec![], true),
            ("fast.v_zero", "0.0.0", vec![], true),
            ("fast.p_upper", "1.0.0", vec!["Bad"], false),
            ("fast.p_digit", "1.0.0", vec!["9lives"], false),
            ("fast.p_long", "1.0.0", vec![too_long.as_str()], false),
            ("fast.p_longest", "1.0.0", vec![long_name.as_str()], true),
            ("fast.p_many", "1.0.0", past.clone(), false),
            ("fast.p_most", "1.0.0", at_bound.clone(), true),
        ];
        let records: Vec<(&str, String)> = cases
            .iter()
            .map(|(id, version, params, _)| {
                (
                    "ConfigSnapshotRegistered",
                    model_payload(id, version, &hash, params, false),
                )
            })
            .collect();
        let control = seed(&f, &records);
        let alice = signed_in(&f, ALICE);
        let body = ok(&get(&f, &ws("/signal-models"), Some(&alice)));
        conforms(&body, MODELS_SCHEMA, "models", "Model", &control);
        let listed: BTreeSet<&str> = body["models"]
            .as_array()
            .expect("a list")
            .iter()
            .map(|m| m["model_id"].as_str().expect("an id"))
            .collect();
        let wanted: BTreeSet<&str> = cases
            .iter()
            .filter(|(_, _, _, nameable)| *nameable)
            .map(|(id, ..)| *id)
            .collect();
        assert_eq!(listed, wanted);
    }
}

/// Only a full user session reads either catalogue.
mod sessions {
    use super::*;

    #[test]
    fn a_reduction_only_session_reads_neither() {
        let f = fixture();
        seed(&f, &[]);
        let cookie = signed_in(&f, ALICE);
        assert_eq!(
            f.sessions.refresh(
                &cookie,
                mandate_api_server::session::Answer::Unreachable,
                at(T0 + 1)
            ),
            Ok(None)
        );
        for path in ["/instruments", "/signal-models"] {
            refused(
                &get(&f, &ws(path), Some(&cookie)),
                403,
                ProblemCode::ReductionOnly,
            );
        }
    }

    #[test]
    fn a_client_credential_is_the_one_401_for_both() {
        let f = fixture();
        seed(&f, &[]);
        let token = [
            "eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCJ9",
            "eyJzdWIiOiJjbGllbnQtMSIsInNjb3BlIjoicmVhZCJ9",
            "c2ln",
        ]
        .join(".");
        let bearer = format!("Bearer {token}");
        for path in ["/instruments", "/signal-models"] {
            for cookie in [None, Some(format!("{COOKIE_NAME}=not-a-session"))] {
                let mut headers = vec![("Origin", APP), ("Authorization", bearer.as_str())];
                if let Some(cookie) = cookie.as_deref() {
                    headers.push(("Cookie", cookie));
                }
                let reply = send(f.addr, "GET", &ws(path), &headers, b"");
                refused(&reply, 401, ProblemCode::Unauthenticated);
            }
        }
    }
}

/// The example, then each of its valid cases (the example with the case's `set` applied), with
/// their labels.
fn examples(example: &str, valid: &str) -> Vec<(String, Json)> {
    let base: Json = serde_json::from_str(example).expect("the example");
    let mut all = vec![("the example".to_owned(), base.clone())];
    let cases: Json = serde_json::from_str(valid).expect("the valid cases");
    for case in cases.as_array().expect("a list") {
        let mut value = base.clone();
        for (pointer, to) in case["set"].as_object().expect("an object") {
            *value.pointer_mut(pointer).expect("the member") = to.clone();
        }
        let label = case["label"].as_str().expect("a label").to_owned();
        all.push((label, value));
    }
    all
}

/// The envelope members a test cannot fix: when the body was served, the build serving it, and the
/// watermark, which [`conforms`] checks against the control stream the test seeded.
const UNFIXED: [&str; 3] = ["served_at", "build", "as_of"];

/// `body` without [`UNFIXED`].
fn fixed(body: &Json) -> Json {
    let mut fixed = body.clone();
    for name in UNFIXED {
        fixed.as_object_mut().expect("an object").remove(name);
    }
    fixed
}

/// Seeds `records`, the row numbered `seq` of each of `stamps` recorded at its time.
fn seed_stamped(
    f: &Fixture,
    records: &[(&str, String)],
    stamps: &[(u64, String)],
) -> Vec<StoredEvent> {
    let mut control = seed(f, records);
    for (seq, at) in stamps {
        let row = control
            .iter_mut()
            .find(|row| row.seq == *seq)
            .expect("the row");
        row.recorded_at.clone_from(at);
    }
    f.journal
        .streams
        .lock()
        .expect("unpoisoned")
        .insert(ctl(), control.clone());
    control
}

/// `content` as a stored object holds it: with members the projection never serves added at each
/// level it projects.
fn unprojected(content: &Json) -> Json {
    let mut object = content.clone();
    object["kind"] = Json::from("quant_model_content");
    object["prompt"] = Json::from("never served");
    if let Some(schema) = object.get_mut("params_schema") {
        schema["extra"] = Json::Bool(true);
        let params = schema.get_mut("params").and_then(Json::as_array_mut);
        for param in params.into_iter().flatten() {
            param["default"] = Json::from(5);
        }
    }
    object
}

/// The published examples, reproduced from stored objects and seeded rows: each served body equals
/// its example apart from [`UNFIXED`], with each object reference the one the test stored under.
mod published_examples {
    use super::*;

    #[test]
    fn the_instruments_example_and_each_valid_case_are_served_as_written() {
        for (label, mut example) in examples(INSTRUMENTS_EXAMPLE, INSTRUMENTS_VALID) {
            let f = fixture();
            let mut records = Vec::new();
            let mut stamps = Vec::new();
            let entries = example["instruments"].as_array_mut().expect("a list");
            for entry in entries {
                let text = |k: &str| entry[k].as_str().expect(k).to_owned();
                let (id, symbol, class) =
                    (text("instrument_id"), text("symbol"), text("asset_class"));
                let at = text("registered_at");
                let hash = stored(&f, &snapshot_object(&id, &symbol, &class));
                entry["snapshot"] = Json::from(hash.as_str());
                let seq = u64::try_from(records.len()).expect("a count") + 2;
                stamps.push((seq, at));
                records.push(("ConfigSnapshotRegistered", snapshot_payload(&hash)));
            }
            let control = seed_stamped(&f, &records, &stamps);
            let alice = signed_in(&f, ALICE);
            let body = ok(&get(&f, &ws("/instruments"), Some(&alice)));
            assert_eq!(fixed(&body), fixed(&example), "{label}");
            conforms(
                &body,
                INSTRUMENTS_SCHEMA,
                "instruments",
                "Instrument",
                &control,
            );
        }
    }

    #[test]
    fn the_signal_models_example_and_each_valid_case_are_served_as_written() {
        for (label, mut example) in examples(MODELS_EXAMPLE, MODELS_VALID) {
            let f = fixture();
            let mut records = Vec::new();
            let mut stamps = Vec::new();
            let entries = example["models"].as_array_mut().expect("a list");
            for entry in entries {
                let text = |k: &str| entry[k].as_str().expect(k).to_owned();
                let (id, version, at) = (
                    text("model_id"),
                    text("model_version"),
                    text("registered_at"),
                );
                let params: Vec<String> = entry["params"]
                    .as_array()
                    .expect("params")
                    .iter()
                    .map(|p| p.as_str().expect("a name").to_owned())
                    .collect();
                let params: Vec<&str> = params.iter().map(String::as_str).collect();
                let admits = entry["admits_instruments"].as_bool().expect("a flag");
                let hash = match &entry["content"] {
                    Json::Null => reference(b"no content stored"),
                    content => stored(&f, &unprojected(content).to_string()),
                };
                entry["content_hash"] = Json::from(hash.as_str());
                let seq = u64::try_from(records.len()).expect("a count") + 2;
                stamps.push((seq, at));
                records.push((
                    "ConfigSnapshotRegistered",
                    model_payload(&id, &version, &hash, &params, admits),
                ));
            }
            let control = seed_stamped(&f, &records, &stamps);
            let alice = signed_in(&f, ALICE);
            let body = ok(&get(&f, &ws("/signal-models"), Some(&alice)));
            assert_eq!(fixed(&body), fixed(&example), "{label}");
            conforms(&body, MODELS_SCHEMA, "models", "Model", &control);
        }
    }
}

/// The price read (§3.11 item 11, DEC-2152): an instrument's last trusted daily close from the bars
/// root the process is given, or `reported` `false` with why, never a stub. The oracle for the close
/// is the test's own list of the sessions it stored, never the server's reader: the clock `T0` falls
/// on Saturday 2026-10-03, so the last completed regular session is Friday 2026-10-02.
mod price_read {
    use super::*;
    use mandate_canon::DecStr;
    use mandate_marketdata::actions::{RecordedActions, write_actions};
    use mandate_marketdata::dataset::Store;
    use mandate_marketdata::model::{
        AssetClass, Bar, CorporateActions, DatasetId, DayRange, Feed, Kind, Records, Split,
        SplitRatio, Symbol,
    };
    use mandate_time::{Date, ExchangeCalendar, new_york_midnight};

    /// The last completed regular session at `T0`.
    const LAST: (u16, u8, u8) = (2026, 10, 2);

    fn last() -> Date {
        Date::new(LAST.0, LAST.1, LAST.2).expect("a date")
    }

    /// A fresh directory for one test's bars, its last component `name`.
    fn bars_dir(test: &str, name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mandate-api-server-price-{test}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join(name)
    }

    /// The 30 trading days ending on `through`, oldest first.
    fn sessions_to(through: Date) -> Vec<Date> {
        let calendar = ExchangeCalendar::us_equities().expect("the calendar");
        let mut d = Date::new(2026, 8, 1).expect("a date");
        let mut days = Vec::new();
        while d <= through {
            if calendar.is_trading_day(d).expect("a day") {
                days.push(d);
            }
            d = d.next().expect("a next day");
        }
        days.split_off(days.len() - 30)
    }

    /// Session `n`'s close as the test stores it.
    fn close_of(n: usize) -> String {
        format!("{}.25", 400 + n % 11)
    }

    fn dataset(symbol: &str, timeframe: &str) -> DatasetId {
        DatasetId::new(
            AssetClass::UsEquity,
            Feed::Iex,
            Kind::Bars(timeframe.parse().expect("a timeframe")),
            Symbol::parse(symbol).expect("a symbol"),
        )
        .expect("a dataset")
    }

    fn bar_at(start: UtcNanos, close: &str) -> Bar {
        let price = DecStr::parse(close).expect("a price");
        Bar {
            start,
            open: price.clone(),
            high: price.clone(),
            low: price.clone(),
            close: price.clone(),
            volume: DecStr::parse("1000000").expect("a volume"),
            vwap: price,
            trade_count: 10,
        }
    }

    /// Stores `symbol`'s 30 daily bars ending on `through` under `root`; answers each session with
    /// the close stored for it.
    fn daily(root: &std::path::Path, symbol: &str, through: Date) -> Vec<(Date, String)> {
        daily_skipping(root, symbol, through, None)
    }

    /// [`daily`], leaving out the session at index `skip` of the 30 when it names one.
    fn daily_skipping(
        root: &std::path::Path,
        symbol: &str,
        through: Date,
        skip: Option<usize>,
    ) -> Vec<(Date, String)> {
        let store = Store::new(root);
        let id = dataset(symbol, "1Day");
        let mut stored = Vec::new();
        for (n, d) in sessions_to(through).into_iter().enumerate() {
            if Some(n) == skip {
                continue;
            }
            let bar = bar_at(new_york_midnight(d).expect("a start"), &close_of(n));
            store
                .put_day(&id, d, &Records::Bars(vec![bar]))
                .expect("stored");
            stored.push((d, close_of(n)));
        }
        stored
    }

    /// A fixture whose control stream registers QQQ (an equity) and BTCUSD (crypto), with `bars`
    /// as the price read's root.
    fn registered(bars: Option<std::path::PathBuf>) -> (Fixture, Vec<StoredEvent>) {
        let f = built_with(true, bars);
        let qqq = stored(&f, &snapshot_object(QQQ_ID, "QQQ", "us_equity"));
        let btc = stored(&f, &snapshot_object(BTC_ID, "BTCUSD", "crypto"));
        let control = seed(
            &f,
            &[
                ("ConfigSnapshotRegistered", snapshot_payload(&qqq)),
                ("ConfigSnapshotRegistered", snapshot_payload(&btc)),
            ],
        );
        (f, control)
    }

    const BTC_ID: &str = "00000000-0000-4000-8000-000000000001";

    fn price_of(f: &Fixture, id: &str) -> Reply {
        let alice = signed_in(f, ALICE);
        get(f, &ws(&format!("/instruments/{id}/price")), Some(&alice))
    }

    /// The body's members against the schema's: the envelope's and the read's required members,
    /// nothing either does not list, and `as_of` naming the control stream's last row.
    fn shaped(body: &Json, control: &[StoredEvent]) {
        let schema: Json = serde_json::from_str(PRICE_SCHEMA).expect("the schema");
        let envelope: Json = serde_json::from_str(ENVELOPE_SCHEMA).expect("the envelope");
        let mut required = listed(&envelope["$defs"]["Envelope"]["required"]);
        required.extend(listed(&schema["required"]));
        let mut allowed = names(&envelope["$defs"]["Envelope"]["properties"]);
        allowed.extend(names(&schema["properties"]));
        let served = names(body);
        assert!(required.is_subset(&served), "{required:?} in {served:?}");
        assert!(served.is_subset(&allowed), "{served:?} within {allowed:?}");
        assert_eq!(body["served_at"], "2026-10-03T04:00:00.000000000Z");
        let last = control.last().expect("a row");
        assert_eq!(
            body["as_of"],
            serde_json::json!([{
                "stream_id": last.stream_id,
                "seq": last.seq,
                "hash": format!("sha256:{}", last.hash),
                "recorded_at": last.recorded_at,
            }])
        );
    }

    /// An unreported body: the instrument, `reported` `false`, `reason`, and no bar member.
    fn unreported(reply: &Reply, control: &[StoredEvent], id: &str, symbol: &str, reason: &str) {
        let body = ok(reply);
        shaped(&body, control);
        let mut members = names(&body);
        for envelope in ["api_version", "build", "served_at", "as_of"] {
            members.remove(envelope);
        }
        assert_eq!(
            members,
            ["instrument_id", "symbol", "reported", "reason"]
                .map(str::to_owned)
                .into(),
            "never a stub: {body}"
        );
        assert_eq!(body["instrument_id"], id);
        assert_eq!(body["symbol"], symbol);
        assert_eq!(body["reported"], false);
        assert_eq!(body["reason"], reason, "{body}");
    }

    #[test]
    #[ignore = "pending E10-11"]
    fn the_last_trusted_session_s_close_is_served_from_the_recording() {
        let root = bars_dir("recorded", "cell-recorded");
        let stored_closes = daily(&root, "QQQ", last());
        let (f, control) = registered(Some(root));
        let body = ok(&price_of(&f, QQQ_ID));
        shaped(&body, &control);
        let (session, close) = stored_closes.last().expect("a session");
        assert_eq!(*session, last());
        assert_eq!(body["instrument_id"], QQQ_ID);
        assert_eq!(body["symbol"], "QQQ");
        assert_eq!(body["reported"], true);
        assert_eq!(body["close"], close.as_str());
        assert_eq!(
            body["bar_start"],
            new_york_midnight(last()).expect("a start").to_string()
        );
        assert_eq!(body["timeframe"], "1Day");
        assert_eq!(body["source"], "recorded");
        assert!(body.get("reason").is_none());
        let example: Json = serde_json::from_str(PRICE_EXAMPLE).expect("the example");
        assert_eq!(
            names(&body),
            names(&example),
            "a reported body has the published example's members"
        );
    }

    #[test]
    #[ignore = "pending E10-11"]
    fn a_generations_root_reads_its_session_s_generation_and_names_alpaca_iex() {
        let root = bars_dir("generations", "alpaca-iex");
        let generation = root.join("daily").join(last().to_string());
        let stored_closes = daily(&generation, "QQQ", last());
        let (f, control) = registered(Some(root));
        let body = ok(&price_of(&f, QQQ_ID));
        shaped(&body, &control);
        assert_eq!(body["source"], "alpaca-iex");
        assert_eq!(
            body["close"],
            stored_closes.last().expect("a session").1.as_str()
        );
    }

    #[test]
    #[ignore = "pending E10-11"]
    fn no_root_or_a_missing_root_is_no_store() {
        for bars in [None, Some(bars_dir("missing", "cell-recorded"))] {
            let (f, control) = registered(bars);
            unreported(&price_of(&f, QQQ_ID), &control, QQQ_ID, "QQQ", "no_store");
        }
    }

    #[test]
    #[ignore = "pending E10-11"]
    fn a_crypto_instrument_is_not_covered_even_with_bars_stored() {
        let root = bars_dir("crypto", "cell-recorded");
        daily(&root, "QQQ", last());
        let (f, control) = registered(Some(root));
        unreported(
            &price_of(&f, BTC_ID),
            &control,
            BTC_ID,
            "BTCUSD",
            "not_covered",
        );
    }

    #[test]
    #[ignore = "pending E10-11"]
    fn the_store_is_checked_before_the_asset_class() {
        let (f, control) = registered(None);
        unreported(
            &price_of(&f, BTC_ID),
            &control,
            BTC_ID,
            "BTCUSD",
            "no_store",
        );
    }

    #[test]
    #[ignore = "pending E10-11"]
    fn a_root_with_no_dataset_for_the_symbol_is_no_bars() {
        let root = bars_dir("nobars", "cell-recorded");
        daily(&root, "SPY", last());
        let (f, control) = registered(Some(root));
        unreported(&price_of(&f, QQQ_ID), &control, QQQ_ID, "QQQ", "no_bars");
    }

    #[test]
    #[ignore = "pending E10-11"]
    fn a_generations_root_without_the_session_s_generation_is_no_bars() {
        let root = bars_dir("nogeneration", "alpaca-iex");
        let earlier = Date::new(2026, 10, 1).expect("a date");
        daily(
            &root.join("daily").join(earlier.to_string()),
            "QQQ",
            earlier,
        );
        let (f, control) = registered(Some(root));
        unreported(&price_of(&f, QQQ_ID), &control, QQQ_ID, "QQQ", "no_bars");
    }

    #[test]
    #[ignore = "pending E10-11"]
    fn bars_that_stop_before_the_last_session_are_untrusted_never_a_stale_close() {
        let root = bars_dir("stale", "cell-recorded");
        daily(&root, "QQQ", Date::new(2026, 10, 1).expect("a date"));
        let (f, control) = registered(Some(root));
        unreported(&price_of(&f, QQQ_ID), &control, QQQ_ID, "QQQ", "untrusted");
    }

    #[test]
    #[ignore = "pending E10-11"]
    fn a_trading_day_missing_inside_the_span_is_untrusted() {
        let root = bars_dir("hole", "cell-recorded");
        let stored = daily_skipping(&root, "QQQ", last(), Some(15));
        assert_eq!(stored.len(), 29, "one session of the 30 left out");
        assert_eq!(stored.last().expect("a session").0, last());
        let (f, control) = registered(Some(root));
        unreported(&price_of(&f, QQQ_ID), &control, QQQ_ID, "QQQ", "untrusted");
    }

    #[test]
    #[ignore = "pending E10-11"]
    fn a_split_inside_the_span_is_untrusted() {
        let root = bars_dir("split", "cell-recorded");
        let stored = daily(&root, "QQQ", last());
        let (first, ex_date) = (stored[0].0, stored[10].0);
        let dir = mandate_bars::daily_dir(&root, "QQQ").expect("the layout");
        write_actions(
            &dir,
            &RecordedActions {
                range: DayRange::new(first, last()).expect("a range"),
                actions: CorporateActions {
                    splits: vec![Split {
                        id: "split".to_owned(),
                        ex_date,
                        ratio: SplitRatio::new(4, 1).expect("a ratio"),
                    }],
                    ..CorporateActions::none(Symbol::parse("QQQ").expect("a symbol"))
                },
            },
        )
        .expect("the actions");
        let (f, control) = registered(Some(root));
        unreported(&price_of(&f, QQQ_ID), &control, QQQ_ID, "QQQ", "untrusted");
    }

    #[test]
    #[ignore = "pending E10-11"]
    fn two_bars_sharing_a_start_are_untrusted() {
        let root = bars_dir("duplicate", "cell-recorded");
        let stored = daily_skipping(&root, "QQQ", last(), Some(29));
        assert_eq!(
            stored.len(),
            29,
            "every session but the last, which is written below"
        );
        let start = new_york_midnight(last()).expect("a start");
        Store::new(&root)
            .put_day(
                &dataset("QQQ", "1Day"),
                last(),
                &Records::Bars(vec![bar_at(start, "500.25"), bar_at(start, "501.25")]),
            )
            .expect("stored");
        let (f, control) = registered(Some(root));
        unreported(&price_of(&f, QQQ_ID), &control, QQQ_ID, "QQQ", "untrusted");
    }

    #[test]
    #[ignore = "pending E10-11"]
    fn another_symbol_s_or_a_minute_dataset_at_the_symbol_s_path_is_untrusted() {
        for (case, from_symbol, timeframe) in [("other", "SPY", "1Day"), ("minute", "QQQ", "1Min")]
        {
            let root = bars_dir(case, "cell-recorded");
            let source = root.join("source");
            let store = Store::new(&source);
            let id = dataset(from_symbol, timeframe);
            for d in sessions_to(last()) {
                let bar = bar_at(new_york_midnight(d).expect("a start"), "400.25");
                store
                    .put_day(&id, d, &Records::Bars(vec![bar]))
                    .expect("stored");
            }
            let from = source.join(id.relative_dir());
            let to = mandate_bars::daily_dir(&root, "QQQ").expect("the layout");
            std::fs::create_dir_all(to.parent().expect("a parent")).expect("the layout's parent");
            std::fs::rename(&from, &to).expect("moved to QQQ's daily path");
            let (f, control) = registered(Some(root));
            unreported(&price_of(&f, QQQ_ID), &control, QQQ_ID, "QQQ", "untrusted");
        }
    }

    #[test]
    #[ignore = "pending E10-11"]
    fn a_corrupt_partition_is_untrusted() {
        let root = bars_dir("corrupt", "cell-recorded");
        daily(&root, "QQQ", last());
        let dir = mandate_bars::daily_dir(&root, "QQQ").expect("the layout");
        std::fs::write(dir.join(format!("{}.parquet", last())), b"not parquet").expect("written");
        let (f, control) = registered(Some(root));
        unreported(&price_of(&f, QQQ_ID), &control, QQQ_ID, "QQQ", "untrusted");
    }

    #[test]
    #[ignore = "pending E10-11"]
    fn an_id_the_catalogue_does_not_list_is_the_one_404() {
        let root = bars_dir("unlisted", "cell-recorded");
        daily(&root, "SPY", last());
        let (f, _) = registered(Some(root));
        refused(&price_of(&f, SPY_ID), 404, ProblemCode::NotFound);
        refused(&price_of(&f, "not-an-id"), 404, ProblemCode::NotFound);
    }

    #[test]
    #[ignore = "pending E10-11"]
    fn it_is_authorized_as_the_catalogue_is() {
        let (f, _) = registered(None);
        let path = ws(&format!("/instruments/{QQQ_ID}/price"));
        refused(&get(&f, &path, None), 401, ProblemCode::Unauthenticated);
        let bob = signed_in(&f, BOB);
        refused(&get(&f, &path, Some(&bob)), 403, ProblemCode::Forbidden);
        let carol = signed_in(&f, CAROL);
        refused(&get(&f, &path, Some(&carol)), 404, ProblemCode::NotFound);
        let alice = signed_in(&f, ALICE);
        let other = format!("/v1/workspaces/{OTHER_WORKSPACE}/instruments/{QQQ_ID}/price");
        refused(&get(&f, &other, Some(&alice)), 404, ProblemCode::NotFound);
    }

    #[test]
    #[ignore = "pending E10-11"]
    fn an_unreadable_snapshot_is_journal_unavailable_as_the_catalogue_answers() {
        let f = built_with(true, None);
        let missing = reference(b"never stored");
        seed(
            &f,
            &[("ConfigSnapshotRegistered", snapshot_payload(&missing))],
        );
        let alice = signed_in(&f, ALICE);
        let path = ws(&format!("/instruments/{QQQ_ID}/price"));
        refused(
            &get(&f, &path, Some(&alice)),
            503,
            ProblemCode::JournalUnavailable,
        );
        refused(
            &get(&f, &ws("/instruments"), Some(&alice)),
            503,
            ProblemCode::JournalUnavailable,
        );
    }

    #[test]
    #[ignore = "pending E10-11"]
    fn a_deployment_with_no_store_is_journal_unavailable() {
        let f = built_with(false, None);
        seed(&f, &[]);
        refused(&price_of(&f, QQQ_ID), 503, ProblemCode::JournalUnavailable);
    }
}
