//! The connection record and the account fingerprint (connections spec §3, §3.1; CN-1, CN-5,
//! CN-12; E7-11). **The connection manager, in the API process**, keeps the deployment's records
//! in a [`Registry`].
//!
//! A record is a reference, never a credential: it names the account by an opaque `account_ref`,
//! its personal-data reference, and a keyed fingerprint the vault computes. The fingerprint
//! detects a second connection to an account already connected; it is never journaled, never
//! returned ([`ConnectionView`], [`EstablishedMembers`]), and never printed.

use std::collections::BTreeSet;
use std::fmt;

use crate::ConnectError;

/// The connection's environment, for life (CN-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Environment {
    Paper,
    Live,
}

/// The broker or venue (connections spec §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Broker {
    Alpaca,
    Robinhood,
    KrakenDerivativesUs,
}

/// How the credential was obtained (connections spec §3, `auth_kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AuthKind {
    ApiKey,
    Oauth,
    McpOauth,
}

/// The states a record can hold (connections spec §9.1). `connecting` keeps no record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConnectionState {
    Active,
    Degraded,
    Suspended,
    Revoked,
}

/// The opaque id mandates name a connection by: `[A-Za-z0-9_-]`, 1 to 64 characters (journal
/// spec §2's identifier grammar).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnectionId(pub(crate) String);

impl ConnectionId {
    pub fn new(text: &str) -> Result<Self, ConnectError> {
        let _ = text;
        Err(ConnectError::Unimplemented { story: "E7-11" })
    }
}

/// The account stream's reference, a ULID (journal spec §2): 26 characters of Crockford's
/// alphabet, the first at most `7`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccountRef(pub(crate) String);

impl AccountRef {
    pub fn new(text: &str) -> Result<Self, ConnectError> {
        let _ = text;
        Err(ConnectError::Unimplemented { story: "E7-11" })
    }
}

/// The personal-data vault reference of the broker account id (journal spec §6.4): an opaque
/// id, never the account id itself.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccountPiiRef(pub(crate) String);

impl AccountPiiRef {
    pub fn new(text: &str) -> Result<Self, ConnectError> {
        let _ = text;
        Err(ConnectError::Unimplemented { story: "E7-11" })
    }
}

/// The keyed hash of (broker, broker account id) the vault computes and returns (connections
/// spec §3.1). It has no `Display` and its bytes stay in this crate; `Debug` prints no byte.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccountFingerprint([u8; 32]);

impl AccountFingerprint {
    pub fn from_vault_hash(hash: [u8; 32]) -> Self {
        Self(hash)
    }
}

impl fmt::Debug for AccountFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountFingerprint(redacted)")
    }
}

/// What a new record is made from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRecord {
    pub connection_id: ConnectionId,
    pub broker: Broker,
    pub environment: Environment,
    pub auth_kind: AuthKind,
    pub scopes: BTreeSet<String>,
    pub account_ref: AccountRef,
    pub account_pii_ref: AccountPiiRef,
    pub fingerprint: AccountFingerprint,
    pub contract_hash: Option<String>,
    pub terms_version: Option<String>,
}

/// One connection's record (connections spec §3). Its members are private: the fingerprint and
/// the personal-data reference leave it only for the uniqueness check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionRecord {
    connection_id: ConnectionId,
    broker: Broker,
    environment: Environment,
    auth_kind: AuthKind,
    scopes: BTreeSet<String>,
    account_ref: AccountRef,
    account_pii_ref: AccountPiiRef,
    fingerprint: AccountFingerprint,
    state: ConnectionState,
    contract_hash: Option<String>,
    terms_version: Option<String>,
}

impl ConnectionRecord {
    /// A new record, `Active`. Refused when it breaks connections spec §3 to §6:
    /// - an Alpaca API key that is not paper (`EnvironmentRefused`; DEC-441 item 3);
    /// - an `auth_kind` the broker does not use, no scopes, or a `contract_hash` present exactly
    ///   when the credential is not MCP (`InvalidRecord`).
    pub fn new(new: NewRecord) -> Result<Self, ConnectError> {
        let _ = new;
        Err(ConnectError::Unimplemented { story: "E7-11" })
    }

    /// What an API response or the CLI may show.
    pub fn view(&self) -> Result<ConnectionView, ConnectError> {
        Err(ConnectError::Unimplemented { story: "E7-11" })
    }

    /// The members `ConnectionEstablished` version 2 takes from the record (journal spec §9.12);
    /// the caller adds the user and the step-up.
    pub fn established_members(&self) -> Result<EstablishedMembers, ConnectError> {
        Err(ConnectError::Unimplemented { story: "E7-11" })
    }
}

/// A record as an API response or the CLI shows it (workspace API §4.5, API-11): references only,
/// never the fingerprint or the personal-data reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionView {
    pub connection_id: ConnectionId,
    pub broker: Broker,
    pub environment: Environment,
    pub auth_kind: AuthKind,
    pub scopes: BTreeSet<String>,
    pub state: ConnectionState,
}

/// `ConnectionEstablished` version 2's members from the record (journal spec §9.12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EstablishedMembers {
    pub connection_id: ConnectionId,
    pub broker: Broker,
    pub environment: Environment,
    pub scopes: BTreeSet<String>,
    pub account_ref: AccountRef,
}

/// An account a connect reached, as the uniqueness check sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub broker: Broker,
    pub environment: Environment,
    pub fingerprint: AccountFingerprint,
}

/// What the uniqueness check decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    /// No record holds the account: a new connection, with a new id and `account_ref`.
    New,
    /// A revoked record holds it: the same connection again (CN-12), keeping its id, its
    /// `account_ref`, and so its account stream and loss carry.
    Reconnect {
        connection_id: ConnectionId,
        account_ref: AccountRef,
    },
}

/// The deployment's connection records (CN-5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registry {
    pub(crate) records: Vec<ConnectionRecord>,
    fingerprint_key_rotating: bool,
}

impl Registry {
    /// `fingerprint_key_rotating` is true while every fingerprint is being recomputed under a
    /// new key (connections spec §3.1).
    pub fn new(records: Vec<ConnectionRecord>, fingerprint_key_rotating: bool) -> Self {
        Self {
            records,
            fingerprint_key_rotating,
        }
    }

    /// Check 4 (connections spec §8.1): refuses an account a record that is not revoked holds
    /// (`AlreadyConnected`, naming that record's id only), reuses a revoked record of the same
    /// broker and environment (`Reconnect`), refuses one of another (`ReconnectMismatch`), and
    /// refuses every connect while the key rotates (`FingerprintRotating`).
    pub fn admit(&self, candidate: &Candidate) -> Result<Admission, ConnectError> {
        let _ = candidate;
        Err(ConnectError::Unimplemented { story: "E7-11" })
    }

    /// Adds a record, or replaces the revoked record a reconnect reuses, re-checking what
    /// [`Registry::admit`] checks; a record whose id another holds is `InvalidConnectionId`.
    pub fn insert(&mut self, record: ConnectionRecord) -> Result<(), ConnectError> {
        let _ = record;
        Err(ConnectError::Unimplemented { story: "E7-11" })
    }

    /// Moves a record to `state`; `Revoked` frees its account for a reconnect only.
    pub fn set_state(
        &mut self,
        connection_id: &ConnectionId,
        state: ConnectionState,
    ) -> Result<(), ConnectError> {
        let _ = (connection_id, state);
        Err(ConnectError::Unimplemented { story: "E7-11" })
    }

    /// The view of one record, if the deployment has it.
    pub fn view(
        &self,
        connection_id: &ConnectionId,
    ) -> Result<Option<ConnectionView>, ConnectError> {
        let _ = connection_id;
        Err(ConnectError::Unimplemented { story: "E7-11" })
    }
}
