//! `mandate vault provision --secrets-namespace` and `--operator-token-stdin` (E21-32, DEC-2630):
//! the token from a pipe by the file's rules, the credentials printed as Kubernetes Secrets to a
//! pipe and never a terminal, `--kubernetes-host` required there, and every refusal before any
//! vault is reached. The last case runs against the OpenBao dev server `cargo xtask ci openbao`
//! names, and checks the refusals first, so it fails the same way without one.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use mandate_cli::vault::{ProvisionArgs, Terminals, kubernetes_auth, run_provision, write_secrets};
use mandate_connections::record::ConnectionId;
use mandate_vault::SourceError;
use mandate_vault::openbao::RoleCredentials;
use mandate_vault::policy::Identity;
use secrecy::SecretString;
use serde_json::Value;

const CANARY: &str = "canary-5be3f1d2-operator-token-do-not-print";
/// An address no vault listens on: the discard port on the loopback.
const NOWHERE: &str = "https://127.0.0.1:9";
/// The `kubernetes` Service's address, as the vault's pod reaches the API server.
const KUBERNETES_HOST: &str = "https://10.43.0.1:443";

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
            "--kubernetes-host",
            KUBERNETES_HOST,
            "--connection",
            "c1",
        ],
        &format!("{CANARY}\n"),
    );
    refused(&output, "unavailable", "an unreachable vault");
}

/// The Secrets are the cluster's path, so provisioning them without `--kubernetes-host` is
/// refused `config` before any vault is reached, and never leaves a vault without Kubernetes
/// auth.
#[test]
fn the_secrets_without_a_kubernetes_host_are_refused_before_any_vault_is_reached() {
    assert_eq!(
        kubernetes_auth(None, Some("ws-ws1")),
        Err(SourceError::Config)
    );
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
    refused(&output, "config", "the Secrets with no Kubernetes host");
}

/// The Secrets are one `v1` `List` on one line, each an `Opaque` Secret in the namespace holding
/// its identity's role id and secret id, in the credentials' order.
#[test]
fn the_secrets_are_one_list_of_each_identitys_secret() {
    let credentials = [
        (Identity::Api, "role-api", "secret-api"),
        (
            Identity::Executor(ConnectionId::new("c1").unwrap()),
            "role-c1",
            "secret-c1",
        ),
    ]
    .map(|(identity, role_id, secret_id)| RoleCredentials {
        identity,
        role_id: role_id.to_owned(),
        secret_id: SecretString::from(secret_id.to_owned()),
    });
    let mut out = Vec::new();
    write_secrets("ws-ws1", &credentials, &mut out).unwrap();
    assert_eq!(out.iter().filter(|byte| **byte == b'\n').count(), 1);
    assert_eq!(out.last(), Some(&b'\n'));
    let list: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(list["apiVersion"], "v1");
    assert_eq!(list["kind"], "List");
    let items = list["items"].as_array().unwrap();
    let found: Vec<(&str, &str, &str)> = items
        .iter()
        .map(|item| {
            assert_eq!(item["kind"], "Secret");
            assert_eq!(item["type"], "Opaque");
            assert_eq!(item["metadata"]["namespace"], "ws-ws1");
            (
                item["metadata"]["name"].as_str().unwrap(),
                item["stringData"]["role-id"].as_str().unwrap(),
                item["stringData"]["secret-id"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            ("vault-approle-api", "role-api", "secret-api"),
            ("vault-approle-executor-6331", "role-c1", "secret-c1"),
        ]
    );
    let mut out = Vec::new();
    assert!(write_secrets("WS", &credentials, &mut out).is_err());
    assert!(out.is_empty());
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
        kubernetes_only: false,
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
        kubernetes_only: false,
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

/// The Secrets are provisioned only with Kubernetes auth set up, which the vault reads its own
/// pod's CA for. The dev server runs in no pod, so it refuses that setup, and the run fails
/// closed, `denied`, printing no Secret and no token: no Secret is ever printed for a vault whose
/// Kubernetes auth is not set up. [`the_secrets_are_one_list_of_each_identitys_secret`] covers
/// what a run that succeeds prints.
#[test]
fn provisioning_the_secrets_outside_a_pod_fails_closed_and_prints_nothing() {
    a_piped_token_is_read_and_the_vault_reached();
    the_secrets_without_a_kubernetes_host_are_refused_before_any_vault_is_reached();
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
            "--kubernetes-host",
            "https://127.0.0.1:6443",
            "--connection",
            "c1",
            "--connection",
            "C_2",
        ],
        &format!("{}\n", token.trim()),
    );
    assert!(!output.status.success(), "{}", stderr(&output));
    assert!(stderr(&output).contains("denied"), "{}", stderr(&output));
    assert!(output.stdout.is_empty());
    assert!(!stderr(&output).contains(token.trim()));
}
