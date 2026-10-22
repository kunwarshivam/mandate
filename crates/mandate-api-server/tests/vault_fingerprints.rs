//! The connect finish's fingerprint port over the vault (DEC-2348 item 4, DEC-2345): one fresh
//! login as the connection manager at the clock's time, the request's fingerprint entry read by
//! its connection and `account_ref`, its reference matched against the check's, then a claim;
//! anything else claims nothing and cannot answer. A release releases the request's claim.

mod common;
mod support_vault;

use std::sync::Arc;

use mandate_api_server::clock::Clock;
use mandate_api_server::finish::{Fingerprints, Judged, ReleaseFailed};
use mandate_api_server::vault_fingerprints::VaultFingerprints;
use mandate_connections::manager::{RequestMembers, StepUpEvidence};
use mandate_connections::record::{AccountPiiRef, AccountRef, Broker, ConnectionId, Environment};
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::fingerprint::{AccountFingerprint, Claim, FingerprintError};
use mandate_vault::policy::Identity;

use common::{ManualClock, at};
use support_vault::{Call, Login, Script};

const CONNECTION: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W50";
const HOLDER: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W51";
const ACCOUNT: &str = "01JB3K6F0C9R2V7N4M8Q1T5W60";
const PII: &str = "pii_01JB3K6F0C9R2V7N4M8Q1T5W70";
const OTHER_PII: &str = "pii_01JB3K6F0C9R2V7N4M8Q1T5W71";
const HASH: &str = "vault:v1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
const NOW: i64 = 1_791_000_000;

fn connection() -> ConnectionId {
    ConnectionId::new(CONNECTION).expect("a connection id")
}

fn account() -> AccountRef {
    AccountRef::new(ACCOUNT).expect("an account ref")
}

fn hash() -> AccountFingerprint {
    AccountFingerprint::new(HASH).expect("a fingerprint")
}

fn members() -> RequestMembers {
    RequestMembers {
        connection_id: connection(),
        account_ref: account(),
        broker: Broker::Alpaca,
        environment: Environment::Paper,
        user: "01JB3K6F0C9R2V7N4M8Q1T5W3Z".to_owned(),
        step_up: StepUpEvidence {
            assertion_id: "asrt-1".to_owned(),
            authenticated_at: at(NOW - 60),
            method: "passkey".to_owned(),
        },
    }
}

/// A manager whose request entry holds the fingerprint and `pii`.
fn entry(pii: &str) -> Script {
    Script {
        fingerprint: Some(Ok((hash(), AccountPiiRef::new(pii).expect("a reference")))),
        ..manager()
    }
}

fn manager() -> Script {
    Arc::into_inner(Script::new(Identity::ConnectionManager)).expect("one owner")
}

fn port(script: &Arc<Script>, clock: Arc<dyn Clock>) -> VaultFingerprints<Login> {
    VaultFingerprints {
        login: Arc::new(Login(Arc::clone(script))),
        clock,
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a runtime")
}

/// The port's judgement of the request for the check's `pii`, from inside an async runtime as
/// the pass asks it.
fn judged(script: Script, clock: Arc<dyn Clock>, pii: &str) -> (Judged, Arc<Script>) {
    let script = Arc::new(script);
    let request = members();
    let answer = runtime().block_on(port(&script, clock).judge(&request, pii));
    (answer, script)
}

fn released(script: Script, clock: Arc<dyn Clock>) -> (Result<(), ReleaseFailed>, Arc<Script>) {
    let script = Arc::new(script);
    let answer = runtime().block_on(port(&script, clock).release(&connection(), &account()));
    (answer, script)
}

/// A clock that cannot be read.
struct NoClock;

impl Clock for NoClock {
    fn now(&self) -> Option<UtcNanos> {
        None
    }
}

#[test]
fn a_unique_account_is_claimed_as_the_connection_manager() {
    let (answer, script) = judged(entry(PII), ManualClock::at(NOW), PII);
    assert_eq!(answer, Judged::Unique);
    assert_eq!(script.logins(), [at(NOW)], "one fresh login");
    assert_eq!(
        script.calls(),
        [
            Call::Fingerprint(connection(), account()),
            Call::Claim(connection(), hash()),
        ]
    );
}

/// Check 4: an account another connection holds is already connected, to that connection.
#[test]
fn an_account_held_by_another_connection_is_already_connected() {
    let holder = ConnectionId::new(HOLDER).expect("a connection id");
    let (answer, _) = judged(
        Script {
            claim: Some(Ok(Claim::HeldBy(holder.clone()))),
            ..entry(PII)
        },
        ManualClock::at(NOW),
        PII,
    );
    assert_eq!(answer, Judged::AlreadyConnected(holder));
}

/// DEC-2345 item 3: an entry whose reference is not the check's is not this request's account,
/// so its hash is never claimed.
#[test]
fn a_reference_that_is_not_the_check_s_cannot_answer_and_claims_nothing() {
    let (answer, script) = judged(entry(OTHER_PII), ManualClock::at(NOW), PII);
    assert_eq!(answer, Judged::CannotAnswer);
    assert_eq!(script.calls(), [Call::Fingerprint(connection(), account())]);
}

#[test]
fn an_entry_that_cannot_be_read_cannot_answer_and_claims_nothing() {
    for error in [
        FingerprintError::Rotating,
        FingerprintError::Missing,
        FingerprintError::Unavailable,
    ] {
        let (answer, script) = judged(
            Script {
                fingerprint: Some(Err(error.clone())),
                ..manager()
            },
            ManualClock::at(NOW),
            PII,
        );
        assert_eq!(answer, Judged::CannotAnswer, "{error:?}");
        assert_eq!(script.calls(), [Call::Fingerprint(connection(), account())]);
    }
}

#[test]
fn a_claim_that_fails_cannot_answer() {
    for error in [FingerprintError::Rotating, FingerprintError::Unavailable] {
        let (answer, _) = judged(
            Script {
                claim: Some(Err(error.clone())),
                ..entry(PII)
            },
            ManualClock::at(NOW),
            PII,
        );
        assert_eq!(answer, Judged::CannotAnswer, "{error:?}");
    }
}

/// No clock, a failed login, or a session that is not the connection manager's asks nothing and
/// cannot answer.
#[test]
fn a_judgement_without_the_manager_s_session_asks_nothing() {
    let (answer, script) = judged(entry(PII), Arc::new(NoClock), PII);
    assert_eq!(answer, Judged::CannotAnswer);
    assert!(script.logins().is_empty());
    let (answer, script) = judged(
        Script {
            login_fails: Some(SourceError::Denied),
            ..entry(PII)
        },
        ManualClock::at(NOW),
        PII,
    );
    assert_eq!(answer, Judged::CannotAnswer);
    assert!(script.calls().is_empty());
    let (answer, script) = judged(
        Script {
            identity: Identity::Api,
            ..entry(PII)
        },
        ManualClock::at(NOW),
        PII,
    );
    assert_eq!(answer, Judged::CannotAnswer);
    assert_eq!(script.logins(), [at(NOW)]);
    assert!(script.calls().is_empty());
}

#[test]
fn a_release_releases_the_request_s_claim_as_the_connection_manager() {
    let (answer, script) = released(manager(), ManualClock::at(NOW));
    assert_eq!(answer, Ok(()));
    assert_eq!(script.logins(), [at(NOW)], "one fresh login");
    assert_eq!(script.calls(), [Call::Release(connection(), account())]);
}

#[test]
fn a_release_that_fails_or_cannot_be_made_fails() {
    for error in [FingerprintError::Rotating, FingerprintError::Unavailable] {
        let (answer, _) = released(
            Script {
                release_fails: Some(error.clone()),
                ..manager()
            },
            ManualClock::at(NOW),
        );
        assert_eq!(answer, Err(ReleaseFailed), "{error:?}");
    }
    let (answer, script) = released(manager(), Arc::new(NoClock));
    assert_eq!(answer, Err(ReleaseFailed));
    assert!(script.logins().is_empty());
    let (answer, script) = released(
        Script {
            login_fails: Some(SourceError::Unavailable),
            ..manager()
        },
        ManualClock::at(NOW),
    );
    assert_eq!(answer, Err(ReleaseFailed));
    assert!(script.calls().is_empty());
    let (answer, script) = released(
        Script {
            identity: Identity::Api,
            ..manager()
        },
        ManualClock::at(NOW),
    );
    assert_eq!(answer, Err(ReleaseFailed));
    assert!(script.calls().is_empty());
}
