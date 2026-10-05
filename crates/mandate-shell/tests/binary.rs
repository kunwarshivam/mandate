//! The `mandate-tracer` binary as an operator runs it: the refusal's stable code first on one line
//! of stderr, and a non-zero exit. They are live: each holds before and after the adapters are
//! implemented.

use std::process::{Command, Output};

use mandate_shell::Stage;

const CONFIG: &str = "crates/mandate-shell/tests/fixtures/tracer/config";

fn tracer(args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mandate-tracer"));
    command.args(args);
    for (name, value) in env {
        command.env(name, value);
    }
    command.output().unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

#[test]
fn a_run_without_the_paper_acknowledgement_is_a_usage_refusal() {
    let output = tracer(&["--mandate", "m.json", "--dataset", "bars"], &[]);
    assert!(!output.status.success());
    assert_eq!(stderr(&output), "usage: --confirm-paper is required\n");
    assert!(output.stdout.is_empty());
}

/// With no mandate file and no dataset, the run refuses before any order whatever is implemented:
/// one line naming a stage's stable code, a non-zero exit, and nothing on stdout.
#[test]
fn a_planning_run_without_its_inputs_refuses_before_anything_is_sent() {
    let output = tracer(
        &[
            "--mandate",
            "no-such-mandate.json",
            "--dataset",
            "no-such-dataset",
            "--config-dir",
            CONFIG,
            "--confirm-paper",
        ],
        &[],
    );
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let text = stderr(&output);
    let (code, _) = text.split_once(": ").unwrap();
    let early = Stage::ALL
        .into_iter()
        .filter(|stage| stage.position() <= Stage::Validate.position())
        .map(Stage::code)
        .collect::<Vec<_>>();
    assert!(early.contains(&code), "{text}");
    assert_eq!(text.lines().count(), 1, "{text}");
}

#[test]
fn a_configured_host_is_refused_before_anything_runs() {
    let output = tracer(
        &[
            "--mandate",
            "m.json",
            "--dataset",
            "bars",
            "--config-dir",
            CONFIG,
            "--confirm-paper",
        ],
        &[("ALPACA_BASE_URL", "https://api.alpaca.markets")],
    );
    assert!(!output.status.success());
    assert_eq!(
        stderr(&output),
        "non_paper_host: ALPACA_BASE_URL looks like a URL; the tracer reaches only the Alpaca paper \
         host\n"
    );
}

#[test]
fn the_shipping_binary_assembles_the_paper_transport_and_trusted_contexts() {
    let source = include_str!("../src/bin/mandate-tracer.rs");
    assert!(
        source.contains("Credentials::from_env()"),
        "the shipping binary must read paper credentials through mandate-alpaca"
    );
    assert!(
        source.contains("AlpacaPaperHttp::new"),
        "the shipping binary must construct the paper-only HTTP transport"
    );
    assert!(
        source.contains("production("),
        "the shipping binary must use the production connector assembly"
    );
    assert!(
        source.contains("load_contexts("),
        "the shipping binary must load explicit trusted run and executor contexts"
    );
    assert!(
        !source.contains("Disconnected"),
        "the shipping binary must not retain the disconnected adapter"
    );
    assert!(
        !source.contains("executor: None"),
        "the shipping binary must supply an executor context"
    );
    assert!(
        !source.contains("run: None"),
        "the shipping binary must supply a run context"
    );
}
