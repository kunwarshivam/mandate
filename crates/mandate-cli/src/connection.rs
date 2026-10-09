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

use std::collections::BTreeSet;
use std::io::Write;

use clap::{Args, Subcommand};
use mandate_canon::Value;
use mandate_journal::{Environment, StreamId};

use crate::control::{
    Caused, ControlError, ControlJournal, Ids, Now, Owner, Submitted, account_stream, code_of,
    commit_caused, control_stream, envelope, object, text,
};
use crate::inbox::InstantIds;
use crate::postgres::{JournalArgs, PgControlJournal};
use crate::version::refused;

const ESTABLISHED: &str = "ConnectionEstablished";
const REVOKED: &str = "ConnectionRevoked";
const CHECKED: &str = "ConnectionChecked";

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
///   names the latest `ConnectionChecked` there of occasion `connect`, or `reconnect` for a
///   reconnect (journal spec §9.8 rule 68);
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
/// The record is appended only at the control stream's head its rows were read at. When another
/// writer moved the stream in between, it reads the rows again and decides again, so two records
/// racing for one account or one connection id commit one, and the other is refused as it would
/// have been after it (CN-5, rule 66; #907 review).
///
/// # Errors
/// [`ControlError::Refused`] with one of [`CODES`], before anything is appended or printed; a pass
/// that loses a race may mint an assertion id that is discarded; [`ControlError::Journal`] as the
/// other control commands.
pub fn record(
    journal: &mut dyn ControlJournal,
    ids: &mut dyn Ids,
    owner: &Owner,
    request: &Request,
    code: Option<&str>,
    now: Now,
    report: &mut dyn Write,
) -> Result<Recorded, ControlError> {
    let (broker, key) = requested(owner, request)?;
    let id = request.connection_id.as_str();
    let mut bound = key.clone();
    bound.push(("causation_id", text(&request.checked)));
    bound.push(("gesture", text("connection_record")));
    let expected = code_of(&object(bound)?);
    for _ in 0..ATTEMPTS {
        let rows = control_rows(journal, owner)?;
        let (own, others): (Vec<_>, Vec<_>) = rows
            .iter()
            .filter(|r| matches!(r.1.as_str(), ESTABLISHED | REVOKED))
            .partition(|r| member(&r.2, "connection_id") == Some(id));
        let reconnect = own.iter().any(|r| r.1 == ESTABLISHED);
        if code == Some(expected.as_str())
            && let Some(earlier) = rerun(&rows, request, &key)?
        {
            print(report, &committed_line(id, &earlier))?;
            return Ok(Recorded::Committed(earlier));
        }
        confirm_check(journal, owner, request, broker, reconnect)?;
        bind(owner, request, &own, &others)?;
        let Some(code) = code else {
            print(report, &shown_lines(owner, request, &expected))?;
            return Ok(Recorded::Shown { code: expected });
        };
        if code != expected {
            return Err(refused("code_mismatch"));
        }
        let caused = Caused {
            head: rows.last().map_or(0, |r| r.0.seq),
            event_type: ESTABLISHED,
            schema_version: 2,
            causation_id: request.checked.clone(),
            key: key.clone(),
        };
        if let Some(submitted) = commit_caused(journal, ids, owner, caused, now)? {
            print(report, &committed_line(id, &submitted))?;
            return Ok(Recorded::Committed(submitted));
        }
    }
    Err(ControlError::Journal(format!(
        "the control stream kept moving for {ATTEMPTS} reads; nothing was recorded"
    )))
}

/// The reads of the control stream after which `record` gives up while other writers keep moving
/// it between its read and its append.
const ATTEMPTS: u32 = 8;

/// The brokers this slice records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Broker {
    Alpaca,
    Robinhood,
}

/// The checks a connection's passing `ConnectionChecked` lists, by broker: Robinhood's MCP
/// connection adds `contract` (journal spec §9.8).
fn needed(broker: Broker) -> &'static [&'static str] {
    match broker {
        Broker::Alpaca => &["account", "environment", "scope"],
        Broker::Robinhood => &["account", "contract", "environment", "scope"],
    }
}

/// Journal spec §2's ULID: 26 Crockford base-32 digits, the first at most `7`.
fn is_ulid(s: &str) -> bool {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut digits = s.bytes();
    let first_fits = matches!(digits.next(), Some(b'0'..=b'7'));
    let rest = digits.filter(|b| ALPHABET.contains(b)).count();
    first_fits && rest == 25 && s.len() == 26
}

/// The refusals `record` makes from the request alone, in [`record`]'s order, and what the record's
/// payload carries besides the step-up evidence.
fn requested(owner: &Owner, request: &Request) -> Result<(Broker, Key), ControlError> {
    StreamId::parse(&format!("ctl:{}", request.connection_id))
        .ok_or_else(|| refused("connection_id_invalid"))?;
    if !is_ulid(&request.account_ref) {
        return Err(refused("account_ref_invalid"));
    }
    let broker = match request.broker.as_str() {
        "alpaca" => Broker::Alpaca,
        "robinhood" => Broker::Robinhood,
        _ => return Err(refused("broker_unsupported")),
    };
    match (broker, owner.environment) {
        (Broker::Alpaca, Environment::Paper) | (Broker::Robinhood, Environment::Live) => {}
        _ => return Err(refused("environment_refused")),
    }
    let scopes: BTreeSet<&str> = request.scopes.iter().map(String::as_str).collect();
    if scopes.is_empty() {
        return Err(refused("scopes_missing"));
    }
    let attestation = match request.margin_attestation.as_deref() {
        None => Value::Null,
        Some(a @ ("cash_account" | "margin_disabled")) => text(a),
        Some(_) => return Err(refused("attestation_invalid")),
    };
    match (owner.environment, &attestation) {
        (Environment::Live, Value::Null) => return Err(refused("attestation_missing")),
        (Environment::Paper, Value::Str(_)) => return Err(refused("attestation_not_live")),
        _ => {}
    }
    let key = vec![
        ("account_ref", text(&request.account_ref)),
        ("broker", text(&request.broker)),
        ("connection_id", text(&request.connection_id)),
        ("environment", text(owner.environment.as_str())),
        ("margin_attestation", attestation),
        (
            "scopes",
            Value::Array(scopes.into_iter().map(text).collect()),
        ),
        ("user", text(&owner.user)),
    ];
    Ok((broker, key))
}

/// The record's payload members besides the step-up evidence.
type Key = Vec<(&'static str, Value)>;

/// One control-stream event: its `seq` and id, its type, and its envelope.
type Row = (Submitted, String, Value);

fn control_rows(journal: &dyn ControlJournal, owner: &Owner) -> Result<Vec<Row>, ControlError> {
    let stream = control_stream(owner)?;
    let rows = journal.rows(&stream)?;
    rows.into_iter()
        .map(|row| {
            let body = envelope(&row)?;
            let at = Submitted {
                event_id: row.event_id,
                seq: row.seq,
            };
            Ok((at, row.event_type, body))
        })
        .collect()
}

/// A text member of an envelope's payload.
fn member<'a>(body: &'a Value, name: &str) -> Option<&'a str> {
    body.get("payload")
        .and_then(|p| p.get(name))
        .and_then(Value::as_str)
}

/// Confirms the connect sequence's check on the bound account stream (DEC-802 item 1).
fn confirm_check(
    journal: &dyn ControlJournal,
    owner: &Owner,
    request: &Request,
    broker: Broker,
    reconnect: bool,
) -> Result<(), ControlError> {
    let stream = account_stream(owner, &request.account_ref)?;
    let occasion = if reconnect { "reconnect" } else { "connect" };
    let rows = journal.rows(&stream)?;
    let row = rows
        .iter()
        .find(|r| r.event_id == request.checked)
        .filter(|r| r.event_type == CHECKED)
        .ok_or_else(|| refused("check_missing"))?;
    let body = envelope(row)?;
    if member(&body, "occasion") != Some(occasion) {
        return Err(refused("check_missing"));
    }
    let later = rows
        .iter()
        .skip_while(|r| r.event_id != request.checked)
        .skip(1);
    for r in later.filter(|r| r.event_type == CHECKED) {
        if member(&envelope(r)?, "occasion") == Some(occasion) {
            return Err(refused("check_missing"));
        }
    }
    if member(&body, "connection_id") != Some(request.connection_id.as_str()) {
        return Err(refused("check_other_connection"));
    }
    let results = body
        .get("payload")
        .and_then(|p| p.get("results"))
        .and_then(Value::as_array)
        .unwrap_or_default();
    let field = |r: &Value, name: &str| r.get(name).and_then(Value::as_str).map(str::to_owned);
    let every_passed = results
        .iter()
        .all(|r| field(r, "result").as_deref() == Some("passed"));
    let listed = needed(broker).iter().all(|n| {
        results
            .iter()
            .any(|r| field(r, "check").as_deref() == Some(*n))
    });
    let pii = body.get("payload").and_then(|p| p.get("account_pii_ref"));
    let read = matches!(pii, Some(Value::Str(_)));
    let environment = body.get("environment").and_then(Value::as_str);
    let same_environment = environment == Some(owner.environment.as_str());
    if [every_passed, listed, read, same_environment].contains(&false) {
        return Err(refused("check_failed"));
    }
    Ok(())
}

/// The control stream's last event, when it is this record with this cause: what a re-run with the
/// same code answers (DEC-290).
fn rerun(rows: &[Row], request: &Request, key: &Key) -> Result<Option<Submitted>, ControlError> {
    let Some((at, event_type, body)) = rows.last() else {
        return Ok(None);
    };
    let step_up = body.get("payload").and_then(|p| p.get("step_up"));
    let mut payload = key.to_vec();
    payload.push(("step_up", step_up.cloned().unwrap_or(Value::Null)));
    let found = object(vec![
        (
            "causation_id",
            body.get("causation_id").cloned().unwrap_or(Value::Null),
        ),
        ("event_type", text(event_type)),
        (
            "payload",
            body.get("payload").cloned().unwrap_or(Value::Null),
        ),
    ])?;
    let this = object(vec![
        ("causation_id", text(&request.checked)),
        ("event_type", text(ESTABLISHED)),
        ("payload", object(payload)?),
    ])?;
    Ok((found == this).then(|| at.clone()))
}

/// What an establishment binds: its broker and environment, and its `account_ref`.
type Binding<'a> = ((Option<&'a str>, Option<&'a str>), Option<&'a str>);

fn binding(r: &Row) -> Binding<'_> {
    let place = (member(&r.2, "broker"), member(&r.2, "environment"));
    (place, member(&r.2, "account_ref"))
}

/// Stream rule 66 and DEC-802 items 2 and 3, against the control stream's establishments and
/// revocations of this connection (`own`) and of every other (`others`).
fn bind(
    owner: &Owner,
    request: &Request,
    own: &[&Row],
    others: &[&Row],
) -> Result<(), ControlError> {
    if own.last().is_some_and(|r| r.1 == ESTABLISHED) {
        return Err(refused("connection_exists"));
    }
    let place = (
        Some(request.broker.as_str()),
        Some(owner.environment.as_str()),
    );
    let wanted = (place, Some(request.account_ref.as_str()));
    let first = own.iter().find(|r| r.1 == ESTABLISHED);
    if first.is_some_and(|r| binding(r) != wanted) {
        return Err(refused("reconnect_mismatch"));
    }
    let established = others.iter().filter(|r| r.1 == ESTABLISHED);
    if established.clone().any(|r| binding(r).1 == wanted.1) {
        return Err(refused("account_ref_bound"));
    }
    if established.clone().any(|r| binding(r).0 == place) {
        return Err(refused("account_maybe_connected"));
    }
    Ok(())
}

fn print(report: &mut dyn Write, lines: &[String]) -> Result<(), ControlError> {
    for line in lines {
        writeln!(report, "{line}")
            .map_err(|e| ControlError::Journal(format!("the report: {e}")))?;
    }
    Ok(())
}

fn committed_line(connection: &str, at: &Submitted) -> Vec<String> {
    vec![format!(
        "recorded {connection} as event {} at seq {}",
        at.event_id, at.seq
    )]
}

/// What the owner confirms: the record, its cause and the code. Never the personal-data reference.
fn shown_lines(owner: &Owner, request: &Request, code: &str) -> Vec<String> {
    let scopes: BTreeSet<&str> = request.scopes.iter().map(String::as_str).collect();
    let scopes = scopes.into_iter().collect::<Vec<_>>().join(" ");
    let attestation = request.margin_attestation.as_deref().unwrap_or("none");
    vec![
        format!(
            "connection {} broker {} environment {} account_ref {}",
            request.connection_id,
            request.broker,
            owner.environment.as_str(),
            request.account_ref
        ),
        format!("scopes {scopes}"),
        format!("margin_attestation {attestation} cause {}", request.checked),
        format!("code {code}"),
    ]
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
    let environment = match args.environment.as_str() {
        "paper" => Environment::Paper,
        "live" => Environment::Live,
        _ => return Err(refused("environment_invalid").into()),
    };
    let owner = Owner {
        workspace: args.workspace.clone(),
        user: args.user.clone(),
        environment,
    };
    requested(&owner, &args.connection)?;
    let mut journal = PgControlJournal::open(&args.target)?;
    let mut ids = InstantIds::new(&owner, now);
    let code = args.code.as_deref();
    let recorded = record(
        &mut journal,
        &mut ids,
        &owner,
        &args.connection,
        code,
        now,
        report,
    )?;
    Ok(recorded)
}
