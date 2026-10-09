//! The unasked dollars ([mandate spec §4.2](../../../docs/specs/mandate.md#42-warnings-and-the-confirmation-screen);
//! DEC-189, DEC-695): an upper bound on the order value of openings and increases the agent could
//! have decided `auto` (§6.2) from the risk clock to the end of its risk day. It is display only:
//! it changes no decision, and nothing in the gate or the autonomy decision reads it.
//!
//! E10-7 slice S1a's tests PR (DEC-77): [`unasked_usd`] returns [`SpecError::Unimplemented`], so
//! every test pending on it fails at the stub until the implementation PR replaces it.

use std::collections::BTreeMap;

use mandate_num::Usd;
use mandate_time::UtcNanos;

use crate::SpecError;
use crate::document::DelegationId;
use crate::validate::ValidatedMandate;

/// One delegation's journaled usage (§6.5 condition 4): the orders it has lifted and their total
/// value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelegationUsage {
    pub orders: u32,
    pub total_usd: Usd,
}

/// The effective policy of §4.3 as the figure reads it: whether `auto` is allowed, and whether the
/// version is `policy_nonconforming`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectivePolicy {
    pub auto_allowed: bool,
    pub nonconforming: bool,
}

/// The figure's inputs besides the mandate, as §4.2's table lists them. `None` is an input that is
/// not known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnaskedInputs {
    /// The journaled risk clock *t* (§5.2); for an agent not yet deployed, the validation instant.
    pub now: Option<UtcNanos>,
    /// §5.3's count of openings and increases submitted in the risk day; 0 for an agent not yet
    /// deployed.
    pub orders_today: Option<u32>,
    /// Usage per delegation. A delegation with no entry has used nothing.
    pub usage: Option<BTreeMap<DelegationId, DelegationUsage>>,
    /// Not known reads as allowing `auto` and conforming, which gives the larger figure.
    pub policy: Option<EffectivePolicy>,
}

/// §4.2's unasked dollars, rounded up to the cent, with exact decimals throughout.
///
/// `Ok(None)` is "not known": the risk clock, today's order count, or delegation usage is missing.
/// It is never shown as 0 (`AGENTS.md` rule 3). The figure is a known 0 when the review date has
/// passed at *t* (§6.2 step 5b), the version is `policy_nonconforming`, or `auto` is not allowed
/// (step 5c). Otherwise it is the sum of the largest slices that the orders left today can take:
/// one unlimited slice for the `auto` default or `auto` rules, and each delegation's slices from its
/// remaining caps. There is no gross-exposure cap, because headroom can grow within the day
/// (DEC-695 item 4).
pub fn unasked_usd(
    mandate: &ValidatedMandate,
    inputs: &UnaskedInputs,
) -> Result<Option<Usd>, SpecError> {
    let _ = (mandate, inputs);
    Err(SpecError::Unimplemented)
}
