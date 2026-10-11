//! The connection a run uses and DEC-529 item 3's lapse check (E7-26, the
//! [first live trade brief](../../../docs/project/tasks/first-live-trade.md)'s G1a): the
//! connector is chosen from the connection record the confirmed mandate names, the environment
//! is the mandate's own, and a live run refuses to start unless the account the record binds is
//! flat and the record is unspent. Every refusal fails closed before anything runs, with its own
//! reason code ([ADR-0001](../../../docs/adr/0001-engineering-setup.md) ES-09). The default
//! build holds no Robinhood host or connector, so the Robinhood connector its record chooses
//! refuses here and only the runner's `live` feature can serve it (LT-1, V-001,
//! [DEC-910](../../../docs/project/decisions/DEC-910.md)).

use mandate_domain::Environment;
use mandate_journal::StoredEvent;
use mandate_shell::control::ControlRecord;
use mandate_spec::Mandate;

/// The broker whose connector the run builds, chosen from the connection record (G1a): Alpaca
/// paper, the only one the default build compiles (V-001, LT-14), and Robinhood live, which
/// only the runner's `live` feature adds (LT-1, DEC-529 item 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Broker {
    Alpaca,
    Robinhood,
}

/// The connection a run uses (G1a): the connector chosen from the connection record the
/// confirmed mandate names, the environment from that mandate, and the account stream the
/// record binds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    /// The broker whose connector the run builds: the record's choice.
    pub broker: Broker,
    /// The confirmed mandate's environment, which the record must state too.
    pub environment: Environment,
    /// The account stream the record binds, which must be the run's own.
    pub account_ref: String,
}

/// Why a run refuses its connection before anything runs (G1a, DEC-529 item 3): the record, the
/// confirmed mandate and the build do not agree, and each disagreement fails closed
/// ([DEC-910](../../../docs/project/decisions/DEC-910.md) item 2).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConnectionRefusal {
    /// The body of every stub in the tests PR (DEC-77).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    /// The confirmed mandate names a connection no record on the control stream establishes.
    #[error("the mandate's connection has no record on the control stream")]
    Unrecorded,
    /// The connection's last record is its revocation, so nothing the record bound serves the
    /// run, a re-established connection excepted.
    #[error("the connection the mandate names was revoked")]
    Revoked,
    /// A member the run reads off the record is missing or not text: a version 1 record or a
    /// corrupt payload, never a guess at what it meant.
    #[error("the connection's record cannot be read")]
    Unreadable,
    /// The record states another environment than the confirmed mandate's: the environment is
    /// the mandate's, and no record moves the run between environments (V-001).
    #[error("the record states environment {named}, not the mandate's")]
    Environment { named: String },
    /// The record names a broker that does not serve the mandate's environment, or one the
    /// runner holds no connector for.
    #[error("the record names the {named} broker, which the mandate's environment does not serve")]
    Broker { named: String },
    /// The record binds an account stream other than the run's own.
    #[error("the record binds another account stream than the run's")]
    Account,
    /// The record's choice exists only behind the `live` feature, which this build does not
    /// hold: the default build compiles no Robinhood host or connector (LT-1).
    #[error("the record chooses a connector this build does not hold")]
    NotBuilt,
}

impl ConnectionRefusal {
    /// The stable reason code of each refusal (ADR-0001 ES-09): `connection_unrecorded`,
    /// `connection_revoked`, `connection_record_unreadable`, `connection_environment`,
    /// `connection_broker`, `connection_account`, `connector_not_built`, and the stub's
    /// `unimplemented`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unimplemented { .. } => "unimplemented",
            Self::Unrecorded => "connection_unrecorded",
            Self::Revoked => "connection_revoked",
            Self::Unreadable => "connection_record_unreadable",
            Self::Environment { .. } => "connection_environment",
            Self::Broker { .. } => "connection_broker",
            Self::Account => "connection_account",
            Self::NotBuilt => "connector_not_built",
        }
    }
}

/// What the account the connection binds holds, as the connector's snapshot of it answered
/// (DEC-470 item 1): every position the snapshot listed, a zero quantity included, and every
/// working order, counted. The account is flat only when both counts are zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    /// How many positions the snapshot listed.
    pub positions: usize,
    /// How many working orders the snapshot listed.
    pub open_orders: usize,
}

/// Why a live run refuses to start (DEC-529 item 3's lapse): the account is not flat
/// (DEC-470 item 1) or the connection record is spent ([DEC-910](../../../docs/project/decisions/DEC-910.md)
/// item 3).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LapseRefusal {
    /// The body of every stub in the tests PR (DEC-77).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    /// The account holds a position or a working order; the counts are carried as data and
    /// printed by no message.
    #[error("the account the connection binds is not flat")]
    NotFlat {
        positions: usize,
        open_orders: usize,
    },
    /// The account stream the record binds holds a fill, so the record is spent and the same
    /// build cannot start another live run on it.
    #[error("the connection record is spent")]
    Spent,
}

impl LapseRefusal {
    /// The stable reason code of each refusal (ADR-0001 ES-09): `account_not_flat`,
    /// `connection_spent`, and the stub's `unimplemented`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unimplemented { .. } => "unimplemented",
            Self::NotFlat { .. } => "account_not_flat",
            Self::Spent => "connection_spent",
        }
    }
}

/// The connection the confirmed mandate's run uses (G1a, DEC-529 item 3): the connector chosen
/// from the connection record `records` hold for the mandate's `connection_id`, the environment
/// the mandate itself states, and the account stream `account_ref` the record must bind. A
/// paper mandate whose connection no record names keeps today's run (V-001's paper default,
/// LT-14); a live mandate requires an established record the build can serve, and a revoked
/// one refuses wherever the stream holds it. Each disagreement fails closed before anything
/// runs, in this order: no record ([`ConnectionRefusal::Unrecorded`]), revoked
/// ([`ConnectionRefusal::Revoked`]), a member the run cannot read
/// ([`ConnectionRefusal::Unreadable`]), another environment
/// ([`ConnectionRefusal::Environment`]), another broker ([`ConnectionRefusal::Broker`]), another
/// account stream ([`ConnectionRefusal::Account`]), or a connector this build does not hold
/// ([`ConnectionRefusal::NotBuilt`]; DEC-910 item 2).
///
/// # Errors
/// Any [`ConnectionRefusal`] that applies, before anything runs.
pub fn connection(
    records: &[ControlRecord],
    mandate: &Mandate,
    account_ref: &str,
) -> Result<Connection, ConnectionRefusal> {
    let _ = (records, mandate, account_ref);
    Err(ConnectionRefusal::Unimplemented { story: "E7-26" })
}

/// DEC-529 item 3's lapse check (G1a): a live run refuses to start unless the account the
/// connection binds is flat — `held` counts the positions and working orders the connector's
/// snapshot listed, DEC-470 item 1 — and the record is unspent — no `FillApplied` on the
/// account stream `rows` the record binds, a `LateFillApplied` included, since the record's own
/// `place_equity_order` that fills is what spends it (DEC-910 item 3). The flat half refuses
/// first, and a paper run makes no check (LT-14).
///
/// # Errors
/// [`LapseRefusal::NotFlat`] or [`LapseRefusal::Spent`], before anything runs.
pub fn lapse(
    connection: &Connection,
    rows: &[StoredEvent],
    held: &Held,
) -> Result<(), LapseRefusal> {
    let _ = (connection, rows, held);
    Err(LapseRefusal::Unimplemented { story: "E7-26" })
}
