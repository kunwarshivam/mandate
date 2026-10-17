//! `mandate vault import` takes its secret from stdin only and prints none of it, whatever
//! refuses it (DEC-692 item 6, CN-1). The host running the tests has no `owlhead_exec`, so every
//! import here is refused; `mandate-vault-local`'s tests cover the stored secret.

use std::io::Write;
use std::process::{Command, Output, Stdio};

const CANARY: &str = "canary-5d1e8b2c-cli-secret-do-not-print";

fn import(args: &[&str], env: &[(&str, &str)], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mandate"))
        .args(args)
        .env_clear()
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the CLI starts");
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(stdin.as_bytes())
        .ok();
    child.wait_with_output().expect("the CLI finishes")
}

fn assert_silent_about_the_canary(output: &Output, case: &str) {
    for stream in [&output.stdout, &output.stderr] {
        let text = String::from_utf8_lossy(stream);
        assert!(!text.contains(CANARY), "{case} printed the canary: {text}");
        assert!(
            !text.contains(&CANARY[..16]),
            "{case} printed part of the canary: {text}"
        );
    }
}

const IMPORT: [&str; 3] = ["vault", "import", "alpaca-oauth-client-secret"];

#[test]
fn an_import_without_its_credentials_is_refused_silently() {
    let output = import(&IMPORT, &[], CANARY);
    assert!(!output.status.success());
    assert_silent_about_the_canary(&output, "no credentials");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("CREDENTIALS_DIRECTORY"),
        "names what is missing: {stderr}"
    );
}

#[test]
fn an_import_on_a_host_without_the_service_users_is_refused_silently() {
    let credentials = std::env::temp_dir();
    let credentials = credentials.to_str().expect("a UTF-8 temporary directory");
    for env in [
        vec![("CREDENTIALS_DIRECTORY", credentials)],
        vec![
            ("CREDENTIALS_DIRECTORY", credentials),
            ("MANDATE_VAULT_TOKEN_KEY", CANARY),
        ],
        vec![
            ("CREDENTIALS_DIRECTORY", credentials),
            ("MANDATE_VAULT_DIR", "/nonexistent/vault"),
        ],
    ] {
        let output = import(&IMPORT, &env, &format!("{CANARY}\n"));
        assert!(!output.status.success(), "{env:?}");
        assert_silent_about_the_canary(&output, &format!("{env:?}"));
    }
}

#[test]
fn the_secret_is_never_an_argument() {
    for args in [
        vec!["vault", "import", "alpaca-oauth-client-secret", CANARY],
        vec!["vault", "import", CANARY],
        vec![
            "vault",
            "import",
            "alpaca-oauth-client-secret",
            "--secret",
            CANARY,
        ],
    ] {
        let output = import(&args, &[], "");
        assert!(!output.status.success(), "{args:?}");
        assert_eq!(output.status.code(), Some(2), "clap refuses {args:?}");
    }
}

#[test]
fn the_import_has_help() {
    let output = import(&["vault", "import", "--help"], &[], "");
    assert!(output.status.success(), "set-secrets.sh probes it");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("alpaca-oauth-client-secret"), "{stdout}");
}
