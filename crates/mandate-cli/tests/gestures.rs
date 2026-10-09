//! D2c (E10-16, DEC-530 item 1): `mandate version create`, `mandate version confirm` and `mandate
//! agent deploy` over DEC-527's owner flags, always in paper. A gesture given no `--code` shows its
//! code and warnings and writes nothing. The binary's refusals need no database: each exits
//! non-zero with its code on stderr and nothing on stdout, and no output names any part of the DSN
//! (`AGENTS.md` rule 7).

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
use std::path::PathBuf;
use std::process::Command as Process;

use clap::Parser;
use clap::error::ErrorKind;
use double::{CONTROL, FixedIds, Journal, at, owner, stream};
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_cli::control::{ControlJournal, Now};
use mandate_cli::deploy::deploy;
use mandate_cli::gestures::{AgentCommand, Shown, VersionCommand, confirmation, deployment};
use mandate_cli::postgres::{JournalArgs, read_stream};
use mandate_cli::version;
use mandate_cli::{Cli, Command};
use mandate_journal::StreamId;
use mandate_journal::{AppendOutcome, ArtifactRef, ArtifactSource, ArtifactStore, StoredEvent};
use mandate_journal_pg::APP_ROLE;
use support::{TestDb, URL_VAR};

type Store = BTreeMap<Digest, Vec<u8>>;

const NOW: i64 = 1_790_000_000;
const AGENT: &str = "agent-a";
const MANDATE: &str = include_str!("fixtures/spy_mandate.json");
const SPY: &str = r#"{"asset_class":"us_equity","etp":"plain","etp_classified_at":"2026-10-05T13:30:00Z","etp_source":"nasdaq_trader_symbol_directory","exchange":"arca","increment":"whole","instrument_id":"b0b6dd9d-8b9b-48a9-ba46-b9d54906e415","symbol":"SPY"}"#;
/// The fixture's model hash, E7-7's, which the binary test swaps for the host's.
const FIXTURE_HASH: &str = "4f3559229f89b27c04b43ec773b0ff0b884895362622964b78dfd791d67e18fc";
const MODEL: &str = r#"{"admits_instruments":false,"content_hash":"sha256:4f3559229f89b27c04b43ec773b0ff0b884895362622964b78dfd791d67e18fc","kind":"model_version","model_id":"quant.ma_crossover","model_version":"1.0.0","params":["fast_periods","slow_periods"]}"#;
const SNAPSHOT: &str = r#"{"admits_instruments":null,"content_hash":"@H","kind":"instrument_snapshot","model_id":null,"model_version":null,"params":[]}"#;
const SEEDED: &str = r#"{"actor":{"build":null,"id":"control_services","kind":"system","version":"0.1.0"},"artifact_refs":[],"causation_id":null,"clock_source":"local","config_refs":{},"correlation_id":null,"envelope_version":1,"environment":"paper","event_id":"9@I","event_time":"2026-10-08T13:00:00.000000000Z","event_type":"ConfigSnapshotRegistered","payload":@P,"pii_refs":[],"schema_version":1,"stream_id":"ctl:ws1"}"#;
const SENTINEL_DSN: &str = "postgres://d2c-user-sentinel:d2c-password-sentinel@d2c-host-sentinel.invalid:1/d2c-db-sentinel";

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

/// Showing a gesture's code runs its checks but the code's and writes nothing: no append, no
/// stored object, no assertion. The codes are DEC-530 item 4's, and the warnings mandate spec
/// §4.2's: W-002, as 1000 x 0.05 at the stop is over 0.02 x 1000 a day. A shown deployment checks
/// what its gesture does, the V-rules among them.
#[test]
fn a_shown_code_is_the_gestures_and_showing_writes_nothing() {
    let (mut journal, mut store) = (Journal::default(), Store::new());
    store.put_artifact(SPY.as_bytes()).unwrap();
    let snapshot = SNAPSHOT.replace("@H", &reference(&canonical(SPY)));
    seed(&mut journal, "ConfigSnapshotRegistered", MODEL);
    seed(&mut journal, "ConfigSnapshotRegistered", &snapshot);
    let (mut ids, owner) = (FixedIds::default(), owner());
    let version = reference(&canonical(MANDATE));
    version::create(&mut journal, &mut store, &owner, MANDATE.as_bytes(), now()).unwrap();
    let written = |journal: &Journal, store: &Store, ids: &FixedIds| {
        (journal.attempts.len(), store.clone(), ids.assertions)
    };
    let before = written(&journal, &store, &ids);
    let shown = confirmation(&journal, &store, &owner, &version, now());
    let code = confirm_code(&version);
    let warnings = vec!["W-002"];
    assert_eq!(shown, Ok(Shown { code, warnings }), "the confirmation's");
    let refused = deployment(&journal, &store, &owner, (AGENT, &version), now());
    let reason = refused.map_err(|e| e.to_string());
    let unconfirmed = matches!(&reason, Err(why) if why.contains("version_unconfirmed"));
    assert!(unconfirmed, "{reason:?}");
    assert_eq!(written(&journal, &store, &ids), before, "nothing written");
    let code = confirm_code(&version);
    let stream = (&mut journal, &mut store);
    version::confirm(stream.0, stream.1, &mut ids, &owner, &version, &code, now()).unwrap();
    let before = written(&journal, &store, &ids);
    let shown = deployment(&journal, &store, &owner, (AGENT, &version), now());
    let (code, warnings) = (deploy_code(AGENT, &version), vec!["W-002"]);
    assert_eq!(shown, Ok(Shown { code, warnings }), "the deployment's");
    assert_eq!(written(&journal, &store, &ids), before, "nothing written");
    let revoked = r#"{"connection_id":"conn_alpaca_paper_01"}"#;
    seed(&mut journal, "ConnectionRevoked", revoked);
    let refused = deployment(&journal, &store, &owner, (AGENT, &version), now());
    let reason = refused.map_err(|e| e.to_string());
    let invalid = matches!(&reason, Err(why) if why.contains("version_invalid"));
    assert!(invalid, "the V-rules are checked: V-001, {reason:?}");
}

/// DEC-530 item 9: a re-run while the agent's active deployment is that version shows that
/// deployment's code, with no warnings, whatever the rules now say, and writes nothing; while
/// another version is active, the agent is refused `agent_active`, even for a confirmed version.
#[test]
fn a_deployed_agent_shows_its_version_and_refuses_another() {
    let (mut journal, mut store) = (Journal::default(), Store::new());
    store.put_artifact(SPY.as_bytes()).unwrap();
    let snapshot = SNAPSHOT.replace("@H", &reference(&canonical(SPY)));
    seed(&mut journal, "ConfigSnapshotRegistered", MODEL);
    seed(&mut journal, "ConfigSnapshotRegistered", &snapshot);
    let (mut ids, owner) = (FixedIds::default(), owner());
    let other = SPY
        .replace(
            "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415",
            "a1a1a1a1-8b9b-48a9-ba46-b9d54906e416",
        )
        .replace("SPY", "QQQ");
    store.put_artifact(other.as_bytes()).unwrap();
    seed(
        &mut journal,
        "ConfigSnapshotRegistered",
        &SNAPSHOT.replace("@H", &reference(&canonical(&other))),
    );
    let second = MANDATE
        .replace(
            "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415",
            "a1a1a1a1-8b9b-48a9-ba46-b9d54906e416",
        )
        .replace("SPY", "QQQ");
    let versions = [
        reference(&canonical(MANDATE)),
        reference(&canonical(&second)),
    ];
    let stream = (&mut journal, &mut store);
    version::create(stream.0, stream.1, &owner, MANDATE.as_bytes(), now()).unwrap();
    let stream = (&mut journal, &mut store);
    let code = confirm_code(&versions[0]);
    version::confirm(
        stream.0,
        stream.1,
        &mut ids,
        &owner,
        &versions[0],
        &code,
        now(),
    )
    .unwrap();
    let code = deploy_code(AGENT, &versions[0]);
    let stream = (&mut journal, &mut store);
    let named = (AGENT, versions[0].as_str());
    deploy(
        stream.0,
        stream.1,
        &mut ids,
        &owner,
        named.0,
        named.1,
        &code,
        now(),
    )
    .unwrap();
    let written = |journal: &Journal, store: &Store, ids: &FixedIds| {
        (journal.attempts.len(), store.clone(), ids.assertions)
    };
    let before = written(&journal, &store, &ids);
    let shown = deployment(&journal, &store, &owner, named, now());
    let (code, warnings) = (code, Vec::new());
    assert_eq!(shown, Ok(Shown { code, warnings }), "the active version");
    assert_eq!(written(&journal, &store, &ids), before, "nothing written");
    let stream = (&mut journal, &mut store);
    version::create(stream.0, stream.1, &owner, second.as_bytes(), now()).unwrap();
    let stream = (&mut journal, &mut store);
    let code = confirm_code(&versions[1]);
    version::confirm(
        stream.0,
        stream.1,
        &mut ids,
        &owner,
        &versions[1],
        &code,
        now(),
    )
    .unwrap();
    let before = written(&journal, &store, &ids);
    let other = deployment(&journal, &store, &owner, (AGENT, &versions[1]), now());
    let reason = other.map_err(|e| e.to_string());
    let active = matches!(&reason, Err(why) if why.contains("agent_active"));
    assert!(active, "another version while one is active: {reason:?}");
    assert_eq!(written(&journal, &store, &ids), before, "nothing written");
}

/// Warnings are shown sorted by code (DEC-530 item 4), for a confirmation and for a deployment: a
/// mandate with a rule after its catch-all warns W-005 as well as W-002 (mandate spec §4.2).
#[test]
fn warnings_are_shown_sorted_by_code() {
    let (mut journal, mut store) = (Journal::default(), Store::new());
    store.put_artifact(SPY.as_bytes()).unwrap();
    let snapshot = SNAPSHOT.replace("@H", &reference(&canonical(SPY)));
    seed(&mut journal, "ConfigSnapshotRegistered", MODEL);
    seed(&mut journal, "ConfigSnapshotRegistered", &snapshot);
    let (mut ids, owner) = (FixedIds::default(), owner());
    let late =
        r#"{"id":"late","then":"deny","when":{"field":"order_usd","op":"gt","value":"100"}}"#;
    let after = MANDATE.replace(
        r#"}}]},"behavior""#,
        &format!(r#"}}}},{late}]}},"behavior""#),
    );
    assert_ne!(after, MANDATE, "the late rule is added");
    let version = reference(&canonical(&after));
    let stream = (&mut journal, &mut store);
    version::create(stream.0, stream.1, &owner, after.as_bytes(), now()).unwrap();
    let warnings = vec!["W-002", "W-005"];
    let code = confirm_code(&version);
    let shown = confirmation(&journal, &store, &owner, &version, now());
    assert_eq!(
        shown,
        Ok(Shown {
            code,
            warnings: warnings.clone()
        })
    );
    let stream = (&mut journal, &mut store);
    let code = confirm_code(&version);
    version::confirm(stream.0, stream.1, &mut ids, &owner, &version, &code, now()).unwrap();
    let code = deploy_code(AGENT, &version);
    let shown = deployment(&journal, &store, &owner, (AGENT, &version), now());
    assert_eq!(shown, Ok(Shown { code, warnings }));
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
                let named = (args.agent.as_str(), args.version.as_str());
                assert_eq!(named, ("agent-a", "sha256:aa"));
                args.code
            }
            other => panic!("{line}: {other:?}"),
        };
        assert_eq!(code.is_some(), line.contains("--code"), "{line}");
        for flag in ["--workspace", "--user", "--journal", "--store"] {
            let at = words.iter().position(|w| *w == flag).unwrap();
            let without: Vec<&str> = [&words[..at], &words[at + 2..]].concat();
            let kind = Cli::try_parse_from(without).err().map(|e| e.kind());
            let missing = Some(ErrorKind::MissingRequiredArgument);
            assert_eq!(kind, missing, "{line} {flag}");
        }
        let live = [&words[..], &["--environment", "live"]].concat();
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

/// The parts of `dsn` no output may name: the scheme, and its user, password, host and database.
fn dsn_parts(dsn: &str) -> Vec<String> {
    let (_, rest) = dsn.split_once("://").unwrap();
    let (credentials, place) = rest.split_once('@').unwrap_or(("", rest));
    let (host, database) = place.split_once('/').unwrap_or((place, ""));
    let host = host.rsplit_once(':').map_or(host, |(name, _)| name);
    let database = database.split('?').next().unwrap();
    let (user, password) = credentials.split_once(':').unwrap_or((credentials, ""));
    let parts = ["postgres://", user, password, host, database];
    parts
        .into_iter()
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Runs the binary with `argv`. A backtrace is off, so the output is what the owner would see.
fn run(argv: &[&str]) -> (bool, String, String) {
    let ran = Process::new(env!("CARGO_BIN_EXE_mandate"))
        .args(argv)
        .env_remove("RUST_BACKTRACE")
        .env_remove("RUST_LIB_BACKTRACE")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&ran.stdout).into_owned();
    (
        ran.status.success(),
        stdout,
        String::from_utf8_lossy(&ran.stderr).into_owned(),
    )
}

/// Asserts that neither output names any of `hidden`.
fn names_none(argv: &[&str], hidden: &[String], stdout: &str, stderr: &str) {
    for part in hidden {
        let named = stdout.contains(part.as_str()) || stderr.contains(part.as_str());
        assert!(!named, "{argv:?} names {part}: {stdout}{stderr}");
    }
}

/// Runs the binary, which must exit non-zero with `code` on stderr, print nothing on stdout, and
/// name none of `hidden` (DEC-527 items 4 and 5).
fn refused(argv: &[&str], code: &str, hidden: &[String]) {
    let (succeeded, stdout, stderr) = run(argv);
    let refused = !succeeded && stdout.is_empty() && stderr.contains(code);
    assert!(
        refused,
        "{argv:?} {code}: exit ok {succeeded}, {stdout}{stderr}"
    );
    names_none(argv, hidden, &stdout, &stderr);
}

/// With no database, through the binary: a bad owner id is refused before the file is read, the
/// journal opened or the store created; an unreadable mandate file before the journal is opened;
/// and a journal whose host does not resolve is `unavailable` for every command, with and without
/// `--code`. `tag` keeps each caller's files apart.
fn refusals_without_a_database(tag: &str) {
    let hidden = dsn_parts(SENTINEL_DSN);
    assert_eq!(
        hidden.len(),
        5,
        "the scheme, user, password, host and database: {hidden:?}"
    );
    let (dir, store) = (
        scratch(&format!("{tag}-files")),
        scratch(&format!("{tag}-store")),
    );
    std::fs::create_dir_all(&dir).unwrap();
    let (missing, file) = (dir.join("missing.json"), dir.join("mandate.json"));
    std::fs::write(&file, MANDATE).unwrap();
    let version = reference(&canonical(MANDATE));
    let (file, missing) = (file.to_str().unwrap(), missing.to_str().unwrap());
    let target = [
        "--journal",
        SENTINEL_DSN,
        "--store",
        store.to_str().unwrap(),
    ];
    let commands: [&[&str]; 5] = [
        &["version", "create", file],
        &["version", "confirm", &version],
        &["version", "confirm", &version, "--code", "0123abcd"],
        &["agent", "deploy", AGENT, &version],
        &["agent", "deploy", AGENT, &version, "--code", "0123abcd"],
    ];
    let as_owner = |workspace, user| ["--workspace", workspace, "--user", user];
    let owners = [
        (as_owner("ws:1", "u1"), "owner_workspace_invalid"),
        (as_owner("ws1", "Founder Name"), "owner_user_invalid"),
    ];
    for (owner, code) in owners {
        for command in commands {
            refused(&[command, &owner, &target].concat(), code, &hidden);
        }
        assert!(!store.exists(), "{code}: the store is not created");
    }
    let owner = as_owner("ws1", "u1");
    let unreadable = [&["version", "create", missing], &owner[..], &target].concat();
    refused(&unreadable, "config_file_unreadable", &hidden);
    assert!(!store.exists(), "a missing file: the store is not created");
    for command in commands {
        refused(&[command, &owner, &target].concat(), "unavailable", &hidden);
    }
    for made in [dir, store] {
        std::fs::remove_dir_all(made).ok();
    }
}

#[test]
fn the_binary_refuses_a_bad_owner_a_missing_file_and_no_database() {
    refusals_without_a_database("refusals");
}

/// A DSN acting as the application role in `db`'s schema.
fn dsn(db: &TestDb) -> String {
    let url = std::env::var(URL_VAR).unwrap();
    let joiner = if url.contains('?') { '&' } else { '?' };
    let options = format!("-c%20search_path%3D{}%20-c%20role%3D{APP_ROLE}", db.schema);
    format!("{url}{joiner}options={options}")
}

/// The binary opens `ctl:ws1`, registers SPY and the host's model, then creates the SPY mandate's
/// version, shows and types its confirmation code, and shows and types the deployment code. A
/// shown code commits nothing; each typed one commits one event with an assertion of its own, and
/// prints its event id and `seq`. Deploying before the confirmation, confirming an unknown
/// version and a wrong code each exit non-zero with their code and commit nothing. No output,
/// stdout or stderr, names any part of the DSN.
#[test]
fn the_binary_creates_confirms_and_deploys_in_postgres() {
    refusals_without_a_database("binary");
    let Some(db) = TestDb::new() else {
        return;
    };
    let (dsn, root) = (dsn(&db), scratch("binary"));
    let hidden = dsn_parts(&std::env::var(URL_VAR).unwrap());
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
    let target = ["--journal", &dsn, "--store", store.to_str().unwrap()];
    let owned = ["--workspace", "ws1", "--user", "u1"];
    let refuse = |command: &[&str], code: &str| {
        refused(&[command, &owned, &target].concat(), code, &hidden);
    };
    let ok = |command: &[&str], owner: &[&str]| {
        let argv = [command, owner, &target].concat();
        let (succeeded, stdout, stderr) = run(&argv);
        assert!(succeeded && stderr.is_empty(), "{argv:?}: {stderr}");
        names_none(&argv, &hidden, &stdout, &stderr);
        stdout
    };
    let args = JournalArgs {
        journal: dsn.as_str().into(),
        store: store.clone(),
    };
    let journal = args.journal;
    let ctl = StreamId::parse(CONTROL).unwrap();
    let count = || read_stream(&journal, &ctl).unwrap().len();
    ok(&["workspace", "open"], &owned[..2]);
    let spy = spy.to_str().unwrap();
    ok(
        &["config", "register", "--kind", "instrument_snapshot", spy],
        &owned,
    );
    ok(
        &["model", "register", "quant.ma_crossover", "1.0.0"],
        &owned,
    );
    let created = ok(&["version", "create", file.to_str().unwrap()], &owned);
    assert!(created.contains(&version), "{created}");
    let before = count();
    let (code, deploying) = (confirm_code(&version), deploy_code(AGENT, &version));
    let unknown = reference(b"no such mandate");
    for given in [vec![], vec!["--code", &deploying]] {
        let deploy = [&["agent", "deploy", AGENT, &version][..], &given].concat();
        refuse(&deploy, "version_unconfirmed");
        let confirm = [&["version", "confirm", &unknown][..], &given].concat();
        refuse(&confirm, "version_unknown");
    }
    let wrong = ["--code", "00000000"];
    let confirm = [&["version", "confirm", &version][..], &wrong].concat();
    refuse(&confirm, "code_mismatch");
    assert_eq!(count(), before, "a refusal commits nothing");
    let shown = ok(&["version", "confirm", &version], &owned);
    assert_eq!(
        shown,
        format!("code {code}\nwarnings W-002\n"),
        "the confirmation shown"
    );
    assert_eq!(count(), before, "showing commits nothing");
    let confirmed = ok(&["version", "confirm", &version, "--code", &code], &owned);
    let shown = ok(&["agent", "deploy", AGENT, &version], &owned);
    assert_eq!(
        shown,
        format!("code {deploying}\nwarnings W-002\n"),
        "the deployment shown"
    );
    let deploy = [&["agent", "deploy", AGENT, &version][..], &wrong].concat();
    refuse(&deploy, "code_mismatch");
    assert_eq!(
        count(),
        before + 1,
        "showing or a wrong code commits nothing"
    );
    let deployed = ok(
        &["agent", "deploy", AGENT, &version, "--code", &deploying],
        &owned,
    );
    let reshown = ok(&["agent", "deploy", AGENT, &version], &owned);
    assert_eq!(
        reshown,
        format!("code {deploying}\nwarnings none\n"),
        "a deployed agent's deployment shown again"
    );
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
