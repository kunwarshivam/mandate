//! The platform operator's setting of the customer real-money gate ([DEC-2134] items 3 and 6;
//! journal spec v0.91 §9.36): `mandate-api-server real-money`, run with the deployment's own
//! settings and credentials, never through an HTTP route. It appends one `RealMoneyGateChanged`,
//! written by a `platform_operator` actor as itself (rule 273), to the hosted workspace's control
//! stream.
//!
//! Turning the gate off is always allowed. Turning it on is the founder's (DEC-79, DEC-2135): it is
//! refused unless the deployment setting [`FOUNDER_WORKSPACE`] names exactly one workspace and that
//! workspace is the hosted one, and it must cite its decision. So a customer workspace stays off.
//!
//! [DEC-2134]: ../../../docs/project/decisions/DEC-2134.md

use mandate_api::idempotency::{Derivation, IdempotencyKey, event_id};
use mandate_api::wire::{Id, encode};
use mandate_canon::Value;
use mandate_identity::{PrincipalId, WorkspaceId};
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;

use crate::commands::{Event, PLATFORM_OPERATOR, Settled, Writer, envelope_by, settle_batch_at};
use crate::journal::Journal;
use crate::versions::{object, text};
use crate::writes::repeated;

/// The deployment setting that names the founder's own workspace, the only one the gate may be
/// turned on for. The founder sets it in the deployment; no route, role or agent writes it.
pub const FOUNDER_WORKSPACE: &str = "MANDATE_API_FOUNDER_WORKSPACE";

/// The operation the setting's id is derived for, from the operator and the command's key
/// (workspace API spec §3.4), so a retry appends nothing new.
const OPERATION: &str = "real_money_gate_changed";

/// The setting's record (§9.36).
const CHANGED: &str = "RealMoneyGateChanged";

/// How many digits a decision number may have (§9.36's `decision_id`).
const DECISION_DIGITS: usize = 5;

/// What the operator asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Requested {
    /// Turn the gate on, citing the accepted decision that allows it.
    On { decision: String },
    /// Turn the gate off.
    Off,
}

/// Why the operator command wrote nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// On, with [`FOUNDER_WORKSPACE`] absent or empty.
    FounderWorkspaceUnset,
    /// On, with [`FOUNDER_WORKSPACE`] naming more than one workspace.
    FounderWorkspaceAmbiguous,
    /// On, for a workspace other than the one [`FOUNDER_WORKSPACE`] names.
    NotTheFoundersWorkspace,
    /// On, citing something that is not a `decision_id`.
    Decision,
    /// The operator, workspace or key is not well formed.
    Arguments,
    /// The key already names a different setting.
    KeyReused,
    /// The control stream could not be read, holds no row, or the append did not settle.
    JournalUnavailable,
}

impl Refusal {
    /// The refusal's code, the one line the command prints.
    pub fn code(self) -> &'static str {
        match self {
            Self::FounderWorkspaceUnset => "founder_workspace_unset",
            Self::FounderWorkspaceAmbiguous => "founder_workspace_ambiguous",
            Self::NotTheFoundersWorkspace => "not_the_founders_workspace",
            Self::Decision => "decision",
            Self::Arguments => "arguments",
            Self::KeyReused => "key_reused",
            Self::JournalUnavailable => "journal_unavailable",
        }
    }
}

/// §9.36's `decision_id`: `DEC-` then a decimal number with no leading zero, at most five digits.
fn is_decision_id(value: &str) -> bool {
    value.strip_prefix("DEC-").is_some_and(|number| {
        (1..=DECISION_DIGITS).contains(&number.len())
            && number.bytes().all(|b| b.is_ascii_digit())
            && !number.starts_with('0')
    })
}

/// Whether `requested` may be written for `workspace`, given the value of [`FOUNDER_WORKSPACE`]:
/// off always; on only when the setting names exactly one workspace, separated by commas or
/// whitespace, and it is `workspace`, and only with a `decision_id`.
///
/// # Errors
/// The [`Refusal`] that stops it, the founder setting's before the decision's.
pub fn admitted(
    requested: &Requested,
    workspace: &str,
    founder: Option<&str>,
) -> Result<(), Refusal> {
    let Requested::On { decision } = requested else {
        return Ok(());
    };
    let named: Vec<&str> = founder
        .unwrap_or_default()
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|name| !name.is_empty())
        .collect();
    let founders = match named.as_slice() {
        [] => return Err(Refusal::FounderWorkspaceUnset),
        [one] => *one,
        _ => return Err(Refusal::FounderWorkspaceAmbiguous),
    };
    if founders != workspace {
        return Err(Refusal::NotTheFoundersWorkspace);
    }
    if !is_decision_id(decision) {
        return Err(Refusal::Decision);
    }
    Ok(())
}

/// What one run of the command names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting<'a> {
    /// The hosted workspace, as a ULID.
    pub workspace: &'a str,
    /// The platform operator, as a ULID.
    pub operator: &'a str,
    /// The command's idempotency key (workspace API spec §3.4).
    pub key: &'a str,
    pub requested: Requested,
}

/// The setting's id and canonical draft, written by the operator on `environment`'s control
/// stream at `now`; `None` for an operator, workspace or key that is not well formed.
pub fn drafted(
    setting: &Setting<'_>,
    environment: &str,
    now: UtcNanos,
) -> Option<(String, Vec<u8>)> {
    WorkspaceId::from_ulid_text(setting.workspace).ok()?;
    PrincipalId::from_ulid_text(setting.operator).ok()?;
    let workspace = Id::try_from(setting.workspace.to_owned()).ok()?;
    let principal = Id::try_from(setting.operator.to_owned()).ok()?;
    let key = IdempotencyKey::parse(setting.key).ok()?;
    let id = event_id(&Derivation {
        workspace: &workspace,
        principal: &principal,
        operation: OPERATION,
        key: &key,
        position: None,
    })
    .ok()?;
    let id = String::from_utf8(encode(&id).ok()?).ok()?;
    let id = id.trim_matches('"').to_owned();
    let (enabled, decision) = match &setting.requested {
        Requested::On { decision } => (true, text(decision)),
        Requested::Off => (false, Value::Null),
    };
    let payload = object(vec![
        ("decision", decision),
        ("enabled", Value::Bool(enabled)),
        ("operator", text(setting.operator)),
    ])?;
    let event = Event {
        event_type: CHANGED,
        schema_version: 1,
        payload,
        artifact_refs: Vec::new(),
        config_refs: Vec::new(),
    };
    let writer = Writer {
        workspace: setting.workspace,
        principal: setting.operator,
        event_id: &id,
        environment,
        now,
    };
    let draft = envelope_by(PLATFORM_OPERATOR, &writer, &event, None)?;
    Some((id, draft))
}

/// Writes `setting` for the hosted workspace when [`admitted`] allows it: reads the control
/// stream for its environment, drafts the record, and appends it. A retry under the same key finds
/// the record it appended; the same key for another setting is refused.
///
/// # Errors
/// The [`Refusal`] that stopped it; nothing is appended.
pub async fn set(
    journal: &dyn Journal,
    setting: &Setting<'_>,
    founder: Option<&str>,
    now: UtcNanos,
) -> Result<StoredEvent, Refusal> {
    admitted(&setting.requested, setting.workspace, founder)?;
    let stream = format!("ctl:{}", setting.workspace);
    let rows = journal
        .rows(&stream)
        .await
        .map_err(|_| Refusal::JournalUnavailable)?;
    let environment = rows
        .first()
        .map(|row| row.environment.clone())
        .ok_or(Refusal::JournalUnavailable)?;
    let (id, draft) = drafted(setting, &environment, now).ok_or(Refusal::Arguments)?;
    let settled = match repeated(&rows, &id, &draft) {
        Some(settled) => settled,
        None => settle_batch_at(journal, &stream, &id, &[draft.as_slice()], now, None).await,
    };
    match settled {
        Settled::Stored(row) => Ok(*row),
        Settled::Conflict => Err(Refusal::KeyReused),
        Settled::Unknown | Settled::Unavailable => Err(Refusal::JournalUnavailable),
    }
}

/// What the command line names after `real-money`: `on --decision <DEC-n>` or `off`, then
/// `--operator <ULID>` and `--key <key>`, each flag once, in any order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub requested: Requested,
    pub operator: String,
    pub key: String,
}

/// Parses the arguments after `real-money`.
///
/// # Errors
/// [`Refusal::Arguments`] for anything else: no verb, a repeated or unknown flag, a flag with no
/// value, a decision given with `off`, or none with `on`.
pub fn command<I: IntoIterator<Item = String>>(arguments: I) -> Result<Command, Refusal> {
    let mut arguments = arguments.into_iter();
    let verb = arguments.next().ok_or(Refusal::Arguments)?;
    let (mut decision, mut operator, mut key) = (None, None, None);
    while let Some(flag) = arguments.next() {
        let slot = match flag.as_str() {
            "--decision" => &mut decision,
            "--operator" => &mut operator,
            "--key" => &mut key,
            _ => return Err(Refusal::Arguments),
        };
        let value = arguments.next().ok_or(Refusal::Arguments)?;
        if slot.replace(value).is_some() {
            return Err(Refusal::Arguments);
        }
    }
    let requested = match (verb.as_str(), decision) {
        ("on", Some(decision)) => Requested::On { decision },
        ("off", None) => Requested::Off,
        _ => return Err(Refusal::Arguments),
    };
    Ok(Command {
        requested,
        operator: operator.ok_or(Refusal::Arguments)?,
        key: key.ok_or(Refusal::Arguments)?,
    })
}
