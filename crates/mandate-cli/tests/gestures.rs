//! D2c (E10-16, DEC-530 item 1): `mandate version create`, `mandate version confirm` and `mandate
//! agent deploy` over DEC-527's owner flags, always in paper. A gesture given no `--code` shows its
//! code and warnings and writes nothing. The binary's refusals need no database: each exits
//! non-zero with its code on stderr and nothing on stdout, and no output names any part of the DSN
//! (`AGENTS.md` rule 7).

use std::path::PathBuf;
use std::process::Command as Process;

use clap::Parser;
use clap::error::ErrorKind;
use mandate_canon::{Digest, parse, to_canonical};
use mandate_cli::gestures::{AgentCommand, VersionCommand};
use mandate_cli::{Cli, Command};

const AGENT: &str = "agent-a";
const MANDATE: &str = include_str!("fixtures/spy_mandate.json");
const SENTINEL_DSN: &str = "postgres://d2c-user-sentinel:d2c-password-sentinel@d2c-host-sentinel.invalid:1/d2c-db-sentinel";

fn reference(bytes: &[u8]) -> String {
    format!("sha256:{}", Digest::of(bytes).to_hex())
}

fn canonical(text: &str) -> Vec<u8> {
    to_canonical(&parse(text.as_bytes()).unwrap())
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
#[ignore = "pending E10-16"]
fn the_binary_refuses_a_bad_owner_a_missing_file_and_no_database() {
    refusals_without_a_database("refusals");
}
