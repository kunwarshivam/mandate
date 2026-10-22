//! The connect finish's teardown over the vault (DEC-2348 item 3): one fresh login as the API at
//! the clock's time, then a delete of the connection's code and of its token, made off the async
//! runtime; anything else deletes nothing or fails the teardown, which the pass retries.

mod common;
mod support_vault;

use std::sync::Arc;

use mandate_api_server::clock::Clock;
use mandate_api_server::finish_pass::{DeleteFailed, Teardown};
use mandate_api_server::vault_teardown::VaultTeardown;
use mandate_connections::record::ConnectionId;
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::policy::{Identity, SecretPath};

use common::{ManualClock, at};
use support_vault::{Call, Login, Script};

const CONNECTION: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W50";
const NOW: i64 = 1_791_000_000;

fn connection() -> ConnectionId {
    ConnectionId::new(CONNECTION).expect("a connection id")
}

/// The teardown's answer for `script`'s vault at `clock`, from inside an async runtime as the
/// pass calls it.
fn deleted(script: &Arc<Script>, clock: Arc<dyn Clock>) -> Result<(), DeleteFailed> {
    let teardown = VaultTeardown {
        login: Arc::new(Login(Arc::clone(script))),
        clock,
    };
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a runtime")
        .block_on(teardown.delete(&connection()))
}

fn with(script: Script) -> Arc<Script> {
    Arc::new(script)
}

fn api() -> Script {
    Arc::into_inner(Script::new(Identity::Api)).expect("one owner")
}

#[test]
fn the_teardown_deletes_the_code_then_the_token_as_the_api() {
    let script = Script::new(Identity::Api);
    assert_eq!(deleted(&script, ManualClock::at(NOW)), Ok(()));
    assert_eq!(script.logins(), [at(NOW)], "one fresh login");
    assert_eq!(
        script.calls(),
        [
            Call::Delete(SecretPath::Code(connection())),
            Call::Delete(SecretPath::Token(connection())),
        ]
    );
}

#[test]
fn a_failed_delete_still_deletes_the_other_and_fails() {
    for failing in [
        SecretPath::Code(connection()),
        SecretPath::Token(connection()),
    ] {
        let script = with(Script {
            delete_fails_on: Some(failing.clone()),
            ..api()
        });
        assert_eq!(
            deleted(&script, ManualClock::at(NOW)),
            Err(DeleteFailed),
            "{failing:?}"
        );
        assert_eq!(
            script.calls(),
            [
                Call::Delete(SecretPath::Code(connection())),
                Call::Delete(SecretPath::Token(connection())),
            ]
        );
    }
}

#[test]
fn a_login_that_fails_deletes_nothing() {
    for refused in [
        SourceError::Unavailable,
        SourceError::Denied,
        SourceError::CredentialFile,
        SourceError::Sealed,
    ] {
        let script = with(Script {
            login_fails: Some(refused.clone()),
            ..api()
        });
        assert_eq!(
            deleted(&script, ManualClock::at(NOW)),
            Err(DeleteFailed),
            "{refused:?}"
        );
        assert!(script.calls().is_empty());
    }
}

/// Only the API's identity may delete codes and tokens; a session holding another is not used.
#[test]
fn a_session_that_is_not_the_api_s_deletes_nothing() {
    let script = Script::new(Identity::ConnectionManager);
    assert_eq!(deleted(&script, ManualClock::at(NOW)), Err(DeleteFailed));
    assert_eq!(script.logins(), [at(NOW)]);
    assert!(script.calls().is_empty());
}

/// A clock that cannot be read.
struct NoClock;

impl Clock for NoClock {
    fn now(&self) -> Option<UtcNanos> {
        None
    }
}

#[test]
fn a_clock_that_cannot_be_read_deletes_nothing() {
    let script = Script::new(Identity::Api);
    assert_eq!(deleted(&script, Arc::new(NoClock)), Err(DeleteFailed));
    assert!(script.logins().is_empty());
    assert!(script.calls().is_empty());
}
