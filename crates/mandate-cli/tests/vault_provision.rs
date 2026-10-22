//! `mandate vault provision` (E21-32, DEC-2022 item 2): it reads the operator's token only from a
//! file its owner alone can read, refuses before it reaches any vault when that or its arguments are
//! wrong, writes nothing on a refusal or an outage, and prints only the names of the files it wrote.
//! The last case runs against the OpenBao dev server `cargo xtask ci openbao` names, and checks the
//! refusals first, so it fails the same way without one.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

use mandate_cli::vault::kubernetes_auth;
use mandate_vault::SourceError;
use mandate_vault::kubernetes::KubernetesAuth;

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

/// A token file made at mode 0600, so the token is never readable by others while it is written,
/// then given `mode`.
fn token_file(text: &str, mode: u32) -> PathBuf {
    let path = fresh("token");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
    path
}

/// Removes the files and directories it holds when dropped, a failed assertion included.
struct Removed(Vec<PathBuf>);

impl Drop for Removed {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::remove_dir_all(path);
            let _ = fs::remove_file(path);
        }
    }
}

fn provision(address: &str, ca: &Path, token: &Path, connections: &[&str], out: &Path) -> Output {
    provision_with(address, ca, token, connections, out, &[])
}

/// As [`provision`], with `extra` arguments after the rest.
fn provision_with(
    address: &str,
    ca: &Path,
    token: &Path,
    connections: &[&str],
    out: &Path,
    extra: &[&str],
) -> Output {
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
    args.extend(extra.iter().map(|a| (*a).to_owned()));
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
        !err.contains(CANARY) && !err.contains(&CANARY[..16]),
        "{case} printed the token: {err}"
    );
    assert!(!out.exists(), "{case} wrote {}", out.display());
}

#[test]
fn a_token_file_open_to_others_or_missing_is_refused_before_any_vault_is_reached() {
    for mode in [0o640, 0o604, 0o644] {
        let out = fresh("out");
        let token = token_file(CANARY, mode);
        let _removed = Removed(vec![token.clone(), out.clone()]);
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
fn an_unreachable_vault_is_an_outage_and_writes_nothing() {
    let out = fresh("out");
    let token = token_file(CANARY, 0o600);
    let _removed = Removed(vec![token.clone(), out.clone()]);
    let output = provision(NOWHERE, &ca(), &token, &["c1"], &out);
    refused(&output, "unavailable", &out, "an unreachable vault");
}

#[test]
fn a_plain_http_address_a_bad_ca_or_a_bad_connection_is_refused() {
    let token = token_file(CANARY, 0o600);
    let out = fresh("out");
    let _removed = Removed(vec![token.clone(), out.clone()]);
    let output = provision("http://127.0.0.1:9", &ca(), &token, &["c1"], &out);
    refused(&output, "config", &out, "a plain-HTTP address");
    let bad_ca = token_file("not a certificate", 0o600);
    let output = provision(NOWHERE, &bad_ca, &token, &["c1"], &out);
    refused(&output, "config", &out, "a CA that is not PEM");
    let output = provision(NOWHERE, &ca(), &token, &["not a connection id!"], &out);
    refused(&output, "(connection)", &out, "a malformed connection id");
}

/// An `--out` the write would refuse is refused before the vault is reached: the address is one no
/// vault listens on, so the refusal is `credential_file`, never `unavailable`, and nothing is
/// written into `--out` (DEC-2022 item 2).
#[test]
fn an_out_the_write_would_refuse_is_refused_before_any_vault_is_reached() {
    let token = token_file(CANARY, 0o600);
    let open = fresh("out-open");
    let planted = fresh("out-planted");
    let parent = fresh("out-parent");
    let _removed = Removed(vec![
        token.clone(),
        open.clone(),
        planted.clone(),
        parent.clone(),
    ]);
    fs::create_dir(&open).unwrap();
    fs::set_permissions(&open, fs::Permissions::from_mode(0o755)).unwrap();
    fs::create_dir(&planted).unwrap();
    fs::set_permissions(&planted, fs::Permissions::from_mode(0o700)).unwrap();
    fs::create_dir(planted.join("executor-c1.secret-id")).unwrap();
    fs::create_dir(&parent).unwrap();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o777)).unwrap();
    let listing = |dir: &Path| -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    };
    for (out, case) in [
        (open.clone(), "a 0755 --out"),
        (planted.clone(), "a directory at an output name"),
        (parent.join("out"), "an --out whose parent others may write"),
    ] {
        let before = if out.exists() {
            listing(&out)
        } else {
            Vec::new()
        };
        let output = provision(NOWHERE, &ca(), &token, &["c1"], &out);
        let err = stderr(&output);
        assert!(!output.status.success(), "{case} succeeded: {err}");
        assert!(
            err.contains("(credential_file)"),
            "{case} reached the vault: {err}"
        );
        assert!(
            !err.contains(&CANARY[..16]),
            "{case} printed the token: {err}"
        );
        assert!(output.stdout.is_empty(), "{case} printed");
        let after = if out.exists() {
            listing(&out)
        } else {
            Vec::new()
        };
        assert_eq!(before, after, "{case} wrote into --out");
    }
    assert!(!parent.join("out").exists());
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
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
    for printed in [&output.stdout, &output.stderr] {
        let printed = String::from_utf8_lossy(printed);
        assert!(
            !printed.contains(&CANARY[..16]),
            "printed the token: {printed}"
        );
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
    let _removed = Removed(vec![token.clone(), out.clone()]);
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

/// DEC-2062 item 3: `--kubernetes-host` names the API server as `https://`, an IPv4 address and a
/// port, and anything else, a name, userinfo, a path or a query included, is refused `config`
/// before any vault is reached; an accepted one reaches the vault, so a vault no one answers for
/// is an outage.
#[test]
fn a_kubernetes_host_that_is_not_https_is_refused_before_any_vault_is_reached() {
    let token = token_file(CANARY, 0o600);
    let out = fresh("out");
    let _removed = Removed(vec![token.clone(), out.clone()]);
    for host in [
        "http://10.43.0.1:443",
        "10.43.0.1:443",
        "",
        "https://",
        "https://u:p@10.43.0.1:443",
        "https://@",
        "https:///x",
        "https://10.43.0.1:443/x",
        "https://10.43.0.1:443?a=b",
        "https://evil.example.com:443",
        "https://10.43.0.1",
        "https://10.43.0.1:99999",
    ] {
        let output = provision_with(
            NOWHERE,
            &ca(),
            &token,
            &["c1"],
            &out,
            &["--kubernetes-host", host],
        );
        refused(
            &output,
            "config",
            &out,
            &format!("the Kubernetes host {host:?}"),
        );
    }
    let output = provision_with(
        NOWHERE,
        &ca(),
        &token,
        &["c1"],
        &out,
        &["--kubernetes-host", "https://10.43.0.1:443"],
    );
    refused(&output, "unavailable", &out, "an https Kubernetes host");
}

/// With `--kubernetes-host`, provisioning asks the vault to set Kubernetes auth up with no CA, so
/// that the vault reads the CA its own pod projects. The dev server runs in no pod and has none to
/// read, so it refuses the configuration, and the run fails closed, `denied`, where the same run
/// without the flag succeeds: the refusal is the Kubernetes setup's, reached past every check.
#[test]
fn provisioning_with_a_kubernetes_host_asks_the_vault_to_set_kubernetes_auth_up() {
    a_kubernetes_host_that_is_not_https_is_refused_before_any_vault_is_reached();
    let Some((address, ca, operator)) = server() else {
        return;
    };
    let token = token_file(fs::read_to_string(operator).unwrap().trim(), 0o600);
    let out = fresh("out");
    let _removed = Removed(vec![token.clone(), out.clone()]);
    let host = ["--kubernetes-host", "https://127.0.0.1:6443"];
    let output = provision_with(&address, &ca, &token, &["c1"], &out, &host);
    refused(&output, "denied", &out, "a Kubernetes host outside a pod");
    let output = provision(&address, &ca, &token, &["c1"], &out);
    assert!(output.status.success(), "{}", stderr(&output));
}

/// The host is passed as it is, with no CA: the vault reads the one its own pod projects. No
/// host is no Kubernetes auth with `--out`, and refused with `--secrets-namespace`, the
/// cluster's path, so a staging vault is never provisioned without it.
#[test]
fn a_kubernetes_host_is_the_api_server_with_no_ca() {
    let in_cluster = Some(KubernetesAuth {
        host: "https://10.43.0.1:443".to_owned(),
        ca_pem: None,
    });
    for namespace in [None, Some("ws-ws1")] {
        assert_eq!(
            kubernetes_auth(Some("https://10.43.0.1:443"), namespace),
            Ok(in_cluster.clone()),
            "{namespace:?}"
        );
        assert_eq!(
            kubernetes_auth(Some("http://10.43.0.1:443"), namespace),
            Err(SourceError::Config),
            "{namespace:?}"
        );
        assert_eq!(
            kubernetes_auth(Some("https://u:p@10.43.0.1:443"), namespace),
            Err(SourceError::Config),
            "{namespace:?}"
        );
    }
    assert_eq!(kubernetes_auth(None, None), Ok(None));
    assert_eq!(
        kubernetes_auth(None, Some("ws-ws1")),
        Err(SourceError::Config)
    );
}

/// Provisioning to files, outside the cluster, may omit `--kubernetes-host`: the run reaches the
/// vault, so a vault no one answers for is an outage, not a refusal.
#[test]
fn provisioning_to_files_needs_no_kubernetes_host() {
    assert_eq!(kubernetes_auth(None, None), Ok(None));
    let token = token_file(CANARY, 0o600);
    let out = fresh("out");
    let _removed = Removed(vec![token.clone(), out.clone()]);
    let output = provision(NOWHERE, &ca(), &token, &["c1"], &out);
    refused(
        &output,
        "unavailable",
        &out,
        "files with no Kubernetes host",
    );
}
