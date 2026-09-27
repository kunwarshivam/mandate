//! The strict reader behind [`Mandate::parse`]: [`schemas/mandate.schema.json`](../../../../schemas/mandate.schema.json)
//! as code, one reader per `$def`, each naming the JSON Pointer it failed at (ES-09, ES-22).
//!
//! Every object in the schema is `additionalProperties: false` and requires every property it lists,
//! so one list of names per object, space-separated as the schema's `required` array reads, is both
//! its allowed and its required members. A pattern is
//! matched as ECMA-262 does, which is what JSON Schema specifies: `$` is the end of the text.

use std::collections::BTreeSet;

use mandate_canon::{Digest, Key, Object, Value};
use mandate_domain::{AssetClass, AssetId, AutonomyDecision, Environment};
use mandate_time::Date;

use super::{
    AgentName, Approval, ApproverRef, Autonomy, Behavior, Cadence, Capital, Channel, ConnectionId,
    EventSource, Goal, HourMinute, InstrumentRef, LadderAction, LadderRung, LimitAction, Mandate,
    ModelId, ModelParam, Notifications, OnComplete, OnTimeout, ParamValue, Protection, QuietHours,
    Research, Risk, Rule, RuleId, ScaleAction, SignalModel, Sizing, SizingMethod, Universe,
};
use crate::condition::{
    Condition, ConditionField, ConditionValue, FieldKind, MAX_CONDITION_DEPTH, Operator,
};
use crate::{DecGrammar, ParseError, Pointer, SchemaDec};

#[cfg(test)]
mod tests;

type Parsed<T> = Result<T, ParseError>;

/// The deepest condition the parse reads: one level past V-017's limit, so V-017 still reports the
/// first level too deep, and nothing deeper is recursed into (DEC-151).
const PARSE_DEPTH: u8 = MAX_CONDITION_DEPTH.saturating_add(1);

const CONDITION_FIELDS: [ConditionField; 16] = [
    ConditionField::Purpose,
    ConditionField::OrderUsd,
    ConditionField::CombinedScore,
    ConditionField::Instrument,
    ConditionField::AssetClass,
    ConditionField::Session,
    ConditionField::FirstTradeInInstrument,
    ConditionField::NewInstrument,
    ConditionField::ThesisConfidence,
    ConditionField::Drawdown,
    ConditionField::DailyPnlFraction,
    ConditionField::PositionUsdAfter,
    ConditionField::GrossUsdAfter,
    ConditionField::BoughtTodayUsd,
    ConditionField::PositionPnlFraction,
    ConditionField::UnusualInput,
];

const OPERATORS: [Operator; 8] = [
    Operator::Eq,
    Operator::Ne,
    Operator::Gt,
    Operator::Gte,
    Operator::Lt,
    Operator::Lte,
    Operator::In,
    Operator::NotIn,
];

const DECISIONS: [(&str, AutonomyDecision); 3] = [
    ("auto", AutonomyDecision::Auto),
    ("ask", AutonomyDecision::Ask),
    ("deny", AutonomyDecision::Deny),
];

const ASSET_CLASSES: [AssetClass; 2] = [AssetClass::Crypto, AssetClass::UsEquity];

/// A value and the pointer it sits at.
struct Node<'a> {
    value: &'a Value,
    path: String,
}

/// An object's members, already checked for any the schema does not list.
struct Members<'a> {
    members: &'a Object,
    path: String,
}

impl<'a> Members<'a> {
    /// The member `name`, or [`ParseError::MissingMember`]: every listed member is required.
    fn get(&self, name: &str) -> Parsed<Node<'a>> {
        let path = format!("{}/{name}", self.path);
        match self.members.get(name) {
            Some(value) => Ok(Node { value, path }),
            None => Err(ParseError::MissingMember {
                path: Pointer::new(&path),
            }),
        }
    }

    fn dec(&self, name: &str, grammar: DecGrammar) -> Parsed<SchemaDec> {
        self.get(name)?.decimal(grammar)
    }

    fn dec_or_null(&self, name: &str, grammar: DecGrammar) -> Parsed<Option<SchemaDec>> {
        self.get(name)?.nullable(|n| n.decimal(grammar))
    }

    fn int(&self, name: &str, min: u32, max: u32) -> Parsed<u32> {
        self.get(name)?.integer(min, max)
    }

    fn flag(&self, name: &str) -> Parsed<bool> {
        match self.get(name)? {
            Node {
                value: Value::Bool(flag),
                ..
            } => Ok(*flag),
            other => Err(other.wrong_type()),
        }
    }

    /// Each item of an array member, read by `read`.
    fn each<T>(
        &self,
        name: &str,
        (min, max): (usize, usize),
        read: fn(&Node<'a>) -> Parsed<T>,
    ) -> Parsed<Vec<T>> {
        self.get(name)?
            .items(min, max, false)?
            .iter()
            .map(read)
            .collect()
    }
}

impl<'a> Node<'a> {
    fn at(&self) -> Pointer {
        Pointer::new(&self.path)
    }

    fn wrong_type(&self) -> ParseError {
        ParseError::WrongType { path: self.at() }
    }

    fn off_pattern(&self) -> ParseError {
        ParseError::OffPattern { path: self.at() }
    }

    fn out_of_bounds(&self) -> ParseError {
        ParseError::OutOfBounds { path: self.at() }
    }

    /// An object whose members are all among `names`; which of them are present is checked as each
    /// is read.
    fn members(&self, names: &str) -> Parsed<Members<'a>> {
        let members = self.value.as_object().ok_or_else(|| self.wrong_type())?;
        let allowed: Vec<&str> = names.split_whitespace().collect();
        if let Some(extra) = members.keys().find(|k| !allowed.contains(&k.as_str())) {
            return Err(ParseError::UnknownMember {
                path: Pointer::new(&format!("{}/{extra}", self.path)),
            });
        }
        Ok(Members {
            members,
            path: self.path.clone(),
        })
    }

    fn text(&self) -> Parsed<&'a str> {
        self.value.as_str().ok_or_else(|| self.wrong_type())
    }

    /// A string of `min..=max` characters. JSON Schema counts code points, not bytes.
    fn bounded_text(&self, min: usize, max: usize) -> Parsed<String> {
        let text = self.text()?;
        if (min..=max).contains(&text.chars().count()) {
            Ok(text.to_owned())
        } else {
            Err(self.out_of_bounds())
        }
    }

    fn patterned(&self, matches: fn(&str) -> bool) -> Parsed<String> {
        let text = self.text()?;
        if matches(text) {
            Ok(text.to_owned())
        } else {
            Err(self.off_pattern())
        }
    }

    fn integer(&self, min: u32, max: u32) -> Parsed<u32> {
        let n = self.value.as_int().ok_or_else(|| self.wrong_type())?;
        u32::try_from(n)
            .ok()
            .filter(|n| (min..=max).contains(n))
            .ok_or_else(|| self.out_of_bounds())
    }

    /// An `enum` or a `const`. Neither has a `type`, so any value off the list, a string or not, is
    /// [`ParseError::NotInEnum`].
    fn one_of<T: Copy>(&self, table: &[(&str, T)]) -> Parsed<T> {
        self.value
            .as_str()
            .and_then(|text| table.iter().find(|(name, _)| *name == text))
            .map(|(_, value)| *value)
            .ok_or_else(|| ParseError::NotInEnum { path: self.at() })
    }

    /// [`Node::one_of`] over a type that names its own members.
    fn named<T: Copy>(&self, all: &[T], name: fn(T) -> &'static str) -> Parsed<T> {
        let table: Vec<(&str, T)> = all.iter().map(|v| (name(*v), *v)).collect();
        self.one_of(&table)
    }

    fn decimal(&self, grammar: DecGrammar) -> Parsed<SchemaDec> {
        match self.value {
            Value::Int(_) => Err(ParseError::DecimalAsNumber { path: self.at() }),
            Value::Str(text) => {
                SchemaDec::parse(text, grammar).map_err(|_| ParseError::OffGrammar {
                    path: self.at(),
                    grammar,
                })
            }
            _ => Err(self.wrong_type()),
        }
    }

    /// `oneOf [X, {type: null}]`.
    fn nullable<T>(&self, read: impl FnOnce(&Self) -> Parsed<T>) -> Parsed<Option<T>> {
        match self.value {
            Value::Null => Ok(None),
            _ => read(self).map(Some),
        }
    }

    /// An array of `min..=max` items; with `unique`, the second of two equal items is
    /// [`ParseError::NotUnique`].
    fn items(&self, min: usize, max: usize, unique: bool) -> Parsed<Vec<Node<'a>>> {
        let items = self.value.as_array().ok_or_else(|| self.wrong_type())?;
        if !(min..=max).contains(&items.len()) {
            return Err(self.out_of_bounds());
        }
        let nodes: Vec<Node<'a>> = items
            .iter()
            .enumerate()
            .map(|(index, value)| Node {
                value,
                path: format!("{}/{index}", self.path),
            })
            .collect();
        for (index, node) in nodes.iter().enumerate() {
            if unique
                && items
                    .get(..index)
                    .is_some_and(|before| before.contains(node.value))
            {
                return Err(ParseError::NotUnique { path: node.at() });
            }
        }
        Ok(nodes)
    }

    /// A unique-items array of enum members, at least `min` of them.
    fn set_of<T: Copy + Ord>(&self, min: usize, table: &[(&str, T)]) -> Parsed<BTreeSet<T>> {
        let items = self.items(min, usize::MAX, true)?;
        items.iter().map(|item| item.one_of(table)).collect()
    }

    /// `$defs/content_ref`: `sha256:` and 64 lowercase hexadecimal digits.
    fn digest(&self) -> Parsed<Digest> {
        self.text()?
            .strip_prefix("sha256:")
            .and_then(Digest::from_hex)
            .ok_or_else(|| self.off_pattern())
    }

    /// `$defs/date`, a pattern and not a calendar: a text it matches that names no real day is kept
    /// in the document for V-015 and read here as no date (DEC-151).
    fn date(&self) -> Parsed<Option<Date>> {
        Ok(Date::parse(&self.patterned(is_date)?).ok())
    }

    fn hour_minute(&self) -> Parsed<HourMinute> {
        let text = self.patterned(is_hour_minute)?;
        let (hour, minute) = text.split_once(':').ok_or_else(|| self.off_pattern())?;
        Ok(HourMinute {
            hour: hour.parse().map_err(|_| self.off_pattern())?,
            minute: minute.parse().map_err(|_| self.off_pattern())?,
        })
    }

    /// `$defs/uuid`.
    fn asset_id(&self) -> Parsed<AssetId> {
        AssetId::parse(self.text()?).map_err(|_| self.off_pattern())
    }
}

/// `^[a-z0-9][a-z0-9-]{0,62}$`.
fn is_name(text: &str) -> bool {
    let lower_or_digit = |b: &u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    let bytes = text.as_bytes();
    bytes.first().is_some_and(lower_or_digit)
        && bytes.len() <= 63
        && bytes.iter().all(|b| lower_or_digit(b) || *b == b'-')
}

/// `$defs/id`: `^[A-Za-z0-9_-]{1,64}$`.
pub(super) fn is_id(text: &str) -> bool {
    (1..=64).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Two ASCII digits between `low` and `high` inclusive. Text order is numeric order at a fixed width.
fn two_digits(text: Option<&str>, low: &str, high: &str) -> bool {
    text.is_some_and(|t| t.bytes().all(|b| b.is_ascii_digit()) && (low..=high).contains(&t))
}

/// `$defs/date`: `^[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])$`.
fn is_date(text: &str) -> bool {
    text.len() == 10
        && text
            .get(..4)
            .is_some_and(|y| y.bytes().all(|b| b.is_ascii_digit()))
        && text.get(4..5) == Some("-")
        && two_digits(text.get(5..7), "01", "12")
        && text.get(7..8) == Some("-")
        && two_digits(text.get(8..10), "01", "31")
}

/// `$defs/hhmm`: `^([01][0-9]|2[0-3]):[0-5][0-9]$`.
fn is_hour_minute(text: &str) -> bool {
    text.len() == 5
        && two_digits(text.get(..2), "00", "23")
        && text.get(2..3) == Some(":")
        && two_digits(text.get(3..), "00", "59")
}

/// A signal model's version: three of `0|[1-9][0-9]{0,5}`, joined by `.`.
fn is_model_version(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            (1..=6).contains(&part.len())
                && part.bytes().all(|b| b.is_ascii_digit())
                && (*part == "0" || !part.starts_with('0'))
        })
}

/// A parameter key, `^[a-z][a-z0-9_]{0,63}$`: exactly the canonical key grammar.
fn is_param_key(text: &str) -> bool {
    Key::new(text).is_ok()
}

pub(super) fn mandate(value: &Value) -> Parsed<Mandate> {
    let root = Node {
        value,
        path: String::new(),
    };
    let doc = root.members(
        "mandate_schema_version name source_text_ref environment connection_id capital goal \
         universe behavior protection risk autonomy notifications",
    )?;
    let version = super::MANDATE_SCHEMA_VERSION;
    let schema_version = doc.get("mandate_schema_version")?;
    if schema_version.value.as_int() != Some(u64::from(version)) {
        return Err(ParseError::NotInEnum {
            path: schema_version.at(),
        });
    }
    Ok(Mandate {
        mandate_schema_version: version,
        name: AgentName(doc.get("name")?.patterned(is_name)?),
        source_text_ref: doc.get("source_text_ref")?.nullable(Node::digest)?,
        environment: doc
            .get("environment")?
            .one_of(&[("paper", Environment::Paper), ("live", Environment::Live)])?,
        connection_id: ConnectionId(doc.get("connection_id")?.patterned(is_id)?),
        capital: capital(&doc.get("capital")?)?,
        goal: goal(&doc.get("goal")?)?,
        universe: universe(&doc.get("universe")?)?,
        behavior: behavior(&doc.get("behavior")?)?,
        protection: protection(&doc.get("protection")?)?,
        risk: risk(&doc.get("risk")?)?,
        autonomy: autonomy(&doc.get("autonomy")?)?,
        notifications: notifications(&doc.get("notifications")?)?,
        source: value.clone(),
    })
}

fn capital(node: &Node<'_>) -> Parsed<Capital> {
    let m = node.members("allocation_usd max_loss_from_allocation")?;
    Ok(Capital {
        allocation_usd: m.dec("allocation_usd", DecGrammar::PositiveDecimal)?,
        max_loss_from_allocation: m.dec("max_loss_from_allocation", DecGrammar::OpenFraction)?,
    })
}

/// `$defs/goal`, a `oneOf` whose branches differ by `type`, so the parse dispatches on it and names
/// the member the chosen branch lacks rather than reporting the whole goal off its union.
fn goal(node: &Node<'_>) -> Parsed<Goal> {
    let members = node.value.as_object().ok_or_else(|| node.wrong_type())?;
    let path = node.path.clone();
    let kind = Members { members, path }.get("type")?.one_of(&[
        ("continuous", 0),
        ("accumulate", 1),
        ("profit_stop", 2),
    ])?;
    let end_date = |m: &Members<'_>| m.get("end_date")?.nullable(Node::date).map(Option::flatten);
    let on_complete = |m: &Members<'_>| {
        m.get("on_complete")?.named(
            &[
                OnComplete::HoldProtected,
                OnComplete::DisarmLadder,
                OnComplete::Release,
            ],
            OnComplete::as_str,
        )
    };
    let usd = DecGrammar::PositiveDecimal;
    match kind {
        0 => {
            let m = node.members("type end_date on_complete")?;
            Ok(Goal::Continuous {
                end_date: end_date(&m)?,
                on_complete: on_complete(&m)?,
            })
        }
        1 => {
            let m = node.members(
                "type instrument target_qty max_avg_price max_spend_usd end_date on_complete",
            )?;
            Ok(Goal::Accumulate {
                instrument: m.get("instrument")?.asset_id()?,
                target_qty: m.dec("target_qty", usd)?,
                max_avg_price: m.dec_or_null("max_avg_price", usd)?,
                max_spend_usd: m.dec("max_spend_usd", usd)?,
                end_date: end_date(&m)?,
                on_complete: on_complete(&m)?,
            })
        }
        _ => {
            let m = node.members("type profit_level end_date")?;
            Ok(Goal::ProfitStop {
                profit_level: m.dec("profit_level", usd)?,
                end_date: end_date(&m)?,
            })
        }
    }
}

fn universe(node: &Node<'_>) -> Parsed<Universe> {
    let m = node.members(
        "pinned pinned_instruments max_instruments asset_classes leveraged_etps_enabled \
         leveraged_etp_disclosure_version",
    )?;
    let classes: Vec<(&str, AssetClass)> = ASSET_CLASSES.map(|c| (c.as_str(), c)).to_vec();
    Ok(Universe {
        pinned: m.flag("pinned")?,
        pinned_instruments: m
            .get("pinned_instruments")?
            .items(0, 20, true)?
            .iter()
            .map(instrument_ref)
            .collect::<Parsed<_>>()?,
        max_instruments: m.int("max_instruments", 1, 20)?,
        asset_classes: m.get("asset_classes")?.set_of(1, &classes)?,
        leveraged_etps_enabled: m.flag("leveraged_etps_enabled")?,
        leveraged_etp_disclosure_version: m
            .get("leveraged_etp_disclosure_version")?
            .nullable(Node::digest)?,
    })
}

fn instrument_ref(node: &Node<'_>) -> Parsed<InstrumentRef> {
    let m = node.members("asset_id symbol asset_class")?;
    Ok(InstrumentRef {
        asset_id: m.get("asset_id")?.asset_id()?,
        symbol: m.get("symbol")?.bounded_text(1, 32)?,
        asset_class: m
            .get("asset_class")?
            .named(&ASSET_CLASSES, AssetClass::as_str)?,
    })
}

fn behavior(node: &Node<'_>) -> Parsed<Behavior> {
    let m = node.members("description signal_models research cadence sizing")?;
    let sizing = m.get("sizing")?;
    let sizing = sizing.members("method entry_threshold exit_threshold rebalance_band")?;
    Ok(Behavior {
        description: m.get("description")?.bounded_text(0, 4000)?,
        signal_models: m.each("signal_models", (1, 10), signal_model)?,
        research: m.get("research")?.nullable(research)?,
        cadence: cadence(&m.get("cadence")?)?,
        sizing: Sizing {
            method: sizing
                .get("method")?
                .one_of(&[("conviction_linear", SizingMethod::ConvictionLinear)])?,
            entry_threshold: sizing.dec("entry_threshold", DecGrammar::UnitPositive)?,
            exit_threshold: sizing.dec("exit_threshold", DecGrammar::UnitPositive)?,
            rebalance_band: sizing.dec("rebalance_band", DecGrammar::Fraction)?,
        },
    })
}

fn signal_model(node: &Node<'_>) -> Parsed<SignalModel> {
    let m =
        node.members("id version content_hash params weight max_output_age_s admits_instruments")?;
    let id = m.get("id")?;
    Ok(SignalModel {
        id: ModelId::parse(id.text()?).map_err(|_| id.off_pattern())?,
        version: m.get("version")?.patterned(is_model_version)?,
        content_hash: m.get("content_hash")?.digest()?,
        params: m.each("params", (0, 32), param)?,
        weight: m.dec("weight", DecGrammar::UnitPositive)?,
        max_output_age_s: m.int("max_output_age_s", 60, 86_400)?,
        admits_instruments: m.flag("admits_instruments")?,
    })
}

/// A parameter's value is `anyOf [decimal, boolean, string of at most 200]`. A string in the
/// `decimal` grammar is the decimal branch; any other string, `-0` and `0.020` among them, is text,
/// and a JSON number matches no branch.
fn param(node: &Node<'_>) -> Parsed<ModelParam> {
    let m = node.members("key value")?;
    let value = m.get("value")?;
    let value = match value.value {
        Value::Bool(flag) => ParamValue::Bool(*flag),
        Value::Int(_) => return Err(ParseError::DecimalAsNumber { path: value.at() }),
        Value::Str(text) => match SchemaDec::parse(text, DecGrammar::Decimal) {
            Ok(decimal) => ParamValue::Decimal(decimal),
            Err(_) => ParamValue::Text(value.bounded_text(0, 200)?),
        },
        _ => return Err(value.wrong_type()),
    };
    Ok(ModelParam {
        key: m.get("key")?.patterned(is_param_key)?,
        value,
    })
}

fn research(node: &Node<'_>) -> Parsed<Research> {
    let m = node.members("interval_s cost_cap_usd_per_day max_revisions_per_lineage")?;
    Ok(Research {
        interval_s: m.int("interval_s", 300, 604_800)?,
        cost_cap_usd_per_day: m.dec("cost_cap_usd_per_day", DecGrammar::PositiveDecimal)?,
        max_revisions_per_lineage: m.int("max_revisions_per_lineage", 0, 10)?,
    })
}

fn cadence(node: &Node<'_>) -> Parsed<Cadence> {
    let m = node.members("interval_s event_sources")?;
    Ok(Cadence {
        interval_s: m.int("interval_s", 60, 86_400)?,
        event_sources: m.get("event_sources")?.set_of(
            0,
            &[
                ("fills", EventSource::Fills),
                ("filings", EventSource::Filings),
                ("news", EventSource::News),
                ("price", EventSource::Price),
                ("schedule", EventSource::Schedule),
            ],
        )?,
    })
}

/// The `if enabled then stop_distance: open_fraction` clause: with protection on, a null stop matches
/// the property's `oneOf` and fails the `then`, whose `open_fraction` is `type: string`, so it is
/// [`ParseError::WrongType`] as `jsonschema` reports it (MC-S15).
fn protection(node: &Node<'_>) -> Parsed<Protection> {
    let m = node.members("enabled stop_distance take_profit_distance crypto_stop_limit_offset")?;
    let enabled = m.flag("enabled")?;
    let open = DecGrammar::OpenFraction;
    Ok(Protection {
        enabled,
        stop_distance: if enabled {
            Some(m.dec("stop_distance", open)?)
        } else {
            m.dec_or_null("stop_distance", open)?
        },
        take_profit_distance: m.dec_or_null("take_profit_distance", DecGrammar::PositiveDecimal)?,
        crypto_stop_limit_offset: m.dec_or_null("crypto_stop_limit_offset", open)?,
    })
}

fn risk(node: &Node<'_>) -> Parsed<Risk> {
    let m = node.members(
        "max_position_usd max_position_fraction max_gross_exposure_usd max_order_usd \
         max_orders_per_day max_daily_loss daily_loss_action max_drawdown drawdown_ladder \
         hysteresis breach_confirm_s daily_breach_min_s scale_lift_after_s reentry_cooldown_s \
         scale_action",
    )?;
    let (usd, open) = (DecGrammar::PositiveDecimal, DecGrammar::OpenFraction);
    Ok(Risk {
        max_position_usd: m.dec("max_position_usd", usd)?,
        max_position_fraction: m.dec("max_position_fraction", DecGrammar::UnitPositive)?,
        max_gross_exposure_usd: m.dec("max_gross_exposure_usd", usd)?,
        max_order_usd: m.dec("max_order_usd", usd)?,
        max_orders_per_day: m.int("max_orders_per_day", 1, 10_000)?,
        max_daily_loss: m.dec("max_daily_loss", open)?,
        daily_loss_action: m.get("daily_loss_action")?.named(
            &[LimitAction::ExitsOnly, LimitAction::FlattenAndPause],
            LimitAction::as_str,
        )?,
        max_drawdown: m.dec("max_drawdown", open)?,
        drawdown_ladder: m.each("drawdown_ladder", (1, 5), ladder_rung)?,
        hysteresis: m.dec("hysteresis", open)?,
        breach_confirm_s: m.int("breach_confirm_s", 0, 300)?,
        daily_breach_min_s: m.int("daily_breach_min_s", 0, 86_400)?,
        scale_lift_after_s: m.int("scale_lift_after_s", 0, 86_400)?,
        reentry_cooldown_s: m.int("reentry_cooldown_s", 0, 604_800)?,
        scale_action: m.get("scale_action")?.one_of(&[
            ("limit_buys", ScaleAction::LimitBuys),
            ("trim_to_target", ScaleAction::TrimToTarget),
        ])?,
    })
}

fn ladder_rung(node: &Node<'_>) -> Parsed<LadderRung> {
    let m = node.members("at action factor")?;
    Ok(LadderRung {
        at: m.dec("at", DecGrammar::OpenFraction)?,
        action: m.get("action")?.named(
            &[
                LadderAction::ScaleSizes,
                LadderAction::ExitsOnly,
                LadderAction::FlattenAndPause,
            ],
            LadderAction::as_str,
        )?,
        factor: m.dec_or_null("factor", DecGrammar::OpenFraction)?,
    })
}

fn autonomy(node: &Node<'_>) -> Parsed<Autonomy> {
    let m = node.members("rules default admission approval")?;
    let approval = m.get("approval")?;
    let approval = approval.members("timeout_s on_timeout approvers two_approver_above_usd")?;
    Ok(Autonomy {
        rules: m.each("rules", (0, 50), rule)?,
        default: m.get("default")?.one_of(&DECISIONS)?,
        admission: m.get("admission")?.one_of(&DECISIONS)?,
        approval: Approval {
            timeout_s: approval.int("timeout_s", 30, 86_400)?,
            on_timeout: approval
                .get("on_timeout")?
                .one_of(&[("skip", OnTimeout::Skip)])?,
            approvers: approval
                .get("approvers")?
                .items(1, usize::MAX, true)?
                .iter()
                .map(|n| ApproverRef::parse(n.text()?).map_err(|_| n.off_pattern()))
                .collect::<Parsed<_>>()?,
            two_approver_above_usd: approval
                .dec_or_null("two_approver_above_usd", DecGrammar::PositiveDecimal)?,
        },
    })
}

fn rule(node: &Node<'_>) -> Parsed<Rule> {
    let m = node.members("id when then")?;
    let id = m.get("id")?;
    Ok(Rule {
        id: RuleId::parse(id.text()?).map_err(|_| id.off_pattern())?,
        when: condition(&m.get("when")?, 1)?,
        then: m.get("then")?.one_of(&DECISIONS)?,
    })
}

/// `$defs/condition`, a `oneOf` of three combinators and a comparison. Each combinator is the object
/// with that one member, so the member present picks the branch; anything else is a comparison.
fn condition(node: &Node<'_>, depth: u8) -> Parsed<Condition> {
    if depth > PARSE_DEPTH {
        return Err(ParseError::TooDeep { path: node.at() });
    }
    let members = node.value.as_object().ok_or_else(|| node.wrong_type())?;
    let deeper = depth.saturating_add(1);
    let children = |name: &str| -> Parsed<Vec<Condition>> {
        let items = node.members(name)?.get(name)?.items(1, 10, false)?;
        items.iter().map(|child| condition(child, deeper)).collect()
    };
    if members.contains_key("all") {
        return children("all").map(Condition::All);
    }
    if members.contains_key("any") {
        return children("any").map(Condition::Any);
    }
    if members.contains_key("not") {
        let child = node.members("not")?.get("not")?;
        return Ok(Condition::Not(Box::new(condition(&child, deeper)?)));
    }
    let m = node.members("field op value")?;
    let field = m
        .get("field")?
        .named(&CONDITION_FIELDS, ConditionField::as_str)?;
    Ok(Condition::Compare {
        field,
        op: m.get("op")?.named(&OPERATORS, Operator::as_str)?,
        value: condition_value(&m.get("value")?, field)?,
    })
}

/// `anyOf [boolean, string of at most 64, non-empty array of at most 50 such strings]`. A string on a
/// decimal field that is in the `decimal` grammar is a decimal; any other string stays text, which
/// V-023 then judges against the field.
fn condition_value(node: &Node<'_>, field: ConditionField) -> Parsed<ConditionValue> {
    match node.value {
        Value::Bool(flag) => Ok(ConditionValue::Bool(*flag)),
        Value::Str(_) => {
            let text = node.bounded_text(0, 64)?;
            match SchemaDec::parse(&text, DecGrammar::Decimal) {
                Ok(decimal) if field.kind() == FieldKind::Decimal => {
                    Ok(ConditionValue::Decimal(decimal))
                }
                _ => Ok(ConditionValue::Text(text)),
            }
        }
        Value::Array(_) => node
            .items(1, 50, false)?
            .iter()
            .map(|item| item.bounded_text(0, 64))
            .collect::<Parsed<_>>()
            .map(ConditionValue::List),
        _ => Err(node.wrong_type()),
    }
}

fn notifications(node: &Node<'_>) -> Parsed<Notifications> {
    let m = node.members("channels quiet_hours")?;
    let channels = m.get("channels")?.set_of(
        1,
        &[
            ("email", Channel::Email),
            ("phone", Channel::Phone),
            ("slack", Channel::Slack),
            ("sms", Channel::Sms),
            ("telegram", Channel::Telegram),
            ("web_push", Channel::WebPush),
        ],
    )?;
    let quiet_hours = m.get("quiet_hours")?.nullable(|node| {
        let m = node.members("start end timezone")?;
        m.get("timezone")?.one_of(&[("America/New_York", ())])?;
        Ok(QuietHours {
            start: m.get("start")?.hour_minute()?,
            end: m.get("end")?.hour_minute()?,
        })
    })?;
    Ok(Notifications {
        channels,
        quiet_hours,
    })
}
