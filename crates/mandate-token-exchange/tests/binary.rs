//! The debug binary's start checks, run as the deployment manager runs it: no arguments, its
//! settings in the environment, and a refusal written as its code alone with exit 2 (DEC-2633
//! item 2). The release binary is run by `cargo xtask ci exchange-release`.

use std::process::{Command, Output};

const CONNECTION: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W50";

/// Runs the binary with `vars` alone in its environment.
fn run(vars: &[(&str, &str)]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mandate-token-exchange"))
        .env_clear()
        .envs(vars.iter().copied())
        .output()
        .expect("the binary runs")
}

fn refused_with(output: &Output, code: &str) {
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stderr), format!("{code}\n"));
    assert!(output.stdout.is_empty(), "{output:?}");
}

#[test]
fn a_token_url_outside_dev_and_ci_refuses_the_start() {
    for tier in ["staging", "production"] {
        let output = run(&[
            ("MANDATE_SUBJECT", CONNECTION),
            ("MANDATE_DEPLOYMENT_TIER", tier),
            (
                "MANDATE_TOKEN_URL",
                "http://127.0.0.1:8091/alpaca/oauth/token",
            ),
        ]);
        refused_with(&output, "token_exchange_cell_tier");
    }
}

#[test]
fn a_missing_subject_refuses_the_start() {
    refused_with(
        &run(&[("MANDATE_DEPLOYMENT_TIER", "dev")]),
        "token_exchange_subject",
    );
}

#[test]
fn a_start_that_passes_its_checks_does_not_yet_exchange() {
    let output = run(&[
        ("MANDATE_SUBJECT", CONNECTION),
        ("MANDATE_DEPLOYMENT_TIER", "dev"),
        (
            "MANDATE_TOKEN_URL",
            "http://127.0.0.1:8091/alpaca/oauth/token",
        ),
    ]);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "token_exchange_not_yet_exchanging\n"
    );
}
