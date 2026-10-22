//! The connect check of a paper Alpaca connection (DEC-2007 item 4; connections spec §8.1 checks
//! 1, 2, 3 and 7; journal spec §9.8): the connect sequence's executor part, which `mandate
//! connection record --checked` then binds (DEC-1171). As `mandate-paper`'s Robinhood check does,
//! it reads the account through the broker, runs [`mandate_connections::checks::run`] over what it
//! read, and appends `StreamOpened` (a new stream only) and `ConnectionChecked` (occasion
//! `connect`) on `acct:{workspace}:{account_ref}` in one batch. A failed check is journaled too.
//!
//! It takes the account stream's writer epoch, so it fences an executor already running there,
//! which the restart then replays; it runs before an executor is started for the account. The
//! account's number is never read: the record holds the opaque `account_pii_ref` it is given, and
//! names it in the envelope's `pii_refs` (rule 62).

use mandate_canon::{Object, Value, is_ulid};
use mandate_connections::checks::{
    AccountRead, CheckInput, CheckReport, Granted, Occasion, Outcome, run,
};
use mandate_connections::record::{
    AccountPiiRef, AuthKind, Broker as Venue, ConnectionId, Environment as Reach,
};
use mandate_executor::{AccountScope, BrokerOutcome, BrokerRequest, IdGen, Seq, WriterEpoch};
use mandate_journal::{AppendOutcome, Environment, StreamId};
use mandate_store::Store;
use mandate_store::envelope::{Envelope, Fields, draft_bytes_with_pii, object, text};
use mandate_time::UtcNanos;

use crate::{Broker, VERSION};

/// What a connect check wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    /// The `ConnectionChecked` record's event id, which `connection record --checked` names.
    pub event_id: String,
    /// Whether every check passed.
    pub passed: bool,
}

/// Runs the connect check of `connection` for the account in `scope` at `now`, reading the account
/// through `broker` and naming it by `pii_ref`.
///
/// The check journals the `paper` environment because the only broker this binary reaches is the
/// in-process fixture (DEC-2007 item 1). The change that wires a host transport binds that value
/// to the transport's paper host, so the record can never claim `paper` for another host
/// (`AGENTS.md` rule 8).
///
/// # Errors
/// A stable code: `connection_id_invalid`, `pii_ref_invalid` for a reference that is not `pii_` and
/// a ULID (journal spec §6.4), `stream_unavailable` when the account
/// stream cannot be taken or read, `check_refused` when the checks cannot run, and
/// `append_refused` when the journal does not commit the batch.
pub fn connect_check<S: Store, B: Broker>(
    store: &mut S,
    broker: &mut B,
    ids: &dyn IdGen,
    scope: &AccountScope,
    connection: &str,
    pii_ref: &str,
    now: UtcNanos,
) -> Result<Checked, &'static str> {
    let connection_id = ConnectionId::new(connection).map_err(|_| "connection_id_invalid")?;
    if !pii_ref.strip_prefix("pii_").is_some_and(is_ulid) {
        return Err("pii_ref_invalid");
    }
    let pii = AccountPiiRef::new(pii_ref).map_err(|_| "pii_ref_invalid")?;
    journal_check(store, ids, scope, now, || {
        let account = match broker.call(&BrokerRequest::GetAccount) {
            Ok(BrokerOutcome::Account(_)) => AccountRead::Read {
                pii_ref: pii,
                dedicated: None,
            },
            _ => AccountRead::Unreadable,
        };
        run(&CheckInput {
            connection_id,
            broker: Venue::Alpaca,
            environment: Reach::Paper,
            auth_kind: AuthKind::ApiKey,
            occasion: Occasion::Connect,
            granted: Granted::KeyPermissions(None),
            account,
            contract: None,
            pinned_contract: None,
        })
        .map_err(|_| "check_refused")
    })
}

/// Takes the account stream of `scope`, then has `report` run the checks, and appends
/// `StreamOpened` (a new stream only) and the `ConnectionChecked` of its report at `now` in one
/// batch. `report` runs only once the stream is this writer's, so what it reads or writes for the
/// check happens after any executor already running there is fenced.
///
/// # Errors
/// `stream_unavailable` when the account stream cannot be taken or read, `report`'s own code,
/// `check_refused` when a record cannot be drafted, and `append_refused` when the journal does not
/// commit the batch.
pub(crate) fn journal_check<S: Store>(
    store: &mut S,
    ids: &dyn IdGen,
    scope: &AccountScope,
    now: UtcNanos,
    report: impl FnOnce() -> Result<CheckReport, &'static str>,
) -> Result<Checked, &'static str> {
    let stream = StreamId::parse(&format!("acct:{}:{}", scope.workspace.0, scope.account.0))
        .ok_or("stream_unavailable")?;
    let epoch = store.take_ownership(&stream).ok_or("stream_unavailable")?;
    let head = store
        .rows(&stream)
        .map(|rows| u64::try_from(rows.len()).unwrap_or(u64::MAX))
        .ok_or("stream_unavailable")?;
    let report = report()?;
    let id = |ordinal: u32| ids.event_id(WriterEpoch(epoch), Seq(head), ordinal).0;
    let envelope = Envelope {
        stream: stream.as_str(),
        environment: Environment::Paper,
        actor_kind: "system",
        actor_id: "executor",
        build: concat!("mandate-executord/", env!("CARGO_PKG_VERSION")),
        version: VERSION,
        event_time: now,
    };
    let mut drafts = Vec::new();
    if head == 0 {
        let opened = object(vec![
            ("account_ref", text(&scope.account.0)),
            ("broker", text("alpaca")),
            ("stream_type", text("account")),
            ("workspace_id", text(&scope.workspace.0)),
        ])
        .ok_or("check_refused")?;
        drafts.push(record(&envelope, &id(0), "StreamOpened", &opened, &[])?);
    }
    let checked_id = id(1);
    let pii_refs: Vec<&str> = report
        .account_pii_ref
        .as_ref()
        .map(AccountPiiRef::as_str)
        .into_iter()
        .collect();
    let payload = checked(&report, now)?;
    drafts.push(record(
        &envelope,
        &checked_id,
        "ConnectionChecked",
        &payload,
        &pii_refs,
    )?);
    match store.append(&stream, head, epoch, now, &drafts) {
        AppendOutcome::Committed(_) | AppendOutcome::AlreadyCommitted(_) => Ok(Checked {
            event_id: checked_id,
            passed: report
                .results
                .iter()
                .all(|(_, outcome)| matches!(outcome, Outcome::Passed)),
        }),
        _ => Err("append_refused"),
    }
}

fn record(
    envelope: &Envelope<'_>,
    event_id: &str,
    event_type: &str,
    payload: &Value,
    pii_refs: &[&str],
) -> Result<Vec<u8>, &'static str> {
    draft_bytes_with_pii(
        envelope,
        &Fields {
            event_id,
            event_type,
            schema_version: 1,
            causation_id: None,
            correlation_id: None,
            config_refs: &Object::new(),
            payload,
        },
        pii_refs,
    )
    .ok_or("check_refused")
}

/// `ConnectionChecked`'s payload (journal spec §9.8): every result, ascending by check (rule 57),
/// each reason present exactly when it failed (rule 58), the account's reference exactly when it
/// was read (rule 62), and the check's time on its whole second as `risk_clock` (§4.7).
fn checked(report: &CheckReport, now: UtcNanos) -> Result<Value, &'static str> {
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
            .ok_or("check_refused")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let risk_clock = UtcNanos::from_parts(now.secs(), 0).map_err(|_| "check_refused")?;
    object(vec![
        (
            "account_pii_ref",
            report
                .account_pii_ref
                .as_ref()
                .map_or(Value::Null, |p| text(p.as_str())),
        ),
        ("connection_id", text(report.connection_id.as_str())),
        ("occasion", text("connect")),
        ("results", Value::Array(results)),
        ("risk_clock", text(&risk_clock.to_string())),
    ])
    .ok_or("check_refused")
}
