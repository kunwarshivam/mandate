//! Autonomy classification ([mandate spec §6.2](../../../docs/specs/mandate.md#62-evaluation),
//! §6.1, §6.4): the six steps between a proposal and a submission, in the order §6.2 writes them.
//!
//! The §6.3 condition language itself is `mandate-spec`'s (DEC-128 item 18); what is here is the
//! order the rules are walked in, the built-in AUTO purposes, the default, the admission ceiling,
//! the approver count, and the [`Facts`] one proposed action presents to a rule.
//!
//! **§4.3's policy overlay is not a parameter.** DEC-128 item 9 names this crate as the caller of
//! [`PolicyOverlay::auto_allowed`](mandate_spec::policy::PolicyOverlay::auto_allowed); the narrowing
//! it describes is applied to [`Classification::decision`] by
//! [`PolicyOverlay::narrow`](mandate_spec::policy::PolicyOverlay::narrow), which is total and
//! already lives beside the overlay, rather than threaded through [`classify`]. An overlay is
//! folded only by `policy::check` over a chain of levels, so taking one here would make §6
//! untestable without a policy chain and would put the same tightening in two places. See the tests
//! PR's Decisions needed.

use core::num::NonZeroU8;

use mandate_domain::{AssetClass, AssetId, AutonomyDecision, MarketSession, Purpose};
use mandate_num::{Ratio, Signed, Unit, Usd};
use mandate_spec::condition::{ConditionField, Facts};
use mandate_spec::document::{Autonomy, OnTimeout, RuleId};

use crate::BuilderError;
use crate::builder::Proposal;

/// The facts one proposed action presents to the §6.3 rules.
///
/// Every field is required and nothing is inferred from another (DEC-85, DEC-130 item 19). In
/// particular `first_trade_in_instrument` is not derived from the position: a re-entry after a
/// round trip is not a first trade (MC-B25), and inferring it would mislabel exactly that case.
///
/// The three exposure fields are the order's **after** values, which is what lets an owner bound
/// what order splitting would otherwise evade (§6.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionContext {
    pub purpose: Purpose,
    pub order_usd: Usd,
    pub combined_score: Unit,
    pub instrument: AssetId,
    pub asset_class: AssetClass,
    pub session: MarketSession,
    pub first_trade_in_instrument: bool,
    pub new_instrument: bool,
    pub thesis_confidence: Unit,
    pub drawdown: Unit,
    pub daily_pnl_fraction: Signed,
    pub position_usd_after: Usd,
    pub gross_usd_after: Usd,
    pub bought_today_usd: Usd,
    pub position_pnl_fraction: Signed,
}

impl Facts for ActionContext {
    fn enum_field(&self, field: ConditionField) -> Option<&str> {
        let _ = field;
        None
    }

    fn decimal_field(&self, field: ConditionField) -> Option<Ratio> {
        let _ = field;
        None
    }

    fn bool_field(&self, field: ConditionField) -> Option<bool> {
        let _ = field;
        None
    }
}

/// What decided an action's autonomy (§6.2 steps 3 to 5).
///
/// [`DecidedBy::AdmissionCeiling`] is reported only when the ceiling actually **changed** the
/// decision (MC-A12, MC-A14) and the matching rule's id otherwise (MC-A13's `rule:routine` under
/// `admission: auto`, MC-A15's `rule:no_new`). Because the ceiling is applied after the rules and
/// only tightens, the owner's `deny` survives an `auto` admission setting (DEC-130 item 13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecidedBy {
    BuiltinRiskReducing,
    Rule(RuleId),
    Default,
    AdmissionCeiling,
}

impl DecidedBy {
    /// The form the reference cases and the journal write: `builtin_risk_reducing`, `rule:<id>`,
    /// `default`, `admission_ceiling`.
    pub fn label(&self) -> String {
        match self {
            Self::BuiltinRiskReducing => "builtin_risk_reducing".to_owned(),
            Self::Rule(id) => format!("rule:{}", id.as_str()),
            Self::Default => "default".to_owned(),
            Self::AdmissionCeiling => "admission_ceiling".to_owned(),
        }
    }
}

/// What an ASK asks for (§6.4).
///
/// `on_timeout` is [`OnTimeout::Skip`] and has no other value, so an unanswered ASK adds no risk
/// (DEC-06). The type is `mandate-spec`'s, which is why it is a unit rather than a choice this
/// crate could get wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApprovalRequest {
    pub approvers_required: NonZeroU8,
    pub on_timeout: OnTimeout,
}

/// An action's autonomy: the decision, what decided it, and the approval an ASK carries.
///
/// `approval` is `Some` exactly for [`AutonomyDecision::Ask`]: an AUTO needs none and a DENY is
/// never overridden (§6.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub decision: AutonomyDecision,
    pub by: DecidedBy,
    pub approval: Option<ApprovalRequest>,
}

/// The §6.2 step 2 dry run's answer, as a **value**.
///
/// This crate never calls the gate (DEC-130 item 2), and it never derives a [`GateVerdict::Defer`]
/// either: the session rule that defers an equity discretionary exit is the gate's (trading spec
/// §9.6), and [`decide`] takes no [`Market`](crate::Market), so it cannot see the session
/// (DEC-130 item 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateVerdict {
    Allow,
    Deny,
    Defer,
}

/// What §6.2 does with a proposal once the dry run has answered.
///
/// [`Outcome::Skipped`] and [`Outcome::Deferred`] are outcomes, not decisions, so a gate verdict
/// cannot be mistaken for an autonomy decision in a `match` (DEC-130 item 14). A deny is skipped
/// and journaled and **no approval is requested** (§6.2 step 2, DEC-05); a defer stores nothing and
/// never becomes a deny (DEC-48).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Skipped,
    Deferred,
    Classified(Classification),
}

/// §6.2 steps 3 to 5: the built-in AUTO purposes, the first matching rule, the default, and the
/// admission ceiling, in that order.
///
/// Step 3 comes first and is unconditional, and that includes coming before the order path's
/// re-check of V-017, V-018, V-020 and V-023: a malformed rule set must not be a reason a risk exit,
/// an owner exit, a protective order or a discretionary exit is refused (`AGENTS.md` rule 13,
/// MI-1, DEC-05). The rules are validated only on the path that reads them.
/// Step 4 takes the **first** matching rule, not the last and not the strictest. Step 5's ceiling
/// applies only when `new_instrument` is true and only **tightens** (MI-17).
pub fn classify(policy: &Autonomy, action: &ActionContext) -> Result<Classification, BuilderError> {
    let _ = (policy, action);
    Err(BuilderError::Unimplemented)
}

/// §6.2 step 2 and then steps 3 to 6: the gate's verdict in, an outcome out.
///
/// This is the **only** way to an autonomy decision for a proposed order, which is what makes "no
/// approval is ever requested for an order the gate would deny" hold by construction rather than by
/// discipline (§6.2 step 2, DEC-130 item 16). A holding proposal is
/// [`BuilderError::NothingProposed`]: a hold and a denied order are two different things on the
/// journal, and collapsing them into one outcome would hide which happened.
pub fn decide(
    policy: &Autonomy,
    proposal: &Proposal,
    verdict: GateVerdict,
) -> Result<Outcome, BuilderError> {
    let _ = (policy, proposal, verdict);
    Err(BuilderError::Unimplemented)
}
