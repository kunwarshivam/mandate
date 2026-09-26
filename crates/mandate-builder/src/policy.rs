//! The autonomy policy and its condition language ([mandate spec §6.2] to §6.4).
//!
//! A policy is built once, from a mandate, and checked then: V-017's depth limit, V-023's type
//! rules, V-018's reserved field, and unique rule ids are load-time errors, so evaluating a
//! condition is a numeric comparison of two exact decimals and never a surprise (DEC-130 item 12).
//!
//! [mandate spec §6.2]: ../../../docs/specs/mandate.md#62-evaluation

use core::num::NonZeroU8;
use std::collections::BTreeSet;

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_num::{Signed, Unit, Usd};

use crate::{BuilderError, MAX_CONDITION_DEPTH};

/// What an order does to the agent's position (spec §6.1). The **gate** assigns this from side and
/// position; a builder proposal carries it as a label the gate re-derives (DEC-130 item 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Purpose {
    Open,
    Increase,
    DiscretionaryExit,
    OwnerExit,
    RiskExit,
    Protective,
}

impl Purpose {
    /// Whether the purpose reduces risk, which spec §6.2 step 3 makes AUTO by a built-in rule
    /// whatever the owner's rules say (DEC-05). Rules see `open` and `increase` alone (§6.3).
    pub fn is_risk_reducing(self) -> bool {
        match self {
            Self::Open | Self::Increase => false,
            Self::DiscretionaryExit | Self::OwnerExit | Self::RiskExit | Self::Protective => true,
        }
    }
}

/// The session an order would reach the market in (spec §6.3). Crypto trades continuously, so
/// `Crypto` is the one value with no regular session and never a deferral (DEC-130 item 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Session {
    PreMarket,
    Regular,
    AfterHours,
    Crypto,
}

/// What the mandate allows for an action (spec §6.2). `Ord` is severity, so the admission ceiling of
/// step 5 is `max` and can only tighten (MI-17, DEC-05).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Decision {
    Auto,
    Ask,
    Deny,
}

/// An autonomy rule's id, unique within a mandate (spec §3).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RuleId(String);

impl RuleId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A field a condition may read (spec §6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Field {
    Purpose,
    OrderUsd,
    CombinedScore,
    Instrument,
    AssetClass,
    Session,
    FirstTradeInInstrument,
    NewInstrument,
    ThesisConfidence,
    Drawdown,
    DailyPnlFraction,
    PositionUsdAfter,
    GrossUsdAfter,
    BoughtTodayUsd,
    PositionPnlFraction,
    /// Reserved until the input-drift detector ships (V-018); a rule naming it is refused.
    UnusualInput,
}

/// What kind of value a field takes, which decides both the operators V-023 allows and how a
/// comparison is made (spec §6.3). Enum fields carry the values they accept: `purpose` takes `open`
/// and `increase` alone, because rules see no other purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Flag,
    Money,
    UnitInterval,
    SignedFraction,
    Enumerated(&'static [&'static str]),
    Text,
}

impl Field {
    /// The kind of value the field takes (spec §6.3, V-023).
    pub fn kind(self) -> Kind {
        match self {
            Self::Purpose => Kind::Enumerated(&["increase", "open"]),
            Self::Session => Kind::Enumerated(&["after_hours", "crypto", "pre_market", "regular"]),
            Self::AssetClass => Kind::Enumerated(&["crypto", "us_equity"]),
            Self::Instrument => Kind::Text,
            Self::OrderUsd
            | Self::PositionUsdAfter
            | Self::GrossUsdAfter
            | Self::BoughtTodayUsd => Kind::Money,
            Self::CombinedScore | Self::Drawdown | Self::ThesisConfidence => Kind::UnitInterval,
            Self::DailyPnlFraction | Self::PositionPnlFraction => Kind::SignedFraction,
            Self::FirstTradeInInstrument | Self::NewInstrument | Self::UnusualInput => Kind::Flag,
        }
    }

    /// The field's name in the mandate document, which an error reports (ES-09).
    pub fn name(self) -> &'static str {
        match self {
            Self::Purpose => "purpose",
            Self::OrderUsd => "order_usd",
            Self::CombinedScore => "combined_score",
            Self::Instrument => "instrument",
            Self::AssetClass => "asset_class",
            Self::Session => "session",
            Self::FirstTradeInInstrument => "first_trade_in_instrument",
            Self::NewInstrument => "new_instrument",
            Self::ThesisConfidence => "thesis_confidence",
            Self::Drawdown => "drawdown",
            Self::DailyPnlFraction => "daily_pnl_fraction",
            Self::PositionUsdAfter => "position_usd_after",
            Self::GrossUsdAfter => "gross_usd_after",
            Self::BoughtTodayUsd => "bought_today_usd",
            Self::PositionPnlFraction => "position_pnl_fraction",
            Self::UnusualInput => "unusual_input",
        }
    }
}

/// A comparison operator (spec §6.3). `In` and `NotIn` take enum and string fields only (V-023).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Op {
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    In,
    NotIn,
}

/// A condition's right-hand value, already typed for its field so a comparison is never lexical
/// (DEC-130 item 12). A value beyond the places its type holds is refused when the policy is built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Flag(bool),
    Text(String),
    Money(Usd),
    Unit(Unit),
    Signed(Signed),
    Set(BTreeSet<String>),
}

/// A condition tree (spec §6.3), nesting at most [`MAX_CONDITION_DEPTH`] levels (V-017).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    All(Vec<Condition>),
    Any(Vec<Condition>),
    Not(Box<Condition>),
    Compare { field: Field, op: Op, value: Value },
}

/// One autonomy rule. Rule order is significant: the first match decides (spec §6.2 step 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub id: RuleId,
    pub when: Condition,
    pub then: Decision,
}

/// The §6 fields of a mandate, and nothing else: the narrow view this crate reads rather than the
/// whole document, which stream F's `mandate-spec` owns (DEC-130 item 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutonomyPolicy {
    rules: Vec<Rule>,
    default: Decision,
    admission: Decision,
    two_approver_above_usd: Option<Usd>,
}

impl AutonomyPolicy {
    /// Builds a policy, refusing what V-017, V-018, and V-023 refuse and any repeated rule id, so
    /// every later evaluation is a comparison that cannot fail on a type (DEC-130 item 12).
    ///
    /// Errors: `condition_too_deep`, `condition_type_mismatch`, `reserved_field`,
    /// `duplicate_rule_id`.
    pub fn new(
        rules: Vec<Rule>,
        default: Decision,
        admission: Decision,
        two_approver_above_usd: Option<Usd>,
    ) -> Result<Self, BuilderError> {
        let mut seen = BTreeSet::new();
        for rule in &rules {
            if !seen.insert(rule.id.as_str().to_owned()) {
                return Err(BuilderError::DuplicateRuleId(rule.id.as_str().to_owned()));
            }
            check_condition(&rule.when, 1)?;
        }
        Ok(Self {
            rules,
            default,
            admission,
            two_approver_above_usd,
        })
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    pub fn default_decision(&self) -> Decision {
        self.default
    }

    /// The ceiling on the decision for the first order in a newly admitted instrument (spec §6.2
    /// step 5); the platform default is `ask` (§7).
    pub fn admission(&self) -> Decision {
        self.admission
    }

    pub fn two_approver_above_usd(&self) -> Option<Usd> {
        self.two_approver_above_usd
    }
}

/// Everything a condition may read about a proposed action (spec §6.3). Every field is required:
/// nothing is inferred from another, because `first_trade_in_instrument` read off the position would
/// mislabel a re-entry after a round trip, which is exactly `MC-B25` (DEC-130 item 19, DEC-85).
///
/// The exposure fields are the order's **after** values, so a rule can bound what order splitting
/// would otherwise evade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionContext {
    pub purpose: Purpose,
    pub order_usd: Usd,
    pub combined_score: Unit,
    pub instrument: InstrumentId,
    pub asset_class: AssetClass,
    pub session: Session,
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

/// What reached the decision (spec §6.2). `AdmissionCeiling` is reported only when the ceiling
/// actually changed the decision, so `MC-A13` reads `rule:routine` under `admission: auto` while
/// `MC-A12` reads the ceiling (DEC-130 item 13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecidedBy {
    BuiltinRiskReducing,
    Rule(RuleId),
    Default,
    AdmissionCeiling,
}

/// `skip`, always (spec §6.4): an approval that times out never adds risk (DEC-06).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnTimeout {
    Skip,
}

/// What an ASK needs (spec §6.4). Two distinct approvers above `two_approver_above_usd`, which is a
/// **strict** comparison, so an order exactly at the threshold needs one (`MC-A08`'s neighbour).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Approval {
    pub approvers_required: NonZeroU8,
    pub on_timeout: OnTimeout,
}

/// The classification of one action, with the rule that decided it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Autonomy {
    pub decision: Decision,
    pub by: DecidedBy,
    /// Present exactly when the decision is [`Decision::Ask`].
    pub approval: Option<Approval>,
}

/// Classifies a proposed action AUTO, ASK, or DENY (spec §6.2 steps 3 to 5): a purpose other than
/// `open` or `increase` is AUTO by a built-in rule; otherwise the **first** matching rule decides,
/// and `autonomy.default` decides when none matches; then, for the first order in a newly admitted
/// instrument, the decision becomes the **stricter** of that and `autonomy.admission`, so the
/// ceiling can tighten but never loosen and an owner's `deny` rule survives `admission: auto`
/// (MI-17, DEC-05).
///
/// An ASK carries its approver count and `on_timeout: skip`.
pub fn classify(policy: &AutonomyPolicy, action: &ActionContext) -> Result<Autonomy, BuilderError> {
    let _ = (policy, action);
    Err(BuilderError::Unimplemented)
}

/// V-017's depth limit, V-018's reserved field, and V-023's type rules, checked once when the policy
/// is built so that evaluating a condition cannot fail (DEC-130 item 12).
fn check_condition(condition: &Condition, depth: u32) -> Result<(), BuilderError> {
    if depth > MAX_CONDITION_DEPTH {
        return Err(BuilderError::ConditionTooDeep);
    }
    let deeper = depth.checked_add(1).ok_or(BuilderError::ConditionTooDeep)?;
    match condition {
        Condition::All(parts) | Condition::Any(parts) => {
            for part in parts {
                check_condition(part, deeper)?;
            }
            Ok(())
        }
        Condition::Not(inner) => check_condition(inner, deeper),
        Condition::Compare { field, op, value } => {
            if *field == Field::UnusualInput {
                return Err(BuilderError::ReservedField);
            }
            if value_matches(field.kind(), *op, value) {
                Ok(())
            } else {
                Err(BuilderError::ConditionTypeMismatch(field.name()))
            }
        }
    }
}

/// V-023: a decimal field takes a decimal of its own kind with an ordering or equality operator and
/// never a set; an enum or string field takes a listed value or a non-empty set of them and never an
/// ordering operator; a boolean field takes `eq` or `ne` on a flag.
fn value_matches(kind: Kind, op: Op, value: &Value) -> bool {
    let ordered = matches!(op, Op::Gt | Op::Gte | Op::Lt | Op::Lte);
    let set_op = matches!(op, Op::In | Op::NotIn);
    match (kind, value) {
        (Kind::Flag, Value::Flag(_)) => matches!(op, Op::Eq | Op::Ne),
        (Kind::Money, Value::Money(_))
        | (Kind::UnitInterval, Value::Unit(_))
        | (Kind::SignedFraction, Value::Signed(_)) => ordered || matches!(op, Op::Eq | Op::Ne),
        (Kind::Enumerated(allowed), Value::Text(text)) => {
            matches!(op, Op::Eq | Op::Ne) && allowed.contains(&text.as_str())
        }
        (Kind::Enumerated(allowed), Value::Set(values)) => {
            set_op && !values.is_empty() && values.iter().all(|v| allowed.contains(&v.as_str()))
        }
        (Kind::Text, Value::Text(_)) => matches!(op, Op::Eq | Op::Ne),
        (Kind::Text, Value::Set(values)) => set_op && !values.is_empty(),
        _ => false,
    }
}
