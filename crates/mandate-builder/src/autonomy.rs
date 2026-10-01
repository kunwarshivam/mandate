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

use std::collections::BTreeSet;

use mandate_domain::{AssetClass, AssetId, AutonomyDecision, MarketSession, Purpose};
use mandate_num::{Ratio, Signed, Unit, Usd};
use mandate_spec::condition::{
    Condition, ConditionField, ConditionValue, Facts, FieldKind, MAX_CONDITION_DEPTH, Operator,
};
use mandate_spec::document::{Approval, Autonomy, OnTimeout, Rule, RuleId};
use mandate_spec::{DecGrammar, SchemaDec};

use crate::BuilderError;
use crate::builder::{Action, Proposal};

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
    /// Who asked for the order (§6.2 step 5a). Not a §6.3 field: no rule can read it, and only the
    /// client ceiling does.
    pub requested_by: RequestedBy,
}

/// Who asked for an order (§6.2 step 5a, DEC-185, DEC-262).
///
/// The platform sets it from the **authenticated channel**, never from the request's content, so it
/// is a required field with no default: a path that builds an [`ActionContext`] must say which
/// channel it came from. [`propose`](crate::propose) sizes only from signal-model outputs, which
/// §6.2 step 5a defines as `agent`, the order builder's own proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequestedBy {
    /// The order builder's own proposal.
    Agent,
    /// The owner, through Owlhead's web app or CLI.
    Owner,
    /// An owner-connected client through the MCP server (DEC-141, E10-6). Its opening and
    /// increasing orders are never AUTO (MI-30).
    Client,
}

/// The projection of an action onto the §6.3 field names.
///
/// Live rather than a stub, under DEC-128 item 22: it is a total function with no rule logic — a
/// field name to a field this struct already holds — and `Condition::matches` cannot be exercised
/// without it, so every autonomy test needs it to construct an answer at all. A `None`-returning stub
/// would also have been a **plausible** one, since `matches` treats an absent fact as an error, and
/// the mutation gate does not recognise `None` as a stub body the way it recognises `Unimplemented`.
/// `crates/mandate-builder/tests/vocabulary.rs` pins every arm live.
///
/// A field this action does not carry is `None`, which `Condition::matches` treats as an error rather
/// than as a false: a rule that cannot be evaluated must never read as "does not match", which would
/// quietly widen autonomy. [`ConditionField::UnusualInput`] is one of those, because V-018 reserves
/// it and [`classify`] refuses a rule naming it before any fact is read.
impl Facts for ActionContext {
    fn enum_field(&self, field: ConditionField) -> Option<&str> {
        match field {
            ConditionField::Purpose => Some(purpose_name(self.purpose)),
            ConditionField::AssetClass => Some(self.asset_class.as_str()),
            ConditionField::Session => Some(self.session.as_str()),
            ConditionField::Instrument => Some(self.instrument.as_str()),
            _ => None,
        }
    }

    fn decimal_field(&self, field: ConditionField) -> Option<Ratio> {
        let text = match field {
            ConditionField::OrderUsd => self.order_usd.to_string(),
            ConditionField::CombinedScore => self.combined_score.to_string(),
            ConditionField::ThesisConfidence => self.thesis_confidence.to_string(),
            ConditionField::Drawdown => self.drawdown.to_string(),
            ConditionField::DailyPnlFraction => self.daily_pnl_fraction.to_string(),
            ConditionField::PositionUsdAfter => self.position_usd_after.to_string(),
            ConditionField::GrossUsdAfter => self.gross_usd_after.to_string(),
            ConditionField::BoughtTodayUsd => self.bought_today_usd.to_string(),
            ConditionField::PositionPnlFraction => self.position_pnl_fraction.to_string(),
            _ => return None,
        };
        Ratio::parse(&text).ok()
    }

    fn bool_field(&self, field: ConditionField) -> Option<bool> {
        match field {
            ConditionField::FirstTradeInInstrument => Some(self.first_trade_in_instrument),
            ConditionField::NewInstrument => Some(self.new_instrument),
            _ => None,
        }
    }
}

/// The §6.3 spelling of a purpose. §6.1's six names, of which a rule can only ever see the first two.
fn purpose_name(purpose: Purpose) -> &'static str {
    match purpose {
        Purpose::Open => "open",
        Purpose::Increase => "increase",
        Purpose::DiscretionaryExit => "discretionary_exit",
        Purpose::OwnerExit => "owner_exit",
        Purpose::RiskExit => "risk_exit",
        Purpose::Protective => "protective",
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
    /// §6.2 step 5a raised the decision to ASK because an owner-connected client requested the
    /// order. Reported only when the ceiling changed the decision, as for the admission ceiling.
    ClientCeiling,
}

impl DecidedBy {
    /// The form the reference cases and the journal write: `builtin_risk_reducing`, `rule:<id>`,
    /// `default`, `admission_ceiling`, `client_ceiling`.
    pub fn label(&self) -> String {
        match self {
            Self::BuiltinRiskReducing => "builtin_risk_reducing".to_owned(),
            Self::Rule(id) => format!("rule:{}", id.as_str()),
            Self::Default => "default".to_owned(),
            Self::AdmissionCeiling => "admission_ceiling".to_owned(),
            Self::ClientCeiling => "client_ceiling".to_owned(),
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

/// §6.2 steps 3 to 5a: the built-in AUTO purposes, the first matching rule, the default, the
/// admission ceiling, and the client ceiling, in that order.
///
/// Step 3 comes first and is unconditional, and that includes coming before the order path's
/// re-check of V-017, V-018, V-020 and V-023: a malformed rule set must not be a reason a risk exit,
/// an owner exit, a protective order or a discretionary exit is refused (`AGENTS.md` rule 13,
/// MI-1, DEC-05). The rules are validated only on the path that reads them.
/// Step 4 takes the **first** matching rule, not the last and not the strictest. Step 5's ceiling
/// applies only when `new_instrument` is true and only **tightens** (MI-17). Step 5a's applies only
/// when an owner-connected client requested the order, comes **last**, after the admission ceiling,
/// so no rule, default or admission setting it follows can make that order AUTO, and only tightens,
/// so a `deny` still denies (MI-30, DEC-185).
///
/// The whole rule set is re-checked before any rule is read, not only the rules the walk reaches: a
/// malformed rule behind a matching one still refuses the action, because a rule set the order path
/// cannot trust is not one it may half-apply, and refusing an opening action adds no risk
/// (`AGENTS.md` rule 3).
pub fn classify(policy: &Autonomy, action: &ActionContext) -> Result<Classification, BuilderError> {
    if action.purpose.reduces_risk() {
        return Ok(Classification {
            decision: AutonomyDecision::Auto,
            by: DecidedBy::BuiltinRiskReducing,
            approval: None,
        });
    }
    check_rules(&policy.rules)?;
    let matched = first_match(&policy.rules, |condition| Ok(condition.matches(action)?))?;
    let (ruled, ruled_by) = match matched {
        Some(rule) => (rule.then, DecidedBy::Rule(rule.id.clone())),
        None => (policy.default, DecidedBy::Default),
    };
    let (decision, by) = if action.new_instrument && policy.admission > ruled {
        (policy.admission, DecidedBy::AdmissionCeiling)
    } else {
        (ruled, ruled_by)
    };
    let (decision, by) = match action.requested_by {
        RequestedBy::Agent | RequestedBy::Owner => (decision, by),
        RequestedBy::Client => client_ceiling(decision, by)?,
    };
    let approval = match decision {
        AutonomyDecision::Ask => Some(ApprovalRequest {
            approvers_required: approvers_required(&policy.approval, action.order_usd)?,
            on_timeout: policy.approval.on_timeout,
        }),
        AutonomyDecision::Auto | AutonomyDecision::Deny => None,
    };
    Ok(Classification {
        decision,
        by,
        approval,
    })
}

/// §6.2 step 5a, MI-30: the stricter of the decision so far and ASK, labelled
/// [`DecidedBy::ClientCeiling`] only when that changed the decision.
fn client_ceiling(
    decision: AutonomyDecision,
    by: DecidedBy,
) -> Result<(AutonomyDecision, DecidedBy), BuilderError> {
    let _ = (decision, by);
    Err(BuilderError::Unimplemented)
}

/// §6.2 step 2 and then steps 3 to 6: the gate's verdict in, an outcome out.
///
/// This is the **only** way to an autonomy decision for a proposed order, which is what makes "no
/// approval is ever requested for an order the gate would deny" hold by construction rather than by
/// discipline (§6.2 step 2, DEC-130 item 16). A holding proposal is
/// [`BuilderError::NothingProposed`]: a hold and a denied order are two different things on the
/// journal, and collapsing them into one outcome would hide which happened.
///
/// A sell is built-in AUTO by its **side**, whatever its label says: long only (DEC-32), a sell can
/// only reduce a position, and the gate re-derives its purpose and denies one above the position
/// (§6.1). A buy is classified by its facts, and a buy that is not labelled `open` or `increase`
/// in both places it carries a purpose is [`BuilderError::MislabelledBuy`] rather than a built-in
/// AUTO, because a label is not a reason to skip the owner's rules on an order that adds risk.
pub fn decide(
    policy: &Autonomy,
    proposal: &Proposal,
    verdict: GateVerdict,
) -> Result<Outcome, BuilderError> {
    let facts = match &proposal.action {
        Action::Hold { .. } => return Err(BuilderError::NothingProposed),
        Action::Sell { .. } => None,
        Action::Buy {
            purpose, action, ..
        } => {
            if purpose.reduces_risk() || *purpose != action.purpose {
                return Err(BuilderError::MislabelledBuy);
            }
            Some(action)
        }
    };
    match verdict {
        GateVerdict::Deny => Ok(Outcome::Skipped),
        GateVerdict::Defer => Ok(Outcome::Deferred),
        GateVerdict::Allow => match facts {
            None => Ok(Outcome::Classified(Classification {
                decision: AutonomyDecision::Auto,
                by: DecidedBy::BuiltinRiskReducing,
                approval: None,
            })),
            Some(action) => classify(policy, action).map(Outcome::Classified),
        },
    }
}

/// §6.2 step 4's walk: the first rule whose condition holds, read in the mandate's order, or `None`.
///
/// The predicate is a parameter so the walk is tested on its own, in this module, apart from the
/// §6.3 evaluation `mandate-spec` owns; [`classify`] passes
/// [`Condition::matches`](mandate_spec::condition::Condition::matches). An error from any rule
/// before a match stops the walk: a rule that cannot be evaluated never reads as "does not match".
fn first_match(
    rules: &[Rule],
    mut holds: impl FnMut(&Condition) -> Result<bool, BuilderError>,
) -> Result<Option<&Rule>, BuilderError> {
    for rule in rules {
        if holds(&rule.when)? {
            return Ok(Some(rule));
        }
    }
    Ok(None)
}

/// The order path's re-check of the rule set: unique ids, then V-017, V-018 and V-023 over each
/// rule's comparisons in order, the first failure reported.
///
/// Per comparison the depth is checked first, then the reserved field, then the type rules, so a
/// comparison that is both reserved and ill-typed reports `reserved_field`: the field can never be
/// reached, whatever it is compared against.
fn check_rules(rules: &[Rule]) -> Result<(), BuilderError> {
    let mut ids = BTreeSet::new();
    if !rules.iter().all(|rule| ids.insert(&rule.id)) {
        return Err(BuilderError::DuplicateRuleId);
    }
    for rule in rules {
        for (comparison, depth) in rule.when.comparisons() {
            if depth > MAX_CONDITION_DEPTH {
                return Err(BuilderError::ConditionTooDeep);
            }
            let Condition::Compare { field, op, value } = comparison else {
                continue;
            };
            if field.is_reserved() {
                return Err(BuilderError::ReservedField);
            }
            if !well_typed(*field, *op, value) {
                return Err(BuilderError::ConditionTypeMismatch);
            }
        }
    }
    Ok(())
}

/// V-023: the operators and values a field admits (§6.3).
///
/// A boolean takes `eq` or `ne` and a boolean. A decimal takes the six comparison operators and a
/// decimal, which for `combined_score` and `drawdown` lies in the closed unit interval. An enum or
/// an instrument takes `eq` or `ne` and one member, or `in` or `not_in` and a non-empty list of
/// members.
fn well_typed(field: ConditionField, op: Operator, value: &ConditionValue) -> bool {
    match (field.kind(), value) {
        (FieldKind::Bool, ConditionValue::Bool(_)) => matches!(op, Operator::Eq | Operator::Ne),
        (FieldKind::Decimal, ConditionValue::Decimal(decimal)) => {
            !op.takes_list()
                && (!field.is_unit_bounded()
                    || SchemaDec::parse(decimal.as_str(), DecGrammar::Fraction).is_ok())
        }
        (FieldKind::Enum | FieldKind::Text, ConditionValue::Text(member)) => {
            matches!(op, Operator::Eq | Operator::Ne) && is_member(field, member)
        }
        (FieldKind::Enum | FieldKind::Text, ConditionValue::List(members)) => {
            op.takes_list()
                && !members.is_empty()
                && members.iter().all(|member| is_member(field, member))
        }
        _ => false,
    }
}

/// Whether `member` is one of the values §6.3 names for an enum field. Any text is an instrument id
/// a rule may name: an id the agent never trades makes the comparison false, not the rule invalid.
///
/// `purpose` names only `open` and `increase`, because rules see no other purpose, and `session`
/// has no `overnight`, because no order may trade there (DEC-30).
fn is_member(field: ConditionField, member: &str) -> bool {
    match field {
        ConditionField::Purpose => matches!(member, "open" | "increase"),
        ConditionField::AssetClass => matches!(member, "us_equity" | "crypto"),
        ConditionField::Session => {
            matches!(member, "pre_market" | "regular" | "after_hours" | "crypto")
        }
        _ => true,
    }
}

/// §6.4: two approvers when the order value is strictly **above** `two_approver_above_usd`, one
/// otherwise and when it is `null`.
///
/// A threshold wider than [`Usd`] holds is refused rather than rounded: rounding it could move an
/// order across it, and refusing an opening action adds no risk (DEC-130 item 8).
fn approvers_required(approval: &Approval, order_usd: Usd) -> Result<NonZeroU8, BuilderError> {
    let Some(threshold) = &approval.two_approver_above_usd else {
        return Ok(NonZeroU8::MIN);
    };
    if order_usd > threshold.to_usd()? {
        Ok(NonZeroU8::MIN.saturating_add(1))
    } else {
        Ok(NonZeroU8::MIN)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::error::Error;

    use mandate_num::{Conviction, Price, Qty, UsdExact};
    use mandate_spec::condition::{ConditionField as F, Operator as O};
    use mandate_spec::document::ApproverRef;

    use super::*;
    use crate::builder::{Combined, OrderShape, Sizes};

    type Checked = Result<(), Box<dyn Error>>;

    fn id(text: &str) -> Result<RuleId, Box<dyn Error>> {
        Ok(RuleId::parse(text)?)
    }

    fn dec(text: &str) -> Result<SchemaDec, Box<dyn Error>> {
        Ok(SchemaDec::parse(text, DecGrammar::Decimal)?)
    }

    fn compare(field: ConditionField, op: Operator, value: ConditionValue) -> Condition {
        Condition::Compare { field, op, value }
    }

    fn over(dollars: &str) -> Result<Condition, Box<dyn Error>> {
        Ok(compare(
            ConditionField::OrderUsd,
            Operator::Gt,
            ConditionValue::Decimal(dec(dollars)?),
        ))
    }

    fn rule(name: &str, when: Condition, then: AutonomyDecision) -> Result<Rule, Box<dyn Error>> {
        Ok(Rule {
            id: id(name)?,
            when,
            then,
        })
    }

    fn policy(
        rules: Vec<Rule>,
        default: AutonomyDecision,
        admission: AutonomyDecision,
        two_above: Option<&str>,
    ) -> Result<Autonomy, Box<dyn Error>> {
        Ok(Autonomy {
            rules,
            default,
            admission,
            approval: Approval {
                timeout_s: 600,
                on_timeout: OnTimeout::Skip,
                approvers: vec![ApproverRef::parse("role:approver")?],
                two_approver_above_usd: two_above
                    .map(|t| SchemaDec::parse(t, DecGrammar::PositiveDecimal))
                    .transpose()?,
            },
        })
    }

    fn opening(order_usd: &str, new_instrument: bool) -> Result<ActionContext, Box<dyn Error>> {
        Ok(ActionContext {
            purpose: Purpose::Open,
            order_usd: Usd::parse(order_usd)?,
            combined_score: Unit::parse("0.8")?,
            instrument: AssetId::parse("7b4a1c2e-1111-4a2b-9c3d-000000000001")?,
            asset_class: AssetClass::Crypto,
            session: MarketSession::Crypto,
            first_trade_in_instrument: false,
            new_instrument,
            thesis_confidence: Unit::ZERO,
            drawdown: Unit::ZERO,
            daily_pnl_fraction: Signed::ZERO,
            position_usd_after: Usd::ZERO,
            gross_usd_after: Usd::ZERO,
            bought_today_usd: Usd::ZERO,
            position_pnl_fraction: Signed::ZERO,
            requested_by: RequestedBy::Agent,
        })
    }

    fn classified(
        policy: &Autonomy,
        action: &ActionContext,
    ) -> Result<(AutonomyDecision, String, Option<u8>), Box<dyn Error>> {
        let decided = classify(policy, action)?;
        Ok((
            decided.decision,
            decided.by.label(),
            decided.approval.map(|a| a.approvers_required.get()),
        ))
    }

    fn proposal(action: Action) -> Proposal {
        Proposal {
            action,
            combined: Combined {
                outputs_used: BTreeSet::new(),
                exit_conviction: Conviction::ZERO,
                buy_conviction: Conviction::ZERO,
                score: Unit::ZERO,
            },
            sizes: Sizes {
                cap: UsdExact::zero(),
                current_mv: UsdExact::zero(),
                target_value: None,
                delta: None,
            },
            clipped_by: BTreeSet::new(),
        }
    }

    fn buy(label: Purpose, facts: ActionContext) -> Result<Proposal, Box<dyn Error>> {
        Ok(proposal(Action::Buy {
            purpose: label,
            qty: Qty::parse("1")?,
            limit_price: Price::parse("100")?,
            order_usd: facts.order_usd,
            action: facts,
        }))
    }

    fn sell(label: Purpose) -> Result<Proposal, Box<dyn Error>> {
        Ok(proposal(Action::Sell {
            purpose: label,
            qty: Qty::parse("1")?,
            limit_price: Price::parse("100")?,
            order_usd: Usd::parse("100")?,
            shape: OrderShape::Limit,
        }))
    }

    /// §6.2 step 4: the walk stops at the first rule that holds, reads nothing after it, and reports
    /// that rule rather than a later or a stricter one.
    #[test]
    fn the_walk_takes_the_first_holding_rule_and_reads_no_further() -> Checked {
        let rules = vec![
            rule("first", over("900")?, AutonomyDecision::Deny)?,
            rule("second", over("100")?, AutonomyDecision::Auto)?,
            rule("third", over("200")?, AutonomyDecision::Deny)?,
        ];
        let only_the_first_fails = over("900")?;
        let read = Cell::new(0u8);
        let found = first_match(&rules, |condition| {
            read.set(read.get().saturating_add(1));
            Ok(*condition != only_the_first_fails)
        })?;
        assert_eq!(found.map(|r| r.id.as_str()), Some("second"));
        assert_eq!(read.get(), 2, "the third rule is never read");
        assert_eq!(first_match(&rules, |_| Ok(false))?, None, "no rule holds");
        assert_eq!(
            first_match(&[], |_| Ok(true))?,
            None,
            "an empty rule set has no match"
        );
        Ok(())
    }

    /// §6.3: a rule that cannot be evaluated stops the walk with its error; it never reads as a
    /// rule that does not match, which would let the next, looser rule decide.
    #[test]
    fn a_rule_that_cannot_be_evaluated_stops_the_walk() -> Checked {
        let rules = vec![
            rule("broken", over("1")?, AutonomyDecision::Deny)?,
            rule("loose", over("0")?, AutonomyDecision::Auto)?,
        ];
        let walked = first_match(&rules, |_| Err(BuilderError::ConditionTypeMismatch));
        assert_eq!(walked.err(), Some(BuilderError::ConditionTypeMismatch));
        Ok(())
    }

    /// §6.2 steps 4 and 5 with no rules: the default decides, and the ceiling tightens it for a new
    /// instrument only, naming itself only when it changed the answer.
    #[test]
    fn the_default_decides_and_the_ceiling_only_tightens() -> Checked {
        let auto_default = policy(
            Vec::new(),
            AutonomyDecision::Auto,
            AutonomyDecision::Ask,
            None,
        )?;
        assert_eq!(
            classified(&auto_default, &opening("300", false)?)?,
            (AutonomyDecision::Auto, "default".to_owned(), None)
        );
        assert_eq!(
            classified(&auto_default, &opening("300", true)?)?,
            (
                AutonomyDecision::Ask,
                "admission_ceiling".to_owned(),
                Some(1)
            )
        );
        let equal = policy(
            Vec::new(),
            AutonomyDecision::Ask,
            AutonomyDecision::Ask,
            None,
        )?;
        assert_eq!(
            classified(&equal, &opening("300", true)?)?,
            (AutonomyDecision::Ask, "default".to_owned(), Some(1)),
            "an admission as strict as the default changes nothing, so `by` stays the default"
        );
        let loose_admission = policy(
            Vec::new(),
            AutonomyDecision::Deny,
            AutonomyDecision::Auto,
            None,
        )?;
        assert_eq!(
            classified(&loose_admission, &opening("300", true)?)?,
            (AutonomyDecision::Deny, "default".to_owned(), None),
            "an `auto` admission never loosens a deny"
        );
        let strict_admission = policy(
            Vec::new(),
            AutonomyDecision::Auto,
            AutonomyDecision::Deny,
            None,
        )?;
        assert_eq!(
            classified(&strict_admission, &opening("300", false)?)?,
            (AutonomyDecision::Auto, "default".to_owned(), None),
            "a held instrument never sees the ceiling"
        );
        assert_eq!(
            classified(&strict_admission, &opening("300", true)?)?,
            (AutonomyDecision::Deny, "admission_ceiling".to_owned(), None)
        );
        Ok(())
    }

    /// §6.4: two approvers strictly above the threshold, one at and below it and when it is `null`,
    /// and every ASK skips on timeout.
    #[test]
    fn the_approver_count_is_strict_and_an_ask_skips_on_timeout() -> Checked {
        let asking = policy(
            Vec::new(),
            AutonomyDecision::Ask,
            AutonomyDecision::Ask,
            Some("500"),
        )?;
        for (order, approvers) in [("500.01", 2), ("500", 1), ("499.99", 1)] {
            assert_eq!(
                classified(&asking, &opening(order, false)?)?.2,
                Some(approvers),
                "an order of {order}"
            );
        }
        let unset = policy(
            Vec::new(),
            AutonomyDecision::Ask,
            AutonomyDecision::Ask,
            None,
        )?;
        assert_eq!(classified(&unset, &opening("100000", false)?)?.2, Some(1));
        let decided = classify(&asking, &opening("600", false)?)?;
        assert_eq!(
            decided.approval.map(|a| a.on_timeout),
            Some(OnTimeout::Skip)
        );
        Ok(())
    }

    /// The re-check: ids are unique, and the depth, the reserved field, and the type rules are
    /// checked in that order over every rule, including one behind a rule that would match.
    #[test]
    fn the_rule_set_is_rechecked_before_any_rule_is_read() -> Checked {
        let unique = vec![
            rule("one", over("1")?, AutonomyDecision::Ask)?,
            rule("two", over("1")?, AutonomyDecision::Ask)?,
        ];
        assert_eq!(check_rules(&unique), Ok(()));
        let duplicated = vec![
            rule("same", over("1")?, AutonomyDecision::Ask)?,
            rule("same", over("2")?, AutonomyDecision::Ask)?,
        ];
        assert_eq!(check_rules(&duplicated), Err(BuilderError::DuplicateRuleId));

        let nest = |levels: u8, leaf: Condition| {
            (1..levels).fold(leaf, |inner, _| Condition::Not(Box::new(inner)))
        };
        let four = vec![rule("deep", nest(4, over("1")?), AutonomyDecision::Ask)?];
        assert_eq!(check_rules(&four), Ok(()), "four levels are allowed");
        let five = vec![rule("deep", nest(5, over("1")?), AutonomyDecision::Ask)?];
        assert_eq!(check_rules(&five), Err(BuilderError::ConditionTooDeep));

        let reserved_and_mistyped = compare(
            ConditionField::UnusualInput,
            Operator::Gt,
            ConditionValue::Decimal(dec("1")?),
        );
        assert_eq!(
            check_rules(&[rule("drift", reserved_and_mistyped, AutonomyDecision::Ask)?]),
            Err(BuilderError::ReservedField),
            "the reserved field is reported before the type rules"
        );
        let mistyped_then_deep = Condition::All(vec![
            compare(
                ConditionField::Purpose,
                Operator::Gt,
                ConditionValue::Text("open".to_owned()),
            ),
            nest(4, over("1")?),
        ]);
        assert_eq!(
            check_rules(&[rule("mixed", mistyped_then_deep, AutonomyDecision::Ask)?]),
            Err(BuilderError::ConditionTypeMismatch),
            "comparisons are checked in order, the first failure reported"
        );
        let behind_a_match = vec![
            rule("fine", over("0")?, AutonomyDecision::Auto)?,
            rule(
                "late",
                compare(
                    ConditionField::NewInstrument,
                    Operator::Lt,
                    ConditionValue::Bool(true),
                ),
                AutonomyDecision::Deny,
            )?,
        ];
        assert_eq!(
            check_rules(&behind_a_match),
            Err(BuilderError::ConditionTypeMismatch)
        );
        Ok(())
    }

    /// V-023 over each kind of field, one accepted and one refused comparison per clause.
    #[test]
    fn the_type_rules_admit_exactly_what_v_023_allows() -> Checked {
        let text = |t: &str| ConditionValue::Text(t.to_owned());
        let list =
            |items: &[&str]| ConditionValue::List(items.iter().map(|t| (*t).to_owned()).collect());
        let decimal = |t: &str| dec(t).map(ConditionValue::Decimal);
        let cases = [
            (F::NewInstrument, O::Eq, ConditionValue::Bool(true), true),
            (
                F::FirstTradeInInstrument,
                O::Ne,
                ConditionValue::Bool(false),
                true,
            ),
            (F::NewInstrument, O::Gt, ConditionValue::Bool(true), false),
            (F::NewInstrument, O::Eq, decimal("1")?, false),
            (F::OrderUsd, O::Gte, decimal("2")?, true),
            (F::OrderUsd, O::In, decimal("2")?, false),
            (F::OrderUsd, O::Eq, text("2"), false),
            (F::OrderUsd, O::In, list(&["2"]), false),
            (F::CombinedScore, O::Lte, decimal("1")?, true),
            (F::CombinedScore, O::Gte, decimal("0")?, true),
            (F::CombinedScore, O::Gt, decimal("1.5")?, false),
            (F::Drawdown, O::Lt, decimal("-0.1")?, false),
            (F::ThesisConfidence, O::Lt, decimal("0.6")?, true),
            (F::Purpose, O::Eq, text("open"), true),
            (F::Purpose, O::Ne, text("increase"), true),
            (F::Purpose, O::Eq, text("risk_exit"), false),
            (F::Purpose, O::Gt, text("open"), false),
            (F::Purpose, O::In, text("open"), false),
            (F::Purpose, O::In, list(&["open", "increase"]), true),
            (F::Purpose, O::NotIn, list(&["open"]), true),
            (F::Purpose, O::In, list(&[]), false),
            (F::Purpose, O::In, list(&["open", "protective"]), false),
            (F::Purpose, O::Eq, list(&["open"]), false),
            (F::Purpose, O::Eq, ConditionValue::Bool(true), false),
            (F::AssetClass, O::Eq, text("us_equity"), true),
            (F::AssetClass, O::Eq, text("crypto"), true),
            (F::AssetClass, O::Eq, text("equity"), false),
            (
                F::Session,
                O::In,
                list(&["pre_market", "regular", "after_hours", "crypto"]),
                true,
            ),
            (F::Session, O::Eq, text("overnight"), false),
            (
                F::Instrument,
                O::Eq,
                text("7b4a1c2e-1111-4a2b-9c3d-000000000001"),
                true,
            ),
            (F::Instrument, O::NotIn, list(&["anything"]), true),
            (F::Instrument, O::Lt, text("a"), false),
        ];
        for (field, op, value, allowed) in cases {
            assert_eq!(
                well_typed(field, op, &value),
                allowed,
                "{} {} {value:?}",
                field.as_str(),
                op.as_str()
            );
        }
        Ok(())
    }

    /// §6.2 step 2: a hold is nothing to decide under any verdict; a deny skips and a defer defers
    /// before any rule is read; an allowed sell is the built-in AUTO by its side.
    #[test]
    fn decide_maps_each_verdict_and_refuses_a_hold() -> Checked {
        let unreadable = policy(
            vec![rule(
                "drift",
                compare(
                    ConditionField::UnusualInput,
                    Operator::Eq,
                    ConditionValue::Bool(true),
                ),
                AutonomyDecision::Deny,
            )?],
            AutonomyDecision::Deny,
            AutonomyDecision::Deny,
            None,
        )?;
        let hold = proposal(Action::Hold {
            reason: crate::HoldReason::BetweenThresholds,
        });
        for verdict in [GateVerdict::Allow, GateVerdict::Deny, GateVerdict::Defer] {
            assert_eq!(
                decide(&unreadable, &hold, verdict),
                Err(BuilderError::NothingProposed)
            );
        }
        let opening_buy = buy(Purpose::Open, opening("300", false)?)?;
        assert_eq!(
            decide(&unreadable, &opening_buy, GateVerdict::Deny)?,
            Outcome::Skipped,
            "a deny never reaches the rules, so an unreadable rule set cannot turn it into an error"
        );
        assert_eq!(
            decide(&unreadable, &opening_buy, GateVerdict::Defer)?,
            Outcome::Deferred
        );
        assert_eq!(
            decide(&unreadable, &opening_buy, GateVerdict::Allow),
            Err(BuilderError::ReservedField),
            "an allowed buy does reach the rules"
        );
        for label in [Purpose::DiscretionaryExit, Purpose::Open] {
            assert_eq!(
                decide(&unreadable, &sell(label)?, GateVerdict::Allow)?,
                Outcome::Classified(Classification {
                    decision: AutonomyDecision::Auto,
                    by: DecidedBy::BuiltinRiskReducing,
                    approval: None,
                }),
                "a sell labelled {label:?}"
            );
            assert_eq!(
                decide(&unreadable, &sell(label)?, GateVerdict::Defer)?,
                Outcome::Deferred
            );
        }
        let asking = policy(
            Vec::new(),
            AutonomyDecision::Ask,
            AutonomyDecision::Ask,
            None,
        )?;
        let increase = ActionContext {
            purpose: Purpose::Increase,
            ..opening("300", false)?
        };
        assert_eq!(
            decide(
                &asking,
                &buy(Purpose::Increase, increase)?,
                GateVerdict::Allow
            )?,
            Outcome::Classified(Classification {
                decision: AutonomyDecision::Ask,
                by: DecidedBy::Default,
                approval: Some(ApprovalRequest {
                    approvers_required: NonZeroU8::MIN,
                    on_timeout: OnTimeout::Skip,
                }),
            })
        );
        Ok(())
    }

    /// A buy adds risk whatever it is labelled, so a reducing label, or two labels that disagree,
    /// is refused rather than waved through §6.2 step 3.
    #[test]
    fn a_buy_labelled_as_an_exit_is_refused() -> Checked {
        let auto = policy(
            Vec::new(),
            AutonomyDecision::Deny,
            AutonomyDecision::Deny,
            None,
        )?;
        for label in [
            Purpose::DiscretionaryExit,
            Purpose::OwnerExit,
            Purpose::RiskExit,
            Purpose::Protective,
        ] {
            let facts = ActionContext {
                purpose: label,
                ..opening("300", false)?
            };
            assert_eq!(
                decide(&auto, &buy(label, facts)?, GateVerdict::Allow),
                Err(BuilderError::MislabelledBuy),
                "{label:?}"
            );
        }
        assert_eq!(
            decide(
                &auto,
                &buy(Purpose::Increase, opening("300", false)?)?,
                GateVerdict::Allow
            ),
            Err(BuilderError::MislabelledBuy),
            "the proposal says increase and its facts say open"
        );
        assert_eq!(
            decide(
                &auto,
                &buy(Purpose::Open, opening("300", false)?)?,
                GateVerdict::Allow
            )?,
            Outcome::Classified(Classification {
                decision: AutonomyDecision::Deny,
                by: DecidedBy::Default,
                approval: None,
            }),
            "a consistently labelled buy reaches the rules"
        );
        Ok(())
    }
}
