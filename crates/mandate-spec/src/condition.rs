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
    /// them at §6.2 step 4. A field the facts do not carry, or an operator the value's type does not
    /// admit (V-023), is [`SpecError::InvalidInput`] naming the field, not a silent false: a rule that
    /// cannot be evaluated must not read as "does not match", which would quietly widen autonomy.
    ///
    /// `all` and `any` evaluate **every** child rather than stopping at the first that decides, so an
    /// unevaluatable comparison is an error wherever it sits in the tree and never hides behind a
    /// sibling. Decimals compare as exact [`Ratio`]s, never as text, so `10` is above `9`; a value
    /// wider than a `Ratio` holds is [`SpecError::Num`] rather than a rounded comparison.
    pub fn matches(&self, facts: &dyn Facts) -> Result<bool, SpecError> {
        match self {
            Self::All(children) => {
                let mut held = true;
                for child in children {
                    held &= child.matches(facts)?;
                }
                Ok(held)
            }
            Self::Any(children) => {
                let mut held = false;
                for child in children {
                    held |= child.matches(facts)?;
                }
                Ok(held)
            }
            Self::Not(child) => child.matches(facts).map(|held| !held),
            Self::Compare { field, op, value } => compare(*field, *op, value, facts),
        }
    }

    /// True for the catch-all `purpose in [open, increase]` that W-005 warns about when a rule
    /// follows it and can therefore never match.
    ///
    /// Only a top-level `purpose in [...]` naming both purposes is one: rules see nothing but `open`
    /// and `increase` actions, so that comparison matches every action a later rule could.
    pub fn is_catch_all(&self) -> bool {
        match self {
            Self::Compare {
                field: ConditionField::Purpose,
                op: Operator::In,
                value: ConditionValue::List(members),
            } => ["open", "increase"]
                .iter()
                .all(|purpose| members.iter().any(|member| member == purpose)),
            _ => false,
        }
    }
}

/// One §6.3 comparison against the fact the field names.
fn compare(
    field: ConditionField,
    op: Operator,
    value: &ConditionValue,
    facts: &dyn Facts,
) -> Result<bool, SpecError> {
    let unusable = || SpecError::InvalidInput {
        what: field.as_str(),
    };
    match value {
        ConditionValue::Decimal(wanted) => {
            let fact = facts.decimal_field(field).ok_or_else(unusable)?;
            let wanted = wanted.to_ratio()?;
            match op {
                Operator::Eq => Ok(fact == wanted),
                Operator::Ne => Ok(fact != wanted),
                Operator::Gt => Ok(fact > wanted),
                Operator::Gte => Ok(fact >= wanted),
                Operator::Lt => Ok(fact < wanted),
                Operator::Lte => Ok(fact <= wanted),
                Operator::In | Operator::NotIn => Err(unusable()),
            }
        }
        ConditionValue::Bool(wanted) => {
            let fact = facts.bool_field(field).ok_or_else(unusable)?;
            match op {
                Operator::Eq => Ok(fact == *wanted),
                Operator::Ne => Ok(fact != *wanted),
                _ => Err(unusable()),
            }
        }
        ConditionValue::Text(wanted) => {
            let fact = facts.enum_field(field).ok_or_else(unusable)?;
            match op {
                Operator::Eq => Ok(fact == wanted),
                Operator::Ne => Ok(fact != wanted),
                _ => Err(unusable()),
            }
        }
        ConditionValue::List(members) => {
            let fact = facts.enum_field(field).ok_or_else(unusable)?;
            let listed = members.iter().any(|member| member == fact);
            match op {
                Operator::In => Ok(listed),
                Operator::NotIn => Ok(!listed),
                _ => Err(unusable()),
            }
        }
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::DecGrammar;

    type Checked = Result<(), Box<dyn std::error::Error>>;

    /// Facts from three maps, so a test states exactly the fields it reads and a missing one is
    /// absent.
    #[derive(Default)]
    struct Stated {
        enums: BTreeMap<ConditionField, &'static str>,
        decimals: BTreeMap<ConditionField, Ratio>,
        bools: BTreeMap<ConditionField, bool>,
    }

    impl Facts for Stated {
        fn enum_field(&self, field: ConditionField) -> Option<&str> {
            self.enums.get(&field).copied()
        }
        fn decimal_field(&self, field: ConditionField) -> Option<Ratio> {
            self.decimals.get(&field).copied()
        }
        fn bool_field(&self, field: ConditionField) -> Option<bool> {
            self.bools.get(&field).copied()
        }
    }

    fn facts() -> Result<Stated, Box<dyn std::error::Error>> {
        let mut stated = Stated::default();
        stated.enums.insert(ConditionField::Purpose, "open");
        stated
            .decimals
            .insert(ConditionField::OrderUsd, Ratio::parse("10")?);
        stated.bools.insert(ConditionField::NewInstrument, true);
        Ok(stated)
    }

    fn order_usd(op: Operator, value: &str) -> Result<Condition, Box<dyn std::error::Error>> {
        Ok(Condition::Compare {
            field: ConditionField::OrderUsd,
            op,
            value: ConditionValue::Decimal(SchemaDec::parse(value, DecGrammar::Decimal)?),
        })
    }

    fn purpose(op: Operator, value: ConditionValue) -> Condition {
        Condition::Compare {
            field: ConditionField::Purpose,
            op,
            value,
        }
    }

    fn new_instrument(op: Operator, value: bool) -> Condition {
        Condition::Compare {
            field: ConditionField::NewInstrument,
            op,
            value: ConditionValue::Bool(value),
        }
    }

    fn unevaluatable() -> Condition {
        Condition::Compare {
            field: ConditionField::Drawdown,
            op: Operator::Gt,
            value: ConditionValue::Bool(true),
        }
    }

    /// Each ordering operator against a value below, at, and above the fact of 10, numerically:
    /// `9` and `9.5` are below `10` although they sort after it as text.
    #[test]
    fn decimals_compare_numerically_with_every_operator() -> Checked {
        let facts = facts()?;
        let table = [
            (Operator::Eq, [false, true, false]),
            (Operator::Ne, [true, false, true]),
            (Operator::Gt, [true, false, false]),
            (Operator::Gte, [true, true, false]),
            (Operator::Lt, [false, false, true]),
            (Operator::Lte, [false, true, true]),
        ];
        for (op, expected) in table {
            for (value, held) in ["9.5", "10", "10.5"].into_iter().zip(expected) {
                assert_eq!(
                    order_usd(op, value)?.matches(&facts)?,
                    held,
                    "10 {} {value}",
                    op.as_str()
                );
            }
        }
        assert!(order_usd(Operator::Gt, "9")?.matches(&facts)?);
        Ok(())
    }

    #[test]
    fn booleans_texts_and_lists_compare_as_written() -> Checked {
        let facts = facts()?;
        assert!(new_instrument(Operator::Eq, true).matches(&facts)?);
        assert!(!new_instrument(Operator::Eq, false).matches(&facts)?);
        assert!(!new_instrument(Operator::Ne, true).matches(&facts)?);
        assert!(new_instrument(Operator::Ne, false).matches(&facts)?);

        let text = |t: &str| ConditionValue::Text(t.to_owned());
        assert!(purpose(Operator::Eq, text("open")).matches(&facts)?);
        assert!(!purpose(Operator::Eq, text("increase")).matches(&facts)?);
        assert!(!purpose(Operator::Ne, text("open")).matches(&facts)?);
        assert!(purpose(Operator::Ne, text("increase")).matches(&facts)?);

        let list =
            |items: &[&str]| ConditionValue::List(items.iter().map(|t| (*t).to_owned()).collect());
        assert!(purpose(Operator::In, list(&["increase", "open"])).matches(&facts)?);
        assert!(!purpose(Operator::In, list(&["increase"])).matches(&facts)?);
        assert!(!purpose(Operator::NotIn, list(&["open"])).matches(&facts)?);
        assert!(purpose(Operator::NotIn, list(&["increase"])).matches(&facts)?);
        Ok(())
    }

    /// An operator the value's type does not admit, or a fact the action does not carry, is an
    /// error naming the field, never a false.
    #[test]
    fn an_unevaluatable_comparison_is_an_error_not_a_false() -> Checked {
        let facts = facts()?;
        let invalid = |what| Err(SpecError::InvalidInput { what });
        let list = ConditionValue::List(vec!["10".to_owned()]);
        let text = ConditionValue::Text("open".to_owned());
        let cases = [
            (order_usd(Operator::In, "10")?, "order_usd"),
            (order_usd(Operator::NotIn, "10")?, "order_usd"),
            (new_instrument(Operator::Gt, true), "new_instrument"),
            (purpose(Operator::Lt, text.clone()), "purpose"),
            (purpose(Operator::Eq, list.clone()), "purpose"),
            (unevaluatable(), "drawdown"),
            (
                Condition::Compare {
                    field: ConditionField::CombinedScore,
                    op: Operator::Gt,
                    value: ConditionValue::Decimal(SchemaDec::parse("0.5", DecGrammar::Decimal)?),
                },
                "combined_score",
            ),
            (
                Condition::Compare {
                    field: ConditionField::Session,
                    op: Operator::Eq,
                    value: text,
                },
                "session",
            ),
            (
                Condition::Compare {
                    field: ConditionField::AssetClass,
                    op: Operator::In,
                    value: list,
                },
                "asset_class",
            ),
        ];
        for (condition, field) in cases {
            assert_eq!(condition.matches(&facts), invalid(field), "{condition:?}");
        }
        let too_wide = order_usd(Operator::Gt, "0.0000000000000000000000001")?;
        assert!(
            matches!(too_wide.matches(&facts), Err(SpecError::Num(_))),
            "a value wider than a ratio holds is refused, not rounded"
        );
        Ok(())
    }

    /// W-005's catch-all is a top-level `purpose in` naming both purposes, extra members allowed; any
    /// other shape can leave an action for a later rule.
    #[test]
    fn a_catch_all_names_both_purposes_with_in() {
        let list =
            |items: &[&str]| ConditionValue::List(items.iter().map(|t| (*t).to_owned()).collect());
        let both = purpose(Operator::In, list(&["increase", "open"]));
        assert!(both.is_catch_all());
        assert!(purpose(Operator::In, list(&["open", "increase", "open"])).is_catch_all());
        for other in [
            purpose(Operator::In, list(&["open"])),
            purpose(Operator::In, list(&["increase"])),
            purpose(Operator::In, list(&["increase", "regular"])),
            purpose(Operator::NotIn, list(&["increase", "open"])),
            purpose(Operator::Eq, ConditionValue::Text("open".to_owned())),
            Condition::Compare {
                field: ConditionField::Session,
                op: Operator::In,
                value: list(&["increase", "open"]),
            },
            Condition::All(vec![both.clone()]),
            Condition::Not(Box::new(both.clone())),
        ] {
            assert!(!other.is_catch_all(), "{other:?}");
        }
    }

    /// `all`, `any`, and `not` as written, the empty `all` true and the empty `any` false, and an
    /// unevaluatable child an error even where a sibling already decided the answer.
    #[test]
    fn combinators_evaluate_every_child() -> Checked {
        let facts = facts()?;
        let yes = || new_instrument(Operator::Eq, true);
        let no = || new_instrument(Operator::Eq, false);
        assert!(Condition::All(vec![yes(), yes()]).matches(&facts)?);
        assert!(!Condition::All(vec![yes(), no()]).matches(&facts)?);
        assert!(!Condition::All(vec![no(), yes()]).matches(&facts)?);
        assert!(Condition::All(Vec::new()).matches(&facts)?);
        assert!(Condition::Any(vec![no(), yes()]).matches(&facts)?);
        assert!(Condition::Any(vec![yes(), no()]).matches(&facts)?);
        assert!(!Condition::Any(vec![no(), no()]).matches(&facts)?);
        assert!(!Condition::Any(Vec::new()).matches(&facts)?);
        assert!(!Condition::Not(Box::new(yes())).matches(&facts)?);
        assert!(Condition::Not(Box::new(no())).matches(&facts)?);

        let broken = Err(SpecError::InvalidInput { what: "drawdown" });
        assert_eq!(
            Condition::All(vec![no(), unevaluatable()]).matches(&facts),
            broken
        );
        assert_eq!(
            Condition::Any(vec![yes(), unevaluatable()]).matches(&facts),
            broken
        );
        assert_eq!(
            Condition::Not(Box::new(unevaluatable())).matches(&facts),
            broken
        );
        Ok(())
    }
}
