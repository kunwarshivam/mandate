//! The connect finish's own process binds only with its four settings set and valid, and logs in as
//! the connection manager alone (DEC-2348 item 3); otherwise it names the settings that are
//! missing or invalid, in order, and binds nothing. Its vault directory must hold the connection
//! manager's own credential pair: the API's pair there is never a fallback.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use mandate_api_server::finish_binding::{
    BoundFinish, DATABASE_URL, FINISH_SETTINGS, FINISH_VAULT_ADDRESS, FINISH_VAULT_DIR, WORKSPACE,
    finish_binding, off_line,
};
use mandate_vault::policy::{Identity, Mounts};

const URL: &str = "postgresql://owlhead_api@127.0.0.1:9/owlhead";
const WORKSPACE_ID: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Y";
const ADDRESS: &str = "https://vault.mandate.svc:8200";
const CA: &str = "-----BEGIN CERTIFICATE-----\nca-canary\n-----END CERTIFICATE-----\n";

static DIRS: Mutex<u32> = Mutex::new(0);

/// A fresh directory holding `ca.crt` with `mode`, or no `ca.crt` when `mode` is `None`, and the
/// connection manager's credential pair.
fn vault_dir(mode: Option<u32>) -> PathBuf {
    let dir = bare_dir(mode);
    write_pair(&dir, "connection-manager", 0o600);
    dir
}

/// Writes `role`'s role id and secret id into `dir` with `mode`.
fn write_pair(dir: &Path, role: &str, mode: u32) {
    for (suffix, value) in [
        ("role-id", "role-canary\n"),
        ("secret-id", "secret-canary\n"),
    ] {
        let file = dir.join(format!("{role}.{suffix}"));
        std::fs::write(&file, value).expect("a credential");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode)).expect("mode");
    }
}

/// A fresh directory holding `ca.crt` with `mode`, or no `ca.crt` when `mode` is `None`, and no
/// credentials.
fn bare_dir(mode: Option<u32>) -> PathBuf {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mandate-api-server-finish-binding-{}-{n}",
        std::process::id()
    ));
    drop(n);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    if let Some(mode) = mode {
        let file = dir.join("ca.crt");
        std::fs::write(&file, CA).expect("the CA");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode)).expect("mode");
    }
    dir
}

/// Every finish setting, valid, with the vault directory `dir`.
fn all(dir: &Path) -> BTreeMap<&'static str, OsString> {
    BTreeMap::from([
        (DATABASE_URL, OsString::from(URL)),
        (WORKSPACE, OsString::from(WORKSPACE_ID)),
        (FINISH_VAULT_ADDRESS, OsString::from(ADDRESS)),
        (FINISH_VAULT_DIR, dir.as_os_str().to_owned()),
    ])
}

fn bind(vars: &BTreeMap<&'static str, OsString>) -> Result<BoundFinish, Vec<&'static str>> {
    finish_binding(|name| vars.get(name).cloned())
}

#[test]
#[ignore = "pending E10-13"]
fn every_setting_valid_binds_the_finish_as_the_connection_manager() {
    let dir = vault_dir(Some(0o600));
    let bound = bind(&all(&dir)).expect("a bound finish");
    assert_eq!(bound.database_url, URL);
    assert_eq!(bound.workspace, WORKSPACE_ID);
    assert_eq!(bound.login.identity, Identity::ConnectionManager);
    assert_eq!(bound.login.config.address, ADDRESS);
    assert_eq!(bound.login.config.ca_pem, CA.as_bytes());
    assert_eq!(bound.login.config.mounts, Mounts::default());
    assert_eq!(bound.login.dir, dir);
}

#[test]
#[ignore = "pending E10-13"]
fn none_set_names_all_four_in_order() {
    assert_eq!(bind(&BTreeMap::new()).err(), Some(FINISH_SETTINGS.to_vec()));
}

#[test]
#[ignore = "pending E10-13"]
fn a_missing_or_invalid_setting_is_named_alone() {
    let dir = vault_dir(Some(0o600));
    let invalid: [(&str, OsString); 14] = [
        (DATABASE_URL, OsString::new()),
        (
            DATABASE_URL,
            OsString::from("postgresql://owlhead_api:pw@127.0.0.1:9/owlhead"),
        ),
        (WORKSPACE, OsString::from("not-a-ulid")),
        (WORKSPACE, OsString::new()),
        (
            FINISH_VAULT_ADDRESS,
            OsString::from("http://vault.mandate.svc:8200"),
        ),
        (
            FINISH_VAULT_ADDRESS,
            OsString::from("https://vault.mandate.svc:8200/v1"),
        ),
        (FINISH_VAULT_ADDRESS, OsString::from("https://")),
        (FINISH_VAULT_DIR, OsString::from("relative/dir")),
        (FINISH_VAULT_DIR, vault_dir(None).into_os_string()),
        (FINISH_VAULT_DIR, vault_dir(Some(0o644)).into_os_string()),
        (FINISH_VAULT_DIR, bare_dir(Some(0o600)).into_os_string()),
        (DATABASE_URL, OsString::from_vec(vec![0xff])),
        (WORKSPACE, OsString::from_vec(vec![0xff])),
        (FINISH_VAULT_ADDRESS, OsString::from_vec(vec![0xff])),
    ];
    for (name, value) in invalid {
        let mut vars = all(&dir);
        vars.insert(name, value.clone());
        assert_eq!(bind(&vars).err(), Some(vec![name]), "{name}={value:?}");
    }
    for name in FINISH_SETTINGS {
        let mut vars = all(&dir);
        vars.remove(name);
        assert_eq!(bind(&vars).err(), Some(vec![name]), "{name} unset");
    }
}

#[test]
fn the_off_line_names_the_settings_and_nothing_they_hold() {
    assert_eq!(
        off_line(&[WORKSPACE, FINISH_VAULT_DIR]),
        "refused: finish off: MANDATE_API_WORKSPACE MANDATE_FINISH_VAULT_DIR missing or invalid"
    );
}

/// A vault directory holding the API's pair, or the connection manager's role id alone, or its pair
/// readable by others, binds nothing: the finish never logs in as another identity.
#[test]
#[ignore = "pending E10-13"]
fn without_the_connection_managers_own_pair_the_finish_binds_nothing() {
    let api_only = bare_dir(Some(0o600));
    write_pair(&api_only, "api", 0o600);
    let role_only = bare_dir(Some(0o600));
    write_pair(&role_only, "connection-manager", 0o600);
    std::fs::remove_file(role_only.join("connection-manager.secret-id")).expect("removed");
    let open = bare_dir(Some(0o600));
    write_pair(&open, "connection-manager", 0o644);
    for dir in [api_only, role_only, open] {
        assert_eq!(
            bind(&all(&dir)).err(),
            Some(vec![FINISH_VAULT_DIR]),
            "{dir:?}"
        );
    }
}
