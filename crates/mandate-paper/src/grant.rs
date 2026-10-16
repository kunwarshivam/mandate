//! The grant an `ask` opening waits for before anything is submitted (E7-26, the
//! [first live trade brief](../../../docs/project/tasks/first-live-trade.md)'s G1b): the mandate
//! says `ask` for every opening, so the cycle builds the order, the gate passes it, and it waits
//! for the founder's grant in the CLI inbox ([DEC-529](../../../docs/project/decisions/DEC-529.md)
//! item 1; mandate spec §6.1, §6.4). The order is sent only after an answer whose content hash
//! equals the hash of the request the run journaled; any difference is a refusal with nothing
//! sent (LT-4, [DEC-912](../../../docs/project/decisions/DEC-912.md) item 1). Admission —
//! mandate spec §6.4's checks and re-validation — stays the runtime's, and the wait releases only
//! on the runtime's own records of an admitted, re-validated grant, never on a raw answer
//! ([DEC-920](../../../docs/project/decisions/DEC-920.md)): this seam is the runner's own door, the
//! second check on the one order a live run holds. No host, no credential and no order is reached
//! from here (ES-19, `AGENTS.md` rule 8, LT-1).

use mandate_canon::Value;
use mandate_shell::control::ControlRecord;
use mandate_time::UtcNanos;

/// The `ask` the run journaled ([DEC-912](../../../docs/project/decisions/DEC-912.md) item 1;
/// mandate spec §6.4): the approval every answer names, the content hash every grant repeats,
/// and the deadline a silent ask dies at — the request's risk clock plus the mandate's
/// `autonomy.approval.timeout_s`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    /// The `ApprovalRequested`'s event id: the only approval whose answers this seam reads.
    pub approval: String,
    /// The request's content hash (`sha256:…`): the hash of exactly the order the approver was
    /// shown and the run would send (LT-4).
    pub content_hash: String,
    /// The request's deadline: at or after it a grant is late and the timeout denies (rule 3).
    pub deadline: UtcNanos,
}

/// One journaled row as the grant wait's fold reads it
/// ([DEC-920](../../../docs/project/decisions/DEC-920.md) item 1): its record — `seq`,
/// `event_type` and the body's `payload` member, the mapping the run's control-stream read
/// performs — beside the two envelope members the fold links the runtime's records by (journal
/// spec §3): the row's own `event_id`, and its `causation_id`, or [`None`] when the body carries
/// none or one that is not text, which links nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Journaled {
    /// The row's own event id.
    pub event_id: String,
    /// The envelope's `causation_id`: for the runtime's `ApprovalResponded`, the
    /// `ApprovalResponseSubmitted` it copies (journal spec §9.7 rule 50); for its
    /// `ApprovalRevalidated`, the `ApprovalResponded` it re-validates.
    pub causation_id: Option<String>,
    /// The row's record.
    pub record: ControlRecord,
}

/// Why an `ask` opening's submit is held or ended ([DEC-912](../../../docs/project/decisions/DEC-912.md)
/// item 1): each refusal is a stop after which nothing is sent by the run, each with its own
/// reason code (ADR-0001 ES-09).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GrantRefusal {
    /// The body of every stub in the tests PR (DEC-77).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    /// No admitted grant yet, before the ask's deadline: the submit stays blocked and the run
    /// keeps waiting (LT-4: sent only after a grant); nothing is sent.
    #[error("the ask waits unanswered")]
    Unanswered,
    /// An answer repeats another content hash than the ask's, or a record the wait reads holds a
    /// member the run cannot read: a refusal with nothing sent (LT-4).
    #[error("the answer repeats another content hash, so nothing is sent")]
    Mismatched,
    /// A `skipped` answer of the ask, or a re-validation that skips: the deny ends it; nothing is
    /// sent.
    #[error("the ask was skipped, so nothing is sent")]
    Denied,
    /// The ask's deadline arrived with no admissible grant: the timeout is a deny (rule 3), and
    /// a grant read at or after the deadline is late (mandate spec §6.4 check 2); nothing is
    /// sent.
    #[error("the ask reached its deadline with no grant")]
    TimedOut,
}

impl GrantRefusal {
    /// The stable reason code of each refusal (ADR-0001 ES-09): `approval_unanswered`,
    /// `content_mismatch`, `approval_skipped`, `approval_timed_out`, and the stub's
    /// `unimplemented`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unimplemented { .. } => "unimplemented",
            Self::Unanswered => "approval_unanswered",
            Self::Mismatched => "content_mismatch",
            Self::Denied => "approval_skipped",
            Self::TimedOut => "approval_timed_out",
        }
    }
}

/// LT-4 ([DEC-912](../../../docs/project/decisions/DEC-912.md) item 1,
/// [DEC-920](../../../docs/project/decisions/DEC-920.md) items 1 to 3): whether the `ask`
/// opening may leave. `control` is the workspace control stream's read and `agent` the agent
/// stream's, each in journal order. The answers are `control`'s `ApprovalResponseSubmitted`
/// rows for the ask's own approval, folded in journal order. An answer whose `verdict` or
/// `content_hash` the run cannot read, whose verdict is neither `approved` nor `skipped`
/// (journal spec §9.7, DEC-533), or which repeats any other content hash than the ask's refuses
/// as a difference with nothing sent; a `skipped` answer repeating the ask's hash denies
/// whenever it is read, admitted or not, because a deny never adds risk (rule 3).
///
/// An `approved` answer repeating the ask's hash is never a release by itself (DEC-920 item 1):
/// admission — mandate spec §6.4's checks 1 to 7, the responder a `user` in the approver set
/// among them (check 3) — and re-validation (checks 8 to 12) are the runtime's (DEC-912 item 1:
/// one admission), and what the wait reads of them is the runtime's own records on `agent`. The
/// answer's copy is the `ApprovalResponded` whose `causation_id` is the answer's event id (rule
/// 50): with none yet the wait holds; a copy naming another approval or another verdict refuses
/// as a difference; a `refused` or `counted` copy is not terminal (§6.4's lifecycle), so the
/// fold reads on to the ask's next answer; a result that is none of the three refuses as a
/// difference. An `admitted` copy goes to its re-validation, the `ApprovalRevalidated` whose
/// `causation_id` is the copy's event id: with none yet the wait holds; `act` releases the submit
/// while `now` is before the ask's deadline (at or after it the grant is late, check 2 and rule
/// 3); `skip` denies; and one naming another approval, or a result the run cannot read, refuses
/// as a difference. With no answer that decides, the submit stays blocked before the deadline
/// and the timeout denies at it. While the release holds, the cycle's own one-order door (FT-1)
/// still decides what is submitted: this seam releases, it never submits.
///
/// `control` and `agent` arrive read once each, in journal order (`ORDER BY seq`, the journal's
/// own read), so the first answer of the approval in the fold is the first on the stream.
///
/// # Errors
/// Any [`GrantRefusal`] that applies; every one of them is a stop with nothing sent.
pub fn grant(
    control: &[Journaled],
    agent: &[Journaled],
    ask: &Asked,
    now: UtcNanos,
) -> Result<(), GrantRefusal> {
    let held = || {
        if now >= ask.deadline {
            GrantRefusal::TimedOut
        } else {
            GrantRefusal::Unanswered
        }
    };
    let ours = |row: &Journaled| member(&row.record, "approval") == Some(ask.approval.as_str());
    let answering =
        |row: &&Journaled| row.record.event_type == "ApprovalResponseSubmitted" && ours(row);
    for answer in control.iter().filter(answering) {
        let (verdict, content_hash) = match (
            member(&answer.record, "verdict"),
            member(&answer.record, "content_hash"),
        ) {
            (Some(verdict), Some(content_hash)) => (verdict, content_hash),
            _ => return Err(GrantRefusal::Mismatched),
        };
        if content_hash != ask.content_hash {
            return Err(GrantRefusal::Mismatched);
        }
        match verdict {
            "approved" => {}
            "skipped" => return Err(GrantRefusal::Denied),
            _ => return Err(GrantRefusal::Mismatched),
        }
        let Some(copy) = caused_by(agent, "ApprovalResponded", &answer.event_id) else {
            return Err(held());
        };
        if !ours(copy) || member(&copy.record, "verdict") != Some("approved") {
            return Err(GrantRefusal::Mismatched);
        }
        match member(&copy.record, "result") {
            Some("admitted") => {}
            Some("refused" | "counted") => continue,
            _ => return Err(GrantRefusal::Mismatched),
        }
        let Some(revalidation) = caused_by(agent, "ApprovalRevalidated", &copy.event_id) else {
            return Err(held());
        };
        if !ours(revalidation) {
            return Err(GrantRefusal::Mismatched);
        }
        return match member(&revalidation.record, "result") {
            Some("act") if now < ask.deadline => Ok(()),
            Some("act") => Err(GrantRefusal::TimedOut),
            Some("skip") => Err(GrantRefusal::Denied),
            _ => Err(GrantRefusal::Mismatched),
        };
    }
    Err(held())
}

/// The first of `rows`, in journal order, of `event_type` whose `causation_id` is `cause`: the
/// runtime's record of the event `cause` names (journal spec §9.7 rule 50; the workspace API
/// spec's trace, `ApprovalRevalidated` → `ApprovalResponded` → `ApprovalResponseSubmitted`).
fn caused_by<'a>(rows: &'a [Journaled], event_type: &str, cause: &str) -> Option<&'a Journaled> {
    rows.iter().find(|row| {
        row.record.event_type == event_type && row.causation_id.as_deref() == Some(cause)
    })
}

/// One text member of `record`'s payload, or [`None`] when it is missing or holds not text: a
/// member the run cannot read is never a guess at what the answer meant (`AGENTS.md` rule 3,
/// [DEC-912](../../../docs/project/decisions/DEC-912.md) item 1), and the answer that holds it
/// refuses as a difference.
fn member<'a>(record: &'a ControlRecord, name: &str) -> Option<&'a str> {
    record.payload.get(name).and_then(Value::as_str)
}
