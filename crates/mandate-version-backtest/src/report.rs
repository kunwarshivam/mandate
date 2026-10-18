//! The version backtest's report at `report_version` 2 ([DEC-1001](../../../../docs/project/decisions/DEC-1001.md)
//! item 2): canonical JSON that `schemas/workspace-api/read-models/backtest.schema.json`'s `Report`
//! validates, stored under SHA-256 of its bytes. It holds no thesis-quality figure for any version
//! (DEC-99) and carries its disclosures as DEC-1003's placeholder names, never text.

use mandate_canon::{Digest, Value};
use mandate_domain::{AgentMode, AssetId};
use mandate_num::{NumError, Price, Qty};
use mandate_risk::Purpose;
use mandate_time::{Date, UtcNanos};

use super::{RecordedOutput, RecordedThesis, VersionInput};
use mandate_backtest::encode::{decimal, metrics, optional, text};
use mandate_backtest::{Metrics, Observation};

use crate::VersionError;

/// The version this module writes. `report_version` 1 is E4-2's baseline report.
pub const VERSION_REPORT_VERSION: u32 = 2;

/// How one decision instant ended (DEC-1001 item 2's nine outcome codes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DecisionOutcome {
    Submitted,
    BuilderHold,
    BuilderRefused,
    GateDeny,
    GateDefer,
    GateHold,
    AutonomyDeny,
    AskUnanswered,
    OrderWorking,
}

impl DecisionOutcome {
    /// Every outcome, in the order the report counts them.
    pub const ALL: [Self; 9] = [
        Self::Submitted,
        Self::BuilderHold,
        Self::BuilderRefused,
        Self::GateDeny,
        Self::GateDefer,
        Self::GateHold,
        Self::AutonomyDeny,
        Self::AskUnanswered,
        Self::OrderWorking,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Submitted => "submitted",
            Self::BuilderHold => "builder_hold",
            Self::BuilderRefused => "builder_refused",
            Self::GateDeny => "gate_deny",
            Self::GateDefer => "gate_defer",
            Self::GateHold => "gate_hold",
            Self::AutonomyDeny => "autonomy_deny",
            Self::AskUnanswered => "ask_unanswered",
            Self::OrderWorking => "order_working",
        }
    }
}

/// One decision instant: what the version's path decided, why, and at what size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionRow {
    pub at: UtcNanos,
    pub date: Date,
    pub purpose: Option<Purpose>,
    pub outcome: DecisionOutcome,
    pub reason: Option<String>,
    pub decided_by: Option<String>,
    pub qty: Option<Qty>,
    pub limit: Option<Price>,
    pub outputs_used: u32,
}

/// What a run cannot know or does not model (DEC-1001 item 2): fixed codes, never prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Limitation {
    NoLookaheadByReceiveTime,
    RecordedModelOutputsOnly,
    AdmissionsReplayedNotRejudged,
    FillsWithinBarRange,
    SpreadFromFillModel,
    SingleInstrument,
    DailyDecisions,
    AsksUnansweredSkip,
    ParticipationFromBarVolume,
    MarksAtBarClose,
    TradingStatusAssumed,
    MechanicsNotThesisQuality,
}

impl Limitation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoLookaheadByReceiveTime => "no_lookahead_by_receive_time",
            Self::RecordedModelOutputsOnly => "recorded_model_outputs_only",
            Self::AdmissionsReplayedNotRejudged => "admissions_replayed_not_rejudged",
            Self::FillsWithinBarRange => "fills_within_bar_range",
            Self::SpreadFromFillModel => "spread_from_fill_model",
            Self::SingleInstrument => "single_instrument",
            Self::DailyDecisions => "daily_decisions",
            Self::AsksUnansweredSkip => "asks_unanswered_skip",
            Self::ParticipationFromBarVolume => "participation_from_bar_volume",
            Self::MarksAtBarClose => "marks_at_bar_close",
            Self::TradingStatusAssumed => "trading_status_assumed",
            Self::MechanicsNotThesisQuality => "mechanics_not_thesis_quality",
        }
    }

    /// The codes that apply to a run: every one, except that a pinned universe has no admission
    /// to replay and no thesis whose quality a reader could mistake the run for.
    pub fn applying(research_agent: bool) -> Vec<Self> {
        let mut all = vec![
            Self::NoLookaheadByReceiveTime,
            Self::RecordedModelOutputsOnly,
            Self::FillsWithinBarRange,
            Self::SpreadFromFillModel,
            Self::SingleInstrument,
            Self::DailyDecisions,
            Self::AsksUnansweredSkip,
            Self::ParticipationFromBarVolume,
            Self::MarksAtBarClose,
            Self::TradingStatusAssumed,
        ];
        if research_agent {
            all.push(Self::AdmissionsReplayedNotRejudged);
            all.push(Self::MechanicsNotThesisQuality);
        }
        all.sort();
        all
    }
}

/// DEC-1003's placeholder names for a backtest report, in a fixed order (Proposed: the wording is
/// counsel's, so only names are carried).
fn disclosures(research_agent: bool) -> Vec<&'static str> {
    let mut names = vec!["LEGEND-HYPOTHETICAL", "NOTE-BACKTEST-LIMITS"];
    if research_agent {
        names.push("NOTE-BACKTEST-MECHANICS");
    }
    names
}

/// The digests that reproduce a report (DEC-1001 item 2, journal spec §9.18).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionDigests {
    pub bars: Digest,
    pub model_outputs: Digest,
    pub theses: Digest,
    pub config: Digest,
}

impl VersionDigests {
    /// Each digest is SHA-256 of the canonical bytes of the input it names: the bar input as E4-2
    /// digests it, the recorded outputs and theses in the order given, and every setting the run
    /// reads besides the mandate, which the report names by its own version hash.
    pub fn of(input: &VersionInput<'_>) -> Result<Self, VersionError> {
        Ok(VersionObjects::of(input)?.digests())
    }
}

/// The canonical objects [`VersionDigests`] digests, so a runner can store each one under its
/// digest and every reference a `BacktestRunRecorded` names resolves (journal spec §6.3, §11 check
/// 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionObjects {
    pub bars: Value,
    pub model_outputs: Value,
    pub theses: Value,
    pub config: Value,
}

impl VersionObjects {
    pub fn of(input: &VersionInput<'_>) -> Result<Self, VersionError> {
        let bars = mandate_backtest::encode::bar_input(
            input.bars,
            input.coverage_start,
            input.first_bar_volumes,
        )?;
        let outputs = input
            .outputs
            .iter()
            .map(output_value)
            .collect::<Result<Vec<_>, _>>()?;
        let theses = input
            .theses
            .iter()
            .map(thesis_value)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            bars,
            model_outputs: Value::Array(outputs),
            theses: Value::Array(theses),
            config: super::record::configuration(input)?,
        })
    }

    /// SHA-256 of each object's canonical bytes.
    pub fn digests(&self) -> VersionDigests {
        VersionDigests {
            bars: digest(&self.bars),
            model_outputs: digest(&self.model_outputs),
            theses: digest(&self.theses),
            config: digest(&self.config),
        }
    }
}

fn digest(value: &Value) -> Digest {
    Digest::of(&mandate_canon::to_canonical(value))
}

fn output_value(recorded: &RecordedOutput) -> Result<Value, VersionError> {
    let output = &recorded.output;
    object(vec![
        ("received_at", decimal(recorded.received_at)),
        ("model_id", text(output.model_id.as_str())),
        ("model_version", text(output.model_version.as_str())),
        ("content_hash", reference(&output.content_hash)),
        ("instrument", text(output.instrument.as_str())),
        ("as_of", decimal(output.as_of)),
        ("expires_at", decimal(output.expires_at)),
        ("direction", text("long")),
        ("conviction", decimal(output.conviction)),
        ("confidence", decimal(output.confidence)),
    ])
}

fn thesis_value(thesis: &RecordedThesis) -> Result<Value, VersionError> {
    object(vec![
        ("thesis_id", text(&thesis.thesis_id)),
        ("received_at", decimal(thesis.received_at)),
        ("instrument", text(thesis.instrument.as_str())),
        ("as_of", decimal(thesis.as_of)),
        ("expires_at", decimal(thesis.expires_at)),
        ("admitted", Value::Bool(thesis.admitted)),
        ("confidence", decimal(thesis.confidence)),
        ("invalidated_at", optional(thesis.invalidated_at)),
    ])
}

/// A content hash in the journal's `ref` form, `sha256:` and 64 lowercase hex characters.
pub(super) fn reference(digest: &Digest) -> Value {
    text(&format!("sha256:{}", digest.to_hex()))
}

/// One version backtest's report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionReport {
    pub mandate_version: Digest,
    pub instrument: AssetId,
    pub research_agent: bool,
    pub inputs: VersionDigests,
    pub strategy: Metrics,
    pub benchmark: Metrics,
    pub excess_total_return: mandate_num::Ratio,
    pub decisions: Vec<DecisionRow>,
    pub limits_latched: Vec<String>,
    pub risk_exits: u32,
    pub most_restrictive_mode: AgentMode,
    pub strategy_equity: Vec<Observation>,
    pub benchmark_equity: Vec<Observation>,
}

impl VersionReport {
    /// The report as DEC-1001 item 2's canonical object.
    pub fn canonical(&self) -> Result<Value, VersionError> {
        let mut outcome_counts = Vec::new();
        for outcome in DecisionOutcome::ALL {
            let count = self
                .decisions
                .iter()
                .filter(|row| row.outcome == outcome)
                .count();
            outcome_counts.push((outcome.as_str(), count_value(count)?));
        }
        let rows = self
            .decisions
            .iter()
            .map(row_value)
            .collect::<Result<Vec<_>, _>>()?;
        object(vec![
            ("kind", text("backtest_report")),
            ("report_version", int(VERSION_REPORT_VERSION)?),
            ("subject", text("mandate_version")),
            ("mandate_version", reference(&self.mandate_version)),
            ("instrument", text(self.instrument.as_str())),
            ("research_agent", Value::Bool(self.research_agent)),
            (
                "inputs",
                object(vec![
                    ("bars", reference(&self.inputs.bars)),
                    ("model_outputs", reference(&self.inputs.model_outputs)),
                    ("theses", reference(&self.inputs.theses)),
                    ("config", reference(&self.inputs.config)),
                ])?,
            ),
            ("strategy", metrics(&self.strategy)?),
            ("benchmark", metrics(&self.benchmark)?),
            ("excess_total_return", decimal(self.excess_total_return)),
            (
                "decisions",
                object(vec![
                    ("count", count_value(self.decisions.len())?),
                    ("outcomes", object(outcome_counts)?),
                    ("gate_reasons", self.reason_counts(true)?),
                    ("hold_reasons", self.reason_counts(false)?),
                    ("rows", Value::Array(rows)),
                ])?,
            ),
            (
                "risk",
                object(vec![
                    (
                        "limits_latched",
                        Value::Array(self.limits_latched.iter().map(|name| text(name)).collect()),
                    ),
                    ("risk_exits", int(self.risk_exits)?),
                    (
                        "most_restrictive_mode",
                        text(mode_name(self.most_restrictive_mode)),
                    ),
                ])?,
            ),
            (
                "equity",
                object(vec![
                    ("strategy", equity_points(&self.strategy_equity)?),
                    ("benchmark", equity_points(&self.benchmark_equity)?),
                ])?,
            ),
            (
                "limitations",
                Value::Array(
                    Limitation::applying(self.research_agent)
                        .into_iter()
                        .map(|code| text(code.as_str()))
                        .collect(),
                ),
            ),
            (
                "disclosures",
                Value::Array(
                    disclosures(self.research_agent)
                        .into_iter()
                        .map(text)
                        .collect(),
                ),
            ),
        ])
    }

    /// `to_canonical` of [`VersionReport::canonical`]: the artifact's bytes.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, VersionError> {
        Ok(mandate_canon::to_canonical(&self.canonical()?))
    }

    /// SHA-256 of the artifact's bytes: the report's `ref` (DEC-1001 item 1).
    pub fn digest(&self) -> Result<Digest, VersionError> {
        Ok(Digest::of(&self.canonical_bytes()?))
    }

    /// Each reason that occurred with its count: the gate's for rows the gate judged, the
    /// builder's for its holds.
    fn reason_counts(&self, gate: bool) -> Result<Value, VersionError> {
        let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for row in &self.decisions {
            let judged_by_gate = matches!(
                row.outcome,
                DecisionOutcome::GateDeny | DecisionOutcome::GateDefer | DecisionOutcome::GateHold
            );
            let held = row.outcome == DecisionOutcome::BuilderHold;
            if let Some(reason) = row.reason.as_deref()
                && ((gate && judged_by_gate) || (!gate && held))
            {
                let entry = counts.entry(reason).or_insert(0);
                *entry = entry.checked_add(1).ok_or(NumError::Overflow)?;
            }
        }
        let members = counts
            .into_iter()
            .map(|(reason, count)| Ok((reason, count_value(count)?)))
            .collect::<Result<Vec<_>, VersionError>>()?;
        object(members)
    }
}

fn count_value(count: usize) -> Result<Value, VersionError> {
    int(u32::try_from(count).map_err(|_| NumError::Overflow)?)
}

fn row_value(row: &DecisionRow) -> Result<Value, VersionError> {
    object(vec![
        ("at", decimal(row.at)),
        ("date", decimal(row.date)),
        (
            "purpose",
            row.purpose
                .map_or(Ok(Value::Null), |purpose| purpose_name(purpose).map(text))?,
        ),
        ("outcome", text(row.outcome.as_str())),
        ("reason", row.reason.as_deref().map_or(Value::Null, text)),
        (
            "decided_by",
            row.decided_by.as_deref().map_or(Value::Null, text),
        ),
        ("qty", optional(row.qty)),
        ("limit", optional(row.limit)),
        ("outputs_used", int(row.outputs_used)?),
    ])
}

/// The four purposes a run can decide. An owner exit and a protective order are never a decision
/// of the run, so either here is refused rather than spelled.
fn purpose_name(purpose: Purpose) -> Result<&'static str, VersionError> {
    match purpose {
        Purpose::Open => Ok("open"),
        Purpose::Increase => Ok("increase"),
        Purpose::DiscretionaryExit => Ok("discretionary_exit"),
        Purpose::RiskExit => Ok("risk_exit"),
        Purpose::OwnerExit | Purpose::Protective => Err(VersionError::Num(NumError::NotCanonical)),
    }
}

fn mode_name(mode: AgentMode) -> &'static str {
    match mode {
        AgentMode::Normal => "normal",
        AgentMode::ExitsOnly => "exits_only",
        AgentMode::Paused => "paused",
        AgentMode::Stopped => "stopped",
    }
}

fn equity_points(observations: &[Observation]) -> Result<Value, VersionError> {
    observations
        .iter()
        .map(|observation| {
            object(vec![
                ("date", decimal(observation.date)),
                ("equity", decimal(observation.equity)),
            ])
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}

/// `mandate_backtest::encode::object` with this crate's error.
pub(crate) fn object(members: Vec<(&str, Value)>) -> Result<Value, VersionError> {
    Ok(mandate_backtest::encode::object(members)?)
}

/// `mandate_backtest::encode::int` with this crate's error.
pub(crate) fn int(count: u32) -> Result<Value, VersionError> {
    Ok(mandate_backtest::encode::int(count)?)
}
