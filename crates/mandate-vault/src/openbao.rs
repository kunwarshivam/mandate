//! OpenBao as a [`SecretSource`] (DEC-434 item 14, DEC-977), over its HTTP API, over TLS only,
//! trusting only the CA certificate it is given, through no proxy and with no redirect.
//!
//! Each process logs in with its own AppRole ([`crate::policy::role_name`]) and holds the token
//! that login returns, with the one policy [`crate::policy::openbao_policy`] gives its identity.
//! The client never checks a policy itself: OpenBao refuses, so the conformance suite proves the
//! server's policies, not this code. Secrets live in the KV v2 mount of [`crate::policy::Mounts`], as
//! `{"value": <base64>}`.
//!
//! Before it logs in, the client reads `sys/seal-status`: a sealed vault is
//! [`SourceError::Sealed`], since unsealing is an operator's step (DEC-977 item 5), and a dev
//! server (in-memory storage) is [`SourceError::DevServerRefused`] unless the `dev-server`
//! feature, which only this crate's tests turn on, allows it.
//!
//! A grant is a single-use AppRole secret id for that connection's exchange role, valid
//! [`crate::source::GRANT_SECS`], whose token lives no longer.

use std::fmt;

use mandate_connections::record::ConnectionId;
use mandate_time::UtcNanos;
use secrecy::{SecretSlice, SecretString};

use crate::SourceError;
use crate::lease::Lease;
use crate::policy::{Identity, Mounts, SecretPath};
use crate::source::{Grant, SecretSource};

const STORY: &str = "E21-32";

/// Where the vault is and how to trust it.
#[derive(Clone)]
pub struct OpenBaoConfig {
    /// `https://host:port`, nothing after it.
    pub address: String,
    /// The CA certificate (PEM) the server's certificate must chain to; the only root trusted.
    pub ca_pem: Vec<u8>,
    /// Where the secrets and roles live.
    pub mounts: Mounts,
}

impl fmt::Debug for OpenBaoConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenBaoConfig")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

/// A logged-in session as one identity.
pub struct OpenBaoSource {
    identity: Identity,
    http: reqwest::Client,
    address: String,
    token: SecretString,
}

impl fmt::Debug for OpenBaoSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenBaoSource")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

impl OpenBaoSource {
    /// Logs in as `identity` with its AppRole `role_id` and `secret_id` (read by the caller from
    /// mounted files, never the environment). The token exchange logs in with
    /// [`OpenBaoSource::login_with_grant`] instead.
    pub async fn login(
        config: &OpenBaoConfig,
        identity: Identity,
        role_id: &str,
        secret_id: &SecretString,
        now: UtcNanos,
    ) -> Result<Self, SourceError> {
        let _ = (config, identity, role_id, secret_id, now);
        Err(SourceError::Unimplemented { story: STORY })
    }

    /// The token exchange's session for the grant's connection, spending the grant.
    pub async fn login_with_grant(
        config: &OpenBaoConfig,
        grant: &Grant,
        now: UtcNanos,
    ) -> Result<Self, SourceError> {
        let _ = (config, grant, now);
        Err(SourceError::Unimplemented { story: STORY })
    }
}

/// Whether a server may be used, from its `sys/seal-status` body: sealed is
/// [`SourceError::Sealed`]; in-memory storage (a dev server) is [`SourceError::DevServerRefused`]
/// unless `allow_dev`; anything unreadable is [`SourceError::Malformed`].
pub fn seal_verdict(body: &[u8], allow_dev: bool) -> Result<(), SourceError> {
    let _ = (body, allow_dev);
    Err(SourceError::Unimplemented { story: STORY })
}

/// One identity's AppRole credentials, as [`provision`] made them.
pub struct RoleCredentials {
    pub identity: Identity,
    pub role_id: String,
    pub secret_id: SecretString,
}

impl fmt::Debug for RoleCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RoleCredentials")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

/// Sets a vault up for `connections`, with an operator's token: the KV v2 mount, the AppRole
/// auth method, one policy and one role per identity, and a secret id for every identity but the
/// token exchanges, whose secret ids are grants. Running it again rewrites the policies and
/// roles, so adding a connection is running it with the longer list.
pub async fn provision(
    config: &OpenBaoConfig,
    operator_token: &SecretString,
    connections: &[ConnectionId],
) -> Result<Vec<RoleCredentials>, SourceError> {
    let _ = (config, operator_token, connections);
    Err(SourceError::Unimplemented { story: STORY })
}

impl SecretSource for OpenBaoSource {
    fn identity(&self) -> &Identity {
        &self.identity
    }

    async fn put(&self, path: &SecretPath, secret: &SecretSlice<u8>) -> Result<(), SourceError> {
        let _ = (path, secret, &self.http, &self.address, &self.token);
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
        let _ = (connection, now);
        Err(SourceError::Unimplemented { story: STORY })
    }

    async fn revoke_grant(&self, grant: &Grant) -> Result<(), SourceError> {
        let _ = grant;
        Err(SourceError::Unimplemented { story: STORY })
    }
}
