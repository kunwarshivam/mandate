//! The RFC 9457 problem document every error is (workspace API spec §3.5).

use serde::{Deserialize, Serialize};

use crate::Unimplemented;
use crate::wire::EventId;

/// One error response. `title` is generic text fixed by `code`, never content (rule 6, API-10);
/// `event_id` is present exactly when `effect` is not [`Effect::None`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    #[serde(rename = "type")]
    pub type_uri: String,
    pub status: u16,
    pub code: ProblemCode,
    pub title: String,
    pub effect: Effect,
    pub event_id: Option<EventId>,
    pub retryable: bool,
    pub violations: Vec<Violation>,
}

impl Problem {
    /// The problem for `code`, with the status §3.5's table gives it, its fixed title and type
    /// URI, and `retryable` for `journal_unavailable` and `rate_limited` only (DEC-681 item 9). An
    /// `event_id` with
    /// [`Effect::None`], or none with `recorded` or `unknown`, is refused.
    ///
    /// # Errors
    /// [`ProblemError::EventIdMismatch`] for that mismatch.
    pub fn of(
        code: ProblemCode,
        effect: Effect,
        event_id: Option<EventId>,
    ) -> Result<Self, ProblemError> {
        let _ = (code, effect, event_id);
        Err(ProblemError::Unimplemented(Unimplemented))
    }
}

/// The stable machine codes of §3.5's table. Closed on the server: it can emit no other code
/// (DEC-681 item 1). An operation's own codes join this enum with the operation's story.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProblemCode {
    Unauthenticated,
    Forbidden,
    NotFound,
    Invalid,
    IdempotencyConflict,
    StaleBase,
    ClassificationChanged,
    StepUpRequired,
    LiveUnavailable,
    ControlStreamFrozen,
    JournalUnavailable,
    RateLimited,
}

/// What the failed call left in the journal (§3.5), a closed safety enum (§3.2): the web app
/// renders "nothing was sent" or "the result is unknown; we are checking" from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    None,
    Recorded,
    Unknown,
}

/// One validation finding: a JSON pointer, a code (a mandate V-code, a policy key, or a wire
/// code), and generic text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Violation {
    pub path: String,
    pub code: String,
    pub message: String,
}

/// Why [`Problem::of`] refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProblemError {
    #[error(transparent)]
    Unimplemented(Unimplemented),
    #[error("event_id must be present exactly when effect is recorded or unknown")]
    EventIdMismatch,
}
