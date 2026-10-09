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

impl Environment {
    /// As the journal and a mandate write it (`paper`, `live`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Live => "live",
        }
    }
}

/// The broker or venue (connections spec §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Broker {
    Alpaca,
    Robinhood,
    KrakenDerivativesUs,
}

impl Broker {
    /// The broker's name as the journal and the CLI write it (journal spec §9.2 `broker`).
    pub fn code(self) -> &'static str {
        match self {
            Self::Alpaca => "alpaca",
            Self::Robinhood => "robinhood",
            Self::KrakenDerivativesUs => "kraken_derivatives_us",
        }
    }

    /// The inverse of [`Broker::code`]: exactly those names, nothing near them
    /// (`InvalidRecord { member: "broker" }`).
    pub fn from_code(code: &str) -> Result<Self, ConnectError> {
        let _ = code;
        Err(ConnectError::Unimplemented { story: "E7-11" })
    }
}

/// How the credential was obtained (connections spec §3, `auth_kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AuthKind {
    ApiKey,
    Oauth,
    McpOauth,
}

impl AuthKind {
    /// As connections spec §3 writes it.
    pub fn code(self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::Oauth => "oauth",
            Self::McpOauth => "mcp_oauth",
        }
    }
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
    pub fn as_str(&self) -> &str {
        &self.0
    }

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
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn new(text: &str) -> Result<Self, ConnectError> {
        let _ = text;
        Err(ConnectError::Unimplemented { story: "E7-11" })
    }
}

/// The personal-data vault reference of the broker account id (journal spec §6.4): an opaque
/// id, never the account id itself. `Debug` prints none of it, so no record, candidate, or error
/// chain that holds one shows it; the journal writer reads it through [`AccountPiiRef::as_str`].
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccountPiiRef(pub(crate) String);

impl fmt::Debug for AccountPiiRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountPiiRef(redacted)")
    }
}

impl AccountPiiRef {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn new(text: &str) -> Result<Self, ConnectError> {
        let _ = text;
        Err(ConnectError::Unimplemented { story: "E7-11" })
    }
}

/// The keyed hash of (broker, broker account id) the vault computes and returns (connections
/// spec §3.1). Its bytes stay in this crate: only the crate's vault boundary makes one, it is
/// compared for equality and nothing else, `Debug` prints no byte, and it has no `Display`. The
/// crate may depend on `thiserror` alone (`xtask/layers.toml`), so it cannot derive `serde`.
///
/// ```compile_fail
/// let _ = mandate_connections::record::AccountFingerprint::from_vault_hash([0; 32]);
/// ```
///
/// ```compile_fail
/// fn shown(fingerprint: &mandate_connections::record::AccountFingerprint) -> String {
///     format!("{fingerprint}")
/// }
/// ```
///
/// ```compile_fail
/// fn ordered(a: &mandate_connections::record::AccountFingerprint, b: &mandate_connections::record::AccountFingerprint) -> bool {
///     a < b
/// }
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct AccountFingerprint([u8; 32]);

impl AccountFingerprint {
    #[allow(
        dead_code,
        reason = "the vault boundary (V1, DEC-692) is its first caller outside the tests"
    )]
    pub(crate) fn from_vault_hash(hash: [u8; 32]) -> Self {
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

/// One connection's record (connections spec §3). Its members are private: the fingerprint leaves
/// it only for the uniqueness check, inside this crate, and the personal-data reference never
/// leaves it but through the journal writer's [`AccountPiiRef::as_str`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionRecord {
    pub(crate) connection_id: ConnectionId,
    pub(crate) broker: Broker,
    pub(crate) environment: Environment,
    pub(crate) auth_kind: AuthKind,
    pub(crate) scopes: BTreeSet<String>,
    pub(crate) account_ref: AccountRef,
    pub(crate) account_pii_ref: AccountPiiRef,
    pub(crate) fingerprint: AccountFingerprint,
    pub(crate) state: ConnectionState,
    pub(crate) contract_hash: Option<String>,
    pub(crate) terms_version: Option<String>,
}

impl ConnectionRecord {
    /// A new record, `Active`. Refused when it breaks connections spec §3 to §6:
    /// - an Alpaca credential, an API key or OAuth, that is not paper (`EnvironmentRefused`;
    ///   DEC-441 item 3, DEC-821 item 7);
    /// - an `auth_kind` the broker does not use (`InvalidRecord { member: "auth_kind" }`): Alpaca
    ///   an API key or OAuth, Robinhood MCP OAuth only, Kraken an API key only;
    /// - `scopes` empty, or one that is empty or holds a character outside printable ASCII or a
    ///   space (`scopes`): a scope is a broker's name for a grant or, for MCP, a tool;
    /// - a `contract_hash` present exactly when the credential is not MCP, or not `sha256:` and 64
    ///   lowercase hex digits (`contract_hash`; §6.2 rule 3);
    /// - a `terms_version` on a broker without platform terms (any but Robinhood), or not
    ///   `sha256:` and 64 lowercase hex digits (`terms_version`).
    pub fn new(new: NewRecord) -> Result<Self, ConnectError> {
        let _ = new;
        Err(ConnectError::Unimplemented { story: "E7-11" })
    }

    /// What an API response or the CLI may show.
    pub fn view(&self) -> ConnectionView {
        ConnectionView {
            connection_id: self.connection_id.clone(),
            broker: self.broker,
            environment: self.environment,
            auth_kind: self.auth_kind,
            scopes: self.scopes.clone(),
            state: self.state,
        }
    }

    /// The members `ConnectionEstablished` version 2 takes from the record (journal spec §9.8);
    /// the caller adds the user and the step-up.
    pub fn established_members(&self) -> EstablishedMembers {
        EstablishedMembers {
            connection_id: self.connection_id.clone(),
            broker: self.broker,
            environment: self.environment,
            scopes: self.scopes.clone(),
            account_ref: self.account_ref.clone(),
        }
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

/// `ConnectionEstablished` version 2's members from the record (journal spec §9.8).
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

/// The deployment's connection records (CN-5). It is not `Clone`: one registry is the one place a
/// connect is admitted, and a copy would admit against a stale view.
///
/// ```compile_fail
/// fn copied(registry: mandate_connections::record::Registry) -> (mandate_connections::record::Registry, mandate_connections::record::Registry) {
///     (registry.clone(), registry)
/// }
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct Registry {
    pub(crate) records: Vec<ConnectionRecord>,
    fingerprint_key_rotating: bool,
}

impl Registry {
    /// The registry over stored records. `fingerprint_key_rotating` is true while every
    /// fingerprint is being recomputed under a new key (connections spec §3.1). Refused when the
    /// records already break CN-5: two with one `connection_id` (`InvalidConnectionId`), or two
    /// with one fingerprint (`AlreadyConnected`, naming the first) or one `account_ref`
    /// (`InvalidAccountRef`).
    pub fn new(
        records: Vec<ConnectionRecord>,
        fingerprint_key_rotating: bool,
    ) -> Result<Self, ConnectError> {
        let _ = (records, fingerprint_key_rotating);
        Err(ConnectError::Unimplemented { story: "E7-11" })
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
    /// [`Registry::admit`] checks, the key's rotation among them (`FingerprintRotating`). A record
    /// whose id another record holds, revoked or not, is `InvalidConnectionId`; one whose
    /// fingerprint a revoked record holds must be that record's reconnect (its id and
    /// `account_ref`, else `AlreadyConnected`; its broker and environment, else
    /// `ReconnectMismatch`); one with a new id and fingerprint whose `account_ref` another record
    /// holds, revoked or not, is `InvalidAccountRef` (CN-5). A reconnect takes the new record's
    /// members but the state the connection was revoked from, not `active`: its account stream
    /// continues, and with it that state (connections spec §9.1, journal spec §9.8). A revoked
    /// record the registry was built with, whose earlier state it was not given, reconnects
    /// `suspended`, the state that needs the most to leave.
    pub fn insert(&mut self, record: ConnectionRecord) -> Result<(), ConnectError> {
        let _ = record;
        Err(ConnectError::Unimplemented { story: "E7-11" })
    }

    /// Moves a record to `state` (connections spec §9.1, journal spec §9.8 rule 60): `degraded`
    /// and `suspended` from `active`; `active` and `suspended` from `degraded`; `active` from
    /// `suspended` (after the owner's acknowledgment); and `revoked` from any of those three.
    /// Every other move is `InvalidTransition`: `revoked` is terminal, since only a reconnect's
    /// [`Registry::insert`] reuses the record, and a move to the state a record already holds
    /// changes nothing, so a caller never journals a change that did not happen. An id no record
    /// holds is `UnknownConnection`. `Revoked` frees its account for a reconnect only.
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

    /// The `ConnectionEstablished` members of one record, if the deployment has it: after a
    /// reconnect, the replacing record's.
    pub fn established(
        &self,
        connection_id: &ConnectionId,
    ) -> Result<Option<EstablishedMembers>, ConnectError> {
        let _ = connection_id;
        Err(ConnectError::Unimplemented { story: "E7-11" })
    }
}
