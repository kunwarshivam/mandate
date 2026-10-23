//! The debug binary's start checks, run as the deployment manager runs it: its settings in the
//! environment, then exactly `run --vault-address <address> --vault-dir <dir>`, and a refusal
//! written as its code alone with exit 2 (DEC-2633 item 2, DEC-2330 item 3), up to the vault
//! login, exit 4. The release binary is run by `cargo xtask ci exchange-release`.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
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

/// The settings a start that passes its checks has.
const SETTINGS: [(&str, &str); 7] = [
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
];

/// A vault address nothing answers on, so no test reaches a vault.
const NO_VAULT: &str = "https://127.0.0.1:1";

/// Runs the binary with [`SETTINGS`] alone in its environment and `args`.
fn run_with(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mandate-token-exchange"))
        .env_clear()
        .envs(SETTINGS)
        .args(args)
        .output()
        .expect("the binary runs")
}

/// A fresh directory, holding `ca.crt` when `with_ca`.
fn vault_dir(name: &str, with_ca: bool) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mandate-token-exchange-binary-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    if with_ca {
        let ca = dir.join("ca.crt");
        std::fs::write(&ca, "-----BEGIN CERTIFICATE-----\n").expect("the CA");
        std::fs::set_permissions(&ca, std::fs::Permissions::from_mode(0o644)).expect("mode");
    }
    dir
}

/// A start that passes its settings needs exactly `run --vault-address <https> --vault-dir <abs>`
/// (DEC-2330 item 3); anything else refuses before the vault.
#[test]
fn a_start_without_exactly_its_arguments_refuses() {
    let dir = vault_dir("arguments", true);
    let dir = dir.to_str().expect("UTF-8");
    for args in [
        vec![],
        vec!["run"],
        vec!["run", "--vault-address", NO_VAULT],
        vec!["run", "--vault-dir", dir, "--vault-address", NO_VAULT],
        vec!["exchange", "--vault-address", NO_VAULT, "--vault-dir", dir],
        vec![
            "run",
            "--vault-address",
            "http://127.0.0.1:1",
            "--vault-dir",
            dir,
        ],
        vec![
            "run",
            "--vault-address",
            "https://127.0.0.1:1/v1",
            "--vault-dir",
            dir,
        ],
        vec![
            "run",
            "--vault-address",
            NO_VAULT,
            "--vault-dir",
            "relative/dir",
        ],
        vec![
            "run",
            "--vault-address",
            NO_VAULT,
            "--vault-dir",
            dir,
            "--extra",
        ],
    ] {
        refused_with(&run_with(&args), "token_exchange_arguments");
    }
}

/// The settings are judged before the arguments, so a refused setting is the refusal named even
/// with the right arguments.
#[test]
fn the_settings_are_judged_before_the_arguments() {
    let dir = vault_dir("order", true);
    let output = Command::new(env!("CARGO_BIN_EXE_mandate-token-exchange"))
        .env_clear()
        .env("MANDATE_DEPLOYMENT_TIER", "dev")
        .env("MANDATE_PROCESS", "executor")
        .args(["run", "--vault-address", NO_VAULT, "--vault-dir"])
        .arg(&dir)
        .output()
        .expect("the binary runs");
    refused_with(&output, "token_exchange_process");
}

/// With its settings and arguments, the process logs in before anything else; with no CA, or no
/// ServiceAccount token to log in with, the login fails, exit 4, and nothing is exchanged.
#[test]
fn a_vault_that_cannot_be_logged_in_to_ends_the_run() {
    for (name, with_ca) in [("no-ca", false), ("ca", true)] {
        let dir = vault_dir(name, with_ca);
        let output = run_with(&[
            "run",
            "--vault-address",
            NO_VAULT,
            "--vault-dir",
            dir.to_str().expect("UTF-8"),
        ]);
        assert_eq!(output.status.code(), Some(4), "{name} {output:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "token_exchange_vault\n"
        );
        assert!(output.stdout.is_empty(), "{output:?}");
    }
}
