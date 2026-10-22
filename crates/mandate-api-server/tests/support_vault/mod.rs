//! A scripted vault login for the API's vault port (DEC-2338 item 3): each login is recorded with
//! its time, and the session it opens records every call made on it, so a test sees that the port
//! made one `put` and nothing else.

#![allow(dead_code, reason = "each test file uses part of the shared helpers")]

use std::sync::{Arc, Mutex};

use mandate_api_server::vault_codes::ApiLogin;
use mandate_connections::record::ConnectionId;
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::lease::Lease;
use mandate_vault::policy::{Identity, SecretPath};
use mandate_vault::source::{Grant, SecretSource};
use secrecy::{ExposeSecret, SecretSlice};

/// One call a session saw: a `put`'s path and bytes, or another method's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Put(SecretPath, Vec<u8>),
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
    pub logins: Mutex<Vec<UtcNanos>>,
    pub calls: Mutex<Vec<Call>>,
}

impl Script {
    pub fn new(identity: Identity) -> Arc<Self> {
        Arc::new(Self {
            identity,
            login_fails: None,
            put_fails: None,
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

/// A session that records each call and refuses all but `put`.
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

    async fn delete(&self, _: &SecretPath) -> Result<(), SourceError> {
        self.other("delete")
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
