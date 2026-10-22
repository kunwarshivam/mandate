//! Journal spec rule 68 at the executor (DEC-2341): its account stream is `connecting` until it
//! holds the copy of its connection's `ConnectionEstablished`, and while connecting the executor
//! opens no risk. It writes the copy itself, once the control services have established the
//! connection its stream's latest `connect` check passed for, and leaves when a refusal or a
//! revocation closes that connection. Nothing here blocks risk reduction: the gate withholds the
//! binding input from openings alone (`AGENTS.md` rule 13).

use std::cell::Cell;
use std::rc::Rc;

use mandate_canon::Object;
use mandate_executor::{
    AccountScope, BindingGateInput, BindingGateRequest, BindingGateSource, EventDraft, EventId,
};
use mandate_journal::{Environment, StoredEvent};
use mandate_time::UtcNanos;

use crate::credential::Unimplemented;

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
    let _ = (account, control, scope, environment);
    Err(Unimplemented)
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
    let _ = (copy, event_id, now);
    Err(Unimplemented)
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
