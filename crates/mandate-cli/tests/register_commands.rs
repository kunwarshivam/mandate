//! D1b (E10-16, DEC-527): `mandate config register` and `mandate model register`, run as the owner
//! two required flags name, always in paper. The refusals need no database, and the Postgres binary
//! test starts with them, so the pending gate sees the stub without one (DEC-510 item 3).

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

use std::path::{Path, PathBuf};
use std::process::Command as Process;

use clap::Parser;
use clap::error::ErrorKind;
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{Value, parse, to_canonical};
use mandate_cli::config::ConfigKind;
use mandate_cli::control::{ControlJournal, Now, Owner};
use mandate_cli::postgres::{JournalArgs, PgControlJournal, read_stream};
use mandate_cli::register::{ConfigCommand, ModelArgs, OwnerArgs, RegisterArgs, run, run_model};
use mandate_cli::{Cli, Command};
use mandate_journal::{AppendOutcome, ArtifactRef, ArtifactSource, Environment, StreamId};
use mandate_journal_pg::APP_ROLE;
use mandate_time::UtcNanos;
use support::{TestDb, URL_VAR};

const SPY: &str = r#"{"asset_class":"us_equity","etp":"plain","etp_classified_at":"2026-10-05T13:30:00Z","etp_source":"nasdaq_trader_symbol_directory","exchange":"arca","increment":"whole","instrument_id":"b0b6dd9d-8b9b-48a9-ba46-b9d54906e415","symbol":"SPY"}"#;
/// `ctl:ws1`'s opening in paper, as the journal vectors' control stream opens (journal spec §2): the
/// stream these commands append to is opened before them, by a writer that is not the CLI.
const OPENING: &str = r#"{"actor":{"build":"sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","id":"control_services","kind":"system","version":"0.1.0"},"artifact_refs":[],"causation_id":null,"clock_source":"local","config_refs":{},"correlation_id":null,"envelope_version":1,"environment":"paper","event_id":"01J8Y0A0A000000000000000S1","event_time":"2026-09-20T13:00:00.000000000Z","event_type":"StreamOpened","payload":{"stream_type":"control","workspace_id":"ws1"},"pii_refs":[],"schema_version":1,"stream_id":"ctl:ws1"}"#;
const SENTINELS: [&str; 3] = [
    "d1b-user-sentinel",
    "d1b-password-sentinel",
    "d1b-host-sentinel.invalid",
];

fn now() -> Now {
    Now {
        at: UtcNanos::from_parts(1_790_000_000, 0).unwrap(),
        secs: 1_790_000_000,
    }
}

/// A fresh, absent directory for one test.
fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("mandate-cli-d1b-{}-{name}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    root
}

fn owner_args(workspace: &str, user: &str) -> OwnerArgs {
    let (workspace, user) = (workspace.to_owned(), user.to_owned());
    OwnerArgs { workspace, user }
}

fn snapshot_args(owner: OwnerArgs, file: &Path, journal: &str, store: &Path) -> RegisterArgs {
    RegisterArgs {
        kind: ConfigKind::InstrumentSnapshot,
        file: file.to_owned(),
        owner,
        target: JournalArgs {
            journal: journal.into(),
            store: store.to_owned(),
        },
    }
}

/// The message a command refused with, checked to carry `code` and no part of the DSN.
fn refused_with<T: std::fmt::Debug>(result: anyhow::Result<T>, code: &str) {
    let why = result.map(|t| format!("not refused: {t:?}"));
    let why = why.unwrap_or_else(|e| format!("{e:#}"));
    assert!(why.contains(code), "{code}: {why}");
    let named: Vec<&str> = SENTINELS.into_iter().filter(|s| why.contains(s)).collect();
    assert!(named.is_empty(), "{code} names {named:?}: {why}");
}

#[test]
#[ignore = "pending E10-16"]
fn the_owner_is_two_required_flags_and_always_paper() {
    let paper = Owner {
        workspace: "ws1".into(),
        user: "user_owner-01".into(),
        environment: Environment::Paper,
    };
    assert_eq!(
        owner_args("ws1", "user_owner-01").owner(),
        Ok(paper.clone())
    );
    let line = "mandate config register --kind instrument_snapshot spy.json --workspace ws1 \
                --user user_owner-01 --journal postgres://h/j --store s";
    let full: Vec<&str> = line.split_whitespace().collect();
    let parsed = Cli::try_parse_from(&full)
        .map_err(|e| e.to_string())
        .unwrap();
    let Command::Config(ConfigCommand::Register(args)) = parsed.command else {
        panic!("config register")
    };
    let parsed = (args.kind, args.owner.owner());
    assert_eq!(parsed, (ConfigKind::InstrumentSnapshot, Ok(paper)));
    for flag in ["--workspace", "--user"] {
        let at = full.iter().position(|a| *a == flag).unwrap();
        let without: Vec<&str> = [&full[..at], &full[at + 2..]].concat();
        let kind = Cli::try_parse_from(without).err().map(|e| e.kind());
        assert_eq!(kind, Some(ErrorKind::MissingRequiredArgument), "{flag}");
    }
    for extra in [["--environment", "live"], ["--env", "live"]] {
        let given: Vec<&str> = full.iter().copied().chain(extra).collect();
        let kind = Cli::try_parse_from(given).err().map(|e| e.kind());
        assert_eq!(kind, Some(ErrorKind::UnknownArgument), "{extra:?}");
    }
    let model = "mandate model register quant.ma_crossover 1.0.0 --user u1 --journal j --store s";
    let kind = Cli::try_parse_from(model.split_whitespace())
        .err()
        .map(|e| e.kind());
    assert_eq!(kind, Some(ErrorKind::MissingRequiredArgument), "model");
}

/// With no database: a bad owner id is refused before the file is read, the journal opened, or the
/// store created; a missing file before the journal is opened; and a snapshot outside DEC-523
/// without a connection. No refusal names any part of the DSN (`AGENTS.md` rule 7).
fn refusals_without_a_database() {
    let [user, password, host] = SENTINELS;
    let dsn = format!("postgres://{user}:{password}@{host}:1/j");
    let (dir, store) = (scratch("refusals"), scratch("refusals-store"));
    std::fs::create_dir_all(&dir).unwrap();
    let missing = dir.join("missing.json");
    let cases = [
        ("", "u1", "owner_workspace_invalid"),
        ("ws:1", "u1", "owner_workspace_invalid"),
        ("ws 1", "u1", "owner_workspace_invalid"),
        ("ws1", "", "owner_user_invalid"),
        ("ws1", "founder@example.com", "owner_user_invalid"),
        ("ws1", "Founder Name", "owner_user_invalid"),
        ("ws1", "User1", "owner_user_invalid"),
        ("ws1", &"u".repeat(65), "owner_user_invalid"),
    ];
    for (workspace, user, code) in cases {
        let owner = owner_args(workspace, user);
        let snapshot = snapshot_args(owner.clone(), &missing, &dsn, &store);
        refused_with(run(&snapshot, now(), &mut Vec::new()), code);
        let model = ModelArgs {
            model_id: "quant.ma_crossover".into(),
            model_version: "1.0.0".into(),
            owner,
            target: snapshot.target.clone(),
        };
        refused_with(run_model(&model, now(), &mut Vec::new()), code);
        assert!(!store.exists(), "{code}: the store is not created");
    }
    let owner = owner_args("ws1", "u1");
    let args = snapshot_args(owner.clone(), &missing, &dsn, &store);
    refused_with(run(&args, now(), &mut Vec::new()), "config_file_unreadable");
    assert!(!store.exists(), "a missing file: the store is not created");
    let nyse = dir.join("nyse.json");
    std::fs::write(&nyse, SPY.replace("arca", "nyse")).unwrap();
    let args = snapshot_args(owner, &nyse, &dsn, &store);
    refused_with(
        run(&args, now(), &mut Vec::new()),
        "instrument_snapshot_invalid",
    );
}

#[test]
#[ignore = "pending E10-16"]
fn a_bad_owner_a_missing_file_or_a_bad_snapshot_is_refused_without_a_database() {
    refusals_without_a_database();
}

/// A DSN acting as the application role in `db`'s schema.
fn dsn(db: &TestDb) -> String {
    let url = std::env::var(URL_VAR).unwrap();
    let joiner = if url.contains('?') { '&' } else { '?' };
    let options = format!("-c%20search_path%3D{}%20-c%20role%3D{APP_ROLE}", db.schema);
    format!("{url}{joiner}options={options}")
}

/// The binary registers SPY's snapshot and the model on `ctl:ws1` in Postgres, as `u1` in paper,
/// with both objects in the store; a re-run of `model register` prints the event it found.
#[test]
#[ignore = "pending E10-16"]
fn the_binary_registers_the_snapshot_and_the_model_in_postgres() {
    refusals_without_a_database();
    let Some(db) = TestDb::new() else {
        return;
    };
    let (dsn, root) = (dsn(&db), scratch("binary"));
    std::fs::create_dir_all(&root).unwrap();
    let (file, store) = (root.join("spy.json"), root.join("store"));
    std::fs::write(&file, SPY).unwrap();
    let ctl = StreamId::parse("ctl:ws1").unwrap();
    let target = JournalArgs {
        journal: dsn.as_str().into(),
        store: store.clone(),
    };
    let mut opener = PgControlJournal::open(&target).unwrap();
    let epoch = opener.take_ownership(&ctl).unwrap();
    let opened = opener.append(&ctl, 0, epoch, now().at, &[OPENING.as_bytes()]);
    assert!(
        matches!(opened, Ok(AppendOutcome::Committed(_))),
        "{opened:?}"
    );
    let mandate = |argv: &[&str]| {
        let owner = ["--workspace", "ws1", "--user", "u1", "--journal", &dsn];
        let run = Process::new(env!("CARGO_BIN_EXE_mandate"))
            .args(argv)
            .args(owner)
            .args(["--store", store.to_str().unwrap()])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&run.stderr);
        assert!(run.status.success(), "{argv:?}: {stderr}");
        String::from_utf8_lossy(&run.stdout).into_owned()
    };
    let kind = ["config", "register", "--kind", "instrument_snapshot"];
    let snapshot = mandate(&[&kind[..], &[file.to_str().unwrap()]].concat());
    let model = ["model", "register", "quant.ma_crossover", "1.0.0"];
    let (first, again) = (mandate(&model), mandate(&model));
    let rows = read_stream(&target.journal, &ctl).unwrap();
    let [_, registered, model_row] = rows.as_slice() else {
        panic!("the opening and two registrations; the re-run found the model's: {rows:?}")
    };
    assert!(snapshot.contains(&registered.event_id), "{snapshot}");
    let id = &model_row.event_id;
    assert!(first.contains(id) && again.contains(id), "{first}{again}");
    let content = mandate_modelhost::content("quant.ma_crossover", "1.0.0").unwrap();
    let spy = to_canonical(&parse(SPY.as_bytes()).unwrap());
    let files = FsArtifactStore::open(&store).unwrap();
    for (row, kind, object) in [
        (registered, "instrument_snapshot", &spy),
        (model_row, "model_version", &content.canonical),
    ] {
        let body = parse(&row.body).unwrap();
        let text = |outer: &str, inner: &str| {
            let member = body.get(outer).and_then(|v| v.get(inner));
            member.and_then(Value::as_str).map(str::to_owned)
        };
        let paper = body.get("environment").and_then(Value::as_str);
        assert_eq!(paper, Some("paper"), "{kind}");
        assert_eq!(text("actor", "id").as_deref(), Some("u1"), "{kind}");
        assert_eq!(text("payload", "kind").as_deref(), Some(kind));
        let stored = files.read_artifact(&ArtifactRef::of(object));
        assert_eq!(stored.as_ref(), Ok(object), "{kind}");
    }
}
