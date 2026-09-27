//! The strict reader behind [`Mandate::parse`]: [`schemas/mandate.schema.json`](../../../../schemas/mandate.schema.json)
//! as code, one reader per `$def`, each naming the JSON Pointer it failed at (ES-09, ES-22).
//!
//! Every object in the schema is `additionalProperties: false` and requires every property it lists,
//! so one list of names per object is both its allowed and its required members. A pattern is
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

const DECISIONS: [AutonomyDecision; 3] = [
    AutonomyDecision::Auto,
    AutonomyDecision::Ask,
    AutonomyDecision::Deny,
];

const ON_COMPLETE: [OnComplete; 3] = [
    OnComplete::HoldProtected,
    OnComplete::DisarmLadder,
    OnComplete::Release,
];

fn decision_name(decision: AutonomyDecision) -> &'static str {
    match decision {
        AutonomyDecision::Auto => "auto",
        AutonomyDecision::Ask => "ask",
        AutonomyDecision::Deny => "deny",
    }
}

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

    /// An object whose members are all in `names`; which of them are present is checked as each is
    /// read.
    fn members(&self, names: &[&str]) -> Parsed<Members<'a>> {
        let members = self.value.as_object().ok_or_else(|| self.wrong_type())?;
        if let Some(extra) = members.keys().find(|k| !names.contains(&k.as_str())) {
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

    fn flag(&self) -> Parsed<bool> {
        match self.value {
            Value::Bool(flag) => Ok(*flag),
            _ => Err(self.wrong_type()),
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
    fn one_of<T: Copy>(&self, all: &[T], name: fn(T) -> &'static str) -> Parsed<T> {
        self.value
            .as_str()
            .and_then(|text| all.iter().copied().find(|v| name(*v) == text))
            .ok_or_else(|| ParseError::NotInEnum { path: self.at() })
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
        if unique {
            let mut seen: Vec<&Value> = Vec::new();
            for node in &nodes {
                if seen.contains(&node.value) {
                    return Err(ParseError::NotUnique { path: node.at() });
                }
                seen.push(node.value);
            }
        }
        Ok(nodes)
    }

    /// A unique-items array of enum members, at least `min` of them.
    fn set_of<T: Copy + Ord>(
        &self,
        min: usize,
        all: &[T],
        name: fn(T) -> &'static str,
    ) -> Parsed<BTreeSet<T>> {
        self.items(min, usize::MAX, true)?
            .iter()
            .map(|item| item.one_of(all, name))
            .collect()
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
        let text = self.patterned(is_date)?;
        Ok(Date::parse(&text).ok())
    }

    fn hour_minute(&self) -> Parsed<HourMinute> {
        let text = self.patterned(is_hour_minute)?;
        let (hour, minute) = text.split_once(':').ok_or_else(|| self.off_pattern())?;
        Ok(HourMinute {
            hour: hour.parse().map_err(|_| self.off_pattern())?,
            minute: minute.parse().map_err(|_| self.off_pattern())?,
        })
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
fn is_id(text: &str) -> bool {
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
    let doc = root.members(&[
        "mandate_schema_version",
        "name",
        "source_text_ref",
        "environment",
        "connection_id",
        "capital",
        "goal",
        "universe",
        "behavior",
        "protection",
        "risk",
        "autonomy",
        "notifications",
    ])?;
    let schema_version = doc.get("mandate_schema_version")?;
    if schema_version.value.as_int() != Some(u64::from(super::MANDATE_SCHEMA_VERSION)) {
        return Err(ParseError::NotInEnum {
            path: schema_version.at(),
        });
    }
    Ok(Mandate {
        mandate_schema_version: super::MANDATE_SCHEMA_VERSION,
        name: AgentName(doc.get("name")?.patterned(is_name)?),
        source_text_ref: doc.get("source_text_ref")?.nullable(Node::digest)?,
        environment: doc.get("environment")?.one_of(
            &[Environment::Paper, Environment::Live],
            |e| match e {
                Environment::Paper => "paper",
                Environment::Live => "live",
            },
        )?,
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
    let m = node.members(&["allocation_usd", "max_loss_from_allocation"])?;
    Ok(Capital {
        allocation_usd: m
            .get("allocation_usd")?
            .decimal(DecGrammar::PositiveDecimal)?,
        max_loss_from_allocation: m
            .get("max_loss_from_allocation")?
            .decimal(DecGrammar::OpenFraction)?,
    })
}

/// `$defs/goal`, a `oneOf` whose branches differ by `type`, so the parse dispatches on it and names
/// the member the chosen branch lacks rather than reporting the whole goal off its union.
fn goal(node: &Node<'_>) -> Parsed<Goal> {
    let members = node.value.as_object().ok_or_else(|| node.wrong_type())?;
    let kind = Members {
        members,
        path: node.path.clone(),
    }
    .get("type")?
    .one_of(&["continuous", "accumulate", "profit_stop"], |kind| kind)?;
    let end_date = |m: &Members<'_>| m.get("end_date")?.nullable(Node::date).map(Option::flatten);
    let on_complete = |m: &Members<'_>| {
        m.get("on_complete")?
            .one_of(&ON_COMPLETE, OnComplete::as_str)
    };
    match kind {
        "continuous" => {
            let m = node.members(&["type", "end_date", "on_complete"])?;
            Ok(Goal::Continuous {
                end_date: end_date(&m)?,
                on_complete: on_complete(&m)?,
            })
        }
        "accumulate" => {
            let m = node.members(&[
                "type",
                "instrument",
                "target_qty",
                "max_avg_price",
                "max_spend_usd",
                "end_date",
                "on_complete",
            ])?;
            Ok(Goal::Accumulate {
                instrument: asset_id(&m.get("instrument")?)?,
                target_qty: m.get("target_qty")?.decimal(DecGrammar::PositiveDecimal)?,
                max_avg_price: m
                    .get("max_avg_price")?
                    .nullable(|n| n.decimal(DecGrammar::PositiveDecimal))?,
                max_spend_usd: m
                    .get("max_spend_usd")?
                    .decimal(DecGrammar::PositiveDecimal)?,
                end_date: end_date(&m)?,
                on_complete: on_complete(&m)?,
            })
        }
        _ => {
            let m = node.members(&["type", "profit_level", "end_date"])?;
            Ok(Goal::ProfitStop {
                profit_level: m
                    .get("profit_level")?
                    .decimal(DecGrammar::PositiveDecimal)?,
                end_date: end_date(&m)?,
            })
        }
    }
}

/// `$defs/uuid`.
fn asset_id(node: &Node<'_>) -> Parsed<AssetId> {
    AssetId::parse(node.text()?).map_err(|_| node.off_pattern())
}

fn universe(node: &Node<'_>) -> Parsed<Universe> {
    let m = node.members(&[
        "pinned",
        "pinned_instruments",
        "max_instruments",
        "asset_classes",
        "leveraged_etps_enabled",
        "leveraged_etp_disclosure_version",
    ])?;
    Ok(Universe {
        pinned: m.get("pinned")?.flag()?,
        pinned_instruments: m
            .get("pinned_instruments")?
            .items(0, 20, true)?
            .iter()
            .map(instrument_ref)
            .collect::<Parsed<_>>()?,
        max_instruments: m.get("max_instruments")?.integer(1, 20)?,
        asset_classes: m.get("asset_classes")?.set_of(
            1,
            &[AssetClass::Crypto, AssetClass::UsEquity],
            AssetClass::as_str,
        )?,
        leveraged_etps_enabled: m.get("leveraged_etps_enabled")?.flag()?,
        leveraged_etp_disclosure_version: m
            .get("leveraged_etp_disclosure_version")?
            .nullable(Node::digest)?,
    })
}

fn instrument_ref(node: &Node<'_>) -> Parsed<InstrumentRef> {
    let m = node.members(&["asset_id", "symbol", "asset_class"])?;
    Ok(InstrumentRef {
        asset_id: asset_id(&m.get("asset_id")?)?,
        symbol: m.get("symbol")?.bounded_text(1, 32)?,
        asset_class: m.get("asset_class")?.one_of(
            &[AssetClass::Crypto, AssetClass::UsEquity],
            AssetClass::as_str,
        )?,
    })
}

fn behavior(node: &Node<'_>) -> Parsed<Behavior> {
    let m = node.members(&[
        "description",
        "signal_models",
        "research",
        "cadence",
        "sizing",
    ])?;
    Ok(Behavior {
        description: m.get("description")?.bounded_text(0, 4000)?,
        signal_models: m
            .get("signal_models")?
            .items(1, 10, false)?
            .iter()
            .map(signal_model)
            .collect::<Parsed<_>>()?,
        research: m.get("research")?.nullable(research)?,
        cadence: cadence(&m.get("cadence")?)?,
        sizing: sizing(&m.get("sizing")?)?,
    })
}

fn signal_model(node: &Node<'_>) -> Parsed<SignalModel> {
    let m = node.members(&[
        "id",
        "version",
        "content_hash",
        "params",
        "weight",
        "max_output_age_s",
        "admits_instruments",
    ])?;
    let id = m.get("id")?;
    Ok(SignalModel {
        id: ModelId::parse(id.text()?).map_err(|_| id.off_pattern())?,
        version: m.get("version")?.patterned(is_model_version)?,
        content_hash: m.get("content_hash")?.digest()?,
        params: m
            .get("params")?
            .items(0, 32, false)?
            .iter()
            .map(param)
            .collect::<Parsed<_>>()?,
        weight: m.get("weight")?.decimal(DecGrammar::UnitPositive)?,
        max_output_age_s: m.get("max_output_age_s")?.integer(60, 86_400)?,
        admits_instruments: m.get("admits_instruments")?.flag()?,
    })
}

/// A parameter's value is `anyOf [decimal, boolean, string of at most 200]`. A string in the
/// `decimal` grammar is the decimal branch; any other string, `-0` and `0.020` among them, is text,
/// and a JSON number matches no branch.
fn param(node: &Node<'_>) -> Parsed<ModelParam> {
    let m = node.members(&["key", "value"])?;
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
    let m = node.members(&[
        "interval_s",
        "cost_cap_usd_per_day",
        "max_revisions_per_lineage",
    ])?;
    Ok(Research {
        interval_s: m.get("interval_s")?.integer(300, 604_800)?,
        cost_cap_usd_per_day: m
            .get("cost_cap_usd_per_day")?
            .decimal(DecGrammar::PositiveDecimal)?,
        max_revisions_per_lineage: m.get("max_revisions_per_lineage")?.integer(0, 10)?,
    })
}

fn cadence(node: &Node<'_>) -> Parsed<Cadence> {
    let m = node.members(&["interval_s", "event_sources"])?;
    Ok(Cadence {
        interval_s: m.get("interval_s")?.integer(60, 86_400)?,
        event_sources: m.get("event_sources")?.set_of(
            0,
            &[
                EventSource::Fills,
                EventSource::Filings,
                EventSource::News,
                EventSource::Price,
                EventSource::Schedule,
            ],
            |source| match source {
                EventSource::Fills => "fills",
                EventSource::Filings => "filings",
                EventSource::News => "news",
                EventSource::Price => "price",
                EventSource::Schedule => "schedule",
            },
        )?,
    })
}

fn sizing(node: &Node<'_>) -> Parsed<Sizing> {
    let m = node.members(&[
        "method",
        "entry_threshold",
        "exit_threshold",
        "rebalance_band",
    ])?;
    Ok(Sizing {
        method: m
            .get("method")?
            .one_of(&[SizingMethod::ConvictionLinear], |_| "conviction_linear")?,
        entry_threshold: m
            .get("entry_threshold")?
            .decimal(DecGrammar::UnitPositive)?,
        exit_threshold: m.get("exit_threshold")?.decimal(DecGrammar::UnitPositive)?,
        rebalance_band: m.get("rebalance_band")?.decimal(DecGrammar::Fraction)?,
    })
}

/// The `if enabled then stop_distance: open_fraction` clause: with protection on, a null stop matches
/// the property's `oneOf` and fails the `then`, whose `open_fraction` is `type: string`, so it is
/// [`ParseError::WrongType`] as `jsonschema` reports it (MC-S15).
fn protection(node: &Node<'_>) -> Parsed<Protection> {
    let m = node.members(&[
        "enabled",
        "stop_distance",
        "take_profit_distance",
        "crypto_stop_limit_offset",
    ])?;
    let enabled = m.get("enabled")?.flag()?;
    let stop = m.get("stop_distance")?;
    let stop_distance = if enabled {
        Some(stop.decimal(DecGrammar::OpenFraction)?)
    } else {
        stop.nullable(|n| n.decimal(DecGrammar::OpenFraction))?
    };
    Ok(Protection {
        enabled,
        stop_distance,
        take_profit_distance: m
            .get("take_profit_distance")?
            .nullable(|n| n.decimal(DecGrammar::PositiveDecimal))?,
        crypto_stop_limit_offset: m
            .get("crypto_stop_limit_offset")?
            .nullable(|n| n.decimal(DecGrammar::OpenFraction))?,
    })
}

fn risk(node: &Node<'_>) -> Parsed<Risk> {
    let m = node.members(&[
        "max_position_usd",
        "max_position_fraction",
        "max_gross_exposure_usd",
        "max_order_usd",
        "max_orders_per_day",
        "max_daily_loss",
        "daily_loss_action",
        "max_drawdown",
        "drawdown_ladder",
        "hysteresis",
        "breach_confirm_s",
        "daily_breach_min_s",
        "scale_lift_after_s",
        "reentry_cooldown_s",
        "scale_action",
    ])?;
    let usd = |name: &str| m.get(name)?.decimal(DecGrammar::PositiveDecimal);
    let open = |name: &str| m.get(name)?.decimal(DecGrammar::OpenFraction);
    Ok(Risk {
        max_position_usd: usd("max_position_usd")?,
        max_position_fraction: m
            .get("max_position_fraction")?
            .decimal(DecGrammar::UnitPositive)?,
        max_gross_exposure_usd: usd("max_gross_exposure_usd")?,
        max_order_usd: usd("max_order_usd")?,
        max_orders_per_day: m.get("max_orders_per_day")?.integer(1, 10_000)?,
        max_daily_loss: open("max_daily_loss")?,
        daily_loss_action: m.get("daily_loss_action")?.one_of(
            &[LimitAction::ExitsOnly, LimitAction::FlattenAndPause],
            LimitAction::as_str,
        )?,
        max_drawdown: open("max_drawdown")?,
        drawdown_ladder: m
            .get("drawdown_ladder")?
            .items(1, 5, false)?
            .iter()
            .map(ladder_rung)
            .collect::<Parsed<_>>()?,
        hysteresis: open("hysteresis")?,
        breach_confirm_s: m.get("breach_confirm_s")?.integer(0, 300)?,
        daily_breach_min_s: m.get("daily_breach_min_s")?.integer(0, 86_400)?,
        scale_lift_after_s: m.get("scale_lift_after_s")?.integer(0, 86_400)?,
        reentry_cooldown_s: m.get("reentry_cooldown_s")?.integer(0, 604_800)?,
        scale_action: m.get("scale_action")?.one_of(
            &[ScaleAction::LimitBuys, ScaleAction::TrimToTarget],
            |action| match action {
                ScaleAction::LimitBuys => "limit_buys",
                ScaleAction::TrimToTarget => "trim_to_target",
            },
        )?,
    })
}

fn ladder_rung(node: &Node<'_>) -> Parsed<LadderRung> {
    let m = node.members(&["at", "action", "factor"])?;
    Ok(LadderRung {
        at: m.get("at")?.decimal(DecGrammar::OpenFraction)?,
        action: m.get("action")?.one_of(
            &[
                LadderAction::ScaleSizes,
                LadderAction::ExitsOnly,
                LadderAction::FlattenAndPause,
            ],
            LadderAction::as_str,
        )?,
        factor: m
            .get("factor")?
            .nullable(|n| n.decimal(DecGrammar::OpenFraction))?,
    })
}

fn autonomy(node: &Node<'_>) -> Parsed<Autonomy> {
    let m = node.members(&["rules", "default", "admission", "approval"])?;
    Ok(Autonomy {
        rules: m
            .get("rules")?
            .items(0, 50, false)?
            .iter()
            .map(rule)
            .collect::<Parsed<_>>()?,
        default: m.get("default")?.one_of(&DECISIONS, decision_name)?,
        admission: m.get("admission")?.one_of(&DECISIONS, decision_name)?,
        approval: approval(&m.get("approval")?)?,
    })
}

fn rule(node: &Node<'_>) -> Parsed<Rule> {
    let m = node.members(&["id", "when", "then"])?;
    let id = m.get("id")?;
    Ok(Rule {
        id: RuleId::parse(id.text()?).map_err(|_| id.off_pattern())?,
        when: condition(&m.get("when")?, 1)?,
        then: m.get("then")?.one_of(&DECISIONS, decision_name)?,
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
        node.members(&[name])?
            .get(name)?
            .items(1, 10, false)?
            .iter()
            .map(|child| condition(child, deeper))
            .collect()
    };
    if members.contains_key("all") {
        return children("all").map(Condition::All);
    }
    if members.contains_key("any") {
        return children("any").map(Condition::Any);
    }
    if members.contains_key("not") {
        let child = node.members(&["not"])?.get("not")?;
        return Ok(Condition::Not(Box::new(condition(&child, deeper)?)));
    }
    let m = node.members(&["field", "op", "value"])?;
    let field = m
        .get("field")?
        .one_of(&CONDITION_FIELDS, ConditionField::as_str)?;
    Ok(Condition::Compare {
        field,
        op: m.get("op")?.one_of(&OPERATORS, Operator::as_str)?,
        value: condition_value(&m.get("value")?, field)?,
    })
}

/// `anyOf [boolean, string of at most 64, non-empty array of at most 50 such strings]`. A string on a
/// decimal field that is in the `decimal` grammar is a decimal; any other string stays text, which
/// V-023 then judges against the field.
fn condition_value(node: &Node<'_>, field: ConditionField) -> Parsed<ConditionValue> {
    match node.value {
        Value::Bool(flag) => Ok(ConditionValue::Bool(*flag)),
        Value::Str(text) => {
            let text = node.bounded_text(0, 64).map(|_| text)?;
            match SchemaDec::parse(text, DecGrammar::Decimal) {
                Ok(decimal) if field.kind() == FieldKind::Decimal => {
                    Ok(ConditionValue::Decimal(decimal))
                }
                _ => Ok(ConditionValue::Text(text.clone())),
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

fn approval(node: &Node<'_>) -> Parsed<Approval> {
    let m = node.members(&[
        "timeout_s",
        "on_timeout",
        "approvers",
        "two_approver_above_usd",
    ])?;
    Ok(Approval {
        timeout_s: m.get("timeout_s")?.integer(30, 86_400)?,
        on_timeout: m
            .get("on_timeout")?
            .one_of(&[OnTimeout::Skip], |_| "skip")?,
        approvers: m
            .get("approvers")?
            .items(1, usize::MAX, true)?
            .iter()
            .map(|n| ApproverRef::parse(n.text()?).map_err(|_| n.off_pattern()))
            .collect::<Parsed<_>>()?,
        two_approver_above_usd: m
            .get("two_approver_above_usd")?
            .nullable(|n| n.decimal(DecGrammar::PositiveDecimal))?,
    })
}

fn notifications(node: &Node<'_>) -> Parsed<Notifications> {
    let m = node.members(&["channels", "quiet_hours"])?;
    Ok(Notifications {
        channels: m.get("channels")?.set_of(
            1,
            &[
                Channel::Email,
                Channel::Phone,
                Channel::Slack,
                Channel::Sms,
                Channel::Telegram,
                Channel::WebPush,
            ],
            |channel| match channel {
                Channel::Email => "email",
                Channel::Phone => "phone",
                Channel::Slack => "slack",
                Channel::Sms => "sms",
                Channel::Telegram => "telegram",
                Channel::WebPush => "web_push",
            },
        )?,
        quiet_hours: m.get("quiet_hours")?.nullable(quiet_hours)?,
    })
}

fn quiet_hours(node: &Node<'_>) -> Parsed<QuietHours> {
    let m = node.members(&["start", "end", "timezone"])?;
    m.get("timezone")?
        .one_of(&["America/New_York"], |zone| zone)?;
    Ok(QuietHours {
        start: m.get("start")?.hour_minute()?,
        end: m.get("end")?.hour_minute()?,
    })
}
