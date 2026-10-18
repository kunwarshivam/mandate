//! The evaluation report at `report_version` 1 ([DEC-1002](../../../docs/project/decisions/DEC-1002.md)
//! item 7): canonical JSON that `schemas/workspace-api/read-models/evaluation.schema.json`'s
//! `Report` validates, stored under SHA-256 of its bytes. Its one disclosure is DEC-1003's
//! placeholder name `NOTE-FORWARD-PAPER-INTERNAL`, never text.

use core::fmt;

use mandate_canon::{Digest, Int, Key, Object, Value};
use mandate_num::NumError;
use mandate_research::AssetId;
use mandate_research::score::{
    EvaluationDecision, Scorecard, ScoredThesis, Unscoreable, UnscoreableReason,
};
use mandate_time::UtcNanos;

use crate::{CostModel, EvaluationError, EvaluationRequest};

/// The version this crate writes.
pub const EVALUATION_REPORT_VERSION: u32 = 1;

/// A signal model as its theses name it: a new version or content starts a new scorecard.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SignalModelKey {
    pub model_id: String,
    pub model_version: String,
    pub content_hash: Digest,
}

/// One signal model's result: how its theses resolved, and its scorecard or the scorer's refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub key: SignalModelKey,
    pub closed: u32,
    pub open: u32,
    pub outside_window: u32,
    pub outcome: Result<Scorecard, &'static str>,
}

/// The digests that reproduce a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputDigests {
    pub theses: Digest,
    pub closes: Digest,
    pub cost_model: Digest,
}

/// The canonical objects a report's `inputs` digest (DEC-2244 item 6): the thesis records read,
/// the close series, and the cost model, so a caller stores exactly what the record's references
/// name (journal spec §6.3, §11 check 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputObjects {
    pub theses: Value,
    pub closes: Value,
    pub cost_model: Value,
}

/// The input objects of `request`, each the object whose digest [`crate::evaluate`]'s report
/// holds in its `inputs`.
///
/// # Errors
/// `Num` when a record's `seq` exceeds the canonical integer range, or a member name is not a
/// canonical key.
pub fn input_objects(request: &EvaluationRequest<'_>) -> Result<InputObjects, EvaluationError> {
    let records = request
        .records
        .iter()
        .map(|record| {
            object(vec![
                ("stream_id", text(&record.stream_id)),
                ("seq", int(record.seq)?),
                ("hash", reference(&record.hash)),
                ("mandate_version", reference(&record.mandate_version)),
                ("payload", record.payload.clone()),
            ])
        })
        .collect::<Result<Vec<_>, _>>()?;
    let closes = request
        .closes
        .iter()
        .map(|(id, closes)| {
            let rows = closes
                .iter()
                .map(|close| {
                    object(vec![
                        ("at", decimal(close.at)),
                        ("price", decimal(close.price)),
                    ])
                })
                .collect::<Result<Vec<_>, _>>()?;
            object(vec![
                ("instrument", text(id.as_str())),
                ("closes", Value::Array(rows)),
            ])
        })
        .collect::<Result<Vec<_>, EvaluationError>>()?;
    Ok(InputObjects {
        theses: Value::Array(records),
        closes: Value::Array(closes),
        cost_model: cost_model(request.cost)?,
    })
}

impl InputDigests {
    pub(crate) fn of(request: &EvaluationRequest<'_>) -> Result<Self, EvaluationError> {
        let objects = input_objects(request)?;
        Ok(Self {
            theses: digest(&objects.theses),
            closes: digest(&objects.closes),
            cost_model: digest(&objects.cost_model),
        })
    }
}

fn cost_model(cost: &CostModel) -> Result<Value, EvaluationError> {
    let equities = &cost.fees.equities;
    object(vec![
        ("slippage_per_leg_bps", decimal(cost.slippage_per_leg)),
        ("reference_notional", decimal(cost.reference_notional)),
        (
            "increment",
            text(match cost.increment {
                mandate_num::ShareIncrement::Fractional => "fractional",
                mandate_num::ShareIncrement::Whole => "whole",
            }),
        ),
        ("sec_rate", decimal(equities.sec_rate)),
        ("taf_per_share", decimal(equities.taf_per_share)),
        ("taf_cap", decimal(equities.taf_cap.to_usd())),
        (
            "taf_cap_basis",
            text(match equities.taf_cap_basis {
                mandate_accounting::TafCapBasis::PerExecution => "per_execution",
                mandate_accounting::TafCapBasis::PerOrder => "per_order",
            }),
        ),
        ("cat_per_share", decimal(equities.cat_per_share)),
        (
            "fee_rounding",
            text(match equities.fee_rounding {
                mandate_accounting::FeeRounding::DailyTotal => "daily_total",
                mandate_accounting::FeeRounding::PerFeePerFill => "per_fee_per_fill",
            }),
        ),
    ])
}

/// One evaluation's report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationReport {
    pub decision_id: String,
    pub decision: EvaluationDecision,
    pub cut_off: UtcNanos,
    pub inputs: InputDigests,
    pub basket: Vec<AssetId>,
    pub index: AssetId,
    pub groups: Vec<Group>,
}

impl EvaluationReport {
    /// True only when at least one group was scored and every scored group passed (DEC-1002 item
    /// 7): a refusal is no pass, and an evaluation of nothing passes nothing.
    pub fn passed(&self) -> bool {
        let mut scored = self
            .groups
            .iter()
            .filter_map(|group| group.outcome.as_ref().ok())
            .peekable();
        scored.peek().is_some() && scored.all(|scorecard| scorecard.passed)
    }

    /// The report as DEC-1002 item 7's canonical object.
    pub fn canonical(&self) -> Result<Value, EvaluationError> {
        let window = self.decision.window;
        let groups = self
            .groups
            .iter()
            .map(group_value)
            .collect::<Result<Vec<_>, _>>()?;
        object(vec![
            ("kind", text("forward_paper_evaluation")),
            ("report_version", int(u64::from(EVALUATION_REPORT_VERSION))?),
            (
                "decision",
                object(vec![
                    ("decision_id", text(&self.decision_id)),
                    ("window_from", decimal(window.from)),
                    ("window_to", decimal(window.to)),
                    (
                        "minimum_scoreable",
                        int(u64::from(self.decision.minimum_scoreable))?,
                    ),
                    ("z", decimal(self.decision.z)),
                ])?,
            ),
            ("cut_off", decimal(self.cut_off)),
            (
                "inputs",
                object(vec![
                    ("theses", reference(&self.inputs.theses)),
                    ("closes", reference(&self.inputs.closes)),
                    ("cost_model", reference(&self.inputs.cost_model)),
                ])?,
            ),
            (
                "baselines",
                object(vec![
                    (
                        "basket",
                        Value::Array(self.basket.iter().map(|id| text(id.as_str())).collect()),
                    ),
                    ("index", text(self.index.as_str())),
                ])?,
            ),
            ("groups", Value::Array(groups)),
            ("passed", Value::Bool(self.passed())),
            (
                "disclosures",
                Value::Array(vec![text("NOTE-FORWARD-PAPER-INTERNAL")]),
            ),
        ])
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, EvaluationError> {
        Ok(mandate_canon::to_canonical(&self.canonical()?))
    }

    /// SHA-256 of the artifact's bytes: the report's `ref`.
    pub fn digest(&self) -> Result<Digest, EvaluationError> {
        Ok(Digest::of(&self.canonical_bytes()?))
    }
}

fn group_value(group: &Group) -> Result<Value, EvaluationError> {
    let (refusal, scorecard) = match &group.outcome {
        Ok(scorecard) => (Value::Null, scorecard_value(scorecard)?),
        Err(code) => (text(code), Value::Null),
    };
    object(vec![
        (
            "signal_model",
            object(vec![
                ("model_id", text(&group.key.model_id)),
                ("model_version", text(&group.key.model_version)),
                ("content_hash", reference(&group.key.content_hash)),
            ])?,
        ),
        ("closed", int(u64::from(group.closed))?),
        ("open", int(u64::from(group.open))?),
        ("outside_window", int(u64::from(group.outside_window))?),
        ("refusal", refusal),
        ("scorecard", scorecard),
    ])
}

/// `mandate_research::score::Scorecard`, member for member, as the read model's `Scorecard`.
fn scorecard_value(scorecard: &Scorecard) -> Result<Value, EvaluationError> {
    let theses = scorecard
        .theses
        .iter()
        .map(scored_value)
        .collect::<Result<Vec<_>, _>>()?;
    let unscoreable = scorecard
        .unscoreable
        .iter()
        .map(unscoreable_value)
        .collect::<Result<Vec<_>, _>>()?;
    object(vec![
        ("theses", Value::Array(theses)),
        ("unscoreable", Value::Array(unscoreable)),
        (
            "scoreable_count",
            int(u64::from(scorecard.scoreable_count))?,
        ),
        ("z", decimal(scorecard.z)),
        ("excess_sum_basket", decimal(scorecard.excess_sum_basket)),
        ("excess_sum_index", decimal(scorecard.excess_sum_index)),
        ("mean_excess_basket", decimal(scorecard.mean_excess_basket)),
        ("mean_excess_index", decimal(scorecard.mean_excess_index)),
        (
            "sample_variance_basket",
            optional(scorecard.sample_variance_basket),
        ),
        (
            "sample_variance_index",
            optional(scorecard.sample_variance_index),
        ),
        ("margin_basket", decimal(scorecard.margin_basket)),
        ("margin_index", decimal(scorecard.margin_index)),
        ("lower_bound_basket", decimal(scorecard.lower_bound_basket)),
        ("lower_bound_index", decimal(scorecard.lower_bound_index)),
        ("passed", Value::Bool(scorecard.passed)),
    ])
}

fn scored_value(row: &ScoredThesis) -> Result<Value, EvaluationError> {
    object(vec![
        ("thesis_id", text(row.thesis.as_str())),
        ("lineage_id", text(row.lineage.as_str())),
        ("revision", int(u64::from(row.revision))?),
        ("instrument", text(row.instrument.as_str())),
        ("entry", decimal(row.entry)),
        ("exit", decimal(row.exit)),
        ("round_trip_cost", decimal(row.round_trip_cost)),
        ("net_return", decimal(row.net_return)),
        ("excess_over_basket", decimal(row.excess_over_basket)),
        ("excess_over_index", decimal(row.excess_over_index)),
    ])
}

fn unscoreable_value(row: &Unscoreable) -> Result<Value, EvaluationError> {
    object(vec![
        ("thesis_id", text(row.thesis.as_str())),
        (
            "reason",
            text(match row.reason {
                UnscoreableReason::NoEntryClose => "no_entry_close",
                UnscoreableReason::NoExitClose => "no_exit_close",
                UnscoreableReason::NoSeries => "no_series",
                UnscoreableReason::NotScoreableDirection => "not_scoreable_direction",
            }),
        ),
    ])
}

pub(crate) fn object(members: Vec<(&str, Value)>) -> Result<Value, EvaluationError> {
    members
        .into_iter()
        .map(|(name, value)| {
            Key::new(name)
                .map(|key| (key, value))
                .map_err(|_| EvaluationError::Num(NumError::NotCanonical))
        })
        .collect::<Result<Object, EvaluationError>>()
        .map(Value::Object)
}

pub(crate) fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

pub(crate) fn decimal<T: fmt::Display>(value: T) -> Value {
    Value::Str(value.to_string())
}

fn optional<T: fmt::Display>(value: Option<T>) -> Value {
    value.map_or(Value::Null, decimal)
}

pub(crate) fn int(value: u64) -> Result<Value, EvaluationError> {
    Int::new(value)
        .map(Value::Int)
        .ok_or(EvaluationError::Num(NumError::Overflow))
}

/// A content hash in the journal's `ref` form.
pub(crate) fn reference(digest: &Digest) -> Value {
    text(&format!("sha256:{}", digest.to_hex()))
}

fn digest(value: &Value) -> Digest {
    Digest::of(&mandate_canon::to_canonical(value))
}
