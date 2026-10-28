//! E21-34 (OPS-4; DEC-2000): the `mandate` binary's `agent kill` and `agent pause` against the
//! workspace's Postgres journal, with nothing else running: each scope's kill switch is one
//! `OwnerCommandIssued` on the control stream with its one `OwnerAlertSent` right after it (E8-9k),
//! a pause with no agent in scope commits nothing and succeeds, and a pause that cannot commit
//! exits non-zero. Skipped without `MANDATE_PG_URL`.

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
use mandate_cli::control::ControlJournal;
use mandate_cli::postgres::{JournalArgs, PgControlJournal, read_stream};
use mandate_journal::{AppendOutcome, StreamId};
use mandate_journal_pg::APP_ROLE;
use mandate_time::UtcNanos;
use support::{TestDb, URL_VAR};

/// `ctl:ws1`'s opening in paper, by a writer that is not the CLI.
const OPENING: &str = r#"{"actor":{"build":"sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","id":"control_services","kind":"system","version":"0.1.0"},"artifact_refs":[],"causation_id":null,"clock_source":"local","config_refs":{},"correlation_id":null,"envelope_version":1,"environment":"paper","event_id":"01J8Y0A0A000000000000000S1","event_time":"2026-09-20T13:00:00.000000000Z","event_type":"StreamOpened","payload":{"stream_type":"control","workspace_id":"ws1"},"pii_refs":[],"schema_version":1,"stream_id":"ctl:ws1"}"#;

fn dsn(db: &TestDb) -> String {
    let url = std::env::var(URL_VAR).unwrap();
    let joiner = if url.contains('?') { '&' } else { '?' };
    let options = format!("-c%20search_path%3D{}%20-c%20role%3D{APP_ROLE}", db.schema);
    format!("{url}{joiner}options={options}")
}

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("mandate-cli-o4-{}-{name}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    root
}

#[test]
fn the_binary_commits_each_scope_kill_switch_and_pauses_in_postgres() {
    let Some(db) = TestDb::new() else {
        return;
    };
    let (dsn, store) = (dsn(&db), scratch("store"));
    let ctl = StreamId::parse("ctl:ws1").unwrap();
    let target = JournalArgs {
        journal: dsn.as_str().into(),
        store: store.clone(),
    };
    let mut opener = PgControlJournal::open(&target).unwrap();
    let epoch = opener.take_ownership(&ctl).unwrap();
    let at = UtcNanos::from_parts(1_790_000_000, 0).unwrap();
    let opened = opener.append(&ctl, 0, epoch, at, &[OPENING.as_bytes()]);
    assert!(
        matches!(opened, Ok(AppendOutcome::Committed(_))),
        "{opened:?}"
    );
    let mandate = |argv: &[&str]| -> Output {
        let owner = ["--workspace", "ws1", "--user", "u1", "--journal", &dsn];
        Process::new(env!("CARGO_BIN_EXE_mandate"))
            .args(["agent"])
            .args(argv)
            .args(owner)
            .args(["--store", store.to_str().unwrap()])
            .output()
            .unwrap()
    };
    let succeeds = |argv: &[&str]| {
        let run = mandate(argv);
        let stderr = String::from_utf8_lossy(&run.stderr);
        assert!(run.status.success(), "{argv:?}: {stderr}");
        String::from_utf8(run.stdout).unwrap()
    };
    let kills = [
        (vec!["kill", "--agent", "agent-a"], "agent", "agent-a"),
        (
            vec!["kill", "--connection", "conn_alpaca_paper_01"],
            "connection",
            "conn_alpaca_paper_01",
        ),
        (vec!["kill", "--workspace-wide"], "workspace", "ws1"),
    ];
    for (argv, _, _) in &kills {
        let printed = succeeds(argv);
        assert!(printed.starts_with("kill_switch "), "{printed}");
        assert!(!printed.contains("postgres://"), "{printed}");
    }
    assert_eq!(
        succeeds(&["pause", "--workspace-wide"]).trim(),
        "no deployed agent in scope; nothing committed"
    );
    let paused = succeeds(&["pause", "--agent", "agent-b"]);
    assert!(paused.starts_with("paused agent-b as event "), "{paused}");

    let refused = mandate(&["pause", "--agent", "agent.bad"]);
    assert!(
        !refused.status.success(),
        "a pause that cannot commit fails"
    );
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("1 pauses did not commit"), "{stderr}");

    let rows = read_stream(&target.journal, &ctl).unwrap();
    let events: Vec<Value> = rows[1..].iter().map(|r| parse(&r.body).unwrap()).collect();
    let field = |event: &Value, name: &str| {
        event
            .get("payload")
            .and_then(|p| p.get(name))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let member =
        |event: &Value, name: &str| event.get(name).and_then(Value::as_str).map(str::to_owned);
    let mut expected: Vec<Option<String>> = kills
        .iter()
        .flat_map(|_| {
            [
                Some("OwnerCommandIssued".to_owned()),
                Some("OwnerAlertSent".to_owned()),
            ]
        })
        .collect();
    expected.push(Some("OwnerCommandIssued".to_owned()));
    assert_eq!(
        events
            .iter()
            .map(|e| member(e, "event_type"))
            .collect::<Vec<_>>(),
        expected,
        "each kill switch is followed by its one alert, committed with it (E8-9k, #2572), and the pause by none: {events:?}"
    );
    for (pair, (argv, scope, subject)) in events.chunks(2).zip(&kills) {
        let (kill, alert) = (&pair[0], &pair[1]);
        assert_eq!(
            field(kill, "command").as_deref(),
            Some("kill_switch"),
            "{argv:?}"
        );
        assert_eq!(field(kill, "scope").as_deref(), Some(*scope), "{argv:?}");
        assert_eq!(
            field(kill, "subject").as_deref(),
            Some(*subject),
            "{argv:?}"
        );
        let command = member(kill, "event_id");
        assert!(command.is_some(), "{argv:?}");
        assert_eq!(
            field(alert, "kind").as_deref(),
            Some("kill_switch"),
            "{argv:?}"
        );
        assert_eq!(field(alert, "owner_command"), command, "{argv:?}");
        assert_eq!(field(alert, "subject"), command, "{argv:?}");
        assert_eq!(member(alert, "causation_id"), command, "{argv:?}");
    }
    let pause = &events[2 * kills.len()];
    assert_eq!(field(pause, "command").as_deref(), Some("pause"));
    assert_eq!(field(pause, "subject").as_deref(), Some("agent-b"));
    std::fs::remove_dir_all(store).ok();
}
