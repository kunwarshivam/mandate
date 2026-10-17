//! The local vault as a [`SecretSource`] (DEC-692, DEC-976, DEC-977 item 3). The filesystem and
//! the two Unix users enforce what each process can open; this backend also checks
//! [`crate::policy::allows`] in code before it touches a file, so it refuses exactly what OpenBao
//! refuses.
//!
//! Paths map onto entries of one environment: a connection's code and token under the `key_ref`
//! `connections/<connection_id>`, and the client secret under
//! [`mandate_vault_local::entry::ALPACA_CLIENT_SECRET_KEY_REF`].
//!
//! Grants live in a [`GrantBook`] in memory: the connection manager and the token exchange that
//! uses its grant must share one process on the demo host, where the exchange runs as
//! `owlhead_exec` (DEC-692 item 6 enforces the grant in code there).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use mandate_connections::record::{ConnectionId, Environment};
use mandate_time::UtcNanos;
use mandate_vault_local::store::Opened;
use secrecy::SecretSlice;

use crate::SourceError;
use crate::lease::Lease;
use crate::policy::{Identity, SecretPath};
use crate::source::{Grant, SecretSource};

const STORY: &str = "E21-32";

/// A grant the book issued: its connection, its expiry, and whether it was used.
#[derive(Debug, Clone)]
struct IssuedGrant {
    connection: ConnectionId,
    expires_at: UtcNanos,
    used: bool,
}

/// The grants one process issued, by accessor.
#[derive(Debug, Default)]
pub struct GrantBook {
    issued: Mutex<BTreeMap<String, (IssuedGrant, [u8; 32])>>,
}

/// One process's session with the local vault.
#[derive(Debug)]
pub struct LocalSource {
    identity: Identity,
    vault: Option<Opened>,
    environment: Environment,
    grants: Arc<GrantBook>,
}

impl LocalSource {
    /// A session as `identity` over `vault`, which must be in the role the identity runs as:
    /// the API's for [`Identity::Api`] and [`Identity::ConnectionManager`], the executor's for
    /// the token exchange, an executor, and the secret import, and none for a runtime. Any other
    /// pairing is [`SourceError::Config`]. The token exchange is opened with
    /// [`LocalSource::with_grant`], never here.
    pub fn new(
        identity: Identity,
        vault: Option<Opened>,
        environment: Environment,
        grants: Arc<GrantBook>,
    ) -> Result<Self, SourceError> {
        let _ = (identity, vault, environment, grants);
        Err(SourceError::Unimplemented { story: STORY })
    }

    /// The token exchange's session, through a grant this process's book issued: unused, not
    /// expired at `now`, for its connection. The grant is spent by this call.
    pub fn with_grant(
        grant: &Grant,
        vault: Opened,
        environment: Environment,
        grants: Arc<GrantBook>,
        now: UtcNanos,
    ) -> Result<Self, SourceError> {
        let _ = (grant, vault, environment, grants, now);
        Err(SourceError::Unimplemented { story: STORY })
    }
}

impl SecretSource for LocalSource {
    fn identity(&self) -> &Identity {
        &self.identity
    }

    async fn put(&self, path: &SecretPath, secret: &SecretSlice<u8>) -> Result<(), SourceError> {
        let _ = (path, secret, &self.vault, &self.environment, &self.grants);
        Err(SourceError::Unimplemented { story: STORY })
    }

    async fn read(&self, path: &SecretPath) -> Result<Option<SecretSlice<u8>>, SourceError> {
        let _ = path;
        Err(SourceError::Unimplemented { story: STORY })
    }

    async fn delete(&self, path: &SecretPath) -> Result<(), SourceError> {
        let _ = path;
        Err(SourceError::Unimplemented { story: STORY })
    }

    async fn lease(&self, connection: &ConnectionId, now: UtcNanos) -> Result<Lease, SourceError> {
        let _ = (connection, now);
        Err(SourceError::Unimplemented { story: STORY })
    }

    async fn renew(&self, lease: &Lease, now: UtcNanos) -> Result<Lease, SourceError> {
        let _ = (lease, now);
        Err(SourceError::Unimplemented { story: STORY })
    }

    async fn revoke(&self, lease: Lease) -> Result<(), SourceError> {
        let _ = lease;
        Err(SourceError::Unimplemented { story: STORY })
    }

    async fn issue_grant(
        &self,
        connection: &ConnectionId,
        now: UtcNanos,
    ) -> Result<Grant, SourceError> {
        let _ = (connection, now, IssuedGrant::clone);
        Err(SourceError::Unimplemented { story: STORY })
    }

    async fn revoke_grant(&self, grant: &Grant) -> Result<(), SourceError> {
        let _ = grant;
        Err(SourceError::Unimplemented { story: STORY })
    }
}
