//! The customer real-money gate ([DEC-2134]; journal spec v0.91 §9.36): one decision in front of
//! every workspace API point that can name an environment other than `paper`, creating a mandate
//! version, deploying, starting a connect, and going live.
//!
//! The setting is the `enabled` of the latest `RealMoneyGateChanged` on the workspace's control
//! stream, by `seq`, and off with none or with one this fold cannot read (`AGENTS.md` rule 3). No
//! route here writes it: only a platform operator does (rule 273, [DEC-2134] items 3 and 6). While
//! it is off, a request that names another environment is refused, and its `RealMoneyRefused` is
//! appended before the answer ([DEC-2134] item 4). While it is on, the request passes the gate and
//! meets every check after it unchanged: on is necessary, never sufficient ([DEC-2134] item 5).
//!
//! [DEC-2134]: ../../../docs/project/decisions/DEC-2134.md

use mandate_canon::{Value, parse};
use mandate_journal::StoredEvent;

use crate::commands::{Event, Settled, Writer, envelope};
use crate::http::App;
use crate::routes::{Reader, Refused};
use crate::versions::{object, text};
use crate::writes::{Gate, appended_batch_at, derived_for, principal_text};

/// The setting's record, which this module reads and never writes.
const CHANGED: &str = "RealMoneyGateChanged";
/// The refusal's record (§9.36).
const REFUSED: &str = "RealMoneyRefused";
/// The one environment the gate never judges.
const PAPER: &str = "paper";

/// The workspace's real-money setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Off,
    On,
}

/// What the gate makes of a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// The request names `paper`, which the gate does not judge.
    NotGated,
    /// The setting is off: refused, and the refusal journaled.
    Refused,
    /// The setting is on: the request goes on to the checks after the gate, unchanged.
    Admitted,
}

/// What a gated request was, as `RealMoneyRefused` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    CreateVersion,
    Deploy,
    Connect,
    GoLive,
}

impl Action {
    /// The record's `action`.
    fn wire(self) -> &'static str {
        match self {
            Self::CreateVersion => "create_version",
            Self::Deploy => "deploy",
            Self::Connect => "connect",
            Self::GoLive => "go_live",
        }
    }

    /// The operation the refusal's id is derived for (workspace API spec §3.4): its own, so it
    /// never takes the id of the command it refuses.
    fn operation(self) -> String {
        format!("{REFUSED_OPERATION}_{}", self.wire())
    }
}

/// The prefix of every refusal's operation.
const REFUSED_OPERATION: &str = "real_money_refused";

/// The setting `control` holds: the latest `RealMoneyGateChanged` by `seq`, on only when its body
/// is JSON whose `payload.enabled` is the boolean `true`.
pub fn setting(control: &[StoredEvent]) -> Setting {
    let latest = control
        .iter()
        .filter(|row| row.event_type == CHANGED)
        .max_by_key(|row| row.seq);
    let enabled = latest
        .and_then(|row| parse(&row.body).ok())
        .and_then(|envelope| envelope.get("payload")?.get("enabled").cloned());
    if enabled == Some(Value::Bool(true)) {
        Setting::On
    } else {
        Setting::Off
    }
}

/// The gate's decision on a request naming `requested` under `setting`: exactly `paper` is not
/// judged; any other value is refused while off and admitted past the gate while on.
pub fn decide(setting: Setting, requested: &str) -> Decision {
    if requested == PAPER {
        return Decision::NotGated;
    }
    match setting {
        Setting::Off => Decision::Refused,
        Setting::On => Decision::Admitted,
    }
}

/// Judges a request for `action` that names `live`, by the principal `gate` admitted, against the
/// setting on `reader`'s control stream; a refusal is appended before this returns. `Ok` is the
/// decision, which the caller answers by its later checks; `Err` only for a refusal the journal
/// did not take, which is still a refusal.
///
/// # Errors
/// [`Refused::JournalUnavailable`] when the refusal cannot be drafted or appended.
pub(crate) async fn gated(
    app: &App,
    reader: &Reader,
    gate: &Gate,
    action: Action,
) -> Result<Decision, Refused> {
    let decision = decide(setting(&reader.control), "live");
    if decision != Decision::Refused {
        return Ok(decision);
    }
    let unavailable = Refused::JournalUnavailable;
    let principal = principal_text(gate).ok_or(unavailable)?;
    let environment = reader
        .control
        .first()
        .map(|row| row.environment.clone())
        .ok_or(unavailable)?;
    let (_, id) = derived_for(gate, &reader.workspace, &action.operation()).ok_or(unavailable)?;
    let payload = object(vec![
        ("action", text(action.wire())),
        ("principal", text(&principal)),
        ("reason", text("gate_off")),
        ("requested", text("live")),
    ])
    .ok_or(unavailable)?;
    let event = Event {
        event_type: REFUSED,
        schema_version: 1,
        payload,
        artifact_refs: Vec::new(),
        config_refs: Vec::new(),
    };
    let writer = Writer {
        workspace: &reader.workspace,
        principal: &principal,
        event_id: &id,
        environment: &environment,
        now: gate.now,
    };
    let draft = envelope(&writer, &event).ok_or(unavailable)?;
    match appended_batch_at(app, reader, (&id, &[draft.as_slice()]), gate.now, None).await {
        Settled::Stored(_) => Ok(decision),
        Settled::Conflict | Settled::Unknown | Settled::Unavailable => Err(unavailable),
    }
}
