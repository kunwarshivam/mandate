//! Whether the start binds a connect (DEC-2338 item 5): only with all four connect settings set
//! and valid; with some, no connect and one line naming them; with none, no connect and nothing to
//! say. The authorization page's gate is judged first and still refuses the start.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use mandate_api_server::config::{AUTHORIZE_URL, StartRefusal};
use mandate_api_server::connect_binding::{
    CLIENT_ID, CONNECT_SETTINGS, ConnectBinding, REDIRECT_URI, VAULT_ADDRESS, VAULT_DIR,
    connect_binding,
};
use mandate_connections::start::{AuthorizePage, ClientId, RedirectUri};
use mandate_release::startup::Tier;
use mandate_vault::policy::Mounts;

const CLIENT: &str = "client-canary-41d2";
const REDIRECT: &str = "https://api-staging.owlhead.ai/v1/oauth/alpaca/callback";
const ADDRESS: &str = "https://vault.mandate.svc:8200";
const CA: &str = "-----BEGIN CERTIFICATE-----\nca-canary\n-----END CERTIFICATE-----\n";

static DIRS: Mutex<u32> = Mutex::new(0);

/// A fresh directory holding `ca.crt` with `mode`, or no `ca.crt` when `mode` is `None`.
fn vault_dir(mode: Option<u32>) -> PathBuf {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mandate-api-server-connect-binding-{}-{n}",
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

/// Every connect setting, valid, with the vault directory `dir`.
fn all(dir: &Path) -> BTreeMap<&'static str, OsString> {
    BTreeMap::from([
        (CLIENT_ID, OsString::from(CLIENT)),
        (REDIRECT_URI, OsString::from(REDIRECT)),
        (VAULT_ADDRESS, OsString::from(ADDRESS)),
        (VAULT_DIR, dir.as_os_str().to_owned()),
    ])
}

fn bind(
    tier: Tier,
    vars: &BTreeMap<&'static str, OsString>,
) -> Result<ConnectBinding, StartRefusal> {
    connect_binding(tier, |name| vars.get(name).cloned())
}

/// The settings an incomplete binding names, or a panic naming what it was instead.
fn named(binding: Result<ConnectBinding, StartRefusal>) -> Vec<&'static str> {
    match binding {
        Ok(ConnectBinding::Incomplete(names)) => names,
        other => panic!("an incomplete binding, not {other:?}"),
    }
}

#[test]
#[ignore = "pending E10-13"]
fn with_no_connect_setting_there_is_no_connect_and_nothing_to_say() {
    for tier in [Tier::Dev, Tier::Ci, Tier::Staging, Tier::Production] {
        let binding = bind(tier, &BTreeMap::new()).expect("no refusal");
        assert!(matches!(binding, ConnectBinding::Off), "{tier:?}");
        assert_eq!(binding.off_line(), None);
    }
}

#[test]
#[ignore = "pending E10-13"]
fn with_every_setting_valid_the_connect_is_bound_to_them() {
    let dir = vault_dir(Some(0o600));
    let binding = bind(Tier::Staging, &all(&dir)).expect("no refusal");
    assert_eq!(binding.off_line(), None);
    let ConnectBinding::On(bound) = binding else {
        panic!("a bound connect")
    };
    assert_eq!(bound.authorizer.page, AuthorizePage::alpaca());
    assert_eq!(
        bound.authorizer.client.client_id,
        ClientId(CLIENT.to_owned())
    );
    assert_eq!(
        bound.authorizer.client.redirect_uri,
        RedirectUri::parse(REDIRECT).expect("a redirect URI")
    );
    assert_eq!(bound.login.config.address, ADDRESS);
    assert_eq!(bound.login.config.ca_pem, CA.as_bytes());
    assert_eq!(bound.login.config.mounts, Mounts::default());
    assert_eq!(bound.login.dir, dir);
}

#[test]
#[ignore = "pending E10-13"]
fn each_missing_setting_alone_is_named_and_binds_nothing() {
    let dir = vault_dir(Some(0o600));
    for missing in CONNECT_SETTINGS {
        let mut vars = all(&dir);
        vars.remove(missing);
        assert_eq!(named(bind(Tier::Staging, &vars)), vec![missing]);
    }
}

#[test]
#[ignore = "pending E10-13"]
fn each_invalid_setting_is_named_and_binds_nothing() {
    let dir = vault_dir(Some(0o600));
    let not_utf8 = || OsString::from_vec(b"a\xff".to_vec());
    let cases: Vec<(&'static str, OsString)> = vec![
        (CLIENT_ID, OsString::new()),
        (CLIENT_ID, OsString::from("client id")),
        (CLIENT_ID, OsString::from("c".repeat(257))),
        (CLIENT_ID, not_utf8()),
        (
            REDIRECT_URI,
            OsString::from("http://evil.example/v1/oauth/alpaca/callback"),
        ),
        (
            REDIRECT_URI,
            OsString::from("https://api.owlhead.ai/elsewhere"),
        ),
        (REDIRECT_URI, not_utf8()),
        (
            VAULT_ADDRESS,
            OsString::from("http://vault.mandate.svc:8200"),
        ),
        (
            VAULT_ADDRESS,
            OsString::from("https://vault.mandate.svc:8200/v1"),
        ),
        (VAULT_ADDRESS, OsString::from("https://")),
        (VAULT_ADDRESS, OsString::from("https://user@vault:8200")),
        (VAULT_ADDRESS, not_utf8()),
        (VAULT_DIR, OsString::from("relative/vault")),
        (VAULT_DIR, vault_dir(None).into_os_string()),
        (VAULT_DIR, vault_dir(Some(0o640)).into_os_string()),
        (VAULT_DIR, vault_dir(Some(0o644)).into_os_string()),
        (VAULT_DIR, not_utf8()),
    ];
    for (name, value) in cases {
        let mut vars = all(&dir);
        vars.insert(name, value.clone());
        assert_eq!(
            named(bind(Tier::Staging, &vars)),
            vec![name],
            "{name} {value:?}"
        );
    }
}

#[test]
#[ignore = "pending E10-13"]
fn several_missing_settings_are_named_in_order_on_one_line_without_any_value() {
    let dir = vault_dir(Some(0o600));
    let mut vars = all(&dir);
    vars.remove(REDIRECT_URI);
    vars.insert(VAULT_ADDRESS, OsString::from("http://vault-canary:8200"));
    let binding = bind(Tier::Staging, &vars).expect("no refusal");
    let line = binding.off_line().expect("a line");
    assert_eq!(
        line,
        "connect off: MANDATE_ALPACA_REDIRECT_URI MANDATE_API_VAULT_ADDRESS missing or invalid"
    );
    for value in [
        CLIENT,
        "vault-canary",
        "ca-canary",
        dir.to_str().expect("UTF-8"),
    ] {
        assert!(!line.contains(value), "{line}");
    }
}

/// The authorization page's gate comes first: a URL set where it may not be refuses the start
/// whatever the connect settings say, none of them set included (item 5, DEC-2633 item 3).
#[test]
#[ignore = "pending E10-13"]
fn an_authorize_url_set_where_it_may_not_be_still_refuses_the_start() {
    let dir = vault_dir(Some(0o600));
    let page = OsString::from("http://127.0.0.1:8091/alpaca/oauth/authorize");
    for tier in [Tier::Staging, Tier::Production] {
        for mut vars in [all(&dir), BTreeMap::new()] {
            vars.insert(AUTHORIZE_URL, page.clone());
            assert!(
                matches!(bind(tier, &vars), Err(StartRefusal::AuthorizeUrl)),
                "{tier:?}"
            );
        }
    }
}

/// A dev or CI cell's page reaches the bound connect.
#[cfg(debug_assertions)]
#[test]
#[ignore = "pending E10-13"]
fn a_dev_or_ci_cell_binds_its_fixture_page() {
    let dir = vault_dir(Some(0o600));
    let page = "http://oauth-fixture:8091/alpaca/oauth/authorize";
    for tier in [Tier::Dev, Tier::Ci] {
        let mut vars = all(&dir);
        vars.insert(AUTHORIZE_URL, OsString::from(page));
        let Ok(ConnectBinding::On(bound)) = bind(tier, &vars) else {
            panic!("a bound connect in {tier:?}")
        };
        assert_eq!(bound.authorizer.page.as_str(), page);
    }
}
