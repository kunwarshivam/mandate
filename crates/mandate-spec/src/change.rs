//! Versioning and change classification
//! ([mandate spec §9](../../../docs/specs/mandate.md#9-versioning-and-change-classification-dec-43)).
//!
//! [`classify`] reads two documents and nothing else: no context, no policy, no state. The verdict is
//! the join over the changed paths — increasing if any path is, else reducing if any is, else neutral —
//! and an unlisted path is increasing, which is the fail-safe §9.2 ends on.
//!
//! [`pinning_switch`] is the one exception, and it is named rather than buried: DEC-121 classifies
//! turning bring-your-own-strategy on as **one** risk-reducing change instead of a field-by-field
//! verdict, so it looks at the whole change at once.

use crate::SpecError;
use crate::document::{Mandate, Pointer};

/// What a version does to risk (§9.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChangeClass {
    RiskIncreasing,
    RiskReducing,
    Neutral,
    /// `environment` or `connection_id` changed, which §9.2 calls invalid rather than classifying.
    /// V-031 reports the same thing at validation; neither surface depends on the other
    /// (DEC-128 item 12).
    Invalid,
}

impl ChangeClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RiskIncreasing => "risk_increasing",
            Self::RiskReducing => "risk_reducing",
            Self::Neutral => "neutral",
            Self::Invalid => "invalid",
        }
    }
}

/// A classified change.
///
/// `step_up_required` is exactly `class == RiskIncreasing` (§9.2's last paragraph). Independent
/// approval is a policy question and belongs to
/// [`PolicyOverlay`](crate::policy::PolicyOverlay), not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub class: ChangeClass,
    pub changed_paths: Vec<Pointer>,
    pub step_up_required: bool,
}

/// Classifies `new` against `old`.
pub fn classify(old: &Mandate, new: &Mandate) -> Result<Classification, SpecError> {
    let _ = (old, new);
    Err(SpecError::Unimplemented)
}

/// The paths that differ between two documents.
///
/// Objects are walked; **arrays are compared whole**, so a ladder change reports
/// `/risk/drawdown_ladder` and never `/risk/drawdown_ladder/2/at`. MC-C01 depends on it, and an
/// element-by-element walk is planted bug 18.
pub fn changed_paths(old: &Mandate, new: &Mandate) -> Result<Vec<Pointer>, SpecError> {
    let _ = (old, new);
    Err(SpecError::Unimplemented)
}

/// The five paths DEC-121's pinning switch may touch and no others.
pub const PIN_SWITCH_PATHS: [&str; 5] = [
    "/universe/pinned",
    "/universe/pinned_instruments",
    "/universe/max_instruments",
    "/behavior/research",
    "/behavior/signal_models",
];

/// True when the change is DEC-121's pinning switch: not pinned before and pinned after, from a
/// version that **had** an admitting model, clearing `behavior.research` and every
/// `admits_instruments`, not raising `max_instruments`, and changing nothing outside
/// [`PIN_SWITCH_PATHS`].
///
/// Pinning a version that had no admitting model is increasing instead, and §9.2 says why: such a
/// version has an empty working universe and can open nothing, so pinning hands the agent instruments
/// it could not trade before. DEC-121's reason for calling pinning reducing — that it removes the
/// platform's discretion — does not apply when there was no discretion to remove.
pub fn pinning_switch(old: &Mandate, new: &Mandate, paths: &[Pointer]) -> Result<bool, SpecError> {
    let _ = (old, new, paths);
    Err(SpecError::Unimplemented)
}

/// Whether an autonomy change is reducing (§9.2's autonomy row).
///
/// Reducing only if **every** change is one of the six shapes §9.2 lists; anything else — reordering
/// rules, changing a field, an operator, a compound condition, the approvers, or the timeout — is
/// increasing. MI-11 is the property that matters here, and its oracle does not look at this function
/// at all: it evaluates generated actions under both rule sets and asserts the new decision is never
/// less strict.
pub fn classify_autonomy(
    old: &crate::document::Autonomy,
    new: &crate::document::Autonomy,
) -> Result<ChangeClass, SpecError> {
    let _ = (old, new);
    Err(SpecError::Unimplemented)
}
