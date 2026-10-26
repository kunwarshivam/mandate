//! The interface a process holds with the vault (DEC-434 item 14, infrastructure §5.6).

use std::fmt;

use mandate_connections::account_entry::BrokerAccountId;
use mandate_connections::record::ConnectionId;
use mandate_time::UtcNanos;
use secrecy::{SecretSlice, SecretString};

use crate::SourceError;
use crate::fingerprint::AccountFingerprint;
use crate::lease::Lease;
use crate::policy::{Identity, SecretPath};

/// How long a token-exchange grant lasts: the code's ten minutes (infrastructure §5.2).
pub const GRANT_SECS: i64 = 600;

/// A single-use grant for one connection's token exchange, issued by the connection manager and
/// handed to the token-exchange process it starts (infrastructure §5.2, DEC-821 item 2).
pub struct Grant {
    pub connection: ConnectionId,
    /// Not a secret: which role the grant logs in as.
    pub role_id: String,
    /// The single-use credential the grant is.
    pub secret_id: SecretString,
    /// Names the grant so it can be revoked, without being able to use it.
    pub accessor: String,
    pub expires_at: UtcNanos,
}

impl fmt::Debug for Grant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Grant")
            .field("connection", &self.connection)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// A session with the vault as one [`Identity`]. Every call is checked against
/// [`crate::policy::allows`]: by the local backend in code, by OpenBao by its policies.
#[allow(
    async_fn_in_trait,
    reason = "every caller is in this workspace and awaits on its own runtime; no Send bound is needed"
)]
pub trait SecretSource {
    /// The identity this session holds.
    fn identity(&self) -> &Identity;

    /// Writes `secret` at `path`, replacing any earlier value. Writing is never reading: the
    /// writer cannot read it back.
    async fn put(&self, path: &SecretPath, secret: &SecretSlice<u8>) -> Result<(), SourceError>;

    /// Reads `path` once; `None` when it is absent. Only the token exchange reads, its code and
    /// the client secret; an executor reads its token through [`SecretSource::lease`].
    async fn read(&self, path: &SecretPath) -> Result<Option<SecretSlice<u8>>, SourceError>;

    /// Deletes `path`, every version of it; an absent one is already deleted.
    async fn delete(&self, path: &SecretPath) -> Result<(), SourceError>;

    /// The executor's lease on its connection's token, taken at `now`.
    async fn lease(&self, connection: &ConnectionId, now: UtcNanos) -> Result<Lease, SourceError>;

    /// Renews `lease` at `now`, reading the token again, so a rotated token is picked up.
    async fn renew(&self, lease: &Lease, now: UtcNanos) -> Result<Lease, SourceError>;

    /// Ends `lease` on stop. The session can take no lease after it.
    async fn revoke(&self, lease: Lease) -> Result<(), SourceError>;

    /// Issues a single-use token-exchange grant for `connection`, valid [`GRANT_SECS`] from `now`.
    async fn issue_grant(
        &self,
        connection: &ConnectionId,
        now: UtcNanos,
    ) -> Result<Grant, SourceError>;

    /// Revokes a grant, whether or not it was used.
    async fn revoke_grant(&self, grant: &Grant) -> Result<(), SourceError>;

    /// Takes `connection`'s code once (DEC-2061): of any number of overlapping takers exactly one
    /// gets it, and every other gets `None`, as does a taker that finds no code or a taken one; no
    /// taker ever leaves the code readable. Only that connection's token exchange takes. A source
    /// that holds no codes refuses, as [`SourceError::Denied`].
    async fn take_code(
        &self,
        connection: &ConnectionId,
    ) -> Result<Option<SecretSlice<u8>>, SourceError> {
        let _ = connection;
        Err(SourceError::Denied)
    }

    /// Writes `secret` at `path` only where nothing is there, as [`crate::policy::Access::Create`] allows: an
    /// entry already there is [`SourceError::Denied`], and is left as it is. A source that holds
    /// no personal data refuses, as [`SourceError::Denied`] (DEC-2339).
    async fn create(&self, path: &SecretPath, secret: &SecretSlice<u8>) -> Result<(), SourceError> {
        let _ = (path, secret);
        Err(SourceError::Denied)
    }

    /// The fingerprint of the broker account id `account` under the deployment's key, by the
    /// vault's keyed hash: asked by a connection's executor alone, at its connect (DEC-2345 item
    /// 2). The key never leaves the vault, and the id is sent only to it. A source that holds no
    /// key refuses, as [`SourceError::Denied`].
    async fn hmac_account(
        &self,
        account: &BrokerAccountId,
    ) -> Result<AccountFingerprint, SourceError> {
        let _ = account;
        Err(SourceError::Denied)
    }
}
