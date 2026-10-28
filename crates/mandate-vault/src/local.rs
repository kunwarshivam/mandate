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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use mandate_connections::record::{ConnectionId, Environment};
use mandate_time::UtcNanos;
use mandate_vault_local::VaultError;
use mandate_vault_local::entry::{ALPACA_CLIENT_SECRET_KEY_REF, Entry, KeyRef};
use mandate_vault_local::store::{ApiVault, ExecutorVault, Opened};
use ring::rand::{SecureRandom, SystemRandom};
use secrecy::{ExposeSecret, SecretSlice, SecretString};

use crate::SourceError;
use crate::lease::{Lease, lease_times};
use crate::policy::{Access, Identity, SecretPath, kv_path, require};
use crate::source::{GRANT_SECS, Grant, SecretSource};

/// The role name every local grant carries: the local backend has no roles.
const LOCAL_ROLE: &str = "local";

/// A grant the book issued: its connection, its expiry, its secret, and whether it was used.
struct IssuedGrant {
    connection: ConnectionId,
    expires_at: UtcNanos,
    secret_id: SecretString,
    used: bool,
}

/// The grants one process issued, by accessor.
#[derive(Default)]
pub struct GrantBook {
    issued: Mutex<BTreeMap<String, IssuedGrant>>,
}

impl std::fmt::Debug for GrantBook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GrantBook(redacted)")
    }
}

/// The vault a session holds, in the role its identity runs as.
#[derive(Debug)]
enum Held {
    Api(ApiVault),
    Executor(ExecutorVault),
    Nothing,
}

/// One process's session with the local vault.
#[derive(Debug)]
pub struct LocalSource {
    identity: Identity,
    held: Held,
    environment: Environment,
    grants: Arc<GrantBook>,
    revoked: AtomicBool,
}

impl LocalSource {
    /// A session as `identity` over `vault`, which must be in the role the identity runs as:
    /// the API's for [`Identity::Api`] and [`Identity::ConnectionManager`], the executor's for
    /// an executor, the secret import, and the readers of the platform's own secrets (the model
    /// gateway, the news reader and the email sender, DEC-2020 item 4, DEC-2277), and none for a
    /// runtime. On the demo host the filesystem does not separate those readers from the
    /// executor's entries; the table [`crate::policy::allows`] checks before every read does. Any
    /// other pairing, and the token exchange, which is opened with [`LocalSource::with_grant`], is
    /// [`SourceError::Config`].
    pub fn new(
        identity: Identity,
        vault: Option<Opened>,
        environment: Environment,
        grants: Arc<GrantBook>,
    ) -> Result<Self, SourceError> {
        let held = match (&identity, vault) {
            (Identity::Api | Identity::ConnectionManager, Some(Opened::Api(v))) => Held::Api(v),
            (
                Identity::Executor(_)
                | Identity::SecretImport
                | Identity::ModelGateway
                | Identity::NewsReader
                | Identity::BarsReader
                | Identity::EmailSender,
                Some(Opened::Executor(v)),
            ) => Held::Executor(v),
            (Identity::Runtime, None) => Held::Nothing,
            _ => return Err(SourceError::Config),
        };
        Ok(Self {
            identity,
            held,
            environment,
            grants,
            revoked: AtomicBool::new(false),
        })
    }

    /// The token exchange's session, through a grant this process's book issued: unused, not
    /// expired at `now`, for its connection, with its secret id. The grant is spent by this
    /// call; a refused one is not.
    pub fn with_grant(
        grant: &Grant,
        vault: Opened,
        environment: Environment,
        grants: Arc<GrantBook>,
        now: UtcNanos,
    ) -> Result<Self, SourceError> {
        let Opened::Executor(vault) = vault else {
            return Err(SourceError::Config);
        };
        {
            let mut book = grants.issued.lock().map_err(|_| SourceError::Unavailable)?;
            let issued = book
                .get_mut(&grant.accessor)
                .ok_or(SourceError::GrantInvalid)?;
            let valid = !issued.used
                && now < issued.expires_at
                && issued.connection == grant.connection
                && issued.secret_id.expose_secret() == grant.secret_id.expose_secret();
            if !valid {
                return Err(SourceError::GrantInvalid);
            }
            issued.used = true;
        }
        Ok(Self {
            identity: Identity::TokenExchange(grant.connection.clone()),
            held: Held::Executor(vault),
            environment,
            grants,
            revoked: AtomicBool::new(false),
        })
    }

    /// The local entry for `path`.
    fn entry(&self, path: &SecretPath) -> Result<Entry, SourceError> {
        let key_ref = |c: &ConnectionId| {
            KeyRef::new(&format!("connections/{}", c.as_str())).map_err(|_| SourceError::Config)
        };
        Ok(match path {
            SecretPath::Code(c) => Entry::code(key_ref(c)?, c.clone(), self.environment),
            SecretPath::Token(c) => Entry::token(key_ref(c)?, c.clone(), self.environment),
            SecretPath::ClientSecret => Entry::client_secret(
                KeyRef::new(ALPACA_CLIENT_SECRET_KEY_REF).map_err(|_| SourceError::Config)?,
            ),
            SecretPath::Platform(_) => {
                Entry::client_secret(KeyRef::new(&kv_path(path)?).map_err(|_| SourceError::Config)?)
            }
            SecretPath::AccountId(..)
            | SecretPath::Fingerprint(..)
            | SecretPath::Claims
            | SecretPath::Rotating => return Err(SourceError::Config),
        })
    }

    fn executor(&self) -> Result<&ExecutorVault, SourceError> {
        match &self.held {
            Held::Executor(v) => Ok(v),
            Held::Api(_) | Held::Nothing => Err(SourceError::Denied),
        }
    }

    /// Reads `path` after the policy check, through the executor's file checks.
    fn read_entry(&self, path: &SecretPath) -> Result<Option<SecretSlice<u8>>, SourceError> {
        require(&self.identity, Access::Read, path)?;
        let entry = self.entry(path)?;
        self.executor()?.read(&entry).map_err(from_vault)
    }
}

/// A local vault refusal as a source refusal: a failure to reach the files is an outage, an
/// entry or directory that fails its checks is an integrity refusal, and the rest are
/// refusals. A role the vault refuses never reaches here: the policy check runs first.
fn from_vault(error: VaultError) -> SourceError {
    match error {
        VaultError::Io => SourceError::Unavailable,
        VaultError::FileMismatch { .. }
        | VaultError::DirectoryMismatch { .. }
        | VaultError::EntryMalformed
        | VaultError::EntryRejected => SourceError::Integrity,
        _ => SourceError::Malformed,
    }
}

impl SecretSource for LocalSource {
    fn identity(&self) -> &Identity {
        &self.identity
    }

    /// Takes the code through the local vault's own take (DEC-2628 item 2), after the policy
    /// check.
    async fn take_code(
        &self,
        connection: &ConnectionId,
    ) -> Result<Option<SecretSlice<u8>>, SourceError> {
        let path = SecretPath::Code(connection.clone());
        require(&self.identity, Access::Take, &path)?;
        let entry = self.entry(&path)?;
        self.executor()?.take_code(&entry).map_err(from_vault)
    }

    async fn put(&self, path: &SecretPath, secret: &SecretSlice<u8>) -> Result<(), SourceError> {
        require(&self.identity, Access::Write, path)?;
        let entry = self.entry(path)?;
        match &self.held {
            Held::Api(v) => v.put_code(&entry, secret),
            Held::Executor(v) => v.put(&entry, secret),
            Held::Nothing => return Err(SourceError::Denied),
        }
        .map_err(from_vault)
    }

    async fn read(&self, path: &SecretPath) -> Result<Option<SecretSlice<u8>>, SourceError> {
        self.read_entry(path)
    }

    async fn delete(&self, path: &SecretPath) -> Result<(), SourceError> {
        require(&self.identity, Access::Delete, path)?;
        let entry = self.entry(path)?;
        match &self.held {
            Held::Api(v) => v.delete(&entry),
            Held::Executor(v) => v.delete(&entry),
            Held::Nothing => return Err(SourceError::Denied),
        }
        .map_err(from_vault)
    }

    async fn lease(&self, connection: &ConnectionId, now: UtcNanos) -> Result<Lease, SourceError> {
        if self.revoked.load(Ordering::SeqCst) {
            return Err(SourceError::Denied);
        }
        let secret = self
            .read_entry(&SecretPath::Token(connection.clone()))?
            .ok_or(SourceError::Missing)?;
        let (renew_at, expires_at) = lease_times(now, None).ok_or(SourceError::Config)?;
        Ok(Lease {
            connection: connection.clone(),
            secret,
            expires_at,
            renew_at,
        })
    }

    async fn renew(&self, lease: &Lease, now: UtcNanos) -> Result<Lease, SourceError> {
        self.lease(&lease.connection, now).await
    }

    async fn revoke(&self, lease: Lease) -> Result<(), SourceError> {
        drop(lease);
        self.revoked.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn issue_grant(
        &self,
        connection: &ConnectionId,
        now: UtcNanos,
    ) -> Result<Grant, SourceError> {
        if self.identity != Identity::ConnectionManager {
            return Err(SourceError::Denied);
        }
        let expires_at = now
            .secs()
            .checked_add(GRANT_SECS)
            .and_then(|s| UtcNanos::from_parts(s, now.nanos()).ok())
            .ok_or(SourceError::Config)?;
        let random = SystemRandom::new();
        let mut accessor = [0_u8; 16];
        let mut secret = [0_u8; 32];
        random
            .fill(&mut accessor)
            .and_then(|()| random.fill(&mut secret))
            .map_err(|_| SourceError::Unavailable)?;
        let accessor = hex(&accessor);
        let secret_id = SecretString::from(hex(&secret));
        let mut book = self
            .grants
            .issued
            .lock()
            .map_err(|_| SourceError::Unavailable)?;
        book.insert(
            accessor.clone(),
            IssuedGrant {
                connection: connection.clone(),
                expires_at,
                secret_id: secret_id.clone(),
                used: false,
            },
        );
        Ok(Grant {
            connection: connection.clone(),
            role_id: LOCAL_ROLE.to_owned(),
            secret_id,
            accessor,
            expires_at,
        })
    }

    async fn revoke_grant(&self, grant: &Grant) -> Result<(), SourceError> {
        if self.identity != Identity::ConnectionManager {
            return Err(SourceError::Denied);
        }
        self.grants
            .issued
            .lock()
            .map_err(|_| SourceError::Unavailable)?
            .remove(&grant.accessor);
        Ok(())
    }
}

/// Lowercase hex of `bytes`.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
