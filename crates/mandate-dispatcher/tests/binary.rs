//! The dispatcher binary's edges (DEC-2500 items 1 and 7, DEC-2516): any argument but `run` and
//! `probe` exits 2, and so does a run whose configuration is missing, unreadable, or refused, each
//! saying only why, before it reaches the journal or the mail transport.

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_mandate-dispatcher");

fn scratch(name: &str, text: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("mandate-dispatcher-{}-{name}", std::process::id()));
    std::fs::write(&path, text).unwrap();
    path
}

fn ran(args: &[&str]) -> (Option<i32>, String) {
    let output = Command::new(BIN)
        .args(args)
        .env_remove("MANDATE_JOURNAL_DSN")
        .output()
        .unwrap();
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn an_unknown_command_or_a_missing_flag_exits_2() {
    assert_eq!(ran(&["serve", "--probe", "/tmp/x"]).0, Some(2));
    assert_eq!(ran(&["run"]).0, Some(2));
    assert_eq!(ran(&["run", "--probe", "/tmp/x"]).0, Some(2));
    assert_eq!(ran(&["probe", "sideways", "--probe", "/tmp/x"]).0, Some(2));
    assert_eq!(ran(&["probe", "live", "--probe", "/tmp/x"]).0, Some(2));
}

/// The chart's `exec` probes ask `livez` and `readyz` (DEC-994 item 5); with no process serving
/// the socket, each is answered not ready (1), never refused as a usage error (2).
#[test]
fn the_charts_probe_names_are_asked_and_unanswered_is_not_ready() {
    let socket = std::env::temp_dir().join(format!(
        "mandate-dispatcher-{}-none.sock",
        std::process::id()
    ));
    let socket = socket.to_string_lossy().into_owned();
    for name in ["livez", "readyz"] {
        assert_eq!(
            ran(&["probe", name, "--probe", &socket]).0,
            Some(1),
            "{name}"
        );
    }
}

#[test]
fn a_config_that_cannot_be_read_or_honoured_stops_the_start_and_names_why() {
    let (code, said) = ran(&[
        "run",
        "--config",
        "/nonexistent/x.json",
        "--probe",
        "/tmp/x",
    ]);
    assert_eq!(code, Some(2));
    assert!(said.contains("config_unreadable"), "{said}");
    let pushing = scratch(
        "push.json",
        r#"{"workspace":"ws_1","subjects":[],"audience":[{"recipient":"p_founder","channel":"email"}],
            "app_origin":"https://app.example.invalid","environment":"paper",
            "build":"sha256:3333333333333333333333333333333333333333333333333333333333333333",
            "web_push":true,"push_allowlist":[],"vapid_subject":null}"#,
    );
    let (code, said) = ran(&[
        "run",
        "--config",
        pushing.to_str().unwrap(),
        "--probe",
        "/tmp/x",
    ]);
    assert_eq!(code, Some(2));
    assert!(said.contains("web_push"), "{said}");
    let good = scratch(
        "good.json",
        r#"{"workspace":"ws_1","subjects":[],"audience":[{"recipient":"p_founder","channel":"email"}],
            "app_origin":"https://app.example.invalid","environment":"paper",
            "build":"sha256:3333333333333333333333333333333333333333333333333333333333333333",
            "web_push":false,"push_allowlist":[],"vapid_subject":null}"#,
    );
    let (code, said) = ran(&[
        "run",
        "--config",
        good.to_str().unwrap(),
        "--probe",
        "/tmp/x",
    ]);
    assert_eq!(code, Some(2));
    assert!(said.contains("journal_dsn_missing"), "{said}");
}
