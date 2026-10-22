//! The API's vault port (DEC-2338 items 3 and 4): a paper binding's code is encoded, then written
//! in one `put` at its connection's code path by a fresh login as `Identity::Api`, and nothing
//! else is called; the API's vault identity can write codes but read no token.

mod common;
mod support_vault;

use std::sync::Arc;

use mandate_api_server::clock::Clock;
use mandate_api_server::connect::CodeVault;
use mandate_api_server::vault_codes::{OpenBaoLogin, VaultCodes};
use mandate_connections::ConnectError;
use mandate_connections::code_entry::{self, encode};
use mandate_connections::record::ConnectionId;
use mandate_connections::start::{Binding, Environment, PkceVerifier};
use mandate_connections::vault::AuthorizationCode;
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::openbao::OpenBaoConfig;
use mandate_vault::policy::{Access, Identity, Mounts, PlatformSecret, SecretPath, allows};
use secrecy::{ExposeSecret, SecretString};

use common::{ManualClock, at};
use support_vault::{Call, Login, Script};

const CONNECTION: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W50";
const OTHER: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W51";
const T0: i64 = 1_791_000_000;
const CODE: &str = "code-canary-5e1a";
const VERIFIER: &str = "verifier-canary-0123456789abcdefghijklmnopq.~_";

fn connection(id: &str) -> ConnectionId {
    ConnectionId::new(id).expect("a connection id")
}

fn binding(environment: Environment) -> Binding {
    Binding {
        workspace_id: "ws_01JB3K6F0C9R2V7N4M8Q1T5W3Z".to_owned(),
        user_id: "01JB3K6F0C9R2V7N4M8Q1T5W3Z".to_owned(),
        connection_id: connection(CONNECTION),
        environment,
    }
}

fn code(text: &str) -> AuthorizationCode {
    AuthorizationCode(SecretString::from(text))
}

fn verifier(text: &str) -> PkceVerifier {
    PkceVerifier(SecretString::from(text))
}

fn port(script: &Arc<Script>) -> VaultCodes<Login> {
    VaultCodes {
        login: Login(Arc::clone(script)),
        clock: ManualClock::at(T0) as Arc<dyn Clock>,
    }
}

fn written(script: &Arc<Script>, environment: Environment, c: &str) -> Result<(), ConnectError> {
    port(script).put_code(&binding(environment), code(c), verifier(VERIFIER))
}

/// The entry format 1 holds for `CODE` and `VERIFIER`, computed by the codec.
fn entry() -> Vec<u8> {
    encode(&code(CODE), &verifier(VERIFIER))
        .expect("values of the form")
        .expose_secret()
        .as_bytes()
        .to_vec()
}

#[test]
#[ignore = "pending E10-13"]
fn a_paper_code_is_one_put_at_its_connection_s_code_path_after_one_login() {
    let script = Script::new(Identity::Api);
    assert_eq!(written(&script, Environment::Paper, CODE), Ok(()));
    assert_eq!(
        script.logins(),
        vec![at(T0)],
        "one login, at the clock's now"
    );
    assert_eq!(
        script.calls(),
        vec![Call::Put(SecretPath::Code(connection(CONNECTION)), entry())],
        "one put, of the code entry, and no other call"
    );
    let Call::Put(_, bytes) = &script.calls()[0] else {
        unreachable!("a put")
    };
    let taken = code_entry::decode(bytes).expect("the exchange decodes it");
    assert_eq!(taken.code.0.expose_secret(), CODE);
    assert_eq!(taken.verifier.0.expose_secret(), VERIFIER);
}

#[test]
#[ignore = "pending E10-13"]
fn each_write_logs_in_afresh() {
    let script = Script::new(Identity::Api);
    let codes = port(&script);
    for _ in 0..2 {
        assert_eq!(
            codes.put_code(&binding(Environment::Paper), code(CODE), verifier(VERIFIER)),
            Ok(())
        );
    }
    assert_eq!(
        script.logins().len(),
        2,
        "no session is kept between writes"
    );
}

#[test]
#[ignore = "pending E10-13"]
fn a_live_binding_is_refused_before_any_login() {
    let script = Script::new(Identity::Api);
    assert_eq!(
        written(&script, Environment::Live, CODE),
        Err(ConnectError::EnvironmentRefused)
    );
    assert!(script.logins().is_empty() && script.calls().is_empty());
}

#[test]
#[ignore = "pending E10-13"]
fn a_code_outside_the_entry_s_form_is_refused_before_any_login() {
    let script = Script::new(Identity::Api);
    for c in ["", "a\"b", "a\\b", "a b"] {
        assert_eq!(
            written(&script, Environment::Paper, c),
            Err(ConnectError::CodeEntryInvalid),
            "{c:?}"
        );
    }
    assert!(script.logins().is_empty() && script.calls().is_empty());
}

#[test]
#[ignore = "pending E10-13"]
fn a_failed_login_or_put_is_vault_unavailable() {
    for refused in [
        SourceError::Unavailable,
        SourceError::Denied,
        SourceError::CredentialFile,
        SourceError::Sealed,
    ] {
        let login_fails = Arc::new(Script {
            login_fails: Some(refused.clone()),
            ..Arc::into_inner(Script::new(Identity::Api)).expect("one owner")
        });
        assert_eq!(
            written(&login_fails, Environment::Paper, CODE),
            Err(ConnectError::VaultUnavailable),
            "login {refused:?}"
        );
        assert!(login_fails.calls().is_empty());
        let put_fails = Arc::new(Script {
            put_fails: Some(refused.clone()),
            ..Arc::into_inner(Script::new(Identity::Api)).expect("one owner")
        });
        assert_eq!(
            written(&put_fails, Environment::Paper, CODE),
            Err(ConnectError::VaultUnavailable),
            "put {refused:?}"
        );
    }
}

#[test]
#[ignore = "pending E10-13"]
fn a_session_that_is_not_the_api_s_writes_nothing() {
    for identity in [
        Identity::ConnectionManager,
        Identity::Executor(connection(CONNECTION)),
        Identity::TokenExchange(connection(CONNECTION)),
        Identity::SecretImport,
    ] {
        let script = Script::new(identity.clone());
        assert_eq!(
            written(&script, Environment::Paper, CODE),
            Err(ConnectError::VaultUnavailable),
            "{identity:?}"
        );
        assert!(script.calls().is_empty(), "{identity:?}");
    }
}

/// A clock that cannot be read.
struct NoClock;

impl Clock for NoClock {
    fn now(&self) -> Option<UtcNanos> {
        None
    }
}

#[test]
#[ignore = "pending E10-13"]
fn without_a_clock_nothing_is_logged_in() {
    let script = Script::new(Identity::Api);
    let codes = VaultCodes {
        login: Login(Arc::clone(&script)),
        clock: Arc::new(NoClock) as Arc<dyn Clock>,
    };
    assert_eq!(
        codes.put_code(&binding(Environment::Paper), code(CODE), verifier(VERIFIER)),
        Err(ConnectError::VaultUnavailable)
    );
    assert!(script.logins().is_empty());
}

/// The deployment's login takes its AppRole credentials from the mounted directory only: one
/// without them writes nothing.
#[test]
#[ignore = "pending E10-13"]
fn a_deployment_login_without_its_mounted_credentials_writes_nothing() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mandate-api-server-vault-codes-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let codes = VaultCodes {
        login: OpenBaoLogin {
            config: OpenBaoConfig {
                address: "https://127.0.0.1:1".to_owned(),
                ca_pem: Vec::new(),
                mounts: Mounts::default(),
            },
            dir,
        },
        clock: ManualClock::at(T0) as Arc<dyn Clock>,
    };
    assert_eq!(
        codes.put_code(&binding(Environment::Paper), code(CODE), verifier(VERIFIER)),
        Err(ConnectError::VaultUnavailable)
    );
}

/// What the API's vault identity may do, over every access and every kind of path: write a code,
/// and delete a code or a token. It reads nothing, so no token can reach the API (item 4).
#[test]
fn the_api_s_vault_identity_writes_codes_and_reads_no_token() {
    let paths = [
        SecretPath::Code(connection(CONNECTION)),
        SecretPath::Code(connection(OTHER)),
        SecretPath::Token(connection(CONNECTION)),
        SecretPath::Token(connection(OTHER)),
        SecretPath::ClientSecret,
        SecretPath::Platform(PlatformSecret::ModelProviderKey),
        SecretPath::Platform(PlatformSecret::EmailCredential),
    ];
    let mut allowed = Vec::new();
    for access in [Access::Read, Access::Write, Access::Delete, Access::Take] {
        for path in &paths {
            if allows(&Identity::Api, access, path) {
                allowed.push((access, path.clone()));
            }
        }
    }
    let c = connection(CONNECTION);
    let o = connection(OTHER);
    assert_eq!(
        allowed,
        vec![
            (Access::Write, SecretPath::Code(c.clone())),
            (Access::Write, SecretPath::Code(o.clone())),
            (Access::Delete, SecretPath::Code(c.clone())),
            (Access::Delete, SecretPath::Code(o.clone())),
            (Access::Delete, SecretPath::Token(c)),
            (Access::Delete, SecretPath::Token(o)),
        ]
    );
}
