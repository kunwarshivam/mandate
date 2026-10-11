//! The grant an `ask` opening waits for before anything is submitted (E7-26, the
//! [first live trade brief](../../../docs/project/tasks/first-live-trade.md)'s G1b): the mandate
//! says `ask` for every opening, so the cycle builds the order, the gate passes it, and it waits
//! for the founder's grant in the CLI inbox ([DEC-529](../../../docs/project/decisions/DEC-529.md)
//! item 1; mandate spec §6.1, §6.4). The order is sent only after an answer whose content hash
//! equals the hash of the request the run journaled; any difference is a refusal with nothing
//! sent (LT-4, [DEC-912](../../../docs/project/decisions/DEC-912.md) item 1). Admission —
//! mandate spec §6.4's checks and re-validation — stays the runtime's: this seam is the
//! runner's own door, the second check on the one order a live run holds. No host, no credential
//! and no order is reached from here (ES-19, `AGENTS.md` rule 8, LT-1).

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

/// Why an `ask` opening's submit is held or ended ([DEC-912](../../../docs/project/decisions/DEC-912.md)
/// item 1): each refusal is a stop after which nothing is sent by the run, each with its own
/// reason code (ADR-0001 ES-09).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GrantRefusal {
    /// The body of every stub in the tests PR (DEC-77).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    /// No admissible answer yet, before the ask's deadline: the submit stays blocked and the
    /// run keeps waiting (LT-4: sent only after a grant); nothing is sent.
    #[error("the ask waits unanswered")]
    Unanswered,
    /// An answer repeats another content hash than the ask's, or holds a member the run cannot
    /// read: a refusal with nothing sent (LT-4).
    #[error("the answer repeats another content hash, so nothing is sent")]
    Mismatched,
    /// An admitted `skipped` answer of the ask: the deny ends it; nothing is sent.
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

/// LT-4 ([DEC-912](../../../docs/project/decisions/DEC-912.md) item 1): whether the `ask`
/// opening may leave. The answers are `records`' `ApprovalResponseSubmitted` rows for the ask's
/// own approval, folded in journal order, and the first of them decides: one repeating the
/// ask's content hash releases the submit when its verdict is `approved` and `now` is before
/// the ask's deadline (the same answer at or after the deadline denies, mandate spec §6.4 check 2
/// and rule 3), one whose verdict is `skipped` denies whenever it is read, and one repeating any
/// other content hash — or holding a member the run cannot read — refuses as a difference, with
/// nothing sent; a verdict that is neither `approved` nor `skipped` is no verdict a
/// control-stream answer holds (journal spec §9.7, DEC-533), so it refuses as a difference
/// rather than a guess at what it meant (rule 3). With no answer of this ask, the submit stays
/// blocked before the deadline and the timeout denies at it. While the release holds, the
/// cycle's own one-order door (FT-1) still decides what is submitted, and the runtime's
/// admission still stands in front of both: this seam releases, it never submits.
///
/// `records` arrive read once, in journal order (`ORDER BY seq`, the journal's own read), so
/// the first answer of the approval in the fold is the first on the stream.
///
/// # Errors
/// Any [`GrantRefusal`] that applies; every one of them is a stop with nothing sent.
pub fn grant(records: &[ControlRecord], ask: &Asked, now: UtcNanos) -> Result<(), GrantRefusal> {
    let answering = |record: &ControlRecord| {
        record.event_type == "ApprovalResponseSubmitted"
            && member(record, "approval") == Some(ask.approval.as_str())
    };
    let Some(answer) = records.iter().find(|record| answering(record)) else {
        return Err(if now >= ask.deadline {
            GrantRefusal::TimedOut
        } else {
            GrantRefusal::Unanswered
        });
    };
    let (verdict, content_hash) = match (member(answer, "verdict"), member(answer, "content_hash"))
    {
        (Some(verdict), Some(content_hash)) => (verdict, content_hash),
        _ => return Err(GrantRefusal::Mismatched),
    };
    if content_hash != ask.content_hash {
        return Err(GrantRefusal::Mismatched);
    }
    match verdict {
        "approved" if now < ask.deadline => Ok(()),
        "approved" => Err(GrantRefusal::TimedOut),
        "skipped" => Err(GrantRefusal::Denied),
        _ => Err(GrantRefusal::Mismatched),
    }
}

/// One text member of `record`'s payload, or [`None`] when it is missing or holds not text: a
/// member the run cannot read is never a guess at what the answer meant (`AGENTS.md` rule 3,
/// [DEC-912](../../../docs/project/decisions/DEC-912.md) item 1), and the answer that holds it
/// refuses as a difference.
fn member<'a>(record: &'a ControlRecord, name: &str) -> Option<&'a str> {
    record.payload.get(name).and_then(Value::as_str)
}
