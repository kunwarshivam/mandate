//! D2c (E10-16, DEC-530 item 1): `mandate version create`, `mandate version confirm` and `mandate
//! agent deploy` over DEC-527's owner flags, always in paper. A gesture given no `--code` shows its
//! code and warnings and writes nothing. The refusals need no database, and the Postgres binary test
//! starts with them, so the pending gate sees the stub without one (DEC-510 item 3). Nothing any
//! command prints or refuses with names any part of the DSN (`AGENTS.md` rule 7).

#[path = "../../mandate-journal/tests/common/mod.rs"]
mod common;
#[path = "../../mandate-journal/tests/conformance/mod.rs"]
#[allow(
    unused_imports,
    unused_macros,
    reason = "only `support`'s backend trait is used here; the suite runs in `mandate-journal-pg`"
)]
mod conformance;
#[path = "common/mod.rs"]
mod double;
#[path = "../../mandate-journal-pg/tests/support/mod.rs"]
mod support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command as Process;

use clap::Parser;
use clap::error::ErrorKind;
use double::{AGENT, CONTROL, FixedIds, Journal, at, owner, stream};
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_cli::control::{ControlJournal, Now};
use mandate_cli::gestures::{
    AgentCommand, ConfirmArgs, CreateArgs, DeployArgs, Shown, VersionCommand, confirm, create,
    deploy, deployment,
};
use mandate_cli::postgres::{JournalArgs, read_stream};
use mandate_cli::register::OwnerArgs;
use mandate_cli::version;
use mandate_cli::{Cli, Command};
use mandate_journal::{
    AppendOutcome, ArtifactRef, ArtifactSource, ArtifactStore, StoredEvent, StreamId,
};
use mandate_journal_pg::APP_ROLE;
use support::{TestDb, URL_VAR};

type Store = BTreeMap<Digest, Vec<u8>>;

const NOW: i64 = 1_790_000_000;
const MANDATE: &str = include_str!("fixtures/spy_mandate.json");
const SPY: &str = r#"{"asset_class":"us_equity","etp":"plain","etp_classified_at":"2026-10-05T13:30:00Z","etp_source":"nasdaq_trader_symbol_directory","exchange":"arca","increment":"whole","instrument_id":"b0b6dd9d-8b9b-48a9-ba46-b9d54906e415","symbol":"SPY"}"#;
/// The fixture's model hash, E7-7's, which the binary test swaps for the host's.
const FIXTURE_HASH: &str = "4f3559229f89b27c04b43ec773b0ff0b884895362622964b78dfd791d67e18fc";
const MODEL: &str = r#"{"admits_instruments":false,"content_hash":"sha256:4f3559229f89b27c04b43ec773b0ff0b884895362622964b78dfd791d67e18fc","kind":"model_version","model_id":"quant.ma_crossover","model_version":"1.0.0","params":["fast_periods","slow_periods"]}"#;
const SNAPSHOT: &str = r#"{"admits_instruments":null,"content_hash":"@H","kind":"instrument_snapshot","model_id":null,"model_version":null,"params":[]}"#;
const SEEDED: &str = r#"{"actor":{"build":null,"id":"control_services","kind":"system","version":"0.1.0"},"artifact_refs":[],"causation_id":null,"clock_source":"local","config_refs":{},"correlation_id":null,"envelope_version":1,"environment":"paper","event_id":"9@I","event_time":"2026-10-08T13:00:00.000000000Z","event_type":"ConfigSnapshotRegistered","payload":@P,"pii_refs":[],"schema_version":1,"stream_id":"ctl:ws1"}"#;
const SENTINELS: [&str; 4] = [
    "d2c-user-sentinel",
    "d2c-password-sentinel",
    "d2c-host-sentinel.invalid",
    "d2c-db-sentinel",
];

fn now() -> Now {
    at(NOW)
}

fn reference(bytes: &[u8]) -> String {
    format!("sha256:{}", Digest::of(bytes).to_hex())
}

fn canonical(text: &str) -> Vec<u8> {
    to_canonical(&parse(text.as_bytes()).unwrap())
}

/// DEC-530 item 4's codes, computed here.
fn code(gesture: &str) -> String {
    Digest::of(&canonical(gesture)).to_hex()[..8].to_owned()
}

fn confirm_code(version: &str) -> String {
    code(&format!(
        r#"{{"gesture":"mandate_confirm","mandate_version":"{version}"}}"#
    ))
}

fn deploy_code(agent: &str, version: &str) -> String {
    let gesture = r#""gesture":"agent_deploy""#;
    code(&format!(
        r#"{{"agent_id":"{agent}",{gesture},"mandate_version":"{version}"}}"#
    ))
}

/// Appends one event as another control-stream writer would.
fn seed(journal: &mut Journal, event_type: &str, payload: &str) {
    let control = stream(CONTROL);
    let head = journal.head(&control).unwrap().seq;
    let draft = SEEDED
        .replace("@I", &format!("{head:025}"))
        .replace("@P", payload);
    let draft = draft.replace("ConfigSnapshotRegistered", event_type);
    let epoch = journal.take_ownership(&control).unwrap();
    let outcome = journal.append(&control, head, epoch, now().at, &[draft.as_bytes()]);
    assert!(matches!(outcome, Ok(AppendOutcome::Committed(_))));
}

/// A workspace whose control stream registers the fixture's model and SPY's snapshot.
fn registered() -> (Journal, Store) {
    let (mut journal, mut store) = (Journal::default(), Store::new());
    store.put_artifact(SPY.as_bytes()).unwrap();
    let snapshot = SNAPSHOT.replace("@H", &reference(&canonical(SPY)));
    seed(&mut journal, "ConfigSnapshotRegistered", MODEL);
    seed(&mut journal, "ConfigSnapshotRegistered", &snapshot);
    (journal, store)
}

/// Showing a gesture's code runs its checks but the code's and writes nothing: no append, no
/// stored object, no assertion. The codes are DEC-530 item 4's, and the warnings mandate spec
/// §4.2's: W-002, as 1000 x 0.05 at the stop is over 0.02 x 1000 a day. A shown deployment checks
/// the V-rules as its gesture does.
#[test]
fn a_shown_code_is_the_gestures_and_showing_writes_nothing() {
    let (mut journal, mut store) = registered();
    let mut ids = FixedIds::default();
    let version = reference(&canonical(MANDATE));
    version::create(
        &mut journal,
        &mut store,
        &owner(),
        MANDATE.as_bytes(),
        now(),
    )
    .unwrap();
    let before = (journal.attempts.len(), store.clone());
    let shown = mandate_cli::gestures::confirmation(&journal, &store, &owner(), &version, now());
    let warnings = vec!["W-002"];
    let expected = Shown {
        code: confirm_code(&version),
        warnings,
    };
    assert_eq!(shown, Ok(expected), "the confirmation's code");
    let refused = deployment(&journal, &store, &owner(), (AGENT, &version), now());
    let reason = refused.map_err(|e| e.to_string());
    assert!(
        matches!(&reason, Err(why) if why.contains("version_unconfirmed")),
        "{reason:?}"
    );
    assert_eq!(
        (journal.attempts.len(), &store),
        (before.0, &before.1),
        "nothing written"
    );
    let code = confirm_code(&version);
    version::confirm(
        &mut journal,
        &mut store,
        &mut ids,
        &owner(),
        &version,
        &code,
        now(),
    )
    .unwrap();
    let before = (journal.attempts.len(), store.clone(), ids.assertions);
    let shown = deployment(&journal, &store, &owner(), (AGENT, &version), now());
    let expected = Shown {
        code: deploy_code(AGENT, &version),
        warnings: vec!["W-002"],
    };
    assert_eq!(shown, Ok(expected), "the deployment's code");
    let after = (journal.attempts.len(), store.clone(), ids.assertions);
    assert_eq!(after, before, "nothing written");
    seed(
        &mut journal,
        "ConnectionRevoked",
        r#"{"connection_id":"conn_alpaca_paper_01"}"#,
    );
    let refused = deployment(&journal, &store, &owner(), (AGENT, &version), now());
    let reason = refused.map_err(|e| e.to_string());
    let invalid = matches!(&reason, Err(why) if why.contains("version_invalid"));
    assert!(invalid, "the V-rules are checked: V-001, {reason:?}");
}

/// The three commands parse with DEC-527's required owner flags and P0's target, `--code` is
/// optional, and no `--environment` exists.
#[test]
fn the_commands_take_the_owner_flags_and_no_environment() {
    let flags = "--workspace ws1 --user u1 --journal postgres://h/j --store s";
    let lines = [
        "version create mandate.json",
        "version confirm sha256:aa",
        "version confirm sha256:aa --code 0123abcd",
        "agent deploy agent-a sha256:aa",
        "agent deploy agent-a sha256:aa --code 0123abcd",
    ];
    for line in lines {
        let full = format!("mandate {line} {flags}");
        let words: Vec<&str> = full.split_whitespace().collect();
        let parsed = Cli::try_parse_from(&words).map(|cli| cli.command);
        let code = match parsed {
            Ok(Command::Version(VersionCommand::Create(args))) => {
                assert_eq!(args.file, PathBuf::from("mandate.json"));
                None
            }
            Ok(Command::Version(VersionCommand::Confirm(args))) => {
                assert_eq!(args.version, "sha256:aa");
                args.code
            }
            Ok(Command::Agent(AgentCommand::Deploy(args))) => {
                assert_eq!(
                    (args.agent.as_str(), args.version.as_str()),
                    ("agent-a", "sha256:aa")
                );
                args.code
            }
            other => panic!("{line}: {other:?}"),
        };
        assert_eq!(code.is_some(), line.contains("--code"), "{line}");
        for flag in ["--workspace", "--user", "--journal", "--store"] {
            let at = words.iter().position(|w| *w == flag).unwrap();
            let without: Vec<&str> = [&words[..at], &words[at + 2..]].concat();
            let kind = Cli::try_parse_from(without).err().map(|e| e.kind());
            assert_eq!(
                kind,
                Some(ErrorKind::MissingRequiredArgument),
                "{line} {flag}"
            );
        }
        let live: Vec<&str> = words
            .iter()
            .copied()
            .chain(["--environment", "live"])
            .collect();
        let kind = Cli::try_parse_from(live).err().map(|e| e.kind());
        assert_eq!(kind, Some(ErrorKind::UnknownArgument), "{line}");
    }
}

/// A fresh, absent directory for one test.
fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("mandate-cli-d2c-{}-{name}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    root
}

fn target(journal: &str, store: &Path) -> JournalArgs {
    JournalArgs {
        journal: journal.into(),
        store: store.to_owned(),
    }
}

fn owner_args(workspace: &str, user: &str) -> OwnerArgs {
    let (workspace, user) = (workspace.to_owned(), user.to_owned());
    OwnerArgs { workspace, user }
}

/// Runs `command`, which must refuse with a message carrying `code` and no part of the DSN, and
/// print nothing (DEC-527 items 4 and 5).
fn refuses<T: std::fmt::Debug>(
    code: &str,
    command: impl FnOnce(&mut Vec<u8>) -> anyhow::Result<T>,
) {
    let mut out = Vec::new();
    let why = command(&mut out).map(|t| format!("not refused: {t:?}"));
    let why = why.unwrap_or_else(|e| format!("{e:#}"));
    assert!(why.contains(code), "{code}: {why}");
    let named: Vec<&str> = SENTINELS.into_iter().filter(|s| why.contains(s)).collect();
    assert!(named.is_empty(), "{code} names {named:?}: {why}");
    let printed = String::from_utf8_lossy(&out);
    assert!(out.is_empty(), "{code}: printed {printed}");
}

/// With no database: a bad owner id is refused before the file is read, the journal opened or the
/// store created; an unreadable mandate file before the journal is opened; and a journal whose host
/// does not resolve is an error at once. `tag` keeps each caller's files apart.
fn refusals_without_a_database(tag: &str) {
    let [user, password, host, db] = SENTINELS;
    let dsn = format!("postgres://{user}:{password}@{host}:1/{db}");
    let (dir, store) = (
        scratch(&format!("{tag}-files")),
        scratch(&format!("{tag}-store")),
    );
    std::fs::create_dir_all(&dir).unwrap();
    let (missing, file) = (dir.join("missing.json"), dir.join("mandate.json"));
    std::fs::write(&file, MANDATE).unwrap();
    let version = reference(&canonical(MANDATE));
    let owners = [
        ("ws:1", "u1", "owner_workspace_invalid"),
        ("ws1", "Founder Name", "owner_user_invalid"),
    ];
    for (workspace, user, code) in owners {
        let owner = owner_args(workspace, user);
        let created = CreateArgs {
            file: file.clone(),
            owner: owner.clone(),
            target: target(&dsn, &store),
        };
        refuses(code, |out| create(&created, now(), out));
        for given in [None, Some("0123abcd".to_owned())] {
            let confirmed = ConfirmArgs {
                version: version.clone(),
                code: given.clone(),
                owner: owner.clone(),
                target: target(&dsn, &store),
            };
            refuses(code, |out| confirm(&confirmed, now(), out));
            let deployed = DeployArgs {
                agent: AGENT.into(),
                version: version.clone(),
                code: given,
                owner: owner.clone(),
                target: target(&dsn, &store),
            };
            refuses(code, |out| deploy(&deployed, now(), out));
        }
        assert!(!store.exists(), "{code}: the store is not created");
    }
    let owner = owner_args("ws1", "u1");
    let unreadable = CreateArgs {
        file: missing,
        owner: owner.clone(),
        target: target(&dsn, &store),
    };
    refuses("config_file_unreadable", |out| {
        create(&unreadable, now(), out)
    });
    assert!(!store.exists(), "a missing file: the store is not created");
    let created = CreateArgs {
        file,
        owner,
        target: target(&dsn, &store),
    };
    refuses("unavailable", |out| create(&created, now(), out));
    for made in [dir, store] {
        std::fs::remove_dir_all(made).ok();
    }
}

#[test]
fn a_bad_owner_or_an_unreadable_file_is_refused_without_a_database() {
    refusals_without_a_database("refusals");
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

/// The binary opens `ctl:ws1`, registers SPY and the host's model, then creates the SPY mandate's
/// version, shows and types its confirmation code, and shows and types the deployment code. A
/// shown code commits nothing; each typed one commits one event with an assertion of its own.
/// Nothing printed names the DSN.
#[test]
fn the_binary_creates_confirms_and_deploys_in_postgres() {
    refusals_without_a_database("binary");
    let Some(db) = TestDb::new() else {
        return;
    };
    let (dsn, root) = (dsn(&db), scratch("binary"));
    std::fs::create_dir_all(&root).unwrap();
    let (spy, file, store) = (
        root.join("spy.json"),
        root.join("mandate.json"),
        root.join("store"),
    );
    let content = mandate_modelhost::content("quant.ma_crossover", "1.0.0").unwrap();
    let document = MANDATE.replace(FIXTURE_HASH, &content.hash.to_hex());
    std::fs::write(&spy, SPY).unwrap();
    std::fs::write(&file, &document).unwrap();
    let version = reference(&canonical(&document));
    let ctl = StreamId::parse(CONTROL).unwrap();
    let store_path = store.to_str().unwrap();
    let mandate = |argv: &[&str], owned: bool| {
        let target = ["--journal", &dsn, "--store", store_path];
        let owner: &[&str] = if owned { &["--user", "u1"] } else { &[] };
        let run = Process::new(env!("CARGO_BIN_EXE_mandate"))
            .args(argv)
            .args(["--workspace", "ws1"])
            .args(owner)
            .args(target)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&run.stderr);
        assert!(
            run.status.success() && stderr.is_empty(),
            "{argv:?}: {stderr}"
        );
        let stdout = String::from_utf8(run.stdout).unwrap();
        let hidden = ["postgres://", "d2c-"].map(str::to_owned);
        for shown in hidden.into_iter().chain(url_password()) {
            assert!(!stdout.contains(&shown), "{argv:?} shows {shown}: {stdout}");
        }
        stdout
    };
    let journal = target(&dsn, &store).journal;
    let count = || read_stream(&journal, &ctl).unwrap().len();
    mandate(&["workspace", "open"], false);
    let spy_path = spy.to_str().unwrap();
    mandate(
        &[
            "config",
            "register",
            "--kind",
            "instrument_snapshot",
            spy_path,
        ],
        true,
    );
    mandate(&["model", "register", "quant.ma_crossover", "1.0.0"], true);
    let created = mandate(&["version", "create", file.to_str().unwrap()], true);
    assert!(created.contains(&version), "{created}");
    let before = count();
    let code = confirm_code(&version);
    let shown = mandate(&["version", "confirm", &version], true);
    assert_eq!(
        shown,
        format!("code {code}\nwarnings W-002\n"),
        "the confirmation shown"
    );
    assert_eq!(count(), before, "showing commits nothing");
    let confirmed = mandate(&["version", "confirm", &version, "--code", &code], true);
    let code = deploy_code(AGENT, &version);
    let shown = mandate(&["agent", "deploy", AGENT, &version], true);
    assert_eq!(
        shown,
        format!("code {code}\nwarnings W-002\n"),
        "the deployment shown"
    );
    assert_eq!(count(), before + 1, "showing commits nothing");
    let deployed = mandate(&["agent", "deploy", AGENT, &version, "--code", &code], true);
    let rows = read_stream(&journal, &ctl).unwrap();
    let types: Vec<&str> = rows.iter().map(|r| r.event_type.as_str()).collect();
    let expected = [
        "StreamOpened",
        "ConfigSnapshotRegistered",
        "ConfigSnapshotRegistered",
        "MandateVersionCreated",
        "MandateConfirmed",
        "AgentDeployed",
    ];
    assert_eq!(types, expected);
    let printed = |done: &str, row: &StoredEvent| {
        format!("{done} as event {} at seq {}\n", row.event_id, row.seq)
    };
    assert_eq!(
        confirmed,
        printed("confirmed", &rows[4]),
        "the confirmation"
    );
    assert_eq!(deployed, printed("deployed", &rows[5]), "the deployment");
    let files = FsArtifactStore::open(&store).unwrap();
    let assertion = |row: &StoredEvent| {
        let body = parse(&row.body).unwrap();
        let record = body.get("payload").and_then(|p| p.get("record_ref"));
        let record = ArtifactRef::parse(record.and_then(Value::as_str).unwrap()).unwrap();
        let record = parse(&files.read_artifact(&record).unwrap()).unwrap();
        let step_up = record.get("step_up").and_then(|s| s.get("assertion_id"));
        step_up.and_then(Value::as_str).unwrap().to_owned()
    };
    let minted = [assertion(&rows[4]), assertion(&rows[5])];
    assert_ne!(minted[0], minted[1], "each gesture mints its own assertion");
    std::fs::remove_dir_all(root).ok();
}
