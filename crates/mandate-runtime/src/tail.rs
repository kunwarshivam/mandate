//! A journal read turned into the [`FoldedEvent`]s the runtime folds and is handed, one per row,
//! verbatim and in the journal's own order (journal spec §2, §3; DEC-155 item 2; DEC-914 item 2).
//! It lives with the runtime so that every process that hosts a runtime, the one-shot shell and the
//! long-running daemon alike, reads the journal the same way and none needs the other (E21-30,
//! DEC-1012).

use mandate_canon::{Value, parse};
use mandate_journal::StoredEvent;

use mandate_approval::ActorKind;

use crate::types::{EventId, FoldedEvent, Seq};

/// Why a read could not be folded: a row whose body cannot be read, or holds no payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TailError {
    Body,
    Payload,
}

impl TailError {
    /// The envelope member the refusal names.
    pub fn field(self) -> &'static str {
        match self {
            Self::Body => "stored body",
            Self::Payload => "payload",
        }
    }
}

/// `rows`, the journal's own `ORDER BY seq` read of one stream, as [`FoldedEvent`]s: never
/// re-sorted, never filtered, and with no row's members read from anywhere but the row's own body.
/// The actor is the envelope's own `actor.kind`, mapped fail-closed: `user` folds
/// [`ActorKind::User`]; `system`, `agent`, `broker` and `platform_operator` fold their own kinds;
/// and the `client` of journal spec v0.22 and any kind that cannot be read fold
/// [`ActorKind::System`], which admission never admits (EI-10, DEC-176).
///
/// # Errors
/// [`TailError`] for the first row whose body cannot be read or holds no payload: the whole read is
/// refused, never folded in part (`AGENTS.md` rule 3).
pub fn tailed(rows: &[StoredEvent]) -> Result<Vec<FoldedEvent>, TailError> {
    let mut handed = Vec::new();
    for row in rows {
        let body = parse(&row.body).map_err(|_| TailError::Body)?;
        let payload = body.get("payload").cloned().ok_or(TailError::Payload)?;
        let causation = body
            .get("causation_id")
            .and_then(Value::as_str)
            .map(|id| EventId(id.to_owned()));
        handed.push(FoldedEvent {
            stream: row.stream_id.clone(),
            seq: Seq(row.seq),
            event_id: EventId(row.event_id.clone()),
            event_type: row.event_type.clone(),
            causation_id: causation,
            actor: folded_actor(&body),
            payload,
        });
    }
    Ok(handed)
}

/// The envelope's own `actor.kind`, folded per journal spec §3. The chain names every kind §3 lists
/// in its own arm and no defaulting combinator, and its last arm is the fail-closed fold, so no
/// reading of this map can carry a grant the envelope's writer did not stand behind.
fn folded_actor(body: &Value) -> ActorKind {
    let kind = body
        .get("actor")
        .and_then(|actor| actor.get("kind"))
        .and_then(Value::as_str);
    if kind == Some("user") {
        ActorKind::User
    } else if kind == Some("system") {
        ActorKind::System
    } else if kind == Some("agent") {
        ActorKind::Agent
    } else if kind == Some("broker") {
        ActorKind::Broker
    } else if kind == Some("platform_operator") {
        ActorKind::PlatformOperator
    } else {
        ActorKind::System
    }
}
