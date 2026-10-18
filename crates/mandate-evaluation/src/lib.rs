#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The forward-paper evaluation harness (backlog E17-8 and E15-3,
//! [DEC-1002](../../../docs/project/decisions/DEC-1002.md); DEC-99, DEC-103, DEC-111, DEC-122):
//! the internal paper workspaces' journaled thesis records in, one scorecard per signal model
//! beside the pre-registered baselines out, as a canonical report artifact.
//!
//! `mandate_research::score::evaluate` is the scorer and stays the only place a thesis is scored
//! (DEC-281). What this crate adds is what a caller would otherwise do by hand, each step a place
//! an evaluation could flatter itself:
//!
//! - **Every record is checked** with `mandate_spec::context::check_thesis_record` before it is
//!   read (DEC-414 item 3); one that fails refuses the whole evaluation, naming it.
//! - **Every thesis is scored as an idea**, admitted or refused, and a revision as its own thesis
//!   with no predecessor's score (DEC-99, DEC-111 item 2).
//! - **Only resolved theses are scored**: a horizon ending after the cut-off is `open`, counted and
//!   never scored, and one closing outside the registered window is counted `outside_window`.
//! - **One scorecard per signal model**, keyed by the research agent's id, version and content
//!   hash, so a changed model never inherits a record (DEC-1002 item 4).
//! - **The cost is charged against the thesis**: the configured slippage on both legs plus the fees
//!   `mandate-accounting`'s fold accrues for the buy and the sell, each rounded up (item 5).
//!
//! It never reads a backtest: `xtask/layers.toml` forbids `mandate-backtest` here, so a scorecard
//! cannot be computed from one (DEC-99). Like the scorer it is pure: no clock, no I/O, ordered
//! collections only, and the same records give the same report bytes.

mod record;
mod report;

pub use record::{EvaluationRecord, evaluation_run_recorded};
pub use report::{
    EVALUATION_REPORT_VERSION, EvaluationReport, Group, InputObjects, SignalModelKey, input_objects,
};

use std::collections::BTreeMap;

use mandate_accounting::{Account, AccountType, AssetClass, Execution, Input, InstrumentId, Side};
use mandate_canon::{Digest, Value};
use mandate_num::{Bps, NumError, Ratio, Rounding, ShareIncrement, Usd};
use mandate_research::score::{
    CloseSeries, ClosedThesis, EvaluationDecision, EvaluationInput, ObservedClose, entry_close,
    exit_close,
};
use mandate_research::{AssetId, Direction, LineageId, ResearchError, ThesisId};
use mandate_time::UtcNanos;

/// Why an evaluation produced no report. One cause each, with a stable `code()` (ES-09).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EvaluationError {
    /// A record's verdict does not re-derive from the mandate it was judged under, or its payload
    /// is not journal spec §9.4's shape (DEC-414 item 3). The index is the record's in the input.
    #[error("thesis record {index} does not pass the record check: {code}")]
    RecordRejected { index: usize, code: &'static str },
    /// Two records at one stream position: the same journal record read twice.
    #[error("thesis record {index} repeats an earlier record's stream position")]
    DuplicateRecord { index: usize },
    /// A payload member the harness reads is missing or not of its §9.4 type.
    #[error("thesis record {index} has no well-typed `{member}`")]
    RecordMalformed { index: usize, member: &'static str },
    /// The accounting fold could not price the round trip's fees.
    #[error("the round trip's fees could not be priced: {0}")]
    Accounting(&'static str),
    /// A baseline the request names has no closes in it.
    #[error("a baseline has no closes in the request")]
    BaselineWithoutCloses,
    /// The research crate refused an input (a close series empty or out of order); its code is
    /// kept, since `ResearchError` is neither `Clone` nor `Eq`.
    #[error("the research crate refused an input: {0}")]
    Research(&'static str),
    #[error(transparent)]
    Num(#[from] NumError),
    /// A step its story has not built yet (DEC-77): the stub's own report.
    #[error("not implemented until {story}")]
    Unimplemented { story: &'static str },
}

impl From<ResearchError> for EvaluationError {
    fn from(error: ResearchError) -> Self {
        Self::Research(error.code())
    }
}

impl EvaluationError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::RecordRejected { .. } => "record_rejected",
            Self::RecordMalformed { .. } => "record_malformed",
            Self::DuplicateRecord { .. } => "duplicate_record",
            Self::Accounting(code) => code,
            Self::BaselineWithoutCloses => "baseline_without_closes",
            Self::Research(code) => code,
            Self::Num(e) => e.code(),
            Self::Unimplemented { .. } => "unimplemented",
        }
    }
}

/// One journaled `ThesisProposed` or `ThesisRevised`: where it sits on its agent stream, the
/// mandate version it was judged under (`config_refs.mandate_version`), and its §9.4 payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThesisRecord {
    pub stream_id: String,
    pub seq: u64,
    pub hash: Digest,
    pub mandate_version: Digest,
    pub payload: Value,
}

/// How a thesis's round trip is costed (DEC-1002 item 5): the fill model's slippage per leg (its
/// half spread plus its impact, as the caller configured the fill model; for a `sqrt` impact model
/// the caller passes the impact at the reference notional, or more), the
/// fee configuration the backtest charges, and the reference notional a thesis is costed at.
#[derive(Debug, Clone)]
pub struct CostModel {
    pub slippage_per_leg: Bps,
    pub fees: mandate_accounting::Config,
    pub reference_notional: Usd,
    pub increment: ShareIncrement,
}

/// One instrument's observed closes, oldest first, as the market-data store answered them. The
/// harness builds each `CloseSeries` itself, so the series it scores are exactly the closes its
/// report digests.
pub type Closes = BTreeMap<AssetId, Vec<ObservedClose>>;

/// The baselines (DEC-99, DEC-122): the eligible basket's members and the broad index ETF, by id;
/// their closes are in [`EvaluationRequest::closes`].
#[derive(Debug, Clone, Copy)]
pub struct Baselines<'a> {
    pub basket: &'a [AssetId],
    pub index: &'a AssetId,
}

/// Everything one evaluation reads.
pub struct EvaluationRequest<'a> {
    /// The recorded decision that registered the window, metric and threshold (DEC-122).
    pub decision_id: &'a str,
    pub decision: &'a EvaluationDecision,
    /// The instant the evaluation reads up to: a horizon ending after it is still open.
    pub cut_off: UtcNanos,
    /// The internal paper workspaces' thesis records, in journal order per stream.
    pub records: &'a [ThesisRecord],
    /// The stored mandate documents, by version hash, that `check_thesis_record` reads.
    pub documents: &'a dyn Fn(&Digest) -> Option<Value>,
    /// The closes of every instrument a thesis names and of every baseline, keyed by id.
    pub closes: &'a Closes,
    pub baselines: Baselines<'a>,
    pub cost: &'a CostModel,
}

/// One record as the harness reads it: the signal model that proposed it and the thesis.
struct ReadThesis {
    key: SignalModelKey,
    thesis: ClosedThesis,
}

/// Runs the evaluation (DEC-1002): checks and reads every record, sorts each thesis into open,
/// outside the window, or closed inside it, costs the closed ones, and scores each signal model's
/// closed theses with `score::evaluate`.
///
/// # Errors
/// `record_rejected` and `record_malformed` for a record that cannot be read as checked, naming
/// it; `baseline_without_closes` for a baseline with no closes; a series' own refusals (empty, or
/// out of order); and the arithmetic errors of the cost. A group the scorer refuses is not an error: its refusal is the group's.
pub fn evaluate(request: &EvaluationRequest<'_>) -> Result<EvaluationReport, EvaluationError> {
    let series = request
        .closes
        .iter()
        .map(|(id, closes)| Ok((id.clone(), CloseSeries::new(id.clone(), closes.clone())?)))
        .collect::<Result<BTreeMap<AssetId, CloseSeries>, EvaluationError>>()?;
    let baseline = |id: &AssetId| {
        series
            .get(id)
            .cloned()
            .ok_or(EvaluationError::BaselineWithoutCloses)
    };
    let basket = request
        .baselines
        .basket
        .iter()
        .map(baseline)
        .collect::<Result<Vec<_>, _>>()?;
    let index = baseline(request.baselines.index)?;
    let mut groups: BTreeMap<SignalModelKey, GroupTheses> = BTreeMap::new();
    let mut positions = std::collections::BTreeSet::new();
    for (index, record) in request.records.iter().enumerate() {
        if !positions.insert((record.stream_id.as_str(), record.seq)) {
            return Err(EvaluationError::DuplicateRecord { index });
        }
        mandate_spec::context::check_thesis_record(
            &record.payload,
            &record.mandate_version,
            request.documents,
        )
        .map_err(|error| EvaluationError::RecordRejected {
            index,
            code: error.code(),
        })?;
        let read = read(index, record)?;
        let group = groups.entry(read.key).or_default();
        let window = request.decision.window;
        if read.thesis.horizon_end > request.cut_off || !resolved(&read.thesis, request) {
            group.open = group.open.checked_add(1).ok_or(NumError::Overflow)?;
        } else if read.thesis.horizon_end < window.from || read.thesis.horizon_end > window.to {
            group.outside_window = group
                .outside_window
                .checked_add(1)
                .ok_or(NumError::Overflow)?;
        } else {
            let round_trip_cost = round_trip_cost(&read.thesis, &series, request.cost)?;
            group.closed.push(ClosedThesis {
                round_trip_cost,
                ..read.thesis
            });
        }
    }
    let mut scored = Vec::new();
    for (key, theses) in groups {
        let outcome = mandate_research::score::evaluate(&EvaluationInput {
            decision: request.decision,
            theses: &theses.closed,
            instruments: &series,
            basket: &basket,
            index: &index,
        });
        scored.push(Group {
            key,
            closed: u32::try_from(theses.closed.len()).map_err(|_| NumError::Overflow)?,
            open: theses.open,
            outside_window: theses.outside_window,
            outcome: outcome.map_err(|error| error.code()),
        });
    }
    Ok(EvaluationReport {
        decision_id: request.decision_id.to_owned(),
        decision: request.decision.clone(),
        cut_off: request.cut_off,
        inputs: report::InputDigests::of(request)?,
        basket: request.baselines.basket.to_vec(),
        index: request.baselines.index.clone(),
        groups: scored,
    })
}

/// Whether every price a thesis is scored against has been observed: the thesis instrument's
/// series, when the request has one, and each baseline's hold a close at or after the horizon end
/// (DEC-1002 item 3). Until then the exit edge would read an earlier, stale close, so the thesis is
/// still open. A thesis whose instrument has no series at all is the scorer's `no_series`.
fn resolved(thesis: &ClosedThesis, request: &EvaluationRequest<'_>) -> bool {
    let observed_past_horizon = |id: &AssetId| {
        request
            .closes
            .get(id)
            .is_some_and(|closes| closes.iter().any(|close| close.at >= thesis.horizon_end))
    };
    let instrument = !request.closes.contains_key(&thesis.instrument)
        || observed_past_horizon(&thesis.instrument);
    instrument
        && request.baselines.basket.iter().all(observed_past_horizon)
        && observed_past_horizon(request.baselines.index)
}

/// One signal model's theses, sorted as they resolved.
#[derive(Default)]
struct GroupTheses {
    closed: Vec<ClosedThesis>,
    open: u32,
    outside_window: u32,
}

fn member<'v>(
    index: usize,
    payload: &'v Value,
    member: &'static str,
) -> Result<&'v Value, EvaluationError> {
    payload
        .get(member)
        .ok_or(EvaluationError::RecordMalformed { index, member })
}

fn text<'v>(
    index: usize,
    payload: &'v Value,
    name: &'static str,
) -> Result<&'v str, EvaluationError> {
    match member(index, payload, name)? {
        Value::Str(text) => Ok(text),
        _ => Err(EvaluationError::RecordMalformed {
            index,
            member: name,
        }),
    }
}

fn instant(index: usize, payload: &Value, name: &'static str) -> Result<UtcNanos, EvaluationError> {
    UtcNanos::parse(text(index, payload, name)?).map_err(|_| EvaluationError::RecordMalformed {
        index,
        member: name,
    })
}

/// The members of a §9.4 payload the harness reads: the signal model's identity and the thesis.
/// Admission is not read (DEC-99: every thesis is scored).
fn read(index: usize, record: &ThesisRecord) -> Result<ReadThesis, EvaluationError> {
    let payload = &record.payload;
    let malformed = |member: &'static str| EvaluationError::RecordMalformed { index, member };
    let content_hash = text(index, payload, "content_hash")?
        .strip_prefix("sha256:")
        .and_then(Digest::from_hex)
        .ok_or(malformed("content_hash"))?;
    let revision = member(index, payload, "revision")?
        .as_int()
        .and_then(|revision| u32::try_from(revision).ok())
        .ok_or(malformed("revision"))?;
    let direction = match text(index, payload, "direction")? {
        "long" => Direction::Long,
        _ => Direction::Other,
    };
    Ok(ReadThesis {
        key: SignalModelKey {
            model_id: text(index, payload, "model_id")?.to_owned(),
            model_version: text(index, payload, "model_version")?.to_owned(),
            content_hash,
        },
        thesis: ClosedThesis {
            thesis: ThesisId::new(text(index, payload, "thesis_id")?)
                .map_err(|_| malformed("thesis_id"))?,
            lineage: LineageId::new(text(index, payload, "lineage_id")?)
                .map_err(|_| malformed("lineage_id"))?,
            revision,
            instrument: AssetId::new(text(index, payload, "instrument_id")?)
                .map_err(|_| malformed("instrument_id"))?,
            direction,
            as_of: instant(index, payload, "as_of")?,
            horizon_end: instant(index, payload, "expires_at")?,
            round_trip_cost: Ratio::parse("0")?,
        },
    })
}

/// DEC-1002 item 5: the slippage on both legs plus the fees `mandate-accounting`'s fold accrues for
/// a buy at the entry close and a sell at the exit close of the reference notional, rounded up to
/// the cent, all as a fraction of that notional rounded up at 12 places, so the cost is never
/// understated. (The fold, not `fee_reservation`: a reservation is zero for a sell, which would
/// leave out the SEC fee and the TAF.) A thesis whose entry or exit close does not exist is unscoreable whatever
/// its cost, and is costed at the slippage alone.
fn round_trip_cost(
    thesis: &ClosedThesis,
    series: &BTreeMap<AssetId, CloseSeries>,
    cost: &CostModel,
) -> Result<Ratio, EvaluationError> {
    let both_legs = |notional: Usd| -> Result<Usd, EvaluationError> {
        let leg = notional.times_bps(cost.slippage_per_leg, 12, Rounding::Ceiling)?;
        Ok(leg.checked_add(leg)?)
    };
    let slippage_only = || -> Result<Ratio, EvaluationError> {
        Ok(both_legs(cost.reference_notional)?.ratio_to(
            cost.reference_notional,
            12,
            Rounding::Ceiling,
        )?)
    };
    let Some(series) = series.get(&thesis.instrument) else {
        return slippage_only();
    };
    let (Ok(entry), Ok(exit)) = (
        entry_close(series, thesis.as_of),
        exit_close(series, thesis.horizon_end),
    ) else {
        return slippage_only();
    };
    let qty = cost.reference_notional.shares_at(entry, cost.increment)?;
    if qty.is_zero() {
        return slippage_only();
    }
    let instrument = InstrumentId::new(thesis.instrument.as_str())
        .map_err(|_| EvaluationError::Num(NumError::NotCanonical))?;
    let fill = |side: Side, price, at, id: &str| {
        Input::Fill(Execution {
            fill_id: id.to_owned(),
            client_order_id: None,
            instrument: instrument.clone(),
            asset_class: AssetClass::UsEquity,
            side,
            qty_gross: qty,
            price,
            liquidity: None,
            executed_at: at,
        })
    };
    let notional = qty.notional(entry)?;
    let opened = Account::opening(AccountType::Margin, notional, [])
        .apply(&fill(Side::Buy, entry, thesis.as_of, "entry"), &cost.fees)
        .map_err(|error| EvaluationError::Accounting(error.code()))?
        .account;
    let closed = opened
        .apply(
            &fill(Side::Sell, exit, thesis.horizon_end, "exit"),
            &cost.fees,
        )
        .map_err(|error| EvaluationError::Accounting(error.code()))?
        .account;
    let fees = closed
        .fees_accrued()
        .map_err(|error| EvaluationError::Accounting(error.code()))?
        .round(2, Rounding::Ceiling)?;
    let total = both_legs(notional)?.checked_add(fees)?;
    Ok(total.ratio_to(notional, 12, Rounding::Ceiling)?)
}
