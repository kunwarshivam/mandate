//! The connection record and the account fingerprint (connections spec §3, §3.1; CN-1, CN-5,
//! CN-12; E7-11). **The connection manager, in the API process**, keeps the deployment's records
//! in a [`Registry`].
//!
//! A record is a reference, never a credential: it names the account by an opaque `account_ref`,
//! its personal-data reference, and a keyed fingerprint the vault computes. The fingerprint
//! detects a second connection to an account already connected; it is never journaled, never
//! returned ([`ConnectionView`], [`EstablishedMembers`]), and never printed.

use std::collections::{BTreeMap, BTreeSet};
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
        match code {
            "alpaca" => Ok(Self::Alpaca),
            "robinhood" => Ok(Self::Robinhood),
            "kraken_derivatives_us" => Ok(Self::KrakenDerivativesUs),
            _ => Err(ConnectError::InvalidRecord { member: "broker" }),
        }
    }

    /// Whether the broker takes a credential of this kind: Alpaca an API key or OAuth, Robinhood
    /// MCP OAuth only, Kraken an API key only (connections spec §3 to §5).
    fn takes(self, auth_kind: AuthKind) -> bool {
        matches!(
            (self, auth_kind),
            (Self::Alpaca, AuthKind::ApiKey | AuthKind::Oauth)
                | (Self::Robinhood, AuthKind::McpOauth)
                | (Self::KrakenDerivativesUs, AuthKind::ApiKey)
        )
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

impl ConnectionState {
    /// Whether §9.1 moves a record from `self` to `to`. A move to the same state is not a move.
    fn moves_to(self, to: Self) -> bool {
        matches!(
            (self, to),
            (
                Self::Active,
                Self::Degraded | Self::Suspended | Self::Revoked
            ) | (
                Self::Degraded,
                Self::Active | Self::Suspended | Self::Revoked
            ) | (Self::Suspended, Self::Active | Self::Revoked)
        )
    }
}

/// Journal spec §2's identifier grammar: 1 to 64 of `A-Z`, `a-z`, `0-9`, `_` and `-`.
fn is_identifier(text: &str) -> bool {
    (1..=64).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// One character of Crockford's base-32 alphabet, uppercase: no `I`, `L`, `O` or `U`.
fn is_crockford(b: u8) -> bool {
    b.is_ascii_digit() || (b.is_ascii_uppercase() && !matches!(b, b'I' | b'L' | b'O' | b'U'))
}

/// `sha256:` and exactly 64 lowercase hex digits (connections spec §6.2 rule 3).
fn is_digest(text: &str) -> bool {
    text.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// A broker's name for a grant or, for MCP, a tool: printable ASCII with no space, never empty.
fn is_scope(scope: &str) -> bool {
    !scope.is_empty() && scope.bytes().all(|b| b.is_ascii_graphic())
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
        if is_identifier(text) {
            Ok(Self(text.to_owned()))
        } else {
            Err(ConnectError::InvalidConnectionId)
        }
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
        let first_at_most_7 = text.starts_with(|c| ('0'..='7').contains(&c));
        if text.len() == 26 && first_at_most_7 && text.bytes().all(is_crockford) {
            Ok(Self(text.to_owned()))
        } else {
            Err(ConnectError::InvalidAccountRef)
        }
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
        if is_identifier(text) {
            Ok(Self(text.to_owned()))
        } else {
            Err(ConnectError::InvalidPiiRef)
        }
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
        let invalid = |member| Err(ConnectError::InvalidRecord { member });
        if !new.broker.takes(new.auth_kind) {
            return invalid("auth_kind");
        }
        if new.broker == Broker::Alpaca && new.environment != Environment::Paper {
            return Err(ConnectError::EnvironmentRefused);
        }
        if new.scopes.is_empty() || !new.scopes.iter().all(|scope| is_scope(scope)) {
            return invalid("scopes");
        }
        let over_mcp = new.auth_kind == AuthKind::McpOauth;
        if over_mcp != new.contract_hash.is_some()
            || !new.contract_hash.as_deref().is_none_or(is_digest)
        {
            return invalid("contract_hash");
        }
        if let Some(terms) = new.terms_version.as_deref()
            && (new.broker != Broker::Robinhood || !is_digest(terms))
        {
            return invalid("terms_version");
        }
        Ok(Self {
            connection_id: new.connection_id,
            broker: new.broker,
            environment: new.environment,
            auth_kind: new.auth_kind,
            scopes: new.scopes,
            account_ref: new.account_ref,
            account_pii_ref: new.account_pii_ref,
            fingerprint: new.fingerprint,
            state: ConnectionState::Active,
            contract_hash: new.contract_hash,
            terms_version: new.terms_version,
        })
    }

    /// Whether a reconnect of this record keeps its broker and environment (CN-12).
    fn same_venue(&self, broker: Broker, environment: Environment) -> bool {
        self.broker == broker && self.environment == environment
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
    /// The state each revoked connection was revoked from, which its reconnect resumes.
    revoked_from: BTreeMap<ConnectionId, ConnectionState>,
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
        let mut rest = records.as_slice();
        while let Some((first, later)) = rest.split_first() {
            for other in later {
                if first.connection_id == other.connection_id {
                    return Err(ConnectError::InvalidConnectionId);
                }
                if first.fingerprint == other.fingerprint {
                    return Err(ConnectError::AlreadyConnected {
                        existing: first.connection_id.clone(),
                    });
                }
                if first.account_ref == other.account_ref {
                    return Err(ConnectError::InvalidAccountRef);
                }
            }
            rest = later;
        }
        Ok(Self {
            records,
            fingerprint_key_rotating,
            revoked_from: BTreeMap::new(),
        })
    }

    /// The record holding an account, by its fingerprint: at most one, by CN-5.
    fn holder(&self, fingerprint: &AccountFingerprint) -> Option<&ConnectionRecord> {
        self.records.iter().find(|r| r.fingerprint == *fingerprint)
    }

    fn record(&self, connection_id: &ConnectionId) -> Option<&ConnectionRecord> {
        self.records
            .iter()
            .find(|r| r.connection_id == *connection_id)
    }

    fn not_rotating(&self) -> Result<(), ConnectError> {
        if self.fingerprint_key_rotating {
            Err(ConnectError::FingerprintRotating)
        } else {
            Ok(())
        }
    }

    /// Check 4 (connections spec §8.1): refuses an account a record that is not revoked holds
    /// (`AlreadyConnected`, naming that record's id only), reuses a revoked record of the same
    /// broker and environment (`Reconnect`), refuses one of another (`ReconnectMismatch`), and
    /// refuses every connect while the key rotates (`FingerprintRotating`).
    pub fn admit(&self, candidate: &Candidate) -> Result<Admission, ConnectError> {
        self.not_rotating()?;
        let Some(holder) = self.holder(&candidate.fingerprint) else {
            return Ok(Admission::New);
        };
        if holder.state != ConnectionState::Revoked {
            return Err(ConnectError::AlreadyConnected {
                existing: holder.connection_id.clone(),
            });
        }
        if !holder.same_venue(candidate.broker, candidate.environment) {
            return Err(ConnectError::ReconnectMismatch);
        }
        Ok(Admission::Reconnect {
            connection_id: holder.connection_id.clone(),
            account_ref: holder.account_ref.clone(),
        })
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
        self.not_rotating()?;
        let held = self
            .records
            .iter_mut()
            .find(|r| r.fingerprint == record.fingerprint);
        if let Some(holder) = held {
            if holder.state != ConnectionState::Revoked
                || holder.connection_id != record.connection_id
                || holder.account_ref != record.account_ref
            {
                return Err(ConnectError::AlreadyConnected {
                    existing: holder.connection_id.clone(),
                });
            }
            if !holder.same_venue(record.broker, record.environment) {
                return Err(ConnectError::ReconnectMismatch);
            }
            let resumed = self
                .revoked_from
                .remove(&holder.connection_id)
                .unwrap_or(ConnectionState::Suspended);
            *holder = ConnectionRecord {
                state: resumed,
                ..record
            };
            return Ok(());
        }
        if self.record(&record.connection_id).is_some() {
            return Err(ConnectError::InvalidConnectionId);
        }
        if self
            .records
            .iter()
            .any(|r| r.account_ref == record.account_ref)
        {
            return Err(ConnectError::InvalidAccountRef);
        }
        self.records.push(record);
        Ok(())
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
        let record = self
            .records
            .iter_mut()
            .find(|r| r.connection_id == *connection_id)
            .ok_or(ConnectError::UnknownConnection)?;
        if !record.state.moves_to(state) {
            return Err(ConnectError::InvalidTransition {
                from: record.state,
                to: state,
            });
        }
        if state == ConnectionState::Revoked {
            self.revoked_from
                .insert(record.connection_id.clone(), record.state);
        }
        record.state = state;
        Ok(())
    }

    /// The view of one record, if the deployment has it.
    pub fn view(
        &self,
        connection_id: &ConnectionId,
    ) -> Result<Option<ConnectionView>, ConnectError> {
        Ok(self.record(connection_id).map(ConnectionRecord::view))
    }

    /// The `ConnectionEstablished` members of one record, if the deployment has it: after a
    /// reconnect, the replacing record's.
    pub fn established(
        &self,
        connection_id: &ConnectionId,
    ) -> Result<Option<EstablishedMembers>, ConnectError> {
        Ok(self
            .record(connection_id)
            .map(ConnectionRecord::established_members))
    }
}
