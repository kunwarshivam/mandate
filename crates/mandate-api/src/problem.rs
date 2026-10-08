//! The RFC 9457 problem document every error is (workspace API spec §3.5).

use serde::{Deserialize, Serialize};

use crate::Unimplemented;
use crate::wire::{Decimal, EventId};

/// One error response. `title` is generic text fixed by `code`, never content (rule 6, API-10);
/// `event_id` is present exactly when `effect` is not [`Effect::None`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// The problem for `code`: the status §3.5's table gives it, the type URI
    /// `https://mandate.dev/problems/<code>`, a title fixed by the code that names no content, no
    /// violations, and `retryable` for `journal_unavailable` and `rate_limited` only, with
    /// `membership_unavailable` once #766's codes are served (DEC-681 items 9 and 11).
    ///
    /// # Errors
    /// [`ProblemError::EventIdMismatch`] for an `event_id` with [`Effect::None`], or none with
    /// `recorded` or `unknown`; [`ProblemError::EffectNotAllowed`] for an effect the code cannot
    /// carry: every code is refused before anything is written except `step_up_required` and
    /// `control_stream_frozen`, which may refuse the second half of a batch whose kill switch was
    /// recorded (§5.6), and no code yet reports `unknown` (DEC-681 item 11).
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

/// One finding of a 422 or of validation, shared with the validate read model
/// (`envelope.schema.json#/$defs/Violation`, DEC-682 item 22). `path` is a JSON pointer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Violation {
    /// The body's shape or canonical form; `code` is a lowercase wire code.
    Schema {
        path: String,
        code: String,
        message: String,
    },
    /// A mandate V-rule; `code` is its id (`V-022`).
    Rule {
        path: String,
        code: String,
        message: String,
    },
    /// A value looser than the nearest ancestor that sets it (mandate spec §4.3, FR-1.5).
    Policy {
        path: String,
        key: String,
        level: PolicyLevel,
        value: PolicyValue,
        ancestor_level: PolicyLevel,
        ancestor_value: PolicyValue,
        message: String,
    },
}

/// Mandate spec §4.3's hierarchy, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyLevel {
    Platform,
    Organization,
    Workspace,
    Mandate,
}

/// A value as §4.3 compares it: a number, a permission or requirement, or a set's sorted members.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PolicyValue {
    Number(Decimal),
    Flag(bool),
    Set(Vec<String>),
}

/// Why [`Problem::of`] refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProblemError {
    #[error(transparent)]
    Unimplemented(Unimplemented),
    #[error("event_id must be present exactly when effect is recorded or unknown")]
    EventIdMismatch,
    #[error("this code cannot carry this effect")]
    EffectNotAllowed,
}
