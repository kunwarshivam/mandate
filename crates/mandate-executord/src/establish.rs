//! Journal spec rule 68 at the executor (DEC-2341): its account stream is `connecting` until it
//! holds the copy of its connection's `ConnectionEstablished`, and while connecting the executor
//! opens no risk. It writes the copy itself, once the control services have established the
//! connection its stream's latest `connect` check passed for, and leaves when a refusal or a
//! revocation closes that connection. Nothing here blocks risk reduction: the gate withholds the
//! binding input from openings alone (`AGENTS.md` rule 13).

use std::cell::Cell;
use std::rc::Rc;

use mandate_canon::{Key, Object, Value};
use mandate_executor::{
    AccountScope, BindingGateInput, BindingGateRequest, BindingGateSource, EventDraft, EventId,
};
use mandate_journal::{Environment, StoredEvent, StreamId};
use mandate_store::Store;
use mandate_time::UtcNanos;

use crate::Broker;
use crate::follow::folded;
use crate::process::ExecutorProcess;

const ESTABLISHED: &str = "ConnectionEstablished";
const REFUSED: &str = "ConnectionRefused";
const REVOKED: &str = "ConnectionRevoked";

/// The control stream's establishment the executor copies: its event id, which the copy names as
/// its `causation_id`, and its payload's members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToCopy {
    pub original: EventId,
    pub members: Object,
}

/// Where the account stream stands under rule 68 (DEC-2341 item 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Establishment {
    /// The account stream holds its establishment's copy.
    Established,
    /// The control stream established the connection the latest `connect` check passed for, and
    /// the account stream holds no copy yet.
    ToCopy(ToCopy),
    /// A `ConnectionRefused` of occasion `connect` or a `ConnectionRevoked` closed the connection
    /// before its copy.
    Closed,
    /// None of these: the stream opens no risk.
    Connecting,
}

/// The state of the account stream in `scope` from its rows `account` and the control stream's
/// rows `control` in `environment`: [`Establishment::Established`] once the stream holds a copy;
/// otherwise [`Establishment::Closed`] when the connection of its latest `connect` check is
/// refused or revoked; otherwise [`Establishment::ToCopy`] for a version-2 `ConnectionEstablished`
/// whose `causation_id` is that check, which passed every result, naming its `connection_id` and
/// this stream's `account_ref`; and [`Establishment::Connecting`] for anything else, no check
/// included.
///
/// # Errors
/// `connection_record_unreadable` for a connection record it must read and cannot: the start is
/// refused rather than guessed (DEC-85).
pub fn establishment(
    account: &[StoredEvent],
    control: &[StoredEvent],
    scope: &AccountScope,
    environment: Environment,
) -> Result<Establishment, &'static str> {
    let unreadable = "connection_record_unreadable";
    if account.iter().any(|row| row.event_type == ESTABLISHED) {
        return Ok(Establishment::Established);
    }
    let mut latest = None;
    for row in account
        .iter()
        .filter(|row| row.event_type == "ConnectionChecked")
    {
        let event = folded(row).map_err(|_| unreadable)?;
        let member = |name: &str| event.payload.get(name).and_then(Value::as_str);
        let connection = member("connection_id").ok_or(unreadable)?.to_owned();
        if member("occasion").ok_or(unreadable)? == "connect" {
            let passed = matches!(
                event.payload.get("results"),
                Some(Value::Array(results)) if !results.is_empty()
                    && results
                        .iter()
                        .all(|result| result.get("result").and_then(Value::as_str) == Some("passed"))
            );
            latest = Some((row.event_id.clone(), connection, passed));
        }
    }
    let Some((check, connection, passed)) = latest else {
        return Ok(Establishment::Connecting);
    };
    let mut due = None;
    let mut closed = false;
    for row in control
        .iter()
        .filter(|row| row.environment == environment.as_str())
        .filter(|row| matches!(row.event_type.as_str(), ESTABLISHED | REFUSED | REVOKED))
    {
        let event = folded(row).map_err(|_| unreadable)?;
        let member = |name: &str| event.payload.get(name).and_then(Value::as_str);
        if member("connection_id").ok_or(unreadable)? != connection {
            continue;
        }
        match (row.event_type.as_str(), row.schema_version) {
            (REFUSED, _) => closed |= member("occasion").ok_or(unreadable)? == "connect",
            (REVOKED, _) => closed = true,
            (_, 2)
                if event.causation_id.as_ref().map(|id| id.0.as_str()) == Some(check.as_str())
                    && member("account_ref") == Some(scope.account.0.as_str()) =>
            {
                let Value::Object(members) = &event.payload else {
                    return Err(unreadable);
                };
                due = Some(ToCopy {
                    original: EventId(row.event_id.clone()),
                    members: members.clone(),
                });
            }
            _ => {}
        }
    }
    Ok(match (closed, passed, due) {
        (true, _, _) => Establishment::Closed,
        (false, true, Some(due)) => Establishment::ToCopy(due),
        _ => Establishment::Connecting,
    })
}

/// The copy of `copy` with id `event_id` at `now`: `ConnectionEstablished` version 2 on the
/// account stream, the original's members and `risk_clock` (`now` on its whole second, journal
/// spec §4.7), with the original as `causation_id` (rules 61 and 68).
///
/// # Errors
/// `copy_refused` when the original's members already hold `risk_clock`, or the time has no whole
/// second the journal writes.
pub fn copy_draft(
    copy: &ToCopy,
    event_id: EventId,
    now: UtcNanos,
) -> Result<EventDraft, &'static str> {
    let refused = "copy_refused";
    let key = Key::new("risk_clock").map_err(|_| refused)?;
    if copy.members.contains_key(&key) {
        return Err(refused);
    }
    let clock = UtcNanos::from_parts(now.secs(), 0).map_err(|_| refused)?;
    let mut members = copy.members.clone();
    members.insert(key, Value::Str(clock.to_string()));
    Ok(EventDraft {
        event_id,
        event_type: ESTABLISHED.to_owned(),
        schema_version: 2,
        config_refs: Object::new(),
        causation_id: Some(copy.original.clone()),
        payload: Value::Object(members),
    })
}

/// Whether openings may reach the binding gate: shut while the account stream is connecting, and
/// opened once, by its copy. Clones share one state.
#[derive(Debug, Clone, Default)]
pub struct OpeningGate(Rc<Cell<bool>>);

impl OpeningGate {
    /// Lets openings through from now on.
    pub fn open(&self) {
        self.0.set(true);
    }

    /// Whether openings get through.
    pub fn is_open(&self) -> bool {
        self.0.get()
    }

    /// `inner`, answering no request whose purpose adds risk while this gate is shut, and every
    /// other request as `inner` does, so a kill switch's flatten, a protective order and every
    /// exit keep their input (`AGENTS.md` rule 13).
    pub fn guard(&self, inner: Box<dyn BindingGateSource>) -> Box<dyn BindingGateSource> {
        Box::new(Guarded {
            gate: self.clone(),
            inner,
        })
    }
}

struct Guarded {
    gate: OpeningGate,
    inner: Box<dyn BindingGateSource>,
}

impl BindingGateSource for Guarded {
    fn input(&self, request: &BindingGateRequest<'_>) -> Option<BindingGateInput> {
        if request.purpose.adds_risk() && !self.gate.is_open() {
            return None;
        }
        self.inner.input(request)
    }
}

/// What a pass found the account stream to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connected {
    /// Established, its copy written now if it was not before: `gate` is open.
    Open,
    /// Still connecting: `gate` stays shut.
    Connecting,
    /// Refused or revoked before its copy: the caller drains.
    Closed,
}

/// One pass of the rule-68 gate over `process`'s journal at `now`: reads the account and control
/// streams of `scope`, writes the establishment's copy through `process` when it is due and the
/// process has started (until then the stream is still connecting), and opens `gate` once the
/// stream holds it. A gate once open stays open.
///
/// # Errors
/// [`establishment`]'s code, `stream_unavailable` when a stream cannot be read, and
/// `copy_refused` when the copy does not commit.
pub fn pass<S: Store, B: Broker>(
    process: &mut ExecutorProcess<S, B>,
    gate: &OpeningGate,
    scope: &AccountScope,
    environment: Environment,
    now: UtcNanos,
) -> Result<Connected, &'static str> {
    let stream = |id: String| StreamId::parse(&id).ok_or("stream_unavailable");
    let account = stream(format!("acct:{}:{}", scope.workspace.0, scope.account.0))?;
    let control = stream(format!("ctl:{}", scope.workspace.0))?;
    let rows = |id: &StreamId| process.store().rows(id).ok_or("stream_unavailable");
    let state = establishment(&rows(&account)?, &rows(&control)?, scope, environment)?;
    match state {
        Establishment::Established => {}
        Establishment::ToCopy(_) if !process.state().started() => {
            return Ok(Connected::Connecting);
        }
        Establishment::ToCopy(due) => process.copy_establishment(now, &due)?,
        Establishment::Closed => return Ok(Connected::Closed),
        Establishment::Connecting => return Ok(Connected::Connecting),
    }
    gate.open();
    Ok(Connected::Open)
}
