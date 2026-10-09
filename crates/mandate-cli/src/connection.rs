//! `mandate connection record` (first live trade brief, K1b; E7-11's first slice; DEC-529 items 3
//! and 11): commits a connection's `ConnectionEstablished` version 2 (journal spec §9.8, DEC-800)
//! on the workspace control stream, from the CLI, without reaching any broker.
//!
//! The command reaches no broker and reads no credential. The connect sequence's executor has
//! already reached the account and journaled a `ConnectionChecked` (occasion `connect`) on the
//! account stream the record binds; the command confirms that check before it commits, and names
//! it as the record's `causation_id` (rule 63). It plays the connection manager's part in Phase 1:
//! the check, the binding (DEC-800 item 2), stream rule 64, and, with no vault to compute the
//! account fingerprint, the reading that refuses more: no second connection of the same broker
//! and environment in the workspace, unless it reconnects a revoked one with the same members.
//! What it prints names no account, no personal-data reference, and no fingerprint.

use std::io::Write;

use clap::{Args, Subcommand};

use crate::control::{ControlError, ControlJournal, Ids, Now, Owner, Submitted};
use crate::postgres::JournalArgs;

#[derive(Debug, Subcommand)]
pub enum ConnectionCommand {
    /// Record a checked connection on the workspace control stream. Without `--code`, shows the
    /// record and the code that confirms it, and commits nothing.
    Record(RecordArgs),
}

#[derive(Debug, Args)]
pub struct RecordArgs {
    /// The workspace whose control stream records the connection.
    #[arg(long, value_name = "ID")]
    pub workspace: String,
    /// The workspace admin recording it (opaque).
    #[arg(long, value_name = "ID")]
    pub user: String,
    /// `paper` or `live`: the connection's environment, for life, and the record's.
    #[arg(long, value_name = "ENV")]
    pub environment: String,
    #[command(flatten)]
    pub connection: Request,
    /// The code `record` showed for this record, which confirms it (`cli_confirm`).
    #[arg(long, value_name = "CODE")]
    pub code: Option<String>,
    #[command(flatten)]
    pub target: JournalArgs,
}

/// What the owner asks to record.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct Request {
    /// The connection id mandates name it by.
    #[arg(long = "connection", value_name = "ID")]
    pub connection_id: String,
    /// `alpaca`, `robinhood` or `kraken_derivatives_us`.
    #[arg(long, value_name = "BROKER")]
    pub broker: String,
    /// One granted scope; for an MCP connection, one allowlisted tool. Repeat for each; the record
    /// lists them once each, ascending (journal spec §9.2 rule 19).
    #[arg(long = "scope", value_name = "SCOPE")]
    pub scopes: Vec<String>,
    /// The account stream's ULID the connect sequence opened.
    #[arg(long, value_name = "ULID")]
    pub account_ref: String,
    /// The event id of the connect sequence's passing `ConnectionChecked` on that stream.
    #[arg(long, value_name = "EVENT_ID")]
    pub checked: String,
    /// The owner's attestation that the account is a cash account (`cash_account`) or has margin
    /// disabled (`margin_disabled`), DEC-529 item 11: required for `live` and refused for `paper`,
    /// as journal spec §9.8 rule 64 requires of the record.
    #[arg(long, value_name = "ATTESTATION")]
    pub margin_attestation: Option<String>,
}

/// What `record` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recorded {
    /// No code was given: the code that confirms this exact record, and nothing committed.
    Shown { code: String },
    /// The record committed, or found committed by an earlier run with the same code.
    Committed(Submitted),
}

/// Confirms the connect sequence's check and commits `ConnectionEstablished` version 2 with
/// `cli_confirm` step-up, as `owner.user`, in `owner.environment`, the connection's. Without
/// `code`, prints the record and the code that confirms it instead, and commits nothing. A re-run
/// whose control stream already ends with this record answers that event.
///
/// Refusals, the first that applies:
/// - from the request: `connection_id_invalid` and `account_ref_invalid` (journal spec §2's
///   identifier and ULID); `broker_unsupported` for any broker but `alpaca` and `robinhood` (no
///   crypto in this slice); `environment_refused` for Alpaca outside `paper` (DEC-441 item 3),
///   Robinhood outside `live` (it has no paper environment), or `backtest`; `scopes_missing`;
///   `attestation_missing` for `live` without the attestation, `attestation_invalid` for one that
///   is not `cash_account` or `margin_disabled`, and `attestation_not_live` for one on `paper`
///   (rule 64);
/// - from the account stream `acct:{workspace}:{account_ref}`: `check_missing` unless `checked`
///   names a `ConnectionChecked` there of occasion `connect`, or `reconnect` for a reconnect;
///   `check_other_connection` when it is for another `connection_id`; `check_failed` unless every
///   result passed, scope, environment and account (and contract, for Robinhood's MCP) are among
///   them, `account_pii_ref` is not `null`, and its environment is the owner's;
/// - from the control stream (rule 64): `connection_exists` for an id established and not revoked
///   since; `reconnect_mismatch` for a revoked id established first by version 1 or with another
///   broker, environment or `account_ref`; `account_ref_bound` for an `account_ref` another id's
///   establishment names; `account_maybe_connected` for any other id established with the same
///   broker and environment, revoked or not, since without the vault's fingerprint the command
///   cannot tell its account from this one (DEC-176);
/// - `code_mismatch` for a code that is not the one shown for exactly this record.
///
/// # Errors
/// [`ControlError::Refused`] with one of [`CODES`], before any assertion is minted, anything is
/// appended, or anything is printed; [`ControlError::Journal`] as the other control commands.
pub fn record(
    journal: &mut dyn ControlJournal,
    ids: &mut dyn Ids,
    owner: &Owner,
    request: &Request,
    code: Option<&str>,
    now: Now,
    report: &mut dyn Write,
) -> Result<Recorded, ControlError> {
    let _ = (journal, ids, owner, request, code, now, report);
    Err(ControlError::Unimplemented { story: "E7-11" })
}

/// Every refusal `record` gives, by code.
pub const CODES: [&str; 17] = [
    "account_maybe_connected",
    "account_ref_bound",
    "account_ref_invalid",
    "attestation_invalid",
    "attestation_missing",
    "attestation_not_live",
    "broker_unsupported",
    "check_failed",
    "check_missing",
    "check_other_connection",
    "code_mismatch",
    "connection_exists",
    "connection_id_invalid",
    "environment_invalid",
    "environment_refused",
    "reconnect_mismatch",
    "scopes_missing",
];

/// [`record`] against the workspace's Postgres journal.
///
/// # Errors
/// As [`record`]; `environment_invalid` for an environment that is not `paper` or `live`, and every
/// refusal [`record`] makes from the request alone, before the journal is opened. No message names
/// the DSN.
pub fn run(args: &RecordArgs, now: Now, report: &mut impl Write) -> anyhow::Result<Recorded> {
    let _ = (args, now, report);
    Err(ControlError::Unimplemented { story: "E7-11" }.into())
}
