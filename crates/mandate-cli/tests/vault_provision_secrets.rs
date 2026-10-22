//! `mandate vault provision --secrets-namespace` and `--operator-token-stdin` (E21-32, DEC-2630):
//! the token from a pipe by the file's rules, the credentials printed as Kubernetes Secrets to a
//! pipe and never a terminal, and every refusal before any vault is reached. The last case runs
//! against the OpenBao dev server `cargo xtask ci openbao` names, and checks the refusals first,
//! so it fails the same way without one.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use mandate_cli::vault::{ProvisionArgs, Terminals, run_provision};
use serde_json::Value;

const CANARY: &str = "canary-5be3f1d2-operator-token-do-not-print";
/// An address no vault listens on: the discard port on the loopback.
const NOWHERE: &str = "https://127.0.0.1:9";

fn ca() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault-ca.pem")
}

/// Runs `mandate vault provision` with `args` after the address and CA, `stdin` piped in, and
/// stdout a pipe.
fn provision(address: &str, ca: &Path, args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mandate"))
        .args(["vault", "provision", "--address", address, "--ca"])
        .arg(ca)
        .args(args)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the CLI runs");
    let mut input = child.stdin.take().expect("stdin is piped");
    drop(input.write_all(stdin.as_bytes()));
    drop(input);
    child.wait_with_output().expect("the CLI ends")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The command failed naming `code`, printed nothing to stdout, and none of the canary.
fn refused(output: &Output, code: &str, case: &str) {
    let err = stderr(output);
    assert!(!output.status.success(), "{case} succeeded: {err}");
    assert!(err.contains(code), "{case} does not name `{code}`: {err}");
    assert!(
        output.stdout.is_empty(),
        "{case} printed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        !err.contains(&CANARY[..16]),
        "{case} printed the token: {err}"
    );
}

#[test]
fn a_piped_token_is_read_and_the_vault_reached() {
    let output = provision(
        NOWHERE,
        &ca(),
        &[
            "--operator-token-stdin",
            "--secrets-namespace",
            "ws-ws1",
            "--connection",
            "c1",
        ],
        &format!("{CANARY}\n"),
    );
    refused(&output, "unavailable", "an unreachable vault");
}

#[test]
fn a_piped_token_that_is_not_one_line_is_refused_before_any_vault_is_reached() {
    for (stdin, case) in [
        ("", "an empty pipe"),
        ("\n", "a bare newline"),
        (&format!("{CANARY}\r\n"), "a CRLF ending"),
        (&format!("{CANARY}\nmore\n"), "two lines"),
    ] {
        let output = provision(
            NOWHERE,
            &ca(),
            &["--operator-token-stdin", "--secrets-namespace", "ws-ws1"],
            stdin,
        );
        refused(&output, "credential_file", case);
    }
}

#[test]
fn a_namespace_that_is_not_a_label_is_refused_before_any_vault_is_reached() {
    for namespace in ["WS", "a_b", "-a", "a."] {
        let output = provision(
            NOWHERE,
            &ca(),
            &[
                "--operator-token-stdin",
                &format!("--secrets-namespace={namespace}"),
            ],
            &format!("{CANARY}\n"),
        );
        refused(&output, "config", namespace);
    }
}

/// A terminal never receives the Secrets: the refusal comes before stdin is read.
#[test]
fn a_terminal_never_receives_the_secrets() {
    let args = ProvisionArgs {
        address: NOWHERE.to_owned(),
        ca: ca(),
        operator_token_file: None,
        operator_token_stdin: true,
        connections: vec!["c1".to_owned()],
        out: None,
        secrets_namespace: Some("ws-ws1".to_owned()),
        kubernetes_host: None,
    };
    let stdin = format!("{CANARY}\n");
    let mut piped = stdin.as_bytes();
    let mut printed = Vec::new();
    let terminals = Terminals {
        stdin: false,
        stdout: true,
    };
    let answer =
        run_provision(&args, &mut piped, terminals, &mut printed).map_err(|e| e.to_string());
    let error = answer
        .as_ref()
        .err()
        .map(String::as_str)
        .unwrap_or_default();
    assert!(
        error.contains("(config)") && error.contains("terminal"),
        "{answer:?}"
    );
    assert!(printed.is_empty());
    assert_eq!(piped.len(), stdin.len(), "stdin was read");
}

/// A terminal on stdin is refused before it is read, so a token is never typed where it echoes;
/// with `--operator-token-file`, stdin is not read and a terminal there is no matter.
#[test]
fn a_token_is_never_read_from_a_terminal() {
    let mut args = ProvisionArgs {
        address: NOWHERE.to_owned(),
        ca: ca(),
        operator_token_file: None,
        operator_token_stdin: true,
        connections: vec!["c1".to_owned()],
        out: None,
        secrets_namespace: Some("ws-ws1".to_owned()),
        kubernetes_host: None,
    };
    let terminals = Terminals {
        stdin: true,
        stdout: false,
    };
    let stdin = format!("{CANARY}\n");
    let mut piped = stdin.as_bytes();
    let answer = run_provision(&args, &mut piped, terminals, &mut Vec::new());
    let error = answer.map_err(|e| e.to_string()).err().unwrap_or_default();
    assert!(
        error.contains("(config)") && error.contains("terminal"),
        "{error}"
    );
    assert_eq!(piped.len(), stdin.len(), "stdin was read");
    args.operator_token_stdin = false;
    args.operator_token_file = Some(PathBuf::from("/nonexistent/token"));
    let answer = run_provision(&args, &mut piped, terminals, &mut Vec::new());
    let error = answer.map_err(|e| e.to_string()).err().unwrap_or_default();
    assert!(error.contains("credential_file"), "{error}");
}

#[test]
fn the_token_and_the_output_are_each_named_once() {
    for (args, case) in [
        (&["--secrets-namespace", "ws-ws1"][..], "no token"),
        (&["--operator-token-stdin"][..], "no output"),
        (
            &[
                "--operator-token-stdin",
                "--operator-token-file",
                "t",
                "--out",
                "o",
            ][..],
            "two tokens",
        ),
        (
            &[
                "--operator-token-stdin",
                "--secrets-namespace",
                "ws-ws1",
                "--out",
                "o",
            ][..],
            "two outputs",
        ),
    ] {
        let output = provision(NOWHERE, &ca(), args, &format!("{CANARY}\n"));
        assert_eq!(output.status.code(), Some(2), "{case}: {}", stderr(&output));
        assert!(output.stdout.is_empty(), "{case}");
    }
}

/// The server the environment names, as `.github/scripts/start-openbao.sh` sets it.
fn server() -> Option<(String, PathBuf, PathBuf)> {
    let address = std::env::var("MANDATE_OPENBAO_ADDR").ok()?;
    let ca = std::env::var("MANDATE_OPENBAO_CA").ok()?;
    let token = std::env::var("MANDATE_OPENBAO_OPERATOR_TOKEN_FILE").ok()?;
    Some((address, PathBuf::from(ca), PathBuf::from(token)))
}

#[test]
fn provisioning_prints_each_identitys_secret_and_nothing_else() {
    a_piped_token_is_read_and_the_vault_reached();
    let Some((address, ca, operator)) = server() else {
        assert!(
            std::env::var("MANDATE_OPENBAO_REQUIRED").is_err(),
            "MANDATE_OPENBAO_REQUIRED is set but no OpenBao server is configured"
        );
        return;
    };
    let token = std::fs::read_to_string(operator).unwrap();
    let output = provision(
        &address,
        &ca,
        &[
            "--operator-token-stdin",
            "--secrets-namespace",
            "ws-ws1",
            "--connection",
            "c1",
            "--connection",
            "C_2",
        ],
        &format!("{}\n", token.trim()),
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let list: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(list["apiVersion"], "v1");
    assert_eq!(list["kind"], "List");
    let items = list["items"].as_array().unwrap();
    let mut names: Vec<&str> = items
        .iter()
        .map(|item| item["metadata"]["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "vault-approle-api",
            "vault-approle-connection-manager",
            "vault-approle-email-sender",
            "vault-approle-executor-435f32",
            "vault-approle-executor-6331",
            "vault-approle-model-gateway",
            "vault-approle-runtime",
            "vault-approle-secret-import",
        ]
    );
    for item in items {
        assert_eq!(item["kind"], "Secret");
        assert_eq!(item["metadata"]["namespace"], "ws-ws1");
        for key in ["role-id", "secret-id"] {
            let value = item["stringData"][key].as_str().unwrap();
            assert!(!value.is_empty(), "{key}");
            assert!(!stderr(&output).contains(value), "{key} on stderr");
        }
    }
    assert!(!String::from_utf8_lossy(&output.stdout).contains(token.trim()));
}
