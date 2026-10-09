//! The RFC 9457 problem document every error is (workspace API spec §3.5).

use serde::{Deserialize, Serialize};

use crate::Unimplemented;
use crate::wire::{Decimal, EventId, Id, Ref, rules};

/// One error response. `title` is generic text fixed by `code`, never content (rule 6, API-10);
/// `event_id` is always a member, null exactly when `effect` is [`Effect::None`], so a body that
/// omits it is refused rather than read as null; `current_base` is a member exactly when `code` is
/// [`ProblemCode::StaleBase`] (§3.5: "the body names the current base", API-19).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Problem {
    #[serde(rename = "type")]
    pub type_uri: String,
    pub status: u16,
    pub code: ProblemCode,
    pub title: String,
    pub effect: Effect,
    #[serde(deserialize_with = "Option::deserialize")]
    pub event_id: Option<EventId>,
    pub retryable: bool,
    pub violations: Vec<Violation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_base: Option<CurrentBase>,
}

/// The base now in force that a `stale_base` refusal names: a draft's or version's content
/// reference, or the id of the object whose base moved (`envelope.schema.json#/$defs/Problem`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CurrentBase {
    Ref(Ref),
    Id(Id),
}

impl Problem {
    /// The problem for `code`: the status §3.5's table gives it, the type URI
    /// `https://mandate.dev/problems/<code>`, a title fixed by the code that names no content, no
    /// violations, and `retryable` for `journal_unavailable`, `rate_limited`, and
    /// `membership_unavailable` only (DEC-681 items 9 and 11). The title is the one
    /// `envelope.schema.json` pins for the code, or for identity spec §4.5's codes the one DEC-681
    /// item 14 fixes.
    ///
    /// # Errors
    /// [`ProblemError::EventIdMismatch`] for an `event_id` with [`Effect::None`], or none with
    /// `recorded` or `unknown`; [`ProblemError::EffectNotAllowed`] for an effect the code cannot
    /// carry: every code is refused before anything is written except `step_up_required` and
    /// `control_stream_frozen`, which may refuse the second half of a batch whose kill switch was
    /// recorded (§5.6), and only `outcome_unknown`, once served, reports `unknown`, and only that
    /// (DEC-681 item 11); [`ProblemError::CurrentBaseMismatch`] for a `current_base` with a code
    /// other than `stale_base`, or none with `stale_base`.
    pub fn of(
        code: ProblemCode,
        effect: Effect,
        event_id: Option<EventId>,
        current_base: Option<CurrentBase>,
    ) -> Result<Self, ProblemError> {
        let _ = (code, effect, event_id, current_base);
        Err(ProblemError::Unimplemented(Unimplemented))
    }
}

/// The stable machine codes of §3.5's table. Closed on the server: it can emit no other code
/// (DEC-681 item 1). An operation's own codes join this enum with the operation's story. Six are
/// identity spec §4.5's refusals (DEC-643); five `step_up_*` codes (DEC-686) and `outcome_unknown`
/// stay `x-planned` under E10-10 until the implementation serves them (DEC-689 item 5).
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
    StepUpMissing,
    StepUpStale,
    StepUpReused,
    StepUpMethod,
    StepUpMismatch,
    LiveUnavailable,
    ControlStreamFrozen,
    JournalUnavailable,
    RateLimited,
    OwnRoles,
    OwnerRoleReserved,
    LastOwner,
    LastAdmin,
    ReductionOnly,
    MembershipUnavailable,
    OutcomeUnknown,
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
        ancestor_level: AncestorLevel,
        ancestor_value: PolicyValue,
        message: String,
    },
}

/// The level whose value is looser than its ancestor's: any level of mandate spec §4.3's hierarchy
/// but the platform's, which has no ancestor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyLevel {
    Organization,
    Workspace,
    Mandate,
}

/// The nearest ancestor that sets the value: any level of mandate spec §4.3's hierarchy but the
/// mandate's, which is never an ancestor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AncestorLevel {
    Platform,
    Organization,
    Workspace,
}

/// A value as §4.3 compares it: a number, a permission or requirement, or a set's sorted members.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PolicyValue {
    Number(Decimal),
    Flag(bool),
    Set(Vec<String>),
}

rules!(pending: Problem, Violation);
rules!(none: ProblemCode, Effect);

/// Why [`Problem::of`] refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProblemError {
    #[error(transparent)]
    Unimplemented(Unimplemented),
    #[error("event_id must be present exactly when effect is recorded or unknown")]
    EventIdMismatch,
    #[error("this code cannot carry this effect")]
    EffectNotAllowed,
    #[error("current_base must be present exactly when the code is stale_base")]
    CurrentBaseMismatch,
}
