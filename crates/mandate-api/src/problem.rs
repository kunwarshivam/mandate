//! The RFC 9457 problem document every error is (workspace API spec §3.5).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::wire::{Check, Decimal, EventId, Id, Ref, Rules, is_pointer, is_word, rules};

/// One error response. `title` is generic text fixed by `code`, never content (rule 6, API-10);
/// `event_id` is always a member, null exactly when `effect` is [`Effect::None`], so a body that
/// omits it is refused rather than read as null; `current_base` is a member exactly when `code` is
/// [`ProblemCode::StaleBase`] (§3.5: "the body names the current base", API-19).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
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
        if !code.carries(effect) {
            return Err(ProblemError::EffectNotAllowed);
        }
        if event_id.is_some() == (effect == Effect::None) {
            return Err(ProblemError::EventIdMismatch);
        }
        if current_base.is_some() != (code == ProblemCode::StaleBase) {
            return Err(ProblemError::CurrentBaseMismatch);
        }
        let (name, status, title) = code.pinned();
        Ok(Self {
            type_uri: format!("https://mandate.dev/problems/{name}"),
            status,
            code,
            title: title.to_owned(),
            effect,
            event_id,
            retryable: matches!(
                code,
                ProblemCode::JournalUnavailable
                    | ProblemCode::RateLimited
                    | ProblemCode::MembershipUnavailable
            ),
            violations: Vec::new(),
            current_base,
        })
    }
}

impl ProblemCode {
    /// The code's wire name, §3.5's status, and its fixed title (`envelope.schema.json`, DEC-681
    /// item 14).
    fn pinned(self) -> (&'static str, u16, &'static str) {
        match self {
            Self::Unauthenticated => ("unauthenticated", 401, "Sign in required"),
            Self::Forbidden => ("forbidden", 403, "Not allowed"),
            Self::NotFound => ("not_found", 404, "Not found"),
            Self::Invalid => ("invalid", 422, "Invalid request"),
            Self::IdempotencyConflict => ("idempotency_conflict", 409, "Key already used"),
            Self::StaleBase => ("stale_base", 409, "Changed since loaded"),
            Self::ClassificationChanged => {
                ("classification_changed", 409, "Classification changed")
            }
            Self::StepUpRequired => ("step_up_required", 401, "Confirmation required"),
            Self::StepUpMissing => ("step_up_missing", 401, "Confirmation not valid"),
            Self::StepUpStale => ("step_up_stale", 401, "Confirmation expired"),
            Self::StepUpReused => ("step_up_reused", 401, "Confirmation already used"),
            Self::StepUpMethod => ("step_up_method", 401, "Confirmation method not allowed"),
            Self::StepUpMismatch => ("step_up_mismatch", 401, "Confirmation does not match"),
            Self::LiveUnavailable => ("live_unavailable", 409, "Live trading unavailable"),
            Self::ControlStreamFrozen => ("control_stream_frozen", 503, "Changes are frozen"),
            Self::JournalUnavailable => ("journal_unavailable", 503, "Temporarily unavailable"),
            Self::RateLimited => ("rate_limited", 429, "Too many requests"),
            Self::OwnRoles => ("own_roles", 403, "Cannot change own roles"),
            Self::OwnerRoleReserved => ("owner_role_reserved", 403, "Owner role reserved"),
            Self::LastOwner => ("last_owner", 409, "Owner must remain"),
            Self::LastAdmin => ("last_admin", 409, "Admin must remain"),
            Self::ReductionOnly => ("reduction_only", 403, "Risk reduction only"),
            Self::MembershipUnavailable => {
                ("membership_unavailable", 503, "Membership unavailable")
            }
            Self::OutcomeUnknown => ("outcome_unknown", 503, "Result unknown"),
        }
    }

    /// Whether a refusal with this code may report `effect`: `none` for all but
    /// `outcome_unknown`, `recorded` for the step-up codes and `control_stream_frozen` (§5.6's
    /// batch), and `unknown` for `outcome_unknown` alone (DEC-681 item 11, DEC-686 item 3).
    fn carries(self, effect: Effect) -> bool {
        match effect {
            Effect::None => self != Self::OutcomeUnknown,
            Effect::Recorded => matches!(
                self,
                Self::StepUpRequired
                    | Self::StepUpMissing
                    | Self::StepUpStale
                    | Self::StepUpReused
                    | Self::StepUpMethod
                    | Self::StepUpMismatch
                    | Self::ControlStreamFrozen
            ),
            Effect::Unknown => self == Self::OutcomeUnknown,
        }
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
#[serde(
    remote = "Self",
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields
)]
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

/// Each code's pinned members, `event_id` by `effect`, `current_base` by `code`, and each
/// violation's own rules (`envelope.schema.json#/$defs/Problem`).
impl Rules for Problem {
    fn rules(&self, at: &str, check: &mut Check) {
        let pinned = Self::of(
            self.code,
            self.effect,
            self.event_id.clone(),
            self.current_base.clone(),
        );
        let pinned = pinned.map(|pinned| Self {
            violations: self.violations.clone(),
            ..pinned
        });
        check.rule(pinned.as_ref() == Ok(self), at, "pinned");
        for (index, violation) in self.violations.iter().enumerate() {
            violation.rules(&format!("{at}/violations/{index}"), check);
        }
    }
}

/// A pointer for `path`, a lowercase code for a schema finding, `V-` and three digits for a rule,
/// and a lowercase key and unique lowercase set members for a policy finding.
impl Rules for Violation {
    fn rules(&self, at: &str, check: &mut Check) {
        let (path, named, holds) = match self {
            Self::Schema { path, code, .. } => (path, "code", is_word(code)),
            Self::Rule { path, code, .. } => (path, "code", is_rule(code)),
            Self::Policy {
                path,
                key,
                value,
                ancestor_value,
                ..
            } => {
                value.rules(&format!("{at}/value"), check);
                ancestor_value.rules(&format!("{at}/ancestor_value"), check);
                (path, "key", is_word(key))
            }
        };
        check.rule(is_pointer(path, true), &format!("{at}/path"), "pattern");
        check.rule(holds, &format!("{at}/{named}"), "pattern");
    }
}

/// `^V-[0-9]{3}$`, a mandate V-rule's id.
fn is_rule(code: &str) -> bool {
    let digits = code.strip_prefix("V-").unwrap_or_default();
    digits.len() == 3 && digits.chars().all(|c| c.is_ascii_digit())
}

/// A set's members are unique lowercase words.
impl Rules for PolicyValue {
    fn rules(&self, at: &str, check: &mut Check) {
        if let Self::Set(members) = self {
            let unique: BTreeSet<&String> = members.iter().collect();
            check.rule(unique.len() == members.len(), at, "unique");
            check.rule(members.iter().all(|m| is_word(m)), at, "pattern");
        }
    }
}

rules!(checked: Problem, Violation);
rules!(none: ProblemCode, Effect);

/// Why [`Problem::of`] refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProblemError {
    #[error("event_id must be present exactly when effect is recorded or unknown")]
    EventIdMismatch,
    #[error("this code cannot carry this effect")]
    EffectNotAllowed,
    #[error("current_base must be present exactly when the code is stale_base")]
    CurrentBaseMismatch,
}

crate::wire::object_only!(Problem, Violation);
