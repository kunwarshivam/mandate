//! `mandate vault provision` (E21-32, DEC-2022 item 2): it reads the operator's token only from a
//! file its owner alone can read, refuses before it reaches any vault when that or its arguments are
//! wrong, writes nothing on a refusal or an outage, and prints only the names of the files it wrote.
//! The last case runs against the OpenBao dev server `cargo xtask ci openbao` names, and checks the
//! refusals first, so it fails the same way without one.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const CANARY: &str = "canary-71c4e0aa-operator-token-do-not-print";
/// An address no vault listens on: the discard port on the loopback.
const NOWHERE: &str = "https://127.0.0.1:9";

static NEXT: AtomicU32 = AtomicU32::new(0);

fn fresh(name: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("cli-provision-{}-{n}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    let _ = fs::remove_file(&path);
    path
}

fn ca() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault-ca.pem")
}

fn token_file(text: &str, mode: u32) -> PathBuf {
    let path = fresh("token");
    fs::write(&path, text).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
    path
}

fn provision(address: &str, ca: &Path, token: &Path, connections: &[&str], out: &Path) -> Output {
    let mut args = vec![
        "vault".to_owned(),
        "provision".to_owned(),
        "--address".to_owned(),
        address.to_owned(),
        "--ca".to_owned(),
        ca.display().to_string(),
        "--operator-token-file".to_owned(),
        token.display().to_string(),
        "--out".to_owned(),
        out.display().to_string(),
    ];
    for c in connections {
        args.extend(["--connection".to_owned(), (*c).to_owned()]);
    }
    Command::new(env!("CARGO_BIN_EXE_mandate"))
        .args(&args)
        .env_clear()
        .output()
        .expect("the CLI runs")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The command failed naming `code`, printed nothing to stdout or of the canary, and made no
/// `out`.
fn refused(output: &Output, code: &str, out: &Path, case: &str) {
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
    assert!(!out.exists(), "{case} wrote {}", out.display());
}

#[test]
#[ignore = "pending E21-32"]
fn a_token_file_open_to_others_or_missing_is_refused_before_any_vault_is_reached() {
    for mode in [0o640, 0o604, 0o644] {
        let out = fresh("out");
        let token = token_file(CANARY, mode);
        let output = provision(NOWHERE, &ca(), &token, &["c1"], &out);
        refused(
            &output,
            "credential_file",
            &out,
            &format!("a token of mode {mode:o}"),
        );
    }
    let out = fresh("out");
    let output = provision(NOWHERE, &ca(), &fresh("missing"), &["c1"], &out);
    refused(&output, "credential_file", &out, "a missing token");
}

#[test]
#[ignore = "pending E21-32"]
fn an_unreachable_vault_is_an_outage_and_writes_nothing() {
    let out = fresh("out");
    let token = token_file(CANARY, 0o600);
    let output = provision(NOWHERE, &ca(), &token, &["c1"], &out);
    refused(&output, "unavailable", &out, "an unreachable vault");
}

#[test]
#[ignore = "pending E21-32"]
fn a_plain_http_address_a_bad_ca_or_a_bad_connection_is_refused() {
    let token = token_file(CANARY, 0o600);
    let out = fresh("out");
    let output = provision("http://127.0.0.1:9", &ca(), &token, &["c1"], &out);
    refused(&output, "config", &out, "a plain-HTTP address");
    let bad_ca = token_file("not a certificate", 0o600);
    let output = provision(NOWHERE, &bad_ca, &token, &["c1"], &out);
    refused(&output, "config", &out, "a CA that is not PEM");
    let output = provision(NOWHERE, &ca(), &token, &["not a connection id!"], &out);
    refused(&output, "connection", &out, "a malformed connection id");
}

/// The token is never an argument: the command has no flag that takes one.
#[test]
fn the_token_is_never_an_argument() {
    let output = Command::new(env!("CARGO_BIN_EXE_mandate"))
        .args([
            "vault",
            "provision",
            "--operator-token",
            CANARY,
            "--address",
            NOWHERE,
        ])
        .env_clear()
        .output()
        .expect("the CLI runs");
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&CANARY[..16]));
}

/// The server the environment names, as `.github/scripts/start-openbao.sh` sets it.
fn server() -> Option<(String, PathBuf, PathBuf)> {
    let address = std::env::var("MANDATE_OPENBAO_ADDR").ok()?;
    let ca = std::env::var("MANDATE_OPENBAO_CA").ok()?;
    let token = std::env::var("MANDATE_OPENBAO_OPERATOR_TOKEN_FILE").ok()?;
    Some((address, PathBuf::from(ca), PathBuf::from(token)))
}

#[test]
#[ignore = "pending E21-32"]
fn provisioning_writes_each_identitys_files_and_prints_only_their_names() {
    a_token_file_open_to_others_or_missing_is_refused_before_any_vault_is_reached();
    let Some((address, ca, operator)) = server() else {
        assert!(
            std::env::var("MANDATE_OPENBAO_REQUIRED").is_err(),
            "MANDATE_OPENBAO_REQUIRED is set but no OpenBao server is configured"
        );
        return;
    };
    let token = token_file(fs::read_to_string(operator).unwrap().trim(), 0o600);
    let out = fresh("out");
    let output = provision(&address, &ca, &token, &["c1", "c2"], &out);
    assert!(output.status.success(), "{}", stderr(&output));
    let mut want: Vec<String> = [
        "api",
        "connection-manager",
        "runtime",
        "secret-import",
        "model-gateway",
        "email-sender",
        "executor-c1",
        "executor-c2",
    ]
    .iter()
    .flat_map(|role| [format!("{role}.role-id"), format!("{role}.secret-id")])
    .collect();
    want.sort();
    let mut listed: Vec<String> = fs::read_dir(&out)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    listed.sort();
    assert_eq!(listed, want);
    let printed = String::from_utf8_lossy(&output.stdout).into_owned();
    let mut printed_names: Vec<&str> = printed.lines().collect();
    printed_names.sort_unstable();
    assert_eq!(printed_names, want);
    for name in &want {
        let file = out.join(name);
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600,
            "{name}"
        );
        let value = fs::read_to_string(&file).unwrap();
        assert!(!value.is_empty(), "{name}");
        assert!(
            !printed.contains(&value) && !stderr(&output).contains(&value),
            "{name} printed"
        );
    }
    assert_eq!(
        fs::metadata(&out).unwrap().permissions().mode() & 0o777,
        0o700
    );
}
