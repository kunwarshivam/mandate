//! The connection a run uses and DEC-529 item 3's lapse check (E7-26, the
//! [first live trade brief](../../../docs/project/tasks/first-live-trade.md)'s G1a): the
//! connector is chosen from the connection record the confirmed mandate names, the environment
//! is the mandate's own, and a live run refuses to start unless the account the record binds is
//! flat and the record is unspent. Every refusal fails closed before anything runs, with its own
//! reason code ([ADR-0001](../../../docs/adr/0001-engineering-setup.md) ES-09). The default
//! build holds no Robinhood host or connector, so the Robinhood connector its record chooses
//! refuses here and only the runner's `live` feature can serve it (LT-1, V-001,
//! [DEC-910](../../../docs/project/decisions/DEC-910.md)).

use mandate_canon::Value;
use mandate_domain::Environment;
use mandate_journal::{StoredEvent, StreamId, StreamType};
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
/// (DEC-470 item 1): every position the snapshot listed of a non-zero quantity, long or short,
/// and every working order, counted (DEC-2390 item 1: a row of quantity exactly zero is flat). The account is flat only when both counts are zero.
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
    /// The run handed the lapse no evidence ([DEC-919](../../../docs/project/decisions/DEC-919.md)):
    /// neither the journal's read of the account stream the record binds nor the connector
    /// snapshot's counts, so nothing shows the account flat or the record unspent, and a live
    /// run refuses to start rather than pass on inputs no read produced.
    #[error("the lapse was handed no account-stream read and no account snapshot")]
    NotSupplied,
}

impl LapseRefusal {
    /// The stable reason code of each refusal (ADR-0001 ES-09): `account_not_flat`,
    /// `connection_spent`, `lapse_evidence_not_supplied` (DEC-919), and the stub's
    /// `unimplemented`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unimplemented { .. } => "unimplemented",
            Self::NotFlat { .. } => "account_not_flat",
            Self::Spent => "connection_spent",
            Self::NotSupplied => "lapse_evidence_not_supplied",
        }
    }
}

/// The connection the confirmed mandate's run uses (G1a, DEC-529 item 3): the connector chosen
/// from the connection record `records` hold for the mandate's `connection_id`, the environment
/// the mandate itself states, and the account stream `account_ref` the record must bind. A
/// paper mandate whose connection no record names keeps today's run (V-001's paper default,
/// LT-14); a live mandate requires an established record the build can serve, and a revoked
/// one refuses wherever the stream holds it. The record that decides is the latest by `seq`,
/// and a same-`seq` tie between the connection's establishment and its revocation resolves to
/// the revocation — the safe default that refuses, order-independent, and latent (journal
/// spec §5.1 gives one `seq` to one record on a stream). Each disagreement fails closed before
/// anything runs, in this order: no record ([`ConnectionRefusal::Unrecorded`]), revoked
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
    let named = mandate.connection_id.as_str();
    let ours = |record: &ControlRecord| {
        matches!(
            record.event_type.as_str(),
            "ConnectionEstablished" | "ConnectionRevoked"
        ) && member(record, "connection_id") == Some(named)
    };
    let latest = records
        .iter()
        .filter(|record| ours(record))
        .max_by_key(|record| (record.seq, revocation_rank(record)));
    let Some(record) = latest else {
        return paper_default(mandate, account_ref);
    };
    if record.event_type == "ConnectionRevoked" {
        return Err(ConnectionRefusal::Revoked);
    }
    let broker = member(record, "broker").ok_or(ConnectionRefusal::Unreadable)?;
    let environment = member(record, "environment").ok_or(ConnectionRefusal::Unreadable)?;
    let bound = member(record, "account_ref").ok_or(ConnectionRefusal::Unreadable)?;
    if environment != environment_text(mandate) {
        return Err(ConnectionRefusal::Environment {
            named: environment.to_owned(),
        });
    }
    let broker = broker_serving(broker, mandate.environment)
        .map_err(|named| ConnectionRefusal::Broker { named })?;
    if bound != account_ref {
        return Err(ConnectionRefusal::Account);
    }
    match broker {
        Broker::Alpaca => Ok(Connection {
            broker,
            environment: mandate.environment,
            account_ref: account_ref.to_owned(),
        }),
        Broker::Robinhood => robinhood(mandate.environment, account_ref),
    }
}

/// One text member of `record`'s payload, or [`None`] when it is missing or holds not text: a
/// member the run reads off the record, and [`ConnectionRefusal::Unreadable`] is the only
/// reading of one ([DEC-910](../../../docs/project/decisions/DEC-910.md) item 2). The E7-26
/// wiring reads its rows' text members through this same reader (DEC-914 items 1 and 2:
/// `asked`'s recorded `content_hash` member is one), so it is `pub(crate)` — the visibility
/// flip #1311 reserved to the wiring implementation PR, this one.
pub(crate) fn member<'a>(record: &'a ControlRecord, name: &str) -> Option<&'a str> {
    record.payload.get(name).and_then(Value::as_str)
}

/// The rank the connection's records are collated by after `seq`: [`true`] for the
/// connection's `ConnectionRevoked` (journal spec §9.2), so a revocation at the same `seq` as
/// an establishment outranks it — the safe default that refuses, never a tie that imports an
/// establishment's data over a revocation, and one that does not depend on the order the
/// records are read in (`AGENTS.md` rule 3, DEC-910 item 2). Journal spec §5.1 gives one
/// `seq` to one record on a stream, so the tie is latent; the pins hold it anyway.
fn revocation_rank(record: &ControlRecord) -> bool {
    record.event_type == "ConnectionRevoked"
}

/// V-001's paper default (DEC-910 item 2): a paper mandate whose connection no record names
/// keeps today's run — the Alpaca connector the default build compiles, the mandate's own
/// environment, the account stream the run was given — and a live mandate with no record has
/// neither an environment a run could trade live under nor a connector, so it refuses.
fn paper_default(mandate: &Mandate, account_ref: &str) -> Result<Connection, ConnectionRefusal> {
    match mandate.environment {
        Environment::Paper => Ok(Connection {
            broker: Broker::Alpaca,
            environment: Environment::Paper,
            account_ref: account_ref.to_owned(),
        }),
        Environment::Live => Err(ConnectionRefusal::Unrecorded),
    }
}

/// The mandate's environment as the record states it (mandate spec §3: `paper` or `live`), so
/// the record's member and the mandate's own field say the same text or the record refuses.
fn environment_text(mandate: &Mandate) -> &'static str {
    match mandate.environment {
        Environment::Paper => "paper",
        Environment::Live => "live",
    }
}

/// The broker `text` names, when it serves `environment` (DEC-910 item 2): a live mandate is
/// served by the Robinhood connector alone (DEC-529 item 3) and a paper run by the Alpaca
/// connector the default build compiles (V-001, LT-14), so a paper run has exactly one broker
/// its build can serve. Any other text names a broker that does not serve the mandate's
/// environment or one the runner knows no connector for, and it is refused as it is read.
fn broker_serving(text: &str, environment: Environment) -> Result<Broker, String> {
    match (text, environment) {
        ("alpaca", Environment::Paper) => Ok(Broker::Alpaca),
        ("robinhood", Environment::Live) => Ok(Broker::Robinhood),
        (named, _) => Err(named.to_owned()),
    }
}

/// The Robinhood a record chooses (DEC-910 item 1's runtime seam): one code path, compiled in
/// every build, chooses — and only the `live` feature lets a build serve the choice, so the
/// default build's seam refuses [`ConnectionRefusal::NotBuilt`] and no run of it can reach a
/// Robinhood host at all (LT-1). [`ROBINHOOD_LINKED`] is the only part the feature gates. The
/// tests that pin this seam expect what the build they run in serves, `connector_not_built`
/// without `live` and the connection with it, so they hold in every build (DEC-963, replacing
/// #1297's reading that they fail under `live` by design).
fn robinhood(environment: Environment, account_ref: &str) -> Result<Connection, ConnectionRefusal> {
    if !ROBINHOOD_LINKED {
        return Err(ConnectionRefusal::NotBuilt);
    }
    Ok(Connection {
        broker: Broker::Robinhood,
        environment,
        account_ref: account_ref.to_owned(),
    })
}

/// Whether this build serves the Robinhood a record chooses (DEC-910 item 1): only the
/// runner's `live` feature (DEC-529 item 3) may hold a connector for it, and no CI or release
/// build enables the feature, so this is [`false`] exactly in the default build. A constant,
/// not a function: the mutants gate generates no mutant for it, so the door stays numerically
/// verifiable — the refusal behind it and the construction past it are the mutable surface.
#[cfg(not(feature = "live"))]
const ROBINHOOD_LINKED: bool = false;

/// Whether this build serves the Robinhood a record chooses (DEC-910 item 1): the `live`
/// feature's door, opened only by a `--features live` build the founder builds locally from a
/// commit on `main` (DEC-529 item 3). What serves the choice behind the door — the pinned host,
/// the connector, the preflight — is the live build's own surface, not this constant's.
#[cfg(feature = "live")]
const ROBINHOOD_LINKED: bool = true;

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
    if connection.environment != Environment::Live {
        return Ok(());
    }
    if held.positions > 0 || held.open_orders > 0 {
        return Err(LapseRefusal::NotFlat {
            positions: held.positions,
            open_orders: held.open_orders,
        });
    }
    if rows.iter().any(|row| spends(row, &connection.account_ref)) {
        return Err(LapseRefusal::Spent);
    }
    Ok(())
}

/// Whether `row` is the fill evidence that spends the record (DEC-910 item 3): an executor's
/// `FillApplied` or a reconciliation's `LateFillApplied` alone, on the account stream
/// `account_ref` names — `acct:{workspace_id}:{account_ref}` (journal spec §2), whose account
/// segment the ref is, since the CLI binds one account stream to one connection id. Every
/// other event, and every event of another stream, leaves the record unspent.
fn spends(row: &StoredEvent, account_ref: &str) -> bool {
    if !matches!(row.event_type.as_str(), "FillApplied" | "LateFillApplied") {
        return false;
    }
    StreamId::parse(&row.stream_id).is_some_and(|stream| {
        stream.stream_type() == StreamType::Account
            && stream.as_str().split(':').nth(2) == Some(account_ref)
    })
}
