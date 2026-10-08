//! K1a's Postgres grant (E8-3, DEC-533): the binary's `approvals list`, `show`, `approve` and
//! `skip` over a real journal, which checks every record against its closed schema (journal spec
//! §9.6 and §9.7). The agent stream holds two journal-valid requests; the test plays the runtime,
//! recording the grant [`RUNTIME_LAG`] after the control stream holds it, while `approve` waits.
//! Without `MANDATE_PG_URL` the test says so and passes, as the other Postgres tests do (DEC-109).

#[path = "../../mandate-journal/tests/common/mod.rs"]
mod common;
#[path = "../../mandate-journal/tests/conformance/mod.rs"]
#[allow(
    unused_imports,
    unused_macros,
    reason = "only `support`'s backend trait is used here; the suite runs in `mandate-journal-pg`"
)]
mod conformance;
#[path = "../../mandate-journal-pg/tests/support/mod.rs"]
mod support;

use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command as Process, Output, Stdio};
use std::time::{Duration, SystemTime};

use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_cli::control::ControlJournal;
use mandate_cli::postgres::{JournalArgs, PgControlJournal, read_stream};
use mandate_journal::{AppendOutcome, StoredEvent, StreamId};
use mandate_journal_pg::APP_ROLE;
use mandate_time::UtcNanos;
use support::{TestDb, URL_VAR};

const AGENT: &str = "agent-a";
const MANDATE: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const BUILD: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
/// `ctl:ws1`'s opening in paper, by a writer that is not the CLI (journal spec §2).
const OPENING: &str = r#"{"actor":{"build":"sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","id":"control_services","kind":"system","version":"0.1.0"},"artifact_refs":[],"causation_id":null,"clock_source":"local","config_refs":{},"correlation_id":null,"envelope_version":1,"environment":"paper","event_id":"01J8Y0A0A000000000000000S1","event_time":"2026-09-20T13:00:00.000000000Z","event_type":"StreamOpened","payload":{"stream_type":"control","workspace_id":"ws1"},"pii_refs":[],"schema_version":1,"stream_id":"ctl:ws1"}"#;

/// How long after the answer commits the test's runtime records it: longer than a grant that
/// re-read its outcome without pausing between reads would wait, and well inside `--wait-s 10`.
const RUNTIME_LAG: Duration = Duration::from_secs(2);

/// The wall clock's second, which the binary reads too: a request's deadline must lie ahead of it.
#[allow(
    clippy::disallowed_methods,
    reason = "the binary under test reads the wall clock, so its requests' deadlines are set from it"
)]
fn wall_secs() -> i64 {
    let since = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    i64::try_from(since.as_secs()).unwrap()
}

/// A Crockford ULID of the test's own, numbered.
fn ulid(n: u32) -> String {
    format!("01J8ZA{n:020}")
}

/// Every digest reference in `value`, sorted, each once: the `artifact_refs` journal spec §3
/// requires, found here by the reference's form rather than by the journal's own scan.
fn references(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Str(s)
            if s.len() == 71
                && s.starts_with("sha256:")
                && s[7..]
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) =>
        {
            out.insert(s.clone());
        }
        Value::Array(items) => items.iter().for_each(|v| references(v, out)),
        Value::Object(members) => members.values().for_each(|v| references(v, out)),
        _ => {}
    }
}

/// The canonical bytes of an agent-stream event the runtime would write.
fn agent_event(
    event_id: &str,
    event_type: &str,
    causation: Option<&str>,
    config_refs: &str,
    payload: &Value,
) -> Vec<u8> {
    let mut listed = BTreeSet::new();
    references(payload, &mut listed);
    let refs: Vec<String> = listed.iter().map(|r| format!("\"{r}\"")).collect();
    let causation = causation.map_or_else(|| "null".to_owned(), |id| format!("\"{id}\""));
    let body = format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{event_id}",
        "stream_id":"agent:ws1:{AGENT}","event_type":"{event_type}","schema_version":1,
        "event_time":"2026-09-21T13:00:00.000000000Z","clock_source":"local",
        "causation_id":{causation},"correlation_id":null,
        "actor":{{"kind":"agent","id":"{AGENT}","version":"0.1.0","build":"{BUILD}"}},
        "config_refs":{config_refs},"payload":{},"artifact_refs":[{}],"pii_refs":[]}}"#,
        String::from_utf8(to_canonical(payload)).unwrap(),
        refs.join(",")
    );
    to_canonical(&parse(body.as_bytes()).unwrap())
}

fn value(json: &str) -> Value {
    parse(json.as_bytes()).unwrap()
}

/// A request for `qty` AAPL at 150, due at `deadline`, as journal spec §9.6 closes it: its content
/// object, and the request event whose `content_hash` is that object's.
fn request(event_id: &str, qty: &str, deadline: i64) -> (Value, Vec<u8>) {
    let stamp = UtcNanos::from_parts(deadline, 0).unwrap();
    let usd = 150 * qty.parse::<i64>().unwrap();
    let content = value(&format!(
        r#"{{"action":{{"instrument":"AAPL","asset_class":"us_equity","side":"buy","qty":"{qty}",
            "limit":"150","order_usd":"{usd}","purpose":"open"}},
        "trigger":{{"mandate_version":"{MANDATE}","decided_by":"rule:open"}},
        "evidence":{{"combined_score":{{"label":"combined model score, not a probability of profit",
            "value":"0.75"}},"outputs":[]}},
        "risk_impact":[{{"field":"order_usd","value":"{usd}","cap":null}}],
        "reference_mark":{{"price":"149.5","seq":7}},
        "deadline":"{stamp}",
        "default":"If you do nothing, this action is skipped",
        "choices":["approve","skip"],
        "approvers":{{"required":1,"independent":false}}}}"#
    ));
    let hash = format!("sha256:{}", Digest::of(&to_canonical(&content)).to_hex());
    let payload = value(&format!(
        r#"{{"instrument":"AAPL","asset_class":"us_equity","side":"buy","qty":"{qty}",
        "limit":"150","purpose":"open","mandate_version":"{MANDATE}","decided_by":"rule:open",
        "combined_score":"0.75","reference_mark":{{"price":"149.5","seq":7}},
        "approvers_required":1,"independent_required":false,"deadline":{deadline},
        "timeout_s":300,"on_timeout":"skip","content":{},"content_hash":"{hash}"}}"#,
        String::from_utf8(to_canonical(&content)).unwrap()
    ));
    let man = format!(r#"{{"mandate_version":"{MANDATE}"}}"#);
    let event = agent_event(event_id, "ApprovalRequested", None, &man, &payload);
    (content, event)
}

fn delivered(event_id: &str, approval: &str) -> Vec<u8> {
    let payload = value(&format!(
        r#"{{"approval":"{approval}","channel":"cli_inbox","status":"delivered","message_id":null}}"#
    ));
    let man = format!(r#"{{"mandate_version":"{MANDATE}"}}"#);
    agent_event(event_id, "ApprovalDelivered", None, &man, &payload)
}

/// The runtime's record of an admitted grant and its acting re-validation (journal spec §9.7),
/// copied from the answer the control stream holds.
fn recorded(answer: &Value, approval: &str) -> [Vec<u8>; 2] {
    let payload = |name: &str| answer.get("payload").and_then(|p| p.get(name)).unwrap();
    let answer_id = answer.get("event_id").and_then(Value::as_str).unwrap();
    let (responded_id, revalidated_id) = (ulid(90), ulid(91));
    let man = format!(r#"{{"mandate_version":"{MANDATE}"}}"#);
    let responded = value(&format!(
        r#"{{"approval":"{approval}","verdict":"approved","responder":"u1","role":"approver",
        "result":"admitted","reason":null,"effective_at":{},"step_up":{},
        "quorum":{{"independent":false,"required":1}},"separation_of_duties":null,
        "delegation":null}}"#,
        String::from_utf8(to_canonical(payload("submitted_at"))).unwrap(),
        String::from_utf8(to_canonical(payload("step_up"))).unwrap(),
    ));
    let revalidated = value(&format!(
        r#"{{"approval":"{approval}","result":"act","reason":null,
        "mandate_version_bound":"{MANDATE}","mandate_version_now":"{MANDATE}","mode":"normal",
        "instrument_restricted":false,"decided_by_bound":"rule:open",
        "decided_by_now":"rule:open","dry_run":"allow","dry_run_reason":null,
        "m_req":"149.5","m_now":"150","band_bp":100}}"#
    ));
    [
        agent_event(
            &responded_id,
            "ApprovalResponded",
            Some(answer_id),
            &man,
            &responded,
        ),
        agent_event(
            &revalidated_id,
            "ApprovalRevalidated",
            Some(&responded_id),
            &man,
            &revalidated,
        ),
    ]
}

/// Appends `drafts` to `stream` as a writer that is not the CLI, and asserts they committed.
fn seed(journal: &mut PgControlJournal, stream: &StreamId, drafts: &[&[u8]]) {
    let epoch = journal.take_ownership(stream).unwrap();
    let head = journal.head(stream).unwrap().seq;
    let at = UtcNanos::from_parts(1_790_000_000, 0).unwrap();
    let outcome = journal.append(stream, head, epoch, at, drafts);
    assert!(
        matches!(outcome, Ok(AppendOutcome::Committed(_))),
        "the journal accepts the seeded events: {outcome:?}"
    );
}

/// A DSN acting as the application role in `db`'s schema.
fn dsn(db: &TestDb) -> String {
    let url = std::env::var(URL_VAR).unwrap();
    let joiner = if url.contains('?') { '&' } else { '?' };
    let options = format!("-c%20search_path%3D{}%20-c%20role%3D{APP_ROLE}", db.schema);
    format!("{url}{joiner}options={options}")
}

/// The password in `MANDATE_PG_URL`, if it names one.
fn url_password() -> Option<String> {
    let url = std::env::var(URL_VAR).ok()?;
    let (credentials, _) = url.split_once("://")?.1.split_once('@')?;
    credentials
        .split_once(':')
        .map(|(_, password)| password.to_owned())
}

/// The binary's command line for `argv`, as `u1` in `ws1`, on `dsn` and `store`.
fn mandate(argv: &[&str], dsn: &str, store: &Path) -> Process {
    let mut command = Process::new(env!("CARGO_BIN_EXE_mandate"));
    command
        .args(argv)
        .args(["--workspace", "ws1", "--user", "u1", "--journal", dsn])
        .args(["--store", store.to_str().unwrap()]);
    command
}

/// What a run printed, which never names the DSN or its password; `Err` with stderr when it failed.
fn lines(run: &Output) -> Result<Vec<String>, String> {
    let (stdout, stderr) = (
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    for shown in ["postgres://".to_owned()].into_iter().chain(url_password()) {
        assert!(
            !stdout.contains(&shown) && !stderr.contains(&shown),
            "{stdout}{stderr} shows {shown}"
        );
    }
    if run.status.success() && stderr.is_empty() {
        Ok(stdout.lines().map(str::to_owned).collect())
    } else {
        assert!(stdout.is_empty(), "a refusal prints nothing: {stdout}");
        Err(stderr.into_owned())
    }
}

fn body(row: &StoredEvent) -> Value {
    parse(&row.body).unwrap()
}

fn text<'v>(value: &'v Value, path: &str) -> Option<&'v str> {
    path.split('.')
        .try_fold(value, |v, k| v.get(k))
        .and_then(Value::as_str)
}

/// The control stream's rows once `granting` has committed its answer, read every 100 ms for up to
/// 10 s; a run that ended without committing fails with what it said.
fn answered(target: &JournalArgs, granting: &mut Child) -> Vec<StoredEvent> {
    let ctl = StreamId::parse("ctl:ws1").unwrap();
    for _ in 0..100 {
        let rows = read_stream(&target.journal, &ctl).unwrap();
        if rows.len() == 2 {
            return rows;
        }
        if granting.try_wait().unwrap().is_some() {
            let mut stderr = String::new();
            if let Some(mut pipe) = granting.stderr.take() {
                pipe.read_to_string(&mut stderr).unwrap();
            }
            panic!("the grant ended without committing: {stderr}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("the grant never committed")
}

/// The binary lists both requests without their content, shows the first exactly, grants it with
/// `cli_confirm` evidence and the content hash in `artifact_refs` (DEC-533 item 6), waits for the
/// runtime's record and reports it, refuses a second grant of it, and skips the second request.
/// Every record the journal holds passed its closed schema on append.
#[test]
fn the_binary_lists_shows_grants_and_skips_in_postgres() {
    let Some(db) = TestDb::new() else {
        return;
    };
    let dsn = dsn(&db);
    let root = std::env::temp_dir().join(format!("mandate-cli-grant-{}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    let store = root.join("store");
    let target = JournalArgs {
        journal: dsn.as_str().into(),
        store: store.clone(),
    };
    let mut journal = PgControlJournal::open(&target).unwrap();
    let ctl = StreamId::parse("ctl:ws1").unwrap();
    let agent = StreamId::parse(&format!("agent:ws1:{AGENT}")).unwrap();
    seed(&mut journal, &ctl, &[OPENING.as_bytes()]);
    let opened = value(&format!(
        r#"{{"stream_type":"agent","workspace_id":"ws1","agent_id":"{AGENT}"}}"#
    ));
    let deadline = wall_secs() + 3600;
    let (first, second) = (ulid(1), ulid(2));
    let (content, asked) = request(&first, "2", deadline);
    let (other, asked_too) = request(&second, "3", deadline + 60);
    seed(
        &mut journal,
        &agent,
        &[
            &agent_event(&ulid(0), "StreamOpened", None, "{}", &opened),
            &asked,
            &delivered(&ulid(3), &first),
            &asked_too,
            &delivered(&ulid(4), &second),
        ],
    );
    let run = |argv: &[&str]| lines(&mandate(argv, &dsn, &store).output().unwrap());
    let hash = |content: &Value| format!("sha256:{}", Digest::of(&to_canonical(content)).to_hex());

    let listed = run(&["approvals", "list", "--agent", AGENT]).unwrap();
    assert_eq!(listed.len(), 2, "{listed:?}");
    for (line, approval) in listed.iter().zip([&first, &second]) {
        let words: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(&words[..3], [approval.as_str(), AGENT, "pending"], "{line}");
        assert!(
            !line.contains("AAPL") && !line.contains("sha256:"),
            "{line}"
        );
    }

    let shown = run(&["approvals", "show", "--agent", AGENT, &first]).unwrap();
    let content_hash = hash(&content);
    let code = &content_hash[7..15];
    assert_eq!(
        shown,
        [
            format!("approval {first} deadline {deadline}"),
            String::from_utf8(to_canonical(&content)).unwrap(),
            format!("content_hash {}", hash(&content)),
            format!("code {code}"),
        ]
    );

    let granting = ["approvals", "approve", "--agent", AGENT, "--code", code];
    let mut child = mandate(&granting, &dsn, &store)
        .args(["--wait-s", "10", &first])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let rows = answered(&target, &mut child);
    let answer = body(&rows[1]);
    std::thread::sleep(RUNTIME_LAG);
    seed(
        &mut journal,
        &agent,
        &recorded(&answer, &first).each_ref().map(Vec::as_slice),
    );
    let granted = lines(&child.wait_with_output().unwrap()).unwrap();
    assert_eq!(
        granted,
        [
            format!(
                "granted {first} as event {} at seq 2 for {}",
                rows[1].event_id,
                hash(&content)
            ),
            "Admitted and sent to the executor; the gate still decides.".to_owned(),
        ]
    );
    for (path, expected) in [
        ("event_type", "ApprovalResponseSubmitted"),
        ("environment", "paper"),
        ("actor.kind", "user"),
        ("actor.id", "u1"),
        ("payload.agent", AGENT),
        ("payload.approval", first.as_str()),
        ("payload.verdict", "approved"),
        ("payload.responder", "u1"),
        ("payload.step_up.method", "cli_confirm"),
    ] {
        assert_eq!(text(&answer, path), Some(expected), "{path}");
    }
    let assertion = text(&answer, "payload.step_up.assertion_id").unwrap();
    assert!(assertion.starts_with("cli-"), "{assertion}");
    assert_eq!(
        answer.get("artifact_refs"),
        Some(&Value::Array(vec![Value::Str(hash(&content))])),
        "the answer lists its content hash (DEC-533 item 6)"
    );

    let again = run(&[&granting[..], &["--wait-s", "0", first.as_str()][..]].concat());
    let refusal = again.expect_err("an acted approval is not pending");
    assert!(refusal.contains("not_pending"), "{refusal}");

    let skipped = run(&["approvals", "skip", "--agent", AGENT, &second]).unwrap();
    let rows = read_stream(&target.journal, &ctl).unwrap();
    assert_eq!(rows.len(), 3, "the refused grant committed nothing");
    assert_eq!(
        skipped,
        [format!(
            "skipped {second} as event {} at seq 3",
            rows[2].event_id
        )]
    );
    let skip = body(&rows[2]);
    assert_eq!(text(&skip, "payload.verdict"), Some("skipped"));
    assert_eq!(
        skip.get("payload").and_then(|p| p.get("step_up")),
        Some(&Value::Null)
    );
    assert_eq!(
        skip.get("artifact_refs"),
        Some(&Value::Array(vec![Value::Str(hash(&other))]))
    );

    let listed = run(&["approvals", "list", "--agent", AGENT]).unwrap();
    let states: Vec<&str> = listed
        .iter()
        .map(|line| line.split_whitespace().nth(2).unwrap_or_default())
        .collect();
    assert_eq!(
        states,
        ["pending", "acted"],
        "the skip waits on the runtime's record; the grant acted: {listed:?}"
    );
    std::fs::remove_dir_all(root).ok();
}
