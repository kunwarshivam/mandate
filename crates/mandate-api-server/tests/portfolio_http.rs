//! The Portfolio page's reads (DEC-2413; workspace API spec §4.7): `GET /dashboard`'s account
//! snapshots, `GET /dashboard/history`'s equity points, and `GET /agents/{id}/pnl`'s sub-ledger
//! P&L (trading spec §8.1, §8.2), each served from journal rows alone and never from part of them.

mod common;

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use common::{MemJournal, Reply, WORKSPACE, hosted, send, serve};
use mandate_api_server::config::{SESSION_KEY_FILE, read_session_key};
use mandate_api_server::http::{self, App, Settings};
use mandate_api_server::lifecycle::Drain;
use mandate_api_server::portfolio::{Range, Unread, equity_points, latest_snapshot};
use mandate_api_server::session::{COOKIE_NAME, OsRandom, Sessions};
use mandate_api_server::token::{FixedKeys, Issuer};
use mandate_authn::{Algorithm, IssuerConfig};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_identity::{LookupFailed, MembershipEvent, MembershipRecord, PrincipalId, Role};
use mandate_identity_seal::Seal;
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;

const APP: &str = "https://app.owlhead.test";
/// The clock's instant: 2026-10-03T03:20:00Z.
const T0: i64 = 1_791_000_000;
const SPY: &str = "b0c2a3e4-5f60-4718-9a2b-3c4d5e6f7081";
const QQQ: &str = "9ddbf538-6ee1-46c4-c6ff-8d1d4b9ff6d5";
const DOCUMENT: &str = include_str!("fixtures/mandate.json");

type Records = Arc<Mutex<Result<Vec<MembershipRecord>, LookupFailed>>>;

struct Fixture {
    addr: std::net::SocketAddr,
    journal: Arc<MemJournal>,
    records: Records,
    sessions: Arc<Sessions>,
}

static DIRS: Mutex<u32> = Mutex::new(0);

/// Each row's event id, unique across the process.
static IDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn fixture(artifacts: Option<Arc<dyn mandate_journal::ArtifactSource + Send + Sync>>) -> Fixture {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mandate-api-server-portfolio-{}-{n}",
        std::process::id()
    ));
    drop(n);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let file = dir.join(SESSION_KEY_FILE);
    std::fs::write(&file, [0x44_u8; 32]).expect("the key");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("mode");
    let key = read_session_key(&dir).expect("the key reads");
    let journal = Arc::new(MemJournal::default());
    let records: Records = Arc::new(Mutex::new(Ok(Vec::new())));
    let parsed = Arc::clone(&records);
    let sessions = Arc::new(Sessions::new(&key, Box::new(OsRandom::new())));
    let clock = common::ManualClock::at(T0);
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
        clock: Arc::clone(&clock) as Arc<dyn mandate_api_server::clock::Clock>,
        journal: Arc::clone(&journal) as Arc<dyn mandate_api_server::journal::Journal>,
        members: Arc::new(move |_: &[StoredEvent]| parsed.lock().expect("unpoisoned").clone()),
        hosted: hosted(),
        artifacts,
        objects: None,
        step_up: Arc::new(mandate_api_server::stepup::Unbound),
        challenges: Arc::default(),
        connect: None,
        reduction: Arc::default(),
        folds: Arc::default(),
        bars: None,
        chat: Default::default(),
    };
    Fixture {
        addr: serve(http::api(app, Drain::new())),
        journal,
        records,
        sessions,
    }
}

fn alice() -> PrincipalId {
    PrincipalId::from_ulid_text("01JB3K6F0C9R2V7N4M8Q1T5W3Z").expect("a ULID")
}

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).expect("a time")
}

/// Alice, a viewer since before T0, signed in.
fn viewer(f: &Fixture) -> String {
    *f.records.lock().expect("unpoisoned") = Ok(vec![MembershipRecord::new(
        Seal::grant(),
        1,
        at(T0 - 1_000),
        MembershipEvent::Activated {
            member: alice(),
            invitation: None,
            roles: [Role::Viewer].into_iter().collect::<BTreeSet<_>>(),
            independent_approval_required: false,
            cool_off_ends_at: at(T0 - 1_000),
        },
    )]);
    f.sessions
        .open(alice(), "sha256:aaaa".to_owned(), None, at(T0))
        .expect("opened")
        .value()
        .to_owned()
}

fn get(f: &Fixture, path: &str, cookie: Option<&str>) -> Reply {
    let header = cookie.map(|c| format!("{COOKIE_NAME}={c}"));
    let mut headers = vec![("Origin", APP)];
    if let Some(header) = header.as_deref() {
        headers.push(("Cookie", header));
    }
    send(f.addr, "GET", path, &headers, b"")
}

fn ws(path: &str) -> String {
    format!("/v1/workspaces/{WORKSPACE}{path}")
}

fn ctl() -> String {
    format!("ctl:{WORKSPACE}")
}

fn acct(reference: &str) -> String {
    format!("acct:{WORKSPACE}:{reference}")
}

/// A row at `seq` on `stream`, recorded at `recorded` seconds, with an id unique across streams.
fn row(stream: &str, seq: u64, event_type: &str, payload: &str, recorded: i64) -> StoredEvent {
    let n = IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let event_id = format!("01JBH3BTZ8K2M4N6P8R{n:07}");
    let text = format!(
        r#"{{"environment":"paper","event_id":"{event_id}","event_type":"{event_type}","payload":{payload},"stream_id":"{stream}"}}"#
    );
    let body = to_canonical(&parse(text.as_bytes()).expect("JSON"));
    StoredEvent {
        stream_id: stream.to_owned(),
        seq,
        event_id,
        event_type: event_type.to_owned(),
        schema_version: 1,
        environment: "paper".to_owned(),
        recorded_at: at(recorded).to_string(),
        prev_hash: Digest::ZERO,
        hash: Digest::of(&body),
        body,
    }
}

/// An `AccountSnapshotRecorded` of `equity`, at version 2 with `last_equity` when one is given.
fn snapshot(seq: u64, equity: &str, last_equity: Option<&str>, recorded: i64) -> StoredEvent {
    let figures = last_equity.map_or_else(String::new, |close| {
        format!(
            r#""account_type":"margin","day_trading_regime":"intraday_margin","last_equity":"{close}","maintenance_margin":"0","#
        )
    });
    let payload = format!(
        r#"{{"status":"ACTIVE","crypto_status":"ACTIVE","trading_blocked":false,"account_blocked":false,"trade_suspended_by_user":false,"multiplier":1,"equity":"{equity}","cash":"400","buying_power":"800","non_marginable_buying_power":"400","accrued_fees":"0",{figures}"model_cash":null,"cash_band":null,"cash_in_band":null,"risk_clock":"2026-10-03T03:00:00.000000000Z"}}"#
    );
    let mut laid = row(
        &acct("acct_1"),
        seq,
        "AccountSnapshotRecorded",
        &payload,
        recorded,
    );
    laid.schema_version = if last_equity.is_some() { 2 } else { 1 };
    laid
}

fn json(reply: &Reply) -> Value {
    parse(&reply.body).expect("a JSON body")
}

fn code(reply: &Reply) -> String {
    json(reply)
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn streams_read(served: &Value) -> Vec<String> {
    let Some(Value::Array(as_of)) = served.get("as_of") else {
        panic!("as_of");
    };
    as_of
        .iter()
        .map(|w| {
            w.get("stream_id")
                .and_then(Value::as_str)
                .expect("an id")
                .to_owned()
        })
        .collect()
}

/// The control stream: `agt_a` deployed with the stored document's `version`, its connection
/// established on `acct_1`, and a second connection requested on `acct_2`.
fn control(f: &Fixture, version: &str) {
    let mut established = row(
        &ctl(),
        3,
        "ConnectionEstablished",
        r#"{"connection_id":"conn_alpaca_paper_01","account_ref":"acct_1"}"#,
        T0 - 900,
    );
    established.schema_version = 2;
    f.journal.streams.lock().expect("unpoisoned").insert(
        ctl(),
        vec![
            row(&ctl(), 1, "StreamOpened", "{}", T0 - 1_000),
            row(
                &ctl(),
                2,
                "AgentDeployed",
                &format!(r#"{{"agent_id":"agt_a","mandate_version":"{version}"}}"#),
                T0 - 950,
            ),
            established,
            row(
                &ctl(),
                4,
                "ConnectionRequested",
                r#"{"connection_id":"conn_alpaca_paper_02","account_ref":"acct_2"}"#,
                T0 - 800,
            ),
        ],
    );
}

fn account(f: &Fixture, reference: &str, rows: Vec<StoredEvent>) {
    f.journal
        .streams
        .lock()
        .expect("unpoisoned")
        .insert(acct(reference), rows);
}

fn document() -> (Vec<u8>, String) {
    let bytes = to_canonical(&parse(DOCUMENT.as_bytes()).expect("JSON"));
    let version = format!("sha256:{}", Digest::of(&bytes).to_hex());
    (bytes, version)
}

fn stored(bytes: Vec<u8>) -> Option<Arc<dyn mandate_journal::ArtifactSource + Send + Sync>> {
    Some(Arc::new(std::collections::BTreeMap::from([(
        Digest::of(&bytes),
        bytes,
    )])))
}

#[test]
fn the_reads_need_a_live_session_and_the_view_row() {
    let f = fixture(None);
    control(&f, "sha256:00");
    for path in ["/dashboard", "/dashboard/history", "/agents/agt_a/pnl"] {
        assert_eq!(get(&f, &ws(path), None).status, 401, "{path}");
    }
    let cookie = viewer(&f);
    *f.records.lock().expect("unpoisoned") = Ok(Vec::new());
    for path in ["/dashboard", "/dashboard/history", "/agents/agt_a/pnl"] {
        assert_eq!(
            get(&f, &ws(path), Some(&cookie)).status,
            404,
            "{path}: API-9"
        );
    }
}

#[test]
fn the_dashboard_serves_each_accounts_latest_snapshot_beside_the_agents() {
    let f = fixture(None);
    control(&f, "sha256:00");
    account(
        &f,
        "acct_1",
        vec![
            row(&acct("acct_1"), 1, "StreamOpened", "{}", T0 - 900),
            snapshot(2, "990", Some("1000"), T0 - 600),
            snapshot(3, "1012.5", Some("1000.25"), T0 - 300),
        ],
    );
    let cookie = viewer(&f);
    let reply = get(&f, &ws("/dashboard"), Some(&cookie));
    assert_eq!(
        reply.status,
        200,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    let served = json(&reply);
    let Some(Value::Array(agents)) = served.get("agents") else {
        panic!("agents");
    };
    assert_eq!(agents.len(), 1);
    assert_eq!(
        agents[0].get("agent_id").and_then(Value::as_str),
        Some("agt_a")
    );
    let accounts = parse(
        format!(
            r#"[{{"account_ref":"acct_1","snapshot":{{"equity":"1012.5","cash":"400","buying_power":"800","last_equity":"1000.25","day_change":"12.25","environment":"paper","observed_at":"{}"}}}},{{"account_ref":"acct_2","snapshot":null}}]"#,
            at(T0 - 300)
        )
        .as_bytes(),
    )
    .expect("JSON");
    assert_eq!(
        served.get("accounts"),
        Some(&accounts),
        "the latest snapshot counts; an account with no stream yet has none"
    );
    assert_eq!(
        streams_read(&served),
        [ctl(), acct("acct_1")],
        "the agent and acct_2 streams hold no row"
    );

    assert_eq!(get(&f, &ws("/dashboard?x=1"), Some(&cookie)).status, 422);

    f.journal
        .failing
        .lock()
        .expect("unpoisoned")
        .insert(acct("acct_2"));
    let unread = get(&f, &ws("/dashboard"), Some(&cookie));
    assert_eq!(unread.status, 503);
    assert_eq!(code(&unread), "journal_unavailable");
}

#[test]
fn a_version_one_snapshot_after_a_version_two_reports_no_prior_close() {
    let rows = vec![
        snapshot(1, "990", Some("1000"), T0 - 600),
        snapshot(2, "1012.5", None, T0 - 300),
    ];
    let read = latest_snapshot(&rows).expect("reads").expect("a snapshot");
    assert_eq!(read.get("equity").and_then(Value::as_str), Some("1012.5"));
    assert_eq!(read.get("last_equity"), Some(&Value::Null));
    assert_eq!(read.get("day_change"), Some(&Value::Null));
    assert_eq!(
        latest_snapshot(&rows[..1])
            .expect("reads")
            .and_then(|s| s.get("day_change").cloned()),
        Some(Value::Str("-10".into())),
        "a loss is signed"
    );
    assert_eq!(latest_snapshot(&[]), Ok(None));
}

#[test]
fn a_latest_snapshot_that_does_not_read_is_unavailable_and_an_older_one_is_not_read() {
    let bad = |member: &str| {
        let mut laid = snapshot(2, "1", Some("1"), T0);
        let text = String::from_utf8(laid.body.clone()).expect("UTF-8");
        let broken = text.replace(&format!(r#""{member}":"1""#), &format!(r#""{member}":"x""#));
        let broken = broken.replace(
            &format!(r#""{member}":"400""#),
            &format!(r#""{member}":"x""#),
        );
        let broken = broken.replace(
            &format!(r#""{member}":"800""#),
            &format!(r#""{member}":"x""#),
        );
        laid.body = broken.into_bytes();
        laid
    };
    for member in ["equity", "cash", "buying_power", "last_equity"] {
        assert_eq!(latest_snapshot(&[bad(member)]), Err(Unread), "{member}");
        assert!(
            latest_snapshot(&[bad(member), snapshot(3, "5", None, T0)]).is_ok(),
            "{member}: only the latest is read"
        );
    }
    let mut unparsed = snapshot(2, "1", None, T0);
    unparsed.body = b"{".to_vec();
    assert_eq!(latest_snapshot(&[unparsed]), Err(Unread));
}

#[test]
fn a_range_is_named_by_its_query_and_reaches_back_its_span_in_buckets() {
    let cases = [
        (None, Range::Month, "1m", Some(2_592_000), 86_400),
        (
            Some("range=1m"),
            Range::Month,
            "1m",
            Some(2_592_000),
            86_400,
        ),
        (Some("range=1d"), Range::Day, "1d", Some(86_400), 300),
        (Some("range=1w"), Range::Week, "1w", Some(604_800), 3_600),
        (
            Some("range=3m"),
            Range::Quarter,
            "3m",
            Some(7_776_000),
            86_400,
        ),
        (
            Some("range=1y"),
            Range::Year,
            "1y",
            Some(31_536_000),
            86_400,
        ),
        (Some("range=all"), Range::All, "all", None, 86_400),
    ];
    for (query, range, name, span, bucket) in cases {
        assert_eq!(Range::from_query(query), Some(range), "{query:?}");
        assert_eq!(range.name(), name);
        assert_eq!(range.span_s(), span, "{name}");
        assert_eq!(range.bucket_s(), bucket, "{name}");
    }
    for query in ["range=", "range=2d", "range=1D", "range=1d&x=1", "x=1"] {
        assert_eq!(Range::from_query(Some(query)), None, "{query}");
    }
}

fn points(rows: &[StoredEvent], range: Range) -> Vec<(String, String)> {
    equity_points(rows, at(T0), range)
        .expect("reads")
        .iter()
        .map(|point| {
            let member = |name| {
                point
                    .get(name)
                    .and_then(Value::as_str)
                    .expect(name)
                    .to_owned()
            };
            (member("at"), member("equity"))
        })
        .collect()
}

#[test]
fn the_history_keeps_the_last_snapshot_of_each_bucket_in_the_range() {
    let bucket_start = T0 - T0.rem_euclid(300);
    let rows = vec![
        snapshot(1, "900", None, T0 - 86_401),
        snapshot(2, "950", None, T0 - 86_400),
        snapshot(3, "960", Some("950"), bucket_start - 1),
        snapshot(4, "970", None, bucket_start),
        snapshot(5, "980", None, T0),
        snapshot(6, "999", None, T0 + 1),
    ];
    assert_eq!(
        points(&rows, Range::Day),
        [
            (at(T0 - 86_400).to_string(), "950".to_owned()),
            (at(bucket_start - 1).to_string(), "960".to_owned()),
            (at(T0).to_string(), "980".to_owned()),
        ],
        "a day reaches back 86400 s exactly; 970 is not its bucket's last; a row after now is not served"
    );
    let day_start = T0 - T0.rem_euclid(86_400);
    let daily = vec![
        snapshot(1, "900", None, day_start - 86_400),
        snapshot(2, "910", None, day_start - 1),
        snapshot(3, "920", None, day_start),
        snapshot(4, "930", None, T0),
    ];
    assert_eq!(
        points(&daily, Range::All),
        [
            (at(day_start - 1).to_string(), "910".to_owned()),
            (at(T0).to_string(), "930".to_owned()),
        ]
    );
    assert_eq!(
        points(&daily, Range::Week).len(),
        4,
        "hourly buckets keep all four"
    );
    assert!(points(&[], Range::All).is_empty());
}

#[test]
fn a_snapshot_in_the_range_that_does_not_read_fails_the_history() {
    let mut broken = snapshot(1, "1", None, T0 - 10);
    broken.body = b"{".to_vec();
    assert_eq!(
        equity_points(&[broken.clone()], at(T0), Range::Day),
        Err(Unread)
    );
    let mut old = broken.clone();
    old.recorded_at = at(T0 - 90_000).to_string();
    assert_eq!(
        equity_points(&[old], at(T0), Range::Day),
        Ok(Vec::new()),
        "outside the range"
    );
    let mut undated = snapshot(1, "1", None, T0);
    undated.recorded_at = "yesterday".to_owned();
    assert_eq!(equity_points(&[undated], at(T0), Range::All), Err(Unread));
}

#[test]
fn the_history_route_serves_each_accounts_points_for_its_range() {
    let f = fixture(None);
    control(&f, "sha256:00");
    account(
        &f,
        "acct_1",
        vec![
            snapshot(1, "950", None, T0 - 200_000),
            snapshot(2, "1000", Some("990"), T0 - 60),
        ],
    );
    let cookie = viewer(&f);
    let reply = get(&f, &ws("/dashboard/history?range=1d"), Some(&cookie));
    assert_eq!(
        reply.status,
        200,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    let served = json(&reply);
    assert_eq!(served.get("range").and_then(Value::as_str), Some("1d"));
    let accounts = parse(
        format!(
            r#"[{{"account_ref":"acct_1","points":[{{"at":"{}","equity":"1000"}}]}},{{"account_ref":"acct_2","points":[]}}]"#,
            at(T0 - 60)
        )
        .as_bytes(),
    )
    .expect("JSON");
    assert_eq!(served.get("accounts"), Some(&accounts));
    assert_eq!(streams_read(&served), [ctl(), acct("acct_1")]);
    let month = json(&get(&f, &ws("/dashboard/history"), Some(&cookie)));
    assert_eq!(month.get("range").and_then(Value::as_str), Some("1m"));

    assert_eq!(
        get(&f, &ws("/dashboard/history?range=5y"), Some(&cookie)).status,
        422
    );

    let mut broken = snapshot(3, "1", None, T0 - 1);
    broken.body = b"{".to_vec();
    f.journal
        .streams
        .lock()
        .expect("unpoisoned")
        .get_mut(&acct("acct_1"))
        .expect("seeded")
        .push(broken);
    let unread = get(&f, &ws("/dashboard/history?range=1d"), Some(&cookie));
    assert_eq!(unread.status, 503);
    assert_eq!(code(&unread), "journal_unavailable");
    let dashboard = get(&f, &ws("/dashboard"), Some(&cookie));
    assert_eq!(dashboard.status, 503, "the latest snapshot does not read");

    f.journal
        .failing
        .lock()
        .expect("unpoisoned")
        .insert(acct("acct_2"));
    assert_eq!(
        get(&f, &ws("/dashboard/history"), Some(&cookie)).status,
        503
    );
}

/// The account stream's rows for `agt_a`'s orders and fills: an own order and its fill per tuple
/// `(order, instrument, side, qty, price)`, and another agent's buy of SPY between them.
fn trades(trades: &[(&str, &str, &str, &str, &str)]) -> Vec<StoredEvent> {
    let stream = acct("acct_1");
    let mut rows = vec![row(&stream, 1, "StreamOpened", "{}", T0 - 900)];
    let mut push = |event_type: &str, payload: String| {
        let seq = u64::try_from(rows.len()).expect("a count") + 1;
        let mut laid = row(&stream, seq, event_type, &payload, T0 - 800);
        if event_type == "OrderSubmitted" {
            laid.schema_version = 2;
        }
        rows.push(laid);
    };
    let mut order = |agent: &str,
                     id: &str,
                     instrument: &str,
                     side: &str,
                     qty: &str,
                     price: &str| {
        push(
            "OrderRequestRecorded",
            format!(
                r#"{{"agent_id":"{agent}","intent_id":null,"purpose":"open","extended_hours":false,"stop_price":null,"order_class":null,"take_profit":null,"stop":null,"rung":null,"at_floor":null,"risk_clock":"2026-10-03T03:00:00.000000000Z"}}"#
            ),
        );
        push(
            "OrderSubmitted",
            format!(
                r#"{{"client_order_id":"{id}","attempt":1,"instrument_id":"{instrument}","side":"{side}","type":"limit","tif":"day","qty":"{qty}","limit_price":"{price}","risk_clock":"2026-10-03T03:00:00.000000000Z"}}"#
            ),
        );
        push(
            "FillApplied",
            format!(
                r#"{{"fill_id":"f-{id}","client_order_id":"{id}","instrument_id":"{instrument}","side":"{side}","qty_gross":"{qty}","price":"{price}","trade_date":"2026-10-02","risk_clock":"2026-10-03T03:00:00.000000000Z","fees":[]}}"#
            ),
        );
    };
    order("agt_b", "md-theirs", SPY, "buy", "7", "1");
    for (id, instrument, side, qty, price) in trades {
        order("agt_a", id, instrument, side, qty, price);
    }
    push(
        "MarkUpdated",
        format!(
            r#"{{"instrument_id":"{SPY}","price":"103","source":"quote","feed":"iex","bid":"103","sane":true,"session":"regular","risk_clock":"2026-10-03T03:00:05.000000000Z"}}"#
        ),
    );
    rows
}

#[test]
fn an_agents_pnl_is_its_sub_ledgers_realized_and_unrealized() {
    let (bytes, version) = document();
    let f = fixture(stored(bytes));
    control(&f, &version);
    account(
        &f,
        "acct_1",
        trades(&[
            ("md-1", SPY, "buy", "2", "101.5"),
            ("md-2", SPY, "sell", "1", "104"),
            ("md-3", QQQ, "buy", "1", "10"),
            ("md-4", QQQ, "sell", "1", "12"),
        ]),
    );
    let cookie = viewer(&f);
    let reply = get(&f, &ws("/agents/agt_a/pnl"), Some(&cookie));
    assert_eq!(
        reply.status,
        200,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    let served = json(&reply);
    let mut expected = [
        (
            SPY,
            r#"{"instrument_id":"SPY_ID","filled_qty":"1","realized_gross":"2.5","unrealized_pnl":"1.5"}"#,
        ),
        (
            QQQ,
            r#"{"instrument_id":"QQQ_ID","filled_qty":"0","realized_gross":"2","unrealized_pnl":"0"}"#,
        ),
    ];
    expected.sort_by_key(|(id, _)| *id);
    let listed: Vec<Value> = expected
        .iter()
        .map(|(_, text)| {
            parse(
                text.replace("SPY_ID", SPY)
                    .replace("QQQ_ID", QQQ)
                    .as_bytes(),
            )
            .expect("JSON")
        })
        .collect();
    assert_eq!(
        served.get("instruments"),
        Some(&Value::Array(listed)),
        "SPY: 1 × (104 − 101.5) realized, 1 × 103 − 101.5 unrealized at the sane bid; QQQ closed, \
         1 × (12 − 10) realized and nothing unrealized with no mark; agt_b's buy is not agt_a's"
    );
    for (member, figure) in [
        ("agent_id", "agt_a"),
        ("environment", "paper"),
        ("account_ref", "acct_1"),
        ("realized_gross", "4.5"),
        ("unrealized_pnl", "1.5"),
    ] {
        assert_eq!(
            served.get(member).and_then(Value::as_str),
            Some(figure),
            "{member}"
        );
    }
    assert_eq!(streams_read(&served), [ctl(), acct("acct_1")]);
    assert_eq!(
        get(&f, &ws("/agents/agt_a/pnl?x=1"), Some(&cookie)).status,
        422
    );
    assert_eq!(get(&f, &ws("/agents/agt_x/pnl"), Some(&cookie)).status, 404);

    f.journal
        .failing
        .lock()
        .expect("unpoisoned")
        .insert(acct("acct_1"));
    let unread = get(&f, &ws("/agents/agt_a/pnl"), Some(&cookie));
    assert_eq!(unread.status, 503);
    assert_eq!(code(&unread), "journal_unavailable");
}

#[test]
fn a_figure_not_reported_leaves_its_sum_not_reported() {
    let (bytes, version) = document();
    let f = fixture(stored(bytes));
    control(&f, &version);
    account(&f, "acct_1", trades(&[("md-1", QQQ, "buy", "2", "10")]));
    let cookie = viewer(&f);
    let served = json(&get(&f, &ws("/agents/agt_a/pnl"), Some(&cookie)));
    let Some(Value::Array(instruments)) = served.get("instruments") else {
        panic!("instruments");
    };
    assert_eq!(
        instruments[0].get("unrealized_pnl"),
        Some(&Value::Null),
        "no mark"
    );
    assert_eq!(
        instruments[0].get("realized_gross").and_then(Value::as_str),
        Some("0")
    );
    assert_eq!(served.get("unrealized_pnl"), Some(&Value::Null));
    assert_eq!(
        served.get("realized_gross").and_then(Value::as_str),
        Some("0")
    );

    let mut rows = trades(&[("md-1", SPY, "buy", "2", "10")]);
    let seq = u64::try_from(rows.len()).expect("a count") + 1;
    rows.push(row(
        &acct("acct_1"),
        seq,
        "PositionReleased",
        &format!(r#"{{"agent_id":"agt_a","instrument_id":"{SPY}","qty":"1"}}"#),
        T0 - 700,
    ));
    account(&f, "acct_1", rows);
    let released = json(&get(&f, &ws("/agents/agt_a/pnl"), Some(&cookie)));
    assert_eq!(
        released.get("realized_gross"),
        Some(&Value::Null),
        "a release leaves it to the accounting fold"
    );
    assert_eq!(released.get("unrealized_pnl"), Some(&Value::Null));
}

#[test]
fn an_agent_with_no_account_or_no_fill_has_nothing_realized() {
    let (bytes, version) = document();
    let f = fixture(stored(bytes));
    control(&f, &version);
    let cookie = viewer(&f);
    let served = json(&get(&f, &ws("/agents/agt_a/pnl"), Some(&cookie)));
    assert_eq!(
        served.get("account_ref").and_then(Value::as_str),
        Some("acct_1")
    );
    assert_eq!(served.get("instruments"), Some(&Value::Array(Vec::new())));
    assert_eq!(
        served.get("realized_gross").and_then(Value::as_str),
        Some("0")
    );
    assert_eq!(
        served.get("unrealized_pnl").and_then(Value::as_str),
        Some("0")
    );

    let without = fixture(None);
    control(&without, &version);
    let cookie = viewer(&without);
    let served = json(&get(&without, &ws("/agents/agt_a/pnl"), Some(&cookie)));
    assert_eq!(
        served.get("account_ref"),
        Some(&Value::Null),
        "no document names a connection"
    );
    assert_eq!(served.get("instruments"), Some(&Value::Array(Vec::new())));
}

#[test]
fn a_fill_that_does_not_read_fails_the_pnl() {
    let (bytes, version) = document();
    let f = fixture(stored(bytes));
    control(&f, &version);
    let mut rows = trades(&[("md-1", SPY, "buy", "2", "10")]);
    let seq = u64::try_from(rows.len()).expect("a count") + 1;
    rows.push(row(
        &acct("acct_1"),
        seq,
        "FillApplied",
        r#"{"fill_id":"f-x","client_order_id":"md-1","instrument_id":"b0c2a3e4-5f60-4718-9a2b-3c4d5e6f7081","side":"short","qty_gross":"1","price":"1","trade_date":"2026-10-02","risk_clock":"2026-10-03T03:00:00.000000000Z","fees":[]}"#,
        T0 - 700,
    ));
    account(&f, "acct_1", rows);
    let cookie = viewer(&f);
    let unread = get(&f, &ws("/agents/agt_a/pnl"), Some(&cookie));
    assert_eq!(unread.status, 503, "no P&L from part of its fills");
    assert_eq!(code(&unread), "journal_unavailable");
}
