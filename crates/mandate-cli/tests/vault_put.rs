//! `mandate vault put` (E21-32, DEC-2020, DEC-2630): a platform secret written as the secret-import
//! identity, its credentials and the secret on stdin, three lines and nothing else, refused before
//! any vault is reached when stdin is anything else, and never printed. The last case runs against
//! the OpenBao dev server `cargo xtask ci openbao` names, and checks the refusals first, so it fails
//! the same way without one.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use mandate_cli::vault::{PutArgs, PutSecret, Terminals, put_path, run_put};
use mandate_time::UtcNanos;
use mandate_vault::policy::{PlatformSecret, SecretPath};

const CANARY: &str = "canary-9d1e7a40-model-key-do-not-print";
const ROLE: &str = "canary-role-3f0b";
const SECRET_ID: &str = "canary-secret-id-77c2";
/// An address no vault listens on: the discard port on the loopback.
const NOWHERE: &str = "https://127.0.0.1:9";

fn ca() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault-ca.pem")
}

/// Runs `mandate vault put <secret>` against `address` with `stdin` piped in.
fn put(secret: &str, address: &str, ca: &Path, stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mandate"))
        .args(["vault", "put", secret, "--address", address, "--ca"])
        .arg(ca)
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

/// The command failed naming `code` and printed nothing to stdout, and no line of stdin anywhere.
fn refused(output: &Output, code: &str, case: &str) {
    let err = stderr(output);
    assert!(!output.status.success(), "{case} succeeded: {err}");
    assert!(err.contains(code), "{case} does not name `{code}`: {err}");
    assert!(
        output.stdout.is_empty(),
        "{case} printed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    for line in [CANARY, ROLE, SECRET_ID] {
        assert!(!err.contains(line), "{case} printed {line}: {err}");
    }
}

fn three(role: &str, secret_id: &str, value: &str) -> String {
    format!("{role}\n{secret_id}\n{value}\n")
}

#[test]
fn three_lines_are_read_and_the_vault_reached() {
    for stdin in [
        three(ROLE, SECRET_ID, CANARY),
        format!("{ROLE}\n{SECRET_ID}\n{CANARY}"),
    ] {
        let output = put("model-provider-key", NOWHERE, &ca(), &stdin);
        refused(&output, "unavailable", "an unreachable vault");
    }
}

/// A terminal on stdin is refused before it is read.
#[test]
fn a_terminal_never_gives_the_put_its_secret() {
    let args = PutArgs {
        secret: PutSecret::ModelProviderKey,
        address: NOWHERE.to_owned(),
        ca: ca(),
    };
    let stdin = three(ROLE, SECRET_ID, CANARY);
    let mut piped = stdin.as_bytes();
    let terminals = Terminals {
        stdin: true,
        stdout: false,
    };
    let now = UtcNanos::from_parts(1_800_000_000, 0).unwrap();
    let mut printed = Vec::new();
    let answer =
        run_put(&args, &mut piped, terminals, now, &mut printed).map_err(|e| e.to_string());
    let error = answer
        .as_ref()
        .err()
        .map(String::as_str)
        .unwrap_or_default();
    assert!(
        error.contains("(config)") && error.contains("terminal"),
        "{answer:?}"
    );
    assert_eq!(piped.len(), stdin.len(), "stdin was read");
    assert!(printed.is_empty());
}

/// Stdin of exactly 16384 bytes is read and reaches the vault, the last newline counted among
/// them; one byte more is refused.
#[test]
fn stdin_is_read_up_to_its_bound_and_no_further() {
    let head = format!("{ROLE}\n{SECRET_ID}\n");
    let longest = format!("{head}{}", "k".repeat(16384 - head.len()));
    assert_eq!(longest.len(), 16384);
    let output = put("model-provider-key", NOWHERE, &ca(), &longest);
    refused(&output, "unavailable", "the longest stdin");
    let output = put("model-provider-key", NOWHERE, &ca(), &format!("{longest}k"));
    refused(&output, "credential_file", "one byte more");
    let with_newline = format!("{head}{}\n", "k".repeat(16383 - head.len()));
    assert_eq!(with_newline.len(), 16384);
    let output = put("model-provider-key", NOWHERE, &ca(), &with_newline);
    refused(
        &output,
        "unavailable",
        "the longest stdin, its newline counted",
    );
    let over = format!("{head}{}\n", "k".repeat(16384 - head.len()));
    let output = put("model-provider-key", NOWHERE, &ca(), &over);
    refused(&output, "credential_file", "a newline one byte over");
}

#[test]
fn stdin_that_is_not_three_lines_is_refused_before_any_vault_is_reached() {
    for (stdin, case) in [
        (String::new(), "an empty pipe"),
        (format!("{ROLE}\n{SECRET_ID}\n"), "two lines"),
        (
            format!("{ROLE}\n{SECRET_ID}\n{CANARY}\nmore\n"),
            "four lines",
        ),
        (format!("{ROLE}\n\n{CANARY}\n"), "an empty secret id"),
        (format!("{ROLE}\n{SECRET_ID}\n\n"), "an empty secret"),
        (format!("\n{SECRET_ID}\n{CANARY}\n"), "an empty role id"),
        (
            format!("{ROLE}\r\n{SECRET_ID}\n{CANARY}\n"),
            "a carriage return",
        ),
        (
            format!("{ROLE}\n{SECRET_ID}\n{CANARY}\n\n"),
            "a trailing empty line",
        ),
        (
            format!("{ROLE}\n{SECRET_ID}\n{}\n", "k".repeat(16384)),
            "too long",
        ),
    ] {
        let output = put("model-provider-key", NOWHERE, &ca(), &stdin);
        refused(&output, "credential_file", case);
    }
}

#[test]
fn a_plain_http_address_or_a_bad_ca_is_refused() {
    let stdin = three(ROLE, SECRET_ID, CANARY);
    let output = put("model-provider-key", "http://127.0.0.1:9", &ca(), &stdin);
    refused(&output, "config", "a plain-HTTP address");
    let output = put(
        "email-credential",
        NOWHERE,
        Path::new("/nonexistent/ca.pem"),
        &stdin,
    );
    refused(&output, "config", "a missing CA");
}

/// Each secret `vault put` names is written at its own path: a platform secret at its platform
/// path, and the Alpaca OAuth client secret at the client secret's (DEC-2636 item 1).
#[test]
fn each_secret_is_put_at_its_own_path() {
    assert_eq!(
        put_path(PutSecret::ModelProviderKey),
        Ok((
            SecretPath::Platform(PlatformSecret::ModelProviderKey),
            "model provider's key"
        ))
    );
    assert_eq!(
        put_path(PutSecret::EmailCredential),
        Ok((
            SecretPath::Platform(PlatformSecret::EmailCredential),
            "email credential"
        ))
    );
    assert_eq!(
        put_path(PutSecret::NewsDataKey),
        Ok((
            SecretPath::Platform(PlatformSecret::NewsDataKey),
            "news data key"
        ))
    );
    assert_eq!(
        put_path(PutSecret::AlpacaOauthClientSecret),
        Ok((SecretPath::ClientSecret, "Alpaca OAuth client secret"))
    );
}

/// The client secret's three lines are read and the vault reached, as a platform secret's are.
#[test]
fn the_client_secret_is_read_and_the_vault_reached() {
    let output = put(
        "alpaca-oauth-client-secret",
        NOWHERE,
        &ca(),
        &three(ROLE, SECRET_ID, CANARY),
    );
    refused(
        &output,
        "unavailable",
        "the client secret to an unreachable vault",
    );
}

/// Beside the platform secrets, the client secret is the one entry `vault put` writes: no
/// connection's code or token, and no other name, is ever put.
#[test]
fn only_a_platform_secret_or_the_client_secret_is_put() {
    for secret in [
        "code",
        "token",
        "connections/c1/code",
        "connections/c1/token",
        "client-secret",
        "alpaca-oauth-client-id",
        "Alpaca-OAuth-Client-Secret",
        "",
    ] {
        let output = put(secret, NOWHERE, &ca(), &three(ROLE, SECRET_ID, CANARY));
        assert_eq!(
            output.status.code(),
            Some(2),
            "{secret:?}: {}",
            stderr(&output)
        );
        assert!(!stderr(&output).contains(CANARY), "{secret:?}");
    }
}

/// The server the environment names, as `.github/scripts/start-openbao.sh` sets it.
fn server() -> Option<(String, PathBuf, PathBuf)> {
    let address = std::env::var("MANDATE_OPENBAO_ADDR").ok()?;
    let ca = std::env::var("MANDATE_OPENBAO_CA").ok()?;
    let token = std::env::var("MANDATE_OPENBAO_OPERATOR_TOKEN_FILE").ok()?;
    Some((address, PathBuf::from(ca), PathBuf::from(token)))
}

/// Provisions with the existing file mode, then puts the model provider's key as secret-import
/// with those credentials: the put reaches the vault, which, being the dev server, the CLI refuses
/// outside tests (DEC-977 item 6), so the refusal is `dev_server_refused`, never a stdin or
/// connection refusal, and nothing of stdin is printed. The read back by the model gateway runs on
/// staging, which is no dev server.
#[test]
fn a_put_with_provisioned_credentials_reaches_the_vault() {
    stdin_that_is_not_three_lines_is_refused_before_any_vault_is_reached();
    let Some((address, ca, operator)) = server() else {
        assert!(
            std::env::var("MANDATE_OPENBAO_REQUIRED").is_err(),
            "MANDATE_OPENBAO_REQUIRED is set but no OpenBao server is configured"
        );
        return;
    };
    put_with_provisioned_credentials(&address, &ca, &operator, "model-provider-key", "cli-put");
}

/// The client secret's put with provisioned credentials reaches the dev server as a platform
/// secret's does, so the refusal is `dev_server_refused`, never a stdin or connection refusal.
#[test]
fn a_client_secret_put_with_provisioned_credentials_reaches_the_vault() {
    each_secret_is_put_at_its_own_path();
    the_client_secret_is_read_and_the_vault_reached();
    let Some((address, ca, operator)) = server() else {
        return;
    };
    put_with_provisioned_credentials(
        &address,
        &ca,
        &operator,
        "alpaca-oauth-client-secret",
        "cli-put-client-secret",
    );
}

/// Provisions to files under `name`, then puts `secret` as secret-import with those credentials,
/// and checks the dev server refused it and nothing of stdin was printed.
fn put_with_provisioned_credentials(
    address: &str,
    ca: &Path,
    operator: &Path,
    secret: &str,
    name: &str,
) {
    let out = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    let provisioned = Command::new(env!("CARGO_BIN_EXE_mandate"))
        .args(["vault", "provision", "--address", address, "--ca"])
        .arg(ca)
        .arg("--operator-token-file")
        .arg(operator)
        .arg("--out")
        .arg(&out)
        .env_clear()
        .output()
        .unwrap();
    assert!(provisioned.status.success(), "{}", stderr(&provisioned));
    let file = |name: &str| fs::read_to_string(out.join(name)).unwrap();
    let (role_id, secret_id) = (
        file("secret-import.role-id"),
        file("secret-import.secret-id"),
    );
    let _ = fs::remove_dir_all(&out);
    let output = put(secret, address, ca, &three(&role_id, &secret_id, CANARY));
    refused(&output, "dev_server_refused", "a put to the dev server");
    for line in [&role_id, &secret_id] {
        assert!(
            !stderr(&output).contains(line.as_str()),
            "printed a credential"
        );
    }
}
