//! The `mandate-tracer` binary as an operator runs it: the refusal's stable code first on one line
//! of stderr, and a non-zero exit. They are live: each holds before and after the adapters are
//! implemented.

use std::process::{Command, Output};

use mandate_alpaca::{KEY_ID_VAR, SECRET_VAR};
use mandate_shell::Stage;

const CONFIG: &str = "crates/mandate-shell/tests/fixtures/tracer/config";
const MANDATE: &str = "crates/mandate-shell/tests/fixtures/tracer/mandate.json";

fn tracer(args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mandate-tracer"));
    command.current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    command.args(args);
    command.args([
        "--workspace",
        "test-workspace",
        "--agent",
        "test-agent",
        "--account-ref",
        "test-account",
    ]);
    command.env_remove(KEY_ID_VAR).env_remove(SECRET_VAR);
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

/// The reviewed artifacts are judged first and read no credential: with them valid and no paper
/// credential in the environment, the run stops at the credential read, before any transport
/// exists, so nothing can reach a network.
#[test]
fn valid_artifacts_reach_the_credential_read_and_nothing_further() {
    let output = tracer(
        &[
            "--mandate",
            MANDATE,
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
    assert_eq!(
        stderr(&output),
        "usage: Alpaca paper credentials are unavailable\n"
    );
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

/// The position of `needle` in the binary's `tracer` function, which must be present.
fn position(source: &str, needle: &str) -> usize {
    source
        .find(needle)
        .unwrap_or_else(|| panic!("the shipping binary must call {needle}"))
}

#[test]
fn the_shipping_binary_reads_the_broker_before_it_assembles_the_trusted_contexts() {
    let source = include_str!("../src/bin/mandate-tracer.rs");
    let body = &source[source.find("fn tracer()").unwrap()..];
    let order = [
        "cli::parse_production(",
        "host::refuse_configured_host(",
        "Artifacts::load_production(",
        "artifacts.deployment(",
        "Credentials::from_env()",
        "AlpacaPaperHttp::new(",
        "preflight(",
        "liquidity_facts(",
        "load_contexts_with_clock(",
        "= production(",
        "run(&mut stages",
    ];
    let positions = order.map(|needle| position(body, needle));
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "the shipping binary must run {order:?} in that order"
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
