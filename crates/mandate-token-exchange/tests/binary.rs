//! The debug binary's start checks, run as the deployment manager runs it: no arguments, its
//! settings in the environment, and a refusal written as its code alone with exit 2 (DEC-2633
//! item 2). The release binary is run by `cargo xtask ci exchange-release`.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::process::{Command, Output};

const CONNECTION: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W50";
/// [`CONNECTION`]'s ServiceAccount, computed by hand.
const ACCOUNT: &str = "x636f6e6e5f30314a42334b3646304339523256374e344d3851315435573530";

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

/// A token URL that is not UTF-8 is a value set, never read as unset (DEC-2633 item 2).
#[test]
fn a_token_url_that_is_not_utf8_refuses_the_start() {
    for tier in ["production", "dev"] {
        let output = Command::new(env!("CARGO_BIN_EXE_mandate-token-exchange"))
            .env_clear()
            .env("MANDATE_SUBJECT", CONNECTION)
            .env("MANDATE_DEPLOYMENT_TIER", tier)
            .env(
                "MANDATE_TOKEN_URL",
                OsStr::from_bytes(b"http://127.0.0.1:8091/\xff"),
            )
            .output()
            .expect("the binary runs");
        let code = if tier == "dev" {
            "token_exchange_cell_url"
        } else {
            "token_exchange_cell_tier"
        };
        refused_with(&output, code);
    }
}

#[test]
#[ignore = "pending E10-13"]
fn a_missing_subject_refuses_the_start() {
    refused_with(
        &run(&[
            ("MANDATE_DEPLOYMENT_TIER", "dev"),
            ("MANDATE_PROCESS", "token_exchange"),
        ]),
        "token_exchange_subject",
    );
}

/// A pod started as any process but the token exchange refuses before it reads anything else
/// but the token URL (DEC-1175 item 1).
#[test]
#[ignore = "pending E10-13"]
fn any_process_but_the_token_exchange_refuses_the_start() {
    for process in ["executor", "runtime", ""] {
        refused_with(
            &run(&[
                ("MANDATE_DEPLOYMENT_TIER", "dev"),
                ("MANDATE_PROCESS", process),
                ("MANDATE_SUBJECT", CONNECTION),
            ]),
            "token_exchange_process",
        );
    }
}

/// A pod running as another connection's ServiceAccount refuses before it reads its client.
#[test]
#[ignore = "pending E10-13"]
fn a_pod_on_another_connection_s_account_refuses_the_start() {
    refused_with(
        &run(&[
            ("MANDATE_DEPLOYMENT_TIER", "dev"),
            ("MANDATE_PROCESS", "token_exchange"),
            ("MANDATE_SUBJECT", CONNECTION),
            (
                "MANDATE_SUBJECT_ACCOUNT",
                "x636f6e6e5f30314a42334b3646304339523256374e344d3851315435573531",
            ),
        ]),
        "token_exchange_subject_account",
    );
}

#[test]
#[ignore = "pending E10-13"]
fn a_start_that_passes_its_checks_does_not_yet_exchange() {
    let output = run(&[
        ("MANDATE_SUBJECT", CONNECTION),
        ("MANDATE_DEPLOYMENT_TIER", "dev"),
        ("MANDATE_PROCESS", "token_exchange"),
        ("MANDATE_SUBJECT_ACCOUNT", ACCOUNT),
        ("MANDATE_ALPACA_CLIENT_ID", "PKTESTCLIENT01"),
        (
            "MANDATE_ALPACA_REDIRECT_URI",
            "http://127.0.0.1:8080/v1/oauth/alpaca/callback",
        ),
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
