//! Lane ID1 (DEC-991), over Postgres: the `mandate` binary opens a workspace and writes its
//! founding grant, which the Postgres journal's own append admits; running it again succeeds and
//! commits nothing, as a Kubernetes Job's retry does; another member and an unopened workspace are
//! refused with nothing appended. No output shows a part of the DSN. Skipped without
//! `MANDATE_PG_URL`, as the other Postgres journal tests are.

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

use std::path::PathBuf;
use std::process::{Command as Process, Output};

use mandate_canon::{Value, parse};
use mandate_cli::postgres::read_stream;
use mandate_journal::{StoredEvent, StreamId};
use mandate_journal_pg::APP_ROLE;
use support::{TestDb, URL_VAR};

const FOUNDER: &str = "01J8Z4M0AD0000000000000AD1";
const OTHER: &str = "01J8Z4M0AD0000000000000AD2";

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("mandate-cli-id1-{}-{name}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    root
}

fn dsn_of(db: &TestDb) -> String {
    let url = std::env::var(URL_VAR).unwrap();
    let joiner = if url.contains('?') { '&' } else { '?' };
    let options = format!("-c%20search_path%3D{}%20-c%20role%3D{APP_ROLE}", db.schema);
    format!("{url}{joiner}options={options}")
}

/// The password and the host in `MANDATE_PG_URL`, which no output may show.
fn dsn_parts() -> Vec<String> {
    let url = std::env::var(URL_VAR).unwrap_or_default();
    let Some((credentials, location)) = url
        .split_once("://")
        .and_then(|(_, rest)| rest.split_once('@'))
    else {
        return Vec::new();
    };
    let password = credentials.split_once(':').map(|(_, p)| p.to_owned());
    let host = location.split(['/', ':', '?']).next().map(str::to_owned);
    [password, host]
        .into_iter()
        .flatten()
        .filter(|p| !p.is_empty())
        .collect()
}

fn mandate(argv: &[&str]) -> Output {
    let run = Process::new(env!("CARGO_BIN_EXE_mandate"))
        .args(argv)
        .output()
        .unwrap();
    for part in dsn_parts() {
        for out in [&run.stdout, &run.stderr] {
            let shown = String::from_utf8_lossy(out);
            assert!(!shown.contains(&part), "{argv:?} shows a DSN part: {shown}");
        }
    }
    run
}

fn workspace(verb: &str, args: &[&str], dsn: &str, store: &str) -> Output {
    let mut argv = vec!["workspace", verb];
    argv.extend_from_slice(args);
    argv.extend_from_slice(&["--journal", dsn, "--store", store]);
    mandate(&argv)
}

fn found(member: &str, workspace_id: &str, dsn: &str, store: &str) -> Output {
    let args = [
        "--workspace",
        workspace_id,
        "--member",
        member,
        "--method",
        "oidc",
    ];
    workspace("found", &args, dsn, store)
}

fn rows(dsn: &str, stream: &str) -> Vec<StoredEvent> {
    read_stream(&dsn.into(), &StreamId::parse(stream).unwrap()).unwrap()
}

fn succeeded(run: &Output) -> String {
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success() && stderr.is_empty(), "{stderr}");
    String::from_utf8(run.stdout.clone()).unwrap()
}

fn refused(run: &Output, code: &str) {
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(!run.status.success() && run.stdout.is_empty(), "{stderr}");
    assert!(stderr.contains(code), "{stderr}");
}

#[test]
fn the_binary_founds_a_workspace_once() {
    let Some(db) = TestDb::new() else {
        return;
    };
    let (dsn, store) = (dsn_of(&db), scratch("store"));
    let store = store.to_str().unwrap();
    refused(&found(FOUNDER, "ws1", &dsn, store), "workspace_not_open");
    assert!(rows(&dsn, "ctl:ws1").is_empty());
    succeeded(&workspace("open", &["--workspace", "ws1"], &dsn, store));
    let printed = succeeded(&found(FOUNDER, "ws1", &dsn, store));
    assert!(
        printed.contains(FOUNDER) && printed.contains("seq 2"),
        "{printed}"
    );
    let stream = rows(&dsn, "ctl:ws1");
    let shape: Vec<(u64, &str)> = stream
        .iter()
        .map(|r| (r.seq, r.event_type.as_str()))
        .collect();
    assert_eq!(shape, [(1, "StreamOpened"), (2, "MemberActivated")]);
    let body = parse(&stream[1].body).unwrap();
    let at = |path: &str| {
        path.split('.')
            .try_fold(&body, |v, name| v.get(name))
            .cloned()
    };
    assert_eq!(at("actor.kind"), Some(Value::Str("system".to_owned())));
    assert_eq!(at("payload.member"), Some(Value::Str(FOUNDER.to_owned())));
    assert_eq!(
        at("payload.reason"),
        Some(Value::Str("founding".to_owned()))
    );
    let again = succeeded(&found(FOUNDER, "ws1", &dsn, store));
    assert!(again.contains("already"), "{again}");
    refused(&found(OTHER, "ws1", &dsn, store), "members_exist");
    assert_eq!(rows(&dsn, "ctl:ws1").len(), 2, "nothing more committed");
    std::fs::remove_dir_all(store).ok();
}
