//! The connect check (R0 blocker R1, [DEC-946](../../../docs/project/decisions/DEC-946.md);
//! connections spec §8.1 checks 1, 2, 3 and 7; journal spec §9.8): the connect sequence's
//! executor part for a Robinhood connection, which `mandate connection record` then binds
//! (`--checked`). The runner is the one binary that may dial the broker (DEC-529 item 3), and the
//! CLI, the control services, never calls one (connections spec §5.2), so the check is the
//! runner's `--connect-check` mode.
//!
//! In order: the arguments and the process environment, the `--confirm-live` acknowledgement, the
//! founder-typed account number ([`ACCOUNT_NUMBER_VAR`]), core dumps off, then the login and the
//! reads ([`CheckPorts::observe`]); then [`mandate_connections::checks::run`] over what was read,
//! and `StreamOpened` (a new stream only) and `ConnectionChecked` (occasion `connect`) appended on
//! `acct:{workspace}:{account_ref}` in one batch. A failed check is journaled too, and the mode
//! still refuses ([`PaperError::ConnectCheck`]). The account number is never printed, journaled
//! or kept: the record holds an opaque `account_pii_ref` only.

use mandate_canon::{Int, Key, Object, Value, is_ulid, to_canonical};
use mandate_connections::checks::{
    self, AccountRead, CheckInput, CheckReport, ContractSeen, Granted, Occasion, Outcome,
};
use mandate_connections::record::{AccountPiiRef, AuthKind, Broker, ConnectionId};
use mandate_domain::Environment;
use mandate_journal::{AppendOutcome, StreamId};
use mandate_mcp::Listed;
use mandate_robinhood::{RobinhoodConnector, Tools};
use mandate_shell::envelope::{IdSpace, Ids, account_stream};
use mandate_shell::host::refuse_configured_host;
use mandate_time::UtcNanos;

use crate::PaperError;

/// The variable the founder types the agentic account's number into (DEC-875 item 1: the
/// number comes from the user, never from `get_accounts`). Read once; never printed or stored.
pub const ACCOUNT_NUMBER_VAR: &str = "MANDATE_RH_ACCOUNT_NUMBER";

/// What `--connect-check` was asked for: opaque ids only (TI-8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckArgs {
    pub workspace: String,
    pub account_ref: String,
    pub connection: String,
    /// The journal's DSN, required: the check is nothing until it is journaled.
    pub journal: String,
    pub acknowledged: Environment,
}

/// The contract the first connect listed: its names and whether every allowlisted tool was
/// among them ([`Listed`]), and its hash as lower-case hex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeenContract {
    pub listed: Listed,
    pub hash: String,
}

/// What the connect read through the broker's own host: the contract, `None` when no session
/// was made or its listing could not be read, and whether `get_accounts` showed the recorded
/// account as the one agentic account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observed {
    pub contract: Option<SeenContract>,
    pub agentic_account_read: bool,
}

/// A passing check: the `ConnectionChecked` event id `connection record --checked` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub event_id: String,
}

/// The connect check's effects, the only things the binary supplies.
pub trait CheckPorts {
    /// Turns the process's core dumps off (LT-9), before any credential is read.
    fn harden(&mut self) -> Result<(), PaperError>;

    /// The founder's login at the build's endpoint, then the first connect and the account read
    /// over its session. `Err` only when no login was made, and then nothing is journaled.
    fn observe(&mut self, account_number: &str) -> Result<Observed, PaperError>;

    /// The clock the check is recorded at.
    fn now(&self) -> UtcNanos;

    /// Takes the account stream's ownership: its new writer epoch and its head.
    fn own(&mut self, stream: &StreamId) -> Result<(u64, u64), PaperError>;

    /// Appends `drafts` at `head` under `epoch`, in one batch.
    fn append(
        &mut self,
        stream: &StreamId,
        head: u64,
        epoch: u64,
        at: UtcNanos,
        drafts: &[Vec<u8>],
    ) -> Result<AppendOutcome, PaperError>;
}

/// Parses the arguments after `--connect-check`: `--workspace`, `--account-ref`,
/// `--connection` and `--journal`, each with a value, and exactly one acknowledgement,
/// `--confirm-paper` or `--confirm-live`. Any other argument is unknown, so none can name a host
/// or an account number.
///
/// # Errors
/// [`PaperError::Usage`] for a missing or doubled flag or value, a missing or doubled
/// acknowledgement, or an unknown argument.
pub fn parse_check<I: IntoIterator<Item = String>>(args: I) -> Result<CheckArgs, PaperError> {
    let mut values: [Option<String>; 4] = Default::default();
    let (mut paper, mut live) = (false, false);
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let Some(at) = CHECK_FLAGS.iter().position(|flag| *flag == arg) else {
            match arg.as_str() {
                "--confirm-paper" => paper = true,
                "--confirm-live" => live = true,
                _ => return Err(usage("an argument is unknown".to_owned())),
            }
            continue;
        };
        let value = args.next().filter(|value| !value.starts_with("--"));
        let value = value.ok_or_else(|| usage(format!("{arg} needs a value")))?;
        let slot = values
            .get_mut(at)
            .ok_or_else(|| usage(format!("{arg} is unknown")))?;
        if slot.replace(value).is_some() {
            return Err(usage(format!("{arg} is given twice")));
        }
    }
    let acknowledged = match (paper, live) {
        (true, false) => Environment::Paper,
        (false, true) => Environment::Live,
        (false, false) => return Err(usage("--confirm-live is required".to_owned())),
        (true, true) => {
            return Err(usage(
                "--confirm-paper and --confirm-live are exclusive".to_owned(),
            ));
        }
    };
    let [workspace, account_ref, connection, journal] = values;
    let required = |value: Option<String>, flag: &str| {
        value.ok_or_else(|| usage(format!("{flag} is required")))
    };
    Ok(CheckArgs {
        workspace: required(workspace, "--workspace")?,
        account_ref: required(account_ref, "--account-ref")?,
        connection: required(connection, "--connection")?,
        journal: required(journal, "--journal")?,
        acknowledged,
    })
}

/// The flags of [`parse_check`] that take a value, in its slot order.
const CHECK_FLAGS: [&str; 4] = ["--workspace", "--account-ref", "--connection", "--journal"];

fn usage(message: String) -> PaperError {
    PaperError::Usage(message)
}

/// The connect check, in the module documentation's order. The `StreamOpened` of a new stream
/// and the `ConnectionChecked` are one batch at the head the stream's ownership read, with ids
/// derived from the epoch and the head, so a retried batch is the same batch.
///
/// # Errors
/// Before the login: [`PaperError::Shell`] for an Alpaca variable naming a host,
/// [`PaperError::LiveUnconfirmed`] without `--confirm-live`, [`PaperError::Usage`] for an id that
/// is not one or no account number, and [`CheckPorts::harden`]'s and [`CheckPorts::observe`]'s
/// own errors, after which nothing is journaled. After it: [`PaperError::Journal`] when the batch
/// did not commit, and [`PaperError::ConnectCheck`] when it committed a failed check.
pub fn connect_check<P: CheckPorts>(
    args: &CheckArgs,
    vars: &[(String, String)],
    ports: &mut P,
) -> Result<Checked, PaperError> {
    refuse_configured_host(vars.iter().map(|(name, value)| (name, value)))
        .map_err(PaperError::Shell)?;
    if args.acknowledged != Environment::Live {
        return Err(PaperError::LiveUnconfirmed);
    }
    let connection = ConnectionId::new(&args.connection)
        .map_err(|_| usage("--connection is not an identifier".to_owned()))?;
    if !is_ulid(&args.account_ref) {
        return Err(usage("--account-ref is not a ULID".to_owned()));
    }
    let stream = StreamId::parse(&account_stream(&args.workspace, &args.account_ref))
        .ok_or_else(|| usage("--workspace is not an identifier".to_owned()))?;
    let number = vars
        .iter()
        .find(|(name, _)| name == ACCOUNT_NUMBER_VAR)
        .map(|(_, value)| value.as_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| usage(format!("{ACCOUNT_NUMBER_VAR} is required")))?;
    ports.harden()?;
    let observed = ports.observe(number)?;
    let (epoch, head) = ports.own(&stream)?;
    let ids = Ids {
        space: IdSpace::Account,
    };
    let opened = (head == 0).then(|| ids.derive(epoch, head, 0));
    let checked = ids.derive(epoch, head, u32::from(opened.is_some()));
    let pii = AccountPiiRef::new(&format!("pii_{checked}"))
        .map_err(|_| absent("the account's personal-data reference"))?;
    let report = checks::run(&check_input(connection, &observed, pii))
        .map_err(|_| absent("the connection checks"))?;
    let at = ports.now();
    let mut drafts = Vec::with_capacity(2);
    if let Some(opened) = &opened {
        drafts.push(stream_opened(args, &stream, opened, at)?);
    }
    drafts.push(connection_checked(&stream, &checked, &report, at)?);
    match ports.append(&stream, head, epoch, at, &drafts)? {
        AppendOutcome::Committed(_) | AppendOutcome::AlreadyCommitted(_) => {}
        _ => return Err(PaperError::Journal),
    }
    match report.refusal() {
        None => Ok(Checked { event_id: checked }),
        Some((check, reason)) => Err(PaperError::ConnectCheck {
            event_id: checked,
            check: check.code(),
            reason: reason.code(),
        }),
    }
}

fn absent(what: &'static str) -> PaperError {
    PaperError::Shell(mandate_shell::ShellError::Refused {
        stage: mandate_shell::Stage::Journal,
        cause: mandate_shell::Cause::Absent { what },
    })
}

/// What the connect read: with a contract, the recorded account through `connector`'s own
/// account check (DEC-875 item 1), which passes only for the one agentic account under the
/// recorded number; with none, nothing is called.
pub async fn observe<T: Tools>(
    contract: Option<SeenContract>,
    connector: &RobinhoodConnector<T>,
) -> Observed {
    let agentic_account_read = match contract {
        Some(_) => connector.agentic_account().await.is_ok(),
        None => false,
    };
    Observed {
        contract,
        agentic_account_read,
    }
}

/// The checks' input for a Robinhood MCP connection at its connect (DEC-946 item 3): the listed
/// names are the grant, and with no listing the grant is unread, which the scope check refuses
/// as a mismatch; the account is read only when it was the one agentic account, and is then the
/// dedicated one.
fn check_input(connection: ConnectionId, observed: &Observed, pii: AccountPiiRef) -> CheckInput {
    let granted = match &observed.contract {
        Some(seen) => Granted::Tools(seen.listed.names.clone()),
        None => Granted::KeyPermissions(None),
    };
    let account = if observed.agentic_account_read {
        AccountRead::Read {
            pii_ref: pii,
            dedicated: Some(true),
        }
    } else {
        AccountRead::Unreadable
    };
    CheckInput {
        connection_id: connection,
        broker: Broker::Robinhood,
        environment: mandate_connections::record::Environment::Live,
        auth_kind: AuthKind::McpOauth,
        occasion: Occasion::Connect,
        granted,
        account,
        contract: observed.contract.as_ref().map(|seen| ContractSeen {
            allowlisted_tools_present: seen.listed.allowlisted_present,
            hash: seen.hash.clone(),
        }),
        pinned_contract: None,
    }
}

/// The account stream's `StreamOpened` (journal spec §9.1) for a Robinhood account.
fn stream_opened(
    args: &CheckArgs,
    stream: &StreamId,
    event_id: &str,
    at: UtcNanos,
) -> Result<Vec<u8>, PaperError> {
    let payload = object(vec![
        ("account_ref", text(&args.account_ref)),
        ("broker", text("robinhood")),
        ("stream_type", text("account")),
        ("workspace_id", text(&args.workspace)),
    ])?;
    envelope(stream, event_id, "StreamOpened", at, payload, None)
}

/// `ConnectionChecked` (journal spec §9.8): every result, ascending by check (rule 57), each
/// reason present exactly when it failed (rule 58), and the account's reference exactly when it
/// was read, listed in the envelope's `pii_refs` too (rule 62); its `risk_clock` is the check's
/// time on its whole second (§4.7).
fn connection_checked(
    stream: &StreamId,
    event_id: &str,
    report: &CheckReport,
    at: UtcNanos,
) -> Result<Vec<u8>, PaperError> {
    let mut results = report.results.clone();
    results.sort_by_key(|(check, _)| check.code());
    let results = results
        .iter()
        .map(|(check, outcome)| {
            let (result, reason) = match outcome {
                Outcome::Passed => ("passed", Value::Null),
                Outcome::Failed(reason) => ("failed", text(reason.code())),
            };
            object(vec![
                ("check", text(check.code())),
                ("reason", reason),
                ("result", text(result)),
            ])
        })
        .collect::<Result<Vec<_>, _>>()?;
    let pii = report.account_pii_ref.as_ref().map(AccountPiiRef::as_str);
    let risk_clock = UtcNanos::from_parts(at.secs(), 0).map_err(|_| absent("the risk clock"))?;
    let payload = object(vec![
        ("account_pii_ref", pii.map_or(Value::Null, text)),
        ("connection_id", text(report.connection_id.as_str())),
        ("occasion", text("connect")),
        ("results", Value::Array(results)),
        ("risk_clock", text(&risk_clock.to_string())),
    ])?;
    envelope(stream, event_id, "ConnectionChecked", at, payload, pii)
}

/// One `live` draft written by the account stream's executor (journal spec §3): the fields the
/// journal does not assign at append.
fn envelope(
    stream: &StreamId,
    event_id: &str,
    event_type: &str,
    at: UtcNanos,
    payload: Value,
    pii: Option<&str>,
) -> Result<Vec<u8>, PaperError> {
    let build =
        mandate_canon::Digest::of(concat!("mandate-paper/", env!("CARGO_PKG_VERSION")).as_bytes());
    let actor = object(vec![
        ("build", text(&format!("sha256:{}", build.to_hex()))),
        ("id", text("executor")),
        ("kind", text("system")),
        ("version", text(env!("CARGO_PKG_VERSION"))),
    ])?;
    let one = Int::new(1)
        .map(Value::Int)
        .ok_or_else(|| absent("an envelope integer"))?;
    let fields = object(vec![
        ("actor", actor),
        ("artifact_refs", Value::Array(Vec::new())),
        ("causation_id", Value::Null),
        ("clock_source", text("scheduler")),
        ("config_refs", Value::Object(Object::new())),
        ("correlation_id", Value::Null),
        ("environment", text("live")),
        ("envelope_version", one.clone()),
        ("event_id", text(event_id)),
        ("event_time", text(&at.to_string())),
        ("event_type", text(event_type)),
        ("payload", payload),
        (
            "pii_refs",
            Value::Array(pii.into_iter().map(text).collect()),
        ),
        ("schema_version", one),
        ("stream_id", text(stream.as_str())),
    ])?;
    Ok(to_canonical(&fields))
}

fn text(s: &str) -> Value {
    Value::Str(s.to_owned())
}

fn object(members: Vec<(&'static str, Value)>) -> Result<Value, PaperError> {
    let mut map = Object::new();
    for (name, value) in members {
        let key = Key::new(name).map_err(|_| absent("an envelope member"))?;
        map.insert(key, value);
    }
    Ok(Value::Object(map))
}
