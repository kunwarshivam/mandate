//! The process after its settings (DEC-2330 item 3): exactly its arguments, and a login that
//! reads its CA and its ServiceAccount token before it dials, failing as the vault.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use mandate_connections::ConnectError;
use mandate_connections::record::ConnectionId;
use mandate_connections::start::{ClientId, OAuthClient, RedirectUri};
use mandate_time::UtcNanos;
use mandate_vault::SourceError;

use crate::process::{
    ARGUMENTS_REFUSED, Failed, VAULT_REFUSED, VaultArgs, exchange, log_in, now, vault_args,
};
use crate::startup::{Endpoint, Settings};

const CONNECTION: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W50";
const NO_VAULT: &str = "https://127.0.0.1:1";

fn args(text: &[&str]) -> Option<VaultArgs> {
    vault_args(text.iter().map(OsString::from))
}

/// A fresh directory under the system's temporary directory, holding `ca.crt` when `with_ca`.
fn dir(name: &str, with_ca: bool) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mandate-token-exchange-process-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    if with_ca {
        let ca = dir.join("ca.crt");
        std::fs::write(&ca, "-----BEGIN CERTIFICATE-----\n").unwrap();
        std::fs::set_permissions(&ca, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    dir
}

#[test]
fn exactly_the_pinned_arguments_are_admitted() {
    assert_eq!(
        args(&[
            "run",
            "--vault-address",
            "https://vault.mandate.svc:8200",
            "--vault-dir",
            "/run/mandate/vault"
        ]),
        Some(VaultArgs {
            address: "https://vault.mandate.svc:8200".to_owned(),
            dir: PathBuf::from("/run/mandate/vault"),
        })
    );
    for refused in [
        &[][..],
        &["run"][..],
        &["run", "--vault-address", NO_VAULT, "--vault-dir"][..],
        &["run", "--vault-dir", "/d", "--vault-address", NO_VAULT][..],
        &["exchange", "--vault-address", NO_VAULT, "--vault-dir", "/d"][..],
        &[
            "run",
            "--vault-address",
            "http://127.0.0.1:1",
            "--vault-dir",
            "/d",
        ][..],
        &["run", "--vault-address", "https://", "--vault-dir", "/d"][..],
        &[
            "run",
            "--vault-address",
            "https://v:8200/v1",
            "--vault-dir",
            "/d",
        ][..],
        &[
            "run",
            "--vault-address",
            "https://user@v:8200",
            "--vault-dir",
            "/d",
        ][..],
        &[
            "run",
            "--vault-address",
            NO_VAULT,
            "--vault-dir",
            "relative",
        ][..],
        &["run", "--vault-address", NO_VAULT, "--vault-dir", "/d", "x"][..],
    ] {
        assert_eq!(args(refused), None, "{refused:?}");
    }
    let not_utf8 = OsString::from_vec(b"/d\xff".to_vec());
    assert_eq!(
        vault_args(
            ["run", "--vault-address", NO_VAULT, "--vault-dir"]
                .into_iter()
                .map(OsString::from)
                .chain([not_utf8])
        ),
        None
    );
}

#[test]
fn each_failure_has_its_stable_code() {
    assert_eq!(ARGUMENTS_REFUSED, "token_exchange_arguments");
    assert_eq!(VAULT_REFUSED, "token_exchange_vault");
    assert_eq!(Failed::Vault.code(), "token_exchange_vault");
    assert_eq!(
        Failed::Exchange(ConnectError::CodeMissing).code(),
        "code_missing"
    );
}

/// The login reads the CA, then the ServiceAccount files, before it dials; a missing one is the
/// file's refusal and no vault is reached.
#[test]
fn the_login_reads_its_files_before_it_dials() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let connection = ConnectionId::new(CONNECTION).unwrap();
    let now = UtcNanos::from_parts(1_791_000_000, 0).unwrap();
    for (name, with_ca) in [("no-ca", false), ("no-token", true)] {
        let vault = VaultArgs {
            address: NO_VAULT.to_owned(),
            dir: dir(name, with_ca),
        };
        let token_dir = dir(&format!("{name}-token"), false);
        let refused = runtime
            .block_on(log_in(&vault, &token_dir, &connection, now))
            .map(|_| ());
        assert_eq!(refused, Err(SourceError::CredentialFile), "{name}");
    }
}

/// Without a login there is no exchange: the failure is the vault's.
#[test]
fn without_a_login_nothing_is_exchanged() {
    let settings = Settings {
        connection: ConnectionId::new(CONNECTION).unwrap(),
        endpoint: Endpoint::Live,
        client: OAuthClient {
            client_id: ClientId("PKTESTCLIENT01".to_owned()),
            redirect_uri: RedirectUri::production(),
        },
    };
    let vault = VaultArgs {
        address: NO_VAULT.to_owned(),
        dir: dir("exchange", true),
    };
    let token_dir = dir("exchange-token", false);
    let now = UtcNanos::from_parts(1_791_000_000, 0).unwrap();
    assert_eq!(
        exchange(&settings, &vault, &token_dir, now).map(|_| ()),
        Err(Failed::Vault)
    );
}

/// The process reads a real time for its login: after 2026-01-01, and never `None` on a host
/// whose clock runs.
#[test]
fn the_wall_clock_is_read() {
    let read = now().expect("a time");
    assert!(
        read > UtcNanos::from_parts(1_767_225_600, 0).unwrap(),
        "{read:?}"
    );
}
