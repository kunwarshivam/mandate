//! A scripted vault login for the API's vault port (DEC-2338 item 3): each login is recorded with
//! its time, and the session it opens records every call made on it, so a test sees that the port
//! made one `put` and nothing else.

#![allow(dead_code, reason = "each test file uses part of the shared helpers")]

use std::sync::{Arc, Mutex};

use mandate_api_server::vault_codes::ApiLogin;
use mandate_api_server::vault_fingerprints::ManagerSession;
use mandate_connections::record::{AccountPiiRef, AccountRef, ConnectionId};
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::fingerprint::{AccountFingerprint, Claim, FingerprintError};
use mandate_vault::lease::Lease;
use mandate_vault::policy::{Identity, SecretPath};
use mandate_vault::source::{Grant, SecretSource};
use secrecy::{ExposeSecret, SecretSlice};

/// One call a session saw: a `put`'s path and bytes, a `delete`'s path, or another method's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Put(SecretPath, Vec<u8>),
    Delete(SecretPath),
    Fingerprint(ConnectionId, AccountRef),
    Claim(ConnectionId, AccountFingerprint),
    Release(ConnectionId, AccountRef),
    Other(&'static str),
}

/// What the login and its sessions saw, and how they answer.
#[derive(Debug)]
pub struct Script {
    /// The identity the session opened holds.
    pub identity: Identity,
    /// The login fails with this.
    pub login_fails: Option<SourceError>,
    /// Every `put` fails with this.
    pub put_fails: Option<SourceError>,
    /// A `delete` of this path fails as unavailable.
    pub delete_fails_on: Option<SecretPath>,
    /// What a `fingerprint` answers; `Missing` when unset.
    pub fingerprint: Option<Result<(AccountFingerprint, AccountPiiRef), FingerprintError>>,
    /// What a `claim` answers; `Claimed` when unset.
    pub claim: Option<Result<Claim, FingerprintError>>,
    /// A `release` fails with this.
    pub release_fails: Option<FingerprintError>,
    pub logins: Mutex<Vec<UtcNanos>>,
    pub calls: Mutex<Vec<Call>>,
}

impl Script {
    pub fn new(identity: Identity) -> Arc<Self> {
        Arc::new(Self {
            identity,
            login_fails: None,
            put_fails: None,
            delete_fails_on: None,
            fingerprint: None,
            claim: None,
            release_fails: None,
            logins: Mutex::default(),
            calls: Mutex::default(),
        })
    }

    pub fn logins(&self) -> Vec<UtcNanos> {
        self.logins.lock().expect("unpoisoned").clone()
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().expect("unpoisoned").clone()
    }
}

/// The scripted login.
pub struct Login(pub Arc<Script>);

impl ApiLogin for Login {
    type Session = Session;

    async fn login(&self, now: UtcNanos) -> Result<Session, SourceError> {
        self.0.logins.lock().expect("unpoisoned").push(now);
        match &self.0.login_fails {
            Some(refused) => Err(refused.clone()),
            None => Ok(Session(Arc::clone(&self.0))),
        }
    }
}

/// A session that records each call and refuses all but `put` and `delete`.
pub struct Session(pub Arc<Script>);

impl Session {
    fn other<T>(&self, name: &'static str) -> Result<T, SourceError> {
        self.0
            .calls
            .lock()
            .expect("unpoisoned")
            .push(Call::Other(name));
        Err(SourceError::Denied)
    }
}

impl SecretSource for Session {
    fn identity(&self) -> &Identity {
        &self.0.identity
    }

    async fn put(&self, path: &SecretPath, secret: &SecretSlice<u8>) -> Result<(), SourceError> {
        self.0
            .calls
            .lock()
            .expect("unpoisoned")
            .push(Call::Put(path.clone(), secret.expose_secret().to_vec()));
        match &self.0.put_fails {
            Some(refused) => Err(refused.clone()),
            None => Ok(()),
        }
    }

    async fn read(&self, _: &SecretPath) -> Result<Option<SecretSlice<u8>>, SourceError> {
        self.other("read")
    }

    async fn delete(&self, path: &SecretPath) -> Result<(), SourceError> {
        self.0
            .calls
            .lock()
            .expect("unpoisoned")
            .push(Call::Delete(path.clone()));
        if self.0.delete_fails_on.as_ref() == Some(path) {
            Err(SourceError::Unavailable)
        } else {
            Ok(())
        }
    }

    async fn lease(&self, _: &ConnectionId, _: UtcNanos) -> Result<Lease, SourceError> {
        self.other("lease")
    }

    async fn renew(&self, _: &Lease, _: UtcNanos) -> Result<Lease, SourceError> {
        self.other("renew")
    }

    async fn revoke(&self, _: Lease) -> Result<(), SourceError> {
        self.other("revoke")
    }

    async fn issue_grant(&self, _: &ConnectionId, _: UtcNanos) -> Result<Grant, SourceError> {
        self.other("issue_grant")
    }

    async fn revoke_grant(&self, _: &Grant) -> Result<(), SourceError> {
        self.other("revoke_grant")
    }
}

impl Session {
    fn record(&self, call: Call) {
        self.0.calls.lock().expect("unpoisoned").push(call);
    }
}

impl ManagerSession for Session {
    async fn fingerprint(
        &self,
        connection: &ConnectionId,
        account: &AccountRef,
    ) -> Result<(AccountFingerprint, AccountPiiRef), FingerprintError> {
        self.record(Call::Fingerprint(connection.clone(), account.clone()));
        self.0
            .fingerprint
            .clone()
            .unwrap_or(Err(FingerprintError::Missing))
    }

    async fn claim(
        &self,
        connection: &ConnectionId,
        fingerprint: &AccountFingerprint,
    ) -> Result<Claim, FingerprintError> {
        self.record(Call::Claim(connection.clone(), fingerprint.clone()));
        self.0.claim.clone().unwrap_or(Ok(Claim::Claimed))
    }

    async fn release(
        &self,
        connection: &ConnectionId,
        account: &AccountRef,
    ) -> Result<(), FingerprintError> {
        self.record(Call::Release(connection.clone(), account.clone()));
        self.0.release_fails.clone().map_or(Ok(()), Err)
    }
}
