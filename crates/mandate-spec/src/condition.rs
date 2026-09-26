//! The condition language ([mandate spec §6.3](../../../docs/specs/mandate.md#63-condition-language)).
//!
//! The tree, its type rules (V-017, V-018, V-023), and [`Condition::matches`] live here and nowhere
//! else: §6.3 is part of the document, and only [`validate`](crate::validate) can report a condition
//! that breaks a rule. What lives in `mandate-builder` instead is §6.2 — the gate dry run, the
//! built-in AUTO purposes, first match wins, the default, the admission ceiling — and every
//! implementation of [`Facts`], which is a different rule and a different story (DEC-128 items 18
//! and 21).

use mandate_num::Ratio;

use crate::{SchemaDec, SpecError};

/// How deep §6.3 lets conditions nest (V-017).
pub const MAX_CONDITION_DEPTH: u8 = 4;

/// A condition: a boolean combinator or one comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    All(Vec<Condition>),
    Any(Vec<Condition>),
    Not(Box<Condition>),
    Compare {
        field: ConditionField,
        op: Operator,
        value: ConditionValue,
    },
}

impl Condition {
    /// Every comparison in the tree with its nesting depth, counting the outermost condition as 1.
    ///
    /// This is what V-017 (depth), V-018 (the reserved field), and V-023 (the type rules) are checked
    /// over, so they walk the tree once each rather than each re-deriving it.
    pub fn comparisons(&self) -> Vec<(&Condition, u8)> {
        let mut out = Vec::new();
        collect(self, 1, &mut out);
        out
    }

    /// True when this condition holds for an action's facts.
    ///
    /// The facts come from the caller, because they are the order path's: `mandate-builder` supplies
    /// them at §6.2 step 4. A field the facts do not carry is
    /// [`SpecError::Unimplemented`]-free but still an error, not a silent false: a rule that cannot be
    /// evaluated must not read as "does not match", which would quietly widen autonomy.
    pub fn matches(&self, facts: &dyn Facts) -> Result<bool, SpecError> {
        let _ = facts;
        Err(SpecError::Unimplemented)
    }

    /// True for the catch-all `purpose in [open, increase]` that W-005 warns about when a rule
    /// follows it and can therefore never match.
    pub fn is_catch_all(&self) -> bool {
        false
    }
}

fn collect<'a>(condition: &'a Condition, depth: u8, out: &mut Vec<(&'a Condition, u8)>) {
    match condition {
        Condition::All(children) | Condition::Any(children) => {
            for child in children {
                collect(child, depth.saturating_add(1), out);
            }
        }
        Condition::Not(child) => collect(child, depth.saturating_add(1), out),
        Condition::Compare { .. } => out.push((condition, depth)),
    }
}

/// The fields a rule may read (§6.3). Rules see only `open` and `increase` actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConditionField {
    Purpose,
    OrderUsd,
    /// The order builder's combined model score. **Not** a probability of profit (§6.3, §6.4).
    CombinedScore,
    Instrument,
    AssetClass,
    Session,
    FirstTradeInInstrument,
    /// The order would be the first in an instrument the research agent admitted, and the agent is
    /// flat in it (ADR-0002 part 3).
    NewInstrument,
    /// The admitting thesis's self-reported confidence: uncalibrated, and not a probability of profit
    /// (§8.2, DEC-126).
    ThesisConfidence,
    Drawdown,
    DailyPnlFraction,
    PositionUsdAfter,
    GrossUsdAfter,
    BoughtTodayUsd,
    PositionPnlFraction,
    /// Reserved: not usable until the input-drift detector ships (V-018, DEC-60, E17-5).
    UnusualInput,
}

impl ConditionField {
    pub fn as_str(self) -> &'static str {
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

    /// Which kind of value the field compares against, which is what V-023 checks.
    pub fn kind(self) -> FieldKind {
        match self {
            Self::Purpose | Self::AssetClass | Self::Session => FieldKind::Enum,
            Self::Instrument => FieldKind::Text,
            Self::FirstTradeInInstrument | Self::NewInstrument | Self::UnusualInput => {
                FieldKind::Bool
            }
            Self::OrderUsd
            | Self::CombinedScore
            | Self::ThesisConfidence
            | Self::Drawdown
            | Self::DailyPnlFraction
            | Self::PositionUsdAfter
            | Self::GrossUsdAfter
            | Self::BoughtTodayUsd
            | Self::PositionPnlFraction => FieldKind::Decimal,
        }
    }

    /// True for the two fields §6.3 bounds to the closed unit interval, which V-023 enforces on the
    /// value as well as on the type.
    pub fn is_unit_bounded(self) -> bool {
        matches!(self, Self::CombinedScore | Self::Drawdown)
    }

    /// True for a field no rule may use yet (V-018).
    pub fn is_reserved(self) -> bool {
        matches!(self, Self::UnusualInput)
    }
}

/// What kind of value a field takes, and therefore which operators apply (§6.3, V-023).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FieldKind {
    Enum,
    Text,
    Decimal,
    Bool,
}

/// The operators §6.3 allows. `in` and `not_in` take a non-empty array and only on an enum or string
/// field; a boolean takes `eq` and `ne` alone (V-023).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Operator {
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    In,
    NotIn,
}

impl Operator {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Eq => "eq",
            Self::Ne => "ne",
            Self::Gt => "gt",
            Self::Gte => "gte",
            Self::Lt => "lt",
            Self::Lte => "lte",
            Self::In => "in",
            Self::NotIn => "not_in",
        }
    }

    /// True for the two operators that take a list.
    pub fn takes_list(self) -> bool {
        matches!(self, Self::In | Self::NotIn)
    }

    /// True for the four that order their operands, which only a decimal field admits.
    pub fn is_ordering(self) -> bool {
        matches!(self, Self::Gt | Self::Gte | Self::Lt | Self::Lte)
    }
}

/// The value side of a comparison, as the document holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionValue {
    /// An enum member or an instrument id, as text.
    Text(String),
    Decimal(SchemaDec),
    Bool(bool),
    List(Vec<String>),
}

/// The facts one proposed action presents to the rules.
///
/// Implemented by `mandate-builder`, which knows the order path. A field the action does not have is
/// `None`, and [`Condition::matches`] treats that as an error rather than as a false: an
/// unevaluatable rule must never read as "no match", which would let an action through a rule meant
/// to stop it.
pub trait Facts {
    fn enum_field(&self, field: ConditionField) -> Option<&str>;
    fn decimal_field(&self, field: ConditionField) -> Option<Ratio>;
    fn bool_field(&self, field: ConditionField) -> Option<bool>;
}
