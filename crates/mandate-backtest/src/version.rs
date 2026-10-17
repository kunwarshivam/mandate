//! A backtest of a confirmed mandate version through the decision path its agent runs (backlog
//! E4-4, [DEC-1000](../../../docs/project/decisions/DEC-1000.md); PRD FR-4.1, FR-4.2, FR-4.5).
//!
//! [`run_version`] walks one US equity's bars and, at each decision instant, does what the live
//! agent does, in mandate spec §6.2's order, by calling the crates the agent calls: the risk state
//! (`mandate_spec::risk::RiskState`), the risk exits (`mandate_risk::agent_flatten`,
//! `mandate_risk::trim_proposals`), the order builder (`mandate_builder::propose`), the gate's two
//! passes (`mandate_risk::evaluate`), autonomy (`mandate_builder::classify`), and the executor's
//! protection (`mandate_executor::equity_bracket_prices`, `mandate_executor::protective_shape`).
//! Orders go to `mandate-sim`'s fill model and fills to `mandate-accounting`'s fold, as in E4-2.
//!
//! **There is no model port.** Model outputs and theses arrive as [`RecordedOutput`] and
//! [`RecordedThesis`], each visible only from its receive time, so the run cannot call a model and
//! cannot see a record before it was journaled (DEC-1000 items 3 and 4).
//!
//! Like [`crate::run`], the run is pure: no clock, no file, no randomness, ordered collections only,
//! and the same inputs give the same report bytes (ES-21, FR-4.5).

mod gate;
mod record;
mod report;

pub use record::{RunRecord, backtest_run_recorded};
pub use report::{
    DecisionOutcome, DecisionRow, Limitation, VERSION_REPORT_VERSION, VersionDigests, VersionReport,
};

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::{
    Account, AccountType, AssetClass, Execution, FeeFamily, Input, InstrumentId, Side,
};
use mandate_builder::{
    Action, BuilderMandate, Market, ModelOutput, RiskContext, autonomy_policy, classify, propose,
};
use mandate_canon::Digest;
use mandate_domain::{AgentMode, AssetId, AutonomyDecision, CapabilityProfile, MarketSession};
use mandate_executor::{ProtectionPrices, equity_bracket_prices, protective_shape};
use mandate_num::{
    Adverse, Fraction, MarkPrice, NumError, Price, Qty, Rounding, Signed, SizeFraction, TickRule,
    Unit, Usd,
};
use mandate_risk::{
    AccountFill, DayTrade, DayTradeInput, DayTradeRegime, Decision, EtpClass, Exchange,
    FlattenInitiator, FlattenInput, FlattenPricing, GateConfig, GatePass, Origin, ProposedKind,
    Verdict,
};
use mandate_sim::{
    Eligibility, FirstBarVolumes, Instrument, OrderEnd, OrderKind, OrderRef, Session, SimBar,
    SimConfig, SimFill, SimOrder, TimeInForce, simulate,
};
use mandate_spec::ValidatedMandate;
use mandate_spec::risk::{self, Opening, RiskState, SessionClock, Step};
use mandate_time::{Date, ExchangeCalendar, UtcNanos, new_york_date_and_hour, new_york_midnight};

use crate::{
    BacktestError, BacktestInput, Metrics, MetricsConfig, Observation, RunConfig, RunFill,
    Strategy, Totals, Working,
};

/// Seconds the gate's order-size participation cap looks back over (trading spec §9.6).
const TRAILING_WINDOW_S: u64 = 300;
/// The lower median and the mean `mandate-liquidity` reads need this many periods (§3.2 item 5).
const LIQUIDITY_PERIODS: usize = mandate_liquidity::SESSIONS;
/// The one agent a backtest runs, as the gate and the flatten plan name it.
const AGENT: mandate_risk::AgentId = mandate_risk::AgentId(1);

/// What the run is pinned to besides the mandate: the fill model, fees, the organization's gate
/// settings, the connection's capability profile, the account, and the report's statistics.
#[derive(Debug, Clone)]
pub struct VersionSettings {
    pub sim: SimConfig,
    pub fees: mandate_accounting::Config,
    pub gate: GateConfig,
    pub profile: CapabilityProfile,
    pub account_type: AccountType,
    pub regime: DayTradeRegime,
    pub metrics: MetricsConfig,
    /// The smallest order value the builder proposes (mandate spec §8.3 step 5).
    pub min_order_usd: Usd,
    /// How long a held instrument may go without a sane mark before `stale_mark` (§5.2).
    pub mark_max_age_s: u32,
    /// The collar the buy-and-hold benchmark's one order is priced with (DEC-127 item 13).
    pub benchmark_collar: mandate_num::Bps,
}

/// The run's instrument as the reference data snapshot states it: the gate's eligibility facts and
/// the grid every order and fill sits on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstrumentReference {
    pub asset: AssetId,
    pub ledger_id: InstrumentId,
    pub instrument: Instrument,
    pub tick: TickRule,
    pub qty_increment: Qty,
    pub min_order_size: Qty,
    pub exchange: Exchange,
    /// The ETP classifications the snapshot holds, each with the instant it was made. At a
    /// decision the gate reads the latest made at or before it, and judges its age itself (trading
    /// spec §3.2 item 6), so no classification is ever fresher than the data says.
    pub etp_classifications: Vec<EtpClassification>,
    pub fractionable: bool,
    pub ipo: bool,
    pub ptp_no_exception: bool,
}

/// One dated ETP classification of the run's instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EtpClassification {
    pub at: UtcNanos,
    pub class: EtpClass,
}

/// One model output as the agent stream recorded it: the output, and the instant its
/// `ModelOutputRecorded` was journaled, from which it is visible (DEC-1000 item 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedOutput {
    pub received_at: UtcNanos,
    pub output: ModelOutput,
}

/// One research-agent thesis as the agent stream recorded it, with its admission verdict replayed
/// as journaled (DEC-1000 item 5). `invalidated_at` is the receive time of a recorded invalidation,
/// when there was one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedThesis {
    pub thesis_id: String,
    pub received_at: UtcNanos,
    pub instrument: AssetId,
    pub as_of: UtcNanos,
    pub expires_at: UtcNanos,
    pub admitted: bool,
    pub confidence: Unit,
    pub invalidated_at: Option<UtcNanos>,
}

/// One version backtest's inputs. `bars` is one instrument's bars in strictly increasing order;
/// `corporate_actions` are the ex-dates the data snapshot holds for it.
pub struct VersionInput<'a> {
    pub mandate: &'a ValidatedMandate,
    pub settings: &'a VersionSettings,
    pub instrument: &'a InstrumentReference,
    pub bars: &'a [SimBar],
    pub coverage_start: UtcNanos,
    pub first_bar_volumes: &'a dyn FirstBarVolumes,
    pub outputs: &'a [RecordedOutput],
    pub theses: &'a [RecordedThesis],
    pub corporate_actions: &'a [Date],
}

/// One order the run submitted: its run-wide index, the instant it was decided at, the order, and
/// how it ended (read from the fill model). A protective order is marked as one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionOrder {
    pub index: u32,
    pub decided_at: UtcNanos,
    pub order: SimOrder,
    pub protective: bool,
    pub end: OrderEnd,
}

/// What [`run_version`] produces: the report, and every order and fill it was computed from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRun {
    pub report: VersionReport,
    pub orders: Vec<VersionOrder>,
    pub fills: Vec<RunFill>,
}

/// Runs a confirmed mandate version over one instrument's bars and reports it beside buy-and-hold
/// of the same instrument (DEC-1000, DEC-1001).
///
/// # Errors
/// The input refusals of DEC-1000 items 3, 6 and 8: `asset_class_not_supported`,
/// `corporate_action_in_range`, `record_before_its_data`, `output_not_pinned`,
/// `record_other_instrument`, and `protection_not_placeable`; E4-2's `no_bars` and
/// `bar_trade_date_mismatch`; and the arithmetic, fill-model, accounting, risk-state, gate, and
/// calendar errors the run wraps. Every error refuses the whole run: no partial report exists.
pub fn run_version(input: &VersionInput<'_>) -> Result<VersionRun, BacktestError> {
    check_inputs(input)?;
    let benchmark_config = benchmark_config(input)?;
    let benchmark_input = BacktestInput {
        config: &benchmark_config,
        bars: input.bars,
        coverage_start: input.coverage_start,
        first_bar_volumes: input.first_bar_volumes,
    };
    crate::check_trade_dates(&benchmark_input)?;
    let benchmark = crate::walk(&benchmark_input, benchmark_config.strategy)?;
    let calendar =
        ExchangeCalendar::us_equities().map_err(|_| BacktestError::CalendarUnavailable)?;
    let clock = RegularSeconds {
        calendar: &calendar,
    };
    let mut run = Run::open(input, &benchmark_config, &calendar, &clock)?;
    run.walk()?;
    let starting_cash = run.starting_cash;
    let strategy_metrics = Metrics::of(
        starting_cash,
        &run.equity,
        &run.totals()?,
        &input.settings.metrics,
    )?;
    let benchmark_metrics = benchmark.metrics(&benchmark_config)?;
    let report = VersionReport {
        mandate_version: mandate_version(input.mandate)?,
        instrument: input.instrument.asset.clone(),
        research_agent: research_agent(input.mandate),
        inputs: VersionDigests::of(input)?,
        excess_total_return: strategy_metrics
            .total_return
            .checked_sub(benchmark_metrics.total_return)?,
        strategy: strategy_metrics,
        benchmark: benchmark_metrics,
        decisions: run.rows,
        limits_latched: run.latched.iter().map(|key| key.journal_name()).collect(),
        risk_exits: run.risk_exits,
        most_restrictive_mode: run.most_restrictive,
        strategy_equity: run.equity,
        benchmark_equity: benchmark.equity,
    };
    Ok(VersionRun {
        report,
        orders: run.orders,
        fills: run.fills,
    })
}

/// The confirmed version's content hash, which names the report's mandate (DEC-1001 item 2).
pub(crate) fn mandate_version(mandate: &ValidatedMandate) -> Result<Digest, BacktestError> {
    Ok(mandate
        .mandate()
        .version()
        .map_err(|_| BacktestError::Num(NumError::NotCanonical))?
        .digest())
}

/// Whether the version's universe comes from the research agent: an unpinned universe, which
/// V-036 gives only to a mandate whose research agent admits instruments.
fn research_agent(mandate: &ValidatedMandate) -> bool {
    !mandate.mandate().universe.pinned
}

/// DEC-1000 items 3, 6 and 8, before anything is simulated: every refusal names the input.
fn check_inputs(input: &VersionInput<'_>) -> Result<(), BacktestError> {
    let instrument = input.instrument;
    if instrument.instrument.asset_class != AssetClass::UsEquity {
        return Err(BacktestError::AssetClassNotSupported);
    }
    let (Some(first), Some(last)) = (input.bars.first(), input.bars.last()) else {
        return Err(BacktestError::NoBars);
    };
    if let Some(date) = input
        .corporate_actions
        .iter()
        .find(|date| first.trade_date <= **date && **date <= last.trade_date)
    {
        return Err(BacktestError::CorporateActionInRange(*date));
    }
    let models = BuilderMandate::try_from(input.mandate)?.models;
    for (index, recorded) in input.outputs.iter().enumerate() {
        let output = &recorded.output;
        if output.instrument != instrument.asset {
            return Err(BacktestError::RecordOtherInstrument(index));
        }
        if recorded.received_at < output.as_of {
            return Err(BacktestError::RecordBeforeItsData(index));
        }
        let pinned = models.iter().any(|model| {
            model.id == output.model_id
                && model.version == output.model_version
                && model.content_hash == output.content_hash
        });
        if !pinned {
            return Err(BacktestError::OutputNotPinned(index));
        }
    }
    for (index, thesis) in input.theses.iter().enumerate() {
        if thesis.instrument != instrument.asset {
            return Err(BacktestError::RecordOtherInstrument(index));
        }
        let invalidated_early = thesis
            .invalidated_at
            .is_some_and(|at| at < thesis.received_at);
        if thesis.received_at < thesis.as_of || invalidated_early {
            return Err(BacktestError::RecordBeforeItsData(index));
        }
    }
    Ok(())
}

/// The benchmark's configuration: buy-and-hold of the same instrument from the version's
/// allocation, through the same fill model and fees (DEC-1001 item 2, DEC-127 item 13).
fn benchmark_config(input: &VersionInput<'_>) -> Result<RunConfig, BacktestError> {
    let settings = input.settings;
    Ok(RunConfig {
        sim: settings.sim,
        fees: settings.fees.clone(),
        account_type: settings.account_type,
        instrument_id: input.instrument.ledger_id.clone(),
        instrument: input.instrument.instrument,
        tick: input.instrument.tick,
        starting_cash: allocation(input.mandate)?,
        strategy: Strategy::BuyAndHold {
            collar: settings.benchmark_collar,
        },
        metrics: settings.metrics,
    })
}

fn allocation(mandate: &ValidatedMandate) -> Result<Usd, BacktestError> {
    Ok(Usd::parse(
        mandate.mandate().capital.allocation_usd.as_str(),
    )?)
}

/// The clock §5.2 and §5.5 count an equity's timers on: regular-session seconds of the committed
/// calendar (`mandate-time`'s NYSE data).
struct RegularSeconds<'c> {
    calendar: &'c ExchangeCalendar,
}

impl SessionClock for RegularSeconds<'_> {
    fn seconds_between(
        &self,
        from: UtcNanos,
        to: UtcNanos,
    ) -> Result<u64, mandate_spec::SpecError> {
        let mut total: u64 = 0;
        let mut date = from.date();
        while date <= to.date() {
            for span in self.calendar.sessions(date)? {
                if span.session() != mandate_time::Session::Regular {
                    continue;
                }
                let start = span.start().max(from);
                let end = span.end().min(to);
                if start < end {
                    total = total.saturating_add(whole_seconds(start, end)?);
                }
            }
            date = date.next()?;
        }
        Ok(total)
    }
}

/// Whole seconds from `from` to `to`, never negative.
fn whole_seconds(from: UtcNanos, to: UtcNanos) -> Result<u64, mandate_spec::SpecError> {
    to.secs()
        .checked_sub(from.secs())
        .and_then(|seconds| u64::try_from(seconds).ok())
        .ok_or(mandate_spec::SpecError::ClockWentBackwards)
}

/// One closed period's figures the gate's liquidity inputs read.
#[derive(Debug, Clone, Copy)]
struct ClosedPeriod {
    close: Price,
    volume: Qty,
}

/// The last closed regular bar: its close, the instant it closed, its volume, and how many
/// regular-session seconds it spanned.
#[derive(Debug, Clone, Copy)]
struct LastClose {
    close: Price,
    at: UtcNanos,
    volume: Qty,
    regular_seconds: u64,
}

/// The day's conduct and day-trade inputs, reset at each New York midnight (trading spec §9.2,
/// §9.6).
#[derive(Debug, Clone)]
struct Today {
    orders: u32,
    filled_orders: BTreeSet<u32>,
    fills: Vec<AccountFill>,
    filled_qty: Qty,
    bought_usd: Usd,
    held_overnight: Qty,
}

/// Why a proposal reached the gate: the origin the gate is told, and the order the run would send.
struct Candidate {
    origin: Origin,
    side: Side,
    qty: Qty,
    limit: Price,
    kind: ProposedKind,
    sim_kind: Option<OrderKind>,
    buy_action: Option<mandate_builder::ActionContext>,
    outputs_used: u32,
}

/// The walk's state: the ledger, the risk state, the orders, and what the report needs.
struct Run<'a, 'c> {
    input: &'a VersionInput<'a>,
    config: &'a RunConfig,
    calendar: &'c ExchangeCalendar,
    builder: BuilderMandate,
    gate_mandate: mandate_risk::ValidatedMandate,
    risk_asset: mandate_risk::AssetId,
    starting_cash: Usd,
    account: Account,
    risk: RiskState<'c>,
    last_step: UtcNanos,
    orders: Vec<VersionOrder>,
    fills: Vec<RunFill>,
    working: Option<Working>,
    protection: Option<Working>,
    protection_pending: bool,
    rows: Vec<DecisionRow>,
    equity: Vec<Observation>,
    periods: Vec<ClosedPeriod>,
    last_close: Option<LastClose>,
    prior_close_equity: Usd,
    today_date: Option<Date>,
    today: Today,
    earlier_day_trades: Vec<DayTrade>,
    last_fill_by_side: BTreeMap<mandate_risk::RestingSide, UtcNanos>,
    last_exit_fill_at: Option<UtcNanos>,
    has_prior_fill: bool,
    goal_spent: Usd,
    last_entry_limit: Option<Price>,
    latched: BTreeSet<risk::LimitKey>,
    risk_exits: u32,
    most_restrictive: AgentMode,
    charged: BTreeSet<Date>,
    dates: BTreeSet<Date>,
}

impl<'a, 'c> Run<'a, 'c> {
    fn open(
        input: &'a VersionInput<'a>,
        config: &'a RunConfig,
        calendar: &'c ExchangeCalendar,
        clock: &'c RegularSeconds<'c>,
    ) -> Result<Self, BacktestError> {
        let first = input.bars.first().ok_or(BacktestError::NoBars)?;
        let starting_cash = allocation(input.mandate)?;
        let risk = RiskState::open(
            input.mandate,
            &Opening {
                position_qty: Qty::ZERO,
                avg_cost: first.close,
                asset_class: AssetClass::UsEquity,
                at: first.start,
                inherited_loss_usd: Usd::ZERO,
                mark_max_age_s: input.settings.mark_max_age_s,
            },
            clock,
        )?;
        Ok(Self {
            input,
            config,
            calendar,
            builder: BuilderMandate::try_from(input.mandate)?,
            gate_mandate: gate::mandate(input.mandate)?,
            risk_asset: gate::asset(&input.instrument.asset)?,
            starting_cash,
            account: Account::opening(input.settings.account_type, starting_cash, []),
            risk,
            last_step: first.start,
            orders: Vec::new(),
            fills: Vec::new(),
            working: None,
            protection: None,
            protection_pending: false,
            rows: Vec::new(),
            equity: Vec::new(),
            periods: Vec::new(),
            last_close: None,
            prior_close_equity: starting_cash,
            today_date: None,
            today: Today {
                orders: 0,
                filled_orders: BTreeSet::new(),
                fills: Vec::new(),
                filled_qty: Qty::ZERO,
                bought_usd: Usd::ZERO,
                held_overnight: Qty::ZERO,
            },
            earlier_day_trades: Vec::new(),
            last_fill_by_side: BTreeMap::new(),
            last_exit_fill_at: None,
            has_prior_fill: false,
            goal_spent: Usd::ZERO,
            last_entry_limit: None,
            latched: BTreeSet::new(),
            risk_exits: 0,
            most_restrictive: AgentMode::Normal,
            charged: BTreeSet::new(),
            dates: BTreeSet::new(),
        })
    }

    /// The walk, bar by bar, in DEC-1000 item 1's order: the risk days the bar's start passes, the
    /// decision when the previous bar closed a period, this bar's fills, the protection an ended
    /// order leaves to place, then the close's marks, fee charges and period observation.
    fn walk(&mut self) -> Result<(), BacktestError> {
        let bars = self.input.bars;
        let closing = crate::closing_bars(bars);
        let mut decides = false;
        for (index, bar) in bars.iter().enumerate() {
            self.dates.insert(bar.trade_date);
            self.cross_midnights(bar.start)?;
            for due in self.account.due(bar.start)? {
                self.account = self.account.apply(&due, &self.config.fees)?.account;
            }
            if decides {
                self.decide(index, bar)?;
            }
            self.fold_fills(index, bar)?;
            let close_at = self.close_instant(index, bar)?;
            if self.protection_pending {
                self.place_protection(close_at)?;
            }
            self.close(
                index,
                bar,
                close_at,
                closing.get(index).copied().unwrap_or(false),
            )?;
            decides = closing.get(index).copied().unwrap_or(false);
        }
        Ok(())
    }

    /// The instant a bar closes: the earlier of the next bar's start and its regular session's
    /// end on the committed calendar (DEC-1000 item 3).
    fn close_instant(&self, index: usize, bar: &SimBar) -> Result<UtcNanos, BacktestError> {
        let next = index
            .checked_add(1)
            .and_then(|next| self.input.bars.get(next))
            .map(|next| next.start);
        let session_end = self
            .calendar
            .sessions(bar.trade_date)?
            .into_iter()
            .find(|span| {
                span.session() == mandate_time::Session::Regular && span.contains(bar.start)
            })
            .map(|span| span.end());
        match (next, session_end) {
            (Some(next), Some(end)) => Ok(next.min(end)),
            (None, Some(end)) => Ok(end),
            (Some(next), None) => Ok(next),
            (None, None) => Err(BacktestError::BarOutsideRegularSession(index)),
        }
    }

    /// Steps the risk state with `RiskDayStarted` at each New York midnight after the last step and
    /// at or before `until`, and resets the day's conduct inputs.
    fn cross_midnights(&mut self, until: UtcNanos) -> Result<(), BacktestError> {
        let (mut day, _) = new_york_date_and_hour(self.last_step)?;
        loop {
            day = day.next()?;
            let midnight = new_york_midnight(day)?;
            if midnight > until {
                break;
            }
            if midnight <= self.last_step {
                continue;
            }
            self.step(
                midnight,
                MarketSession::Overnight,
                risk::Input::RiskDayStarted,
            )?;
        }
        let (today, _) = new_york_date_and_hour(until)?;
        if self.today_date != Some(today) {
            self.roll_day(today)?;
        }
        Ok(())
    }

    /// Starts a new risk day: yesterday's day trades join the earlier ones, and the day's counts
    /// start from empty (trading spec §9.2, §9.6).
    fn roll_day(&mut self, today: Date) -> Result<(), BacktestError> {
        if let (Some(_), Some(last)) = (self.today_date, self.last_close) {
            let fold = self.day_trades(last.at)?;
            self.earlier_day_trades.extend(fold.today);
        }
        self.today = Today {
            orders: 0,
            filled_orders: BTreeSet::new(),
            fills: Vec::new(),
            filled_qty: Qty::ZERO,
            bought_usd: Usd::ZERO,
            held_overnight: self.position()?,
        };
        self.today_date = Some(today);
        Ok(())
    }

    fn step(
        &mut self,
        at: UtcNanos,
        session: MarketSession,
        input: risk::Input,
    ) -> Result<(), BacktestError> {
        let outcome = self.risk.step(&Step { at, session, input })?;
        self.last_step = at;
        self.latched
            .extend(outcome.snapshot.latched.iter().copied());
        self.most_restrictive = self.most_restrictive.max(outcome.snapshot.agent_mode);
        Ok(())
    }

    fn position(&self) -> Result<Qty, BacktestError> {
        Ok(self
            .account
            .position(&self.input.instrument.ledger_id)
            .qty()
            .abs())
    }

    /// The bid and the ask at a decision: the last close moved by the fill model's half spread,
    /// each rounded against the agent onto the tick (DEC-1000 item 7).
    fn quote(&self, close: Price) -> Result<(Price, Price), BacktestError> {
        let half = half_spread(&self.input.settings.sim);
        let tick = self.input.instrument.tick;
        Ok((
            close
                .slipped(half, Adverse::Down)?
                .on_tick(tick, Adverse::Down)?,
            close
                .slipped(half, Adverse::Up)?
                .on_tick(tick, Adverse::Up)?,
        ))
    }

    /// One decision at `t`, the start of the bar after a period's close (DEC-1000 item 1).
    fn decide(&mut self, index: usize, bar: &SimBar) -> Result<(), BacktestError> {
        let t = bar.start;
        self.step(t, MarketSession::Regular, risk::Input::Clock)?;
        let date = bar.trade_date;
        let order_working = self
            .working
            .as_ref()
            .is_some_and(|order| !order.free_before(index));
        if order_working {
            self.rows
                .push(DecisionRow::bare(t, date, DecisionOutcome::OrderWorking));
            return Ok(());
        }
        self.working = None;
        let Some(last) = self.last_close else {
            self.rows.push(
                DecisionRow::bare(t, date, DecisionOutcome::BuilderHold)
                    .with_reason("no_fresh_outputs"),
            );
            return Ok(());
        };
        let (bid, ask) = self.quote(last.close)?;
        let candidate = match self.candidate(t, bid, ask)? {
            Ok(candidate) => candidate,
            Err(row) => {
                self.rows.push(row.at(t, date));
                return Ok(());
            }
        };
        let first = self.gate(t, last, bid, ask, &candidate, GatePass::First)?;
        if first.verdict != Verdict::Allow {
            self.rows
                .push(DecisionRow::gated(t, date, &candidate, &first));
            return Ok(());
        }
        let mut decided_by = None;
        if let Some(action) = &candidate.buy_action {
            let classification = classify(autonomy_policy(self.input.mandate), action)?;
            decided_by = Some(classification.by.label());
            let outcome = match classification.decision {
                AutonomyDecision::Auto => None,
                AutonomyDecision::Ask => Some(DecisionOutcome::AskUnanswered),
                AutonomyDecision::Deny => Some(DecisionOutcome::AutonomyDeny),
            };
            if let Some(outcome) = outcome {
                self.rows.push(
                    DecisionRow::gated(t, date, &candidate, &first)
                        .with_outcome(outcome)
                        .decided_by(decided_by),
                );
                return Ok(());
            }
        }
        let protection = self.protection.take();
        let second = self.gate(t, last, bid, ask, &candidate, GatePass::BeforeSubmission)?;
        if second.verdict != Verdict::Allow {
            self.protection = protection;
            self.rows
                .push(DecisionRow::gated(t, date, &candidate, &second).decided_by(decided_by));
            return Ok(());
        }
        if let Some(protection) = protection {
            self.canceled(&protection, index)?;
        }
        let (qty, limit) = second
            .pacing
            .as_ref()
            .map_or((candidate.qty, candidate.limit), |pacing| {
                (pacing.qty, pacing.limit_price)
            });
        let kind = match (candidate.sim_kind, &second.pacing) {
            (Some(OrderKind::Market), Some(pacing)) if pacing.marketable_limit_required => {
                OrderKind::Limit { limit }
            }
            (Some(kind), _) => kind,
            (None, _) => OrderKind::Limit { limit },
        };
        if second.purpose == mandate_risk::Purpose::RiskExit {
            self.risk_exits = self.risk_exits.checked_add(1).ok_or(NumError::Overflow)?;
        }
        if candidate.side == Side::Buy {
            self.last_entry_limit = Some(limit);
        }
        let order = SimOrder {
            side: candidate.side,
            qty,
            kind,
            tif: TimeInForce::Day,
            extended_hours: false,
            eligible_from: Eligibility::DecidedAt {
                at: t,
                approval_required: false,
            },
        };
        self.working = Some(self.submit(t, order, false)?);
        self.rows.push(
            DecisionRow::gated(t, date, &candidate, &second)
                .with_outcome(DecisionOutcome::Submitted)
                .sized(qty, limit)
                .decided_by(decided_by),
        );
        Ok(())
    }

    /// §6.2 step 1, then the builder: a risk exit when the risk state says flatten or trim, else
    /// the builder's proposal, or the row that says why nothing was proposed.
    fn candidate(
        &self,
        t: UtcNanos,
        bid: Price,
        ask: Price,
    ) -> Result<Result<Candidate, DecisionRow>, BacktestError> {
        let snapshot = self.risk.snapshot();
        let position = self.position()?;
        if matches!(snapshot.agent_mode, AgentMode::Paused | AgentMode::Stopped)
            && !position.is_zero()
        {
            return self.flatten(t, bid, position).map(Ok);
        }
        if let Some(trim) = self.trim(t, bid)? {
            return Ok(Ok(trim));
        }
        let outputs = self.fresh_outputs(t);
        let outputs_used = u32::try_from(outputs.len()).map_err(|_| NumError::Overflow)?;
        let market = self.market(t, bid, ask)?;
        let proposal = match propose(
            &self.builder,
            &self.builder_account(bid)?,
            &market,
            &self.risk_context(t, bid)?,
            &outputs,
            t,
        ) {
            Ok(proposal) => proposal,
            Err(error) => {
                return Ok(Err(DecisionRow::bare(
                    t,
                    t.date(),
                    DecisionOutcome::BuilderRefused,
                )
                .with_reason(error.code())
                .outputs(outputs_used)));
            }
        };
        let used =
            u32::try_from(proposal.combined.outputs_used.len()).map_err(|_| NumError::Overflow)?;
        Ok(match proposal.action {
            Action::Hold { reason } => {
                Err(DecisionRow::bare(t, t.date(), DecisionOutcome::BuilderHold)
                    .with_reason(reason.as_str())
                    .outputs(used))
            }
            Action::Buy {
                purpose: _,
                qty,
                limit_price,
                action,
                ..
            } => Ok(Candidate {
                origin: Origin::OrderBuilder,
                side: Side::Buy,
                qty,
                limit: limit_price,
                kind: ProposedKind::Plain,
                sim_kind: None,
                buy_action: Some(action),
                outputs_used: used,
            }),
            Action::Sell {
                purpose: _,
                qty,
                limit_price,
                ..
            } => Ok(Candidate {
                origin: Origin::OrderBuilder,
                side: Side::Sell,
                qty,
                limit: limit_price,
                kind: ProposedKind::Plain,
                sim_kind: None,
                buy_action: None,
                outputs_used: used,
            }),
        })
    }

    /// The agent-scoped flatten plan's sell of the whole sub-ledger position (§5.5, DEC-1000 item
    /// 1.2): a market order where the plan prices it so, else a limit at its floor. The flatten a
    /// `flatten_and_pause` rung or the floor starts is the automated kill switch, whose orders
    /// §5.5 exempts from the agent's mode, so the gate is told that origin, never `risk_engine`,
    /// which `paused` would hold.
    fn flatten(&self, t: UtcNanos, bid: Price, position: Qty) -> Result<Candidate, BacktestError> {
        let session = mandate_risk::session_at(t, &self.input.settings.gate, AssetClass::UsEquity)?;
        let plan = mandate_risk::agent_flatten(&FlattenInput {
            agent: AGENT,
            open_orders: &BTreeMap::new(),
            agent_positions: &[mandate_risk::AgentPosition {
                agent: AGENT,
                instrument: self.risk_asset.clone(),
                asset_class: AssetClass::UsEquity,
                qty: position,
            }],
            broker_positions: &BTreeMap::new(),
            session: session.session,
            initiator: FlattenInitiator::RiskLimit,
            owner_confirmed_bid: None,
            max_exit_offset: self.input.settings.gate.collar_liquid_x,
            owner_floor_price: None,
        })?;
        let sell = plan
            .sells
            .first()
            .ok_or(BacktestError::FlattenWithoutSell)?;
        let (kind, sim_kind, limit) = match (sell.pricing, sell.floor_price) {
            (FlattenPricing::MarketOrLadder, _) => {
                (ProposedKind::Market, Some(OrderKind::Market), bid)
            }
            (FlattenPricing::ExitPriceLadder, Some(floor)) => (ProposedKind::Plain, None, floor),
            (FlattenPricing::ExitPriceLadder, None) => (ProposedKind::Plain, None, bid),
        };
        Ok(Candidate {
            origin: Origin::AutomatedKillSwitch,
            side: Side::Sell,
            qty: sell.qty,
            limit,
            kind,
            sim_kind,
            buy_action: None,
            outputs_used: 0,
        })
    }

    /// §5.5's `trim_to_target` sell, priced at the bid.
    fn trim(&self, t: UtcNanos, bid: Price) -> Result<Option<Candidate>, BacktestError> {
        let snapshot = gate::risk_snapshot(self.risk.snapshot())?;
        let position = self.position()?;
        let instruments = BTreeMap::from([(self.risk_asset.clone(), self.instrument_snapshot(t)?)]);
        let trims = mandate_risk::trim_proposals(
            t,
            &self.input.settings.gate,
            &self.gate_mandate,
            &snapshot,
            &self.agent_snapshot(position, bid)?,
            &self.account_snapshot(position, bid)?,
            &instruments,
        )?;
        Ok(trims.into_iter().next().map(|trim| Candidate {
            origin: Origin::TrimToTarget,
            side: Side::Sell,
            qty: trim.qty,
            limit: bid,
            kind: ProposedKind::Plain,
            sim_kind: None,
            buy_action: None,
            outputs_used: 0,
        }))
    }

    /// The recorded outputs visible at `t`: received at or before it (DEC-1000 item 3). The
    /// builder decides which of them are fresh.
    fn fresh_outputs(&self, t: UtcNanos) -> Vec<ModelOutput> {
        self.input
            .outputs
            .iter()
            .filter(|recorded| recorded.received_at <= t)
            .map(|recorded| recorded.output.clone())
            .collect()
    }

    /// The active admitted thesis for the run's instrument at `t`, the latest received, for a
    /// research-agent version (DEC-1000 item 5).
    fn active_thesis(&self, t: UtcNanos) -> Option<&RecordedThesis> {
        self.input
            .theses
            .iter()
            .filter(|thesis| {
                thesis.admitted
                    && thesis.received_at <= t
                    && t < thesis.expires_at
                    && thesis.invalidated_at.is_none_or(|at| t < at)
            })
            .max_by_key(|thesis| thesis.received_at)
    }

    /// Whether the instrument is in the working universe at `t`: always for a pinned version, and
    /// while an admitted thesis is active for a research-agent version.
    fn in_universe(&self, t: UtcNanos) -> bool {
        !research_agent(self.input.mandate) || self.active_thesis(t).is_some()
    }

    fn market(&self, t: UtcNanos, bid: Price, ask: Price) -> Result<Market, BacktestError> {
        let session = mandate_risk::session_at(t, &self.input.settings.gate, AssetClass::UsEquity)?;
        Ok(Market {
            instrument: self.input.instrument.asset.clone(),
            asset_class: AssetClass::UsEquity,
            session: gate::market_session(session.session),
            in_close_window: session.close_window,
            bid,
            ask,
            increment: self.input.instrument.qty_increment,
            min_order_usd: self.input.settings.min_order_usd,
            fee_rate_cash: mandate_num::FeeRate::parse("0")?,
            fee_rate_asset: mandate_num::FeeRate::parse("0")?,
        })
    }

    fn builder_account(
        &self,
        bid: Price,
    ) -> Result<mandate_builder::AccountSnapshot, BacktestError> {
        let position = self.position()?;
        let basis = self
            .account
            .position(&self.input.instrument.ledger_id)
            .basis();
        let market_value = position.notional(bid)?;
        Ok(mandate_builder::AccountSnapshot {
            agent_equity: self.risk.snapshot().agent_equity,
            position_qty: position,
            cost_basis: basis,
            risk_mark: MarkPrice::from(bid),
            gross_usd: market_value,
            working_opening_cost: Usd::ZERO,
            goal_spent_usd: self.goal_spent,
        })
    }

    fn risk_context(&self, t: UtcNanos, bid: Price) -> Result<RiskContext, BacktestError> {
        let snapshot = self.risk.snapshot();
        let position = self.position()?;
        let basis = self
            .account
            .position(&self.input.instrument.ledger_id)
            .basis()
            .to_usd();
        let position_pnl_fraction = if position.is_zero() || basis.is_zero() {
            Signed::ZERO
        } else {
            let gain = position.notional(bid)?.checked_sub(basis)?;
            Signed::parse(&gain.ratio_to(basis, 12, Rounding::HalfEven)?.to_string())?
        };
        Ok(RiskContext {
            size_factor: SizeFraction::parse(&snapshot.size_factor.to_string())?,
            drawdown: Unit::parse(&snapshot.drawdown.to_string())?,
            daily_pnl_fraction: Signed::parse(&snapshot.daily_pnl_fraction.to_string())?,
            position_pnl_fraction,
            bought_today_usd: self.today.bought_usd,
            has_prior_fill: self.has_prior_fill,
            new_instrument: false,
            thesis_confidence: self
                .active_thesis(t)
                .map_or(Unit::ZERO, |thesis| thesis.confidence),
            risk_day: risk::risk_day(t)?.day,
        })
    }

    fn instrument_snapshot(
        &self,
        t: UtcNanos,
    ) -> Result<mandate_risk::InstrumentSnapshot, BacktestError> {
        let reference = self.input.instrument;
        let classification = reference
            .etp_classifications
            .iter()
            .filter(|classification| classification.at <= t)
            .max_by_key(|classification| classification.at);
        let liquidity = self.liquidity()?;
        Ok(mandate_risk::InstrumentSnapshot {
            instrument: self.risk_asset.clone(),
            asset_class: AssetClass::UsEquity,
            exchange: Some(reference.exchange),
            status_active: true,
            tradable: true,
            fractionable: reference.fractionable,
            ipo: reference.ipo,
            ptp_no_exception: reference.ptp_no_exception,
            etp: classification.map_or(EtpClass::Unclassified, |classification| {
                classification.class
            }),
            etp_classified_at: classification.map(|classification| classification.at),
            quote_currency: Some(mandate_risk::QuoteCurrency::Usd),
            prior_close: self.last_close.map(|last| last.close),
            median_dollar_volume_20d: liquidity
                .as_ref()
                .map(|daily| daily.median_dollar_volume_20d),
            median_dollar_volume_30d: None,
            min_order_size: reference.min_order_size,
            qty_increment: reference.qty_increment,
            halted: false,
            status_feed_current: false,
        })
    }

    /// `mandate-liquidity`'s daily figures over the last 20 closed periods, or none with fewer, in
    /// which case every opening fails closed (DEC-1000 item 7).
    fn liquidity(&self) -> Result<Option<mandate_liquidity::DailyLiquidity>, BacktestError> {
        let Some(from) = self.periods.len().checked_sub(LIQUIDITY_PERIODS) else {
            return Ok(None);
        };
        let window: Vec<mandate_liquidity::DailyBar> = self
            .periods
            .get(from..)
            .unwrap_or_default()
            .iter()
            .map(|period| mandate_liquidity::DailyBar {
                close: period.close,
                volume: period.volume,
            })
            .collect();
        mandate_liquidity::daily_liquidity(&window)
            .map(Some)
            .map_err(|error| BacktestError::Liquidity(error.code()))
    }

    /// The last closed bar's volume spread evenly over its regular seconds, times 300, truncated
    /// to the share grid (DEC-1000 item 7, `participation_from_bar_volume`).
    fn trailing_volume(&self) -> Result<Option<Qty>, BacktestError> {
        let Some(last) = self.last_close else {
            return Ok(None);
        };
        if last.regular_seconds == 0 {
            return Ok(None);
        }
        let share = Fraction::parse(&ratio_text(
            TRAILING_WINDOW_S.min(last.regular_seconds),
            last.regular_seconds,
        )?)?;
        Ok(Some(last.volume.portion(
            share,
            self.input.instrument.instrument.increment,
        )?))
    }

    fn day_trades(&self, now: UtcNanos) -> Result<mandate_risk::DayTradeFold, BacktestError> {
        let held = BTreeMap::from([(self.risk_asset.clone(), self.today.held_overnight)]);
        Ok(mandate_risk::fold_day_trades(&DayTradeInput {
            now,
            earlier: &self.earlier_day_trades,
            held_overnight: &held,
            fills_today: &self.today.fills,
            flagged_pattern_day_trader: false,
        })?)
    }

    fn account_snapshot(
        &self,
        position: Qty,
        bid: Price,
    ) -> Result<mandate_risk::AccountSnapshot, BacktestError> {
        let buying_power = self
            .account
            .buying_power(mandate_accounting::Reservations::NONE)?;
        let market_value = position.notional(bid)?;
        let mut working_orders = BTreeMap::new();
        let mut resting = BTreeMap::new();
        if let Some(protection) = &self.protection {
            let order = protection.order;
            working_orders.insert(
                mandate_risk::ClientOrderId(u64::from(protection.index)),
                mandate_risk::WorkingOrder {
                    agent: AGENT,
                    instrument: self.risk_asset.clone(),
                    side: Side::Sell,
                    max_cost: Usd::ZERO,
                    open_qty: order.qty,
                    protective: true,
                    opening: false,
                    submitted_on: self.today_date.ok_or(BacktestError::NoBars)?,
                },
            );
            resting.insert(
                self.risk_asset.clone(),
                BTreeSet::from([mandate_risk::RestingSide::Sell]),
            );
        }
        Ok(mandate_risk::AccountSnapshot {
            account_type: self.input.settings.account_type,
            state: mandate_risk::AccountState::Active,
            crypto_active: false,
            regime: self.input.settings.regime,
            equity: self.account.equity()?,
            prior_close_equity: self.prior_close_equity,
            model_buying_power: buying_power,
            broker_buying_power: buying_power,
            broker_non_marginable_buying_power: buying_power,
            positions: BTreeMap::from([(self.risk_asset.clone(), position)]),
            market_values: BTreeMap::from([(self.risk_asset.clone(), market_value)]),
            working_orders,
            unknown_orders: BTreeSet::new(),
            related_account_resting: resting,
        })
    }

    fn agent_snapshot(
        &self,
        position: Qty,
        bid: Price,
    ) -> Result<mandate_risk::AgentSnapshot, BacktestError> {
        let snapshot = self.risk.snapshot();
        let restrictions = snapshot
            .instrument_restrictions
            .iter()
            .map(|restriction| gate::instrument_restriction(*restriction))
            .collect();
        let working = self
            .protection
            .iter()
            .map(|protection| mandate_risk::ClientOrderId(u64::from(protection.index)))
            .collect();
        Ok(mandate_risk::AgentSnapshot {
            agent: AGENT,
            mode: gate::agent_mode(snapshot.agent_mode),
            instrument_restrictions: BTreeMap::from([(self.risk_asset.clone(), restrictions)]),
            positions: BTreeMap::from([(self.risk_asset.clone(), position)]),
            market_values: BTreeMap::from([(self.risk_asset.clone(), position.notional(bid)?)]),
            working_orders: working,
            instrument_groups: BTreeMap::new(),
            last_exit_fill_at: self
                .last_exit_fill_at
                .map(|at| BTreeMap::from([(self.risk_asset.clone(), at)]))
                .unwrap_or_default(),
            orders_today: self.today.orders,
            day_trades: self.day_trades(self.last_step)?.ledger,
        })
    }

    fn conduct(&self) -> Result<mandate_risk::ConductState, BacktestError> {
        let filled =
            u32::try_from(self.today.filled_orders.len()).map_err(|_| NumError::Overflow)?;
        let open_protective = self
            .protection
            .as_ref()
            .map_or(Ok(Qty::ZERO), |protection| protection.remaining())?;
        Ok(mandate_risk::ConductState {
            filled_today: BTreeMap::from([(self.risk_asset.clone(), filled)]),
            orders_today_per_instrument: BTreeMap::from([(
                self.risk_asset.clone(),
                self.today.orders,
            )]),
            last_opposite_fill_at: self
                .last_fill_by_side
                .iter()
                .map(|(side, at)| ((self.risk_asset.clone(), *side), *at))
                .collect(),
            participation_today: BTreeMap::from([(
                self.risk_asset.clone(),
                self.today.filled_qty.checked_add(open_protective)?,
            )]),
            resting_since: BTreeMap::new(),
        })
    }

    /// One gate pass over the candidate, against the run's state at `t` (DEC-1000 item 1.4).
    fn gate(
        &self,
        t: UtcNanos,
        last: LastClose,
        bid: Price,
        ask: Price,
        candidate: &Candidate,
        pass: GatePass,
    ) -> Result<Decision, BacktestError> {
        let position = self.position()?;
        let universe = mandate_risk::WorkingUniverse::Known {
            instruments: if self.in_universe(t) {
                BTreeSet::from([self.risk_asset.clone()])
            } else {
                BTreeSet::new()
            },
            pinned: !research_agent(self.input.mandate),
        };
        let fee_reservation = match candidate.side {
            Side::Buy => mandate_accounting::fee_reservation(
                &mandate_accounting::ProspectiveOrder {
                    instrument: self.input.instrument.ledger_id.clone(),
                    asset_class: AssetClass::UsEquity,
                    side: Side::Buy,
                    qty: candidate.qty,
                    limit: candidate.limit,
                    at: t,
                },
                &self.config.fees,
            )?,
            Side::Sell => Usd::ZERO,
        };
        let proposed = mandate_risk::ProposedOrder {
            instrument: self.risk_asset.clone(),
            side: candidate.side,
            qty: candidate.qty,
            limit_price: candidate.limit,
            kind: candidate.kind.clone(),
            tif: mandate_risk::TimeInForce::Day,
            extended_hours: false,
            origin: candidate.origin,
            owner_confirmed_bid: None,
            client_order_id: mandate_risk::ClientOrderId(
                u64::try_from(self.orders.len()).map_err(|_| NumError::Overflow)?,
            ),
            fee_reservation,
        };
        let market = mandate_risk::MarketSnapshot {
            quote: Some(mandate_risk::SaneQuote {
                bid,
                ask,
                at: last.at,
            }),
            last_trade: Some((last.close, last.at)),
            trailing_5m_volume: self.trailing_volume()?,
            adv_20d: self.liquidity()?.map(|daily| daily.adv_20d),
        };
        let mandate = match self
            .risk
            .snapshot()
            .restrictions
            .contains(&risk::Restriction::GoalComplete)
        {
            true => gate::mandate_holding(self.input.mandate)?,
            false => self.gate_mandate.clone(),
        };
        Ok(mandate_risk::evaluate(&mandate_risk::GateInput {
            now: t,
            pass,
            config: &self.input.settings.gate,
            mandate: &mandate,
            risk: &gate::risk_snapshot(self.risk.snapshot())?,
            account: &self.account_snapshot(position, bid)?,
            agent: &self.agent_snapshot(position, bid)?,
            instrument: &self.instrument_snapshot(t)?,
            market: &market,
            conduct: &self.conduct()?,
            universe: &universe,
            proposed: &proposed,
        })?)
    }

    /// Hands one order to the fill model over the whole bar slice, as E4-2 does, and records it.
    fn submit(
        &mut self,
        decided_at: UtcNanos,
        order: SimOrder,
        protective: bool,
    ) -> Result<Working, BacktestError> {
        let index = u32::try_from(self.orders.len()).map_err(|_| NumError::Overflow)?;
        let outcome = simulate(
            &self.input.settings.sim,
            &self.input.instrument.instrument,
            self.input.bars,
            self.input.coverage_start,
            self.input.first_bar_volumes,
            &[order],
        )?;
        let end = outcome.end_of(OrderRef::new(0)).unwrap_or(OrderEnd::Open);
        let numbered = OrderRef::new(self.orders.len());
        self.orders.push(VersionOrder {
            index,
            decided_at,
            order,
            protective,
            end,
        });
        self.today.orders = self.today.orders.checked_add(1).ok_or(NumError::Overflow)?;
        Ok(Working {
            index,
            order,
            fills: outcome
                .fills
                .into_iter()
                .map(|fill| SimFill {
                    order: numbered,
                    ..fill
                })
                .collect(),
            folded: 0,
            end,
        })
    }

    /// Records the cancel of the resting protection before an agent order (trading spec §5.3):
    /// the protection is no longer folded from bar `index` on, and its recorded end is the cancel,
    /// after the last bar it worked through.
    fn canceled(&mut self, protection: &Working, index: usize) -> Result<(), BacktestError> {
        let slot = usize::try_from(protection.index).map_err(|_| NumError::Overflow)?;
        if let Some(order) = self.orders.get_mut(slot) {
            order.end = OrderEnd::Expired {
                at_bar: index.saturating_sub(1),
            };
        }
        Ok(())
    }

    /// Folds bar `index`'s fills of the agent order and of the protection, in that order.
    fn fold_fills(&mut self, index: usize, bar: &SimBar) -> Result<(), BacktestError> {
        let mut folded = Vec::new();
        if let Some(order) = self.working.as_mut() {
            folded.extend(
                order
                    .fold_bar(index, bar.start, self.config)?
                    .into_iter()
                    .map(|fill| (fill, false)),
            );
        }
        if let Some(protection) = self.protection.as_mut() {
            folded.extend(
                protection
                    .fold_bar(index, bar.start, self.config)?
                    .into_iter()
                    .map(|fill| (fill, true)),
            );
        }
        for (fill, protective) in folded {
            self.apply_fill(fill, protective)?;
        }
        let agent_ended = self
            .working
            .as_ref()
            .and_then(Working::stopped_in)
            .is_some_and(|stopped| stopped == index);
        if agent_ended
            && !self.position()?.is_zero()
            && self.input.mandate.mandate().protection.enabled
        {
            self.protection_pending = true;
        }
        if self.position()?.is_zero() {
            self.protection = None;
        }
        Ok(())
    }

    fn apply_fill(&mut self, fill: RunFill, protective: bool) -> Result<(), BacktestError> {
        let execution: &Execution = &fill.execution;
        self.account = self
            .account
            .apply(&Input::Fill(execution.clone()), &self.config.fees)?
            .account;
        let at = execution.executed_at;
        self.step(
            at,
            MarketSession::Regular,
            risk::Input::Fill {
                side: gate::domain_side(execution.side),
                qty: execution.qty_gross,
                price: execution.price,
            },
        )?;
        let notional = execution.qty_gross.notional(execution.price)?;
        match execution.side {
            Side::Buy => {
                self.today.bought_usd = self.today.bought_usd.checked_add(notional)?;
                self.goal_spent = self.goal_spent.checked_add(notional)?;
            }
            Side::Sell => self.last_exit_fill_at = Some(at),
        }
        self.has_prior_fill = true;
        self.today.filled_qty = self.today.filled_qty.checked_add(execution.qty_gross)?;
        let order_index = self
            .orders
            .iter()
            .position(|order| Some(order.index) == fill_order_index(execution))
            .and_then(|position| u32::try_from(position).ok());
        if let Some(order_index) = order_index {
            self.today.filled_orders.insert(order_index);
        }
        self.today.fills.push(AccountFill {
            at,
            instrument: self.risk_asset.clone(),
            asset_class: AssetClass::UsEquity,
            side: execution.side,
            qty: execution.qty_gross,
        });
        self.last_fill_by_side
            .insert(mandate_risk::RestingSide::from(execution.side), at);
        let _ = protective;
        self.fills.push(fill);
        Ok(())
    }

    /// Places the executor's protection on the whole position after an agent order ended (DEC-1000
    /// item 1.7): the bracket prices from the latest entry's limit, then `protective_shape`; none
    /// refuses the run.
    fn place_protection(&mut self, at: UtcNanos) -> Result<(), BacktestError> {
        self.protection_pending = false;
        let position = self.position()?;
        if position.is_zero() {
            return Ok(());
        }
        let protection = &self.input.mandate.mandate().protection;
        let entry = self
            .last_entry_limit
            .ok_or(BacktestError::ProtectionNotPlaceable)?;
        let stop_distance = protection
            .stop_distance
            .as_ref()
            .ok_or(BacktestError::ProtectionNotPlaceable)?;
        let take_profit = protection
            .take_profit_distance
            .as_ref()
            .map(|distance| Fraction::parse(distance.as_str()))
            .transpose()?;
        let prices: ProtectionPrices =
            equity_bracket_prices(entry, Fraction::parse(stop_distance.as_str())?, take_profit)?;
        let offset = protection
            .crypto_stop_limit_offset
            .as_ref()
            .map(|offset| Fraction::parse(offset.as_str()))
            .transpose()?;
        let shape = protective_shape(
            &self.input.settings.profile,
            AssetClass::UsEquity,
            MarketSession::Regular,
            position,
            prices,
            offset,
        )?
        .ok_or(BacktestError::ProtectionNotPlaceable)?;
        let kind = match (shape.oco, shape.stop_price, shape.limit_price) {
            (Some(legs), _, _) => OrderKind::Oco {
                limit: legs.take_profit,
                stop: legs.stop,
            },
            (None, Some(stop), Some(limit)) => OrderKind::StopLimit { stop, limit },
            _ => return Err(BacktestError::ProtectionNotPlaceable),
        };
        let order = SimOrder {
            side: Side::Sell,
            qty: position,
            kind,
            tif: TimeInForce::Gtc,
            extended_hours: false,
            eligible_from: Eligibility::DecidedAt {
                at,
                approval_required: false,
            },
        };
        self.protection = Some(self.submit(at, order, true)?);
        Ok(())
    }

    /// The bar's close: the ledger mark at the close, the risk mark at the bid (DEC-1000 item 7),
    /// the fee charges the close reaches, and the period observation when the bar closes one.
    fn close(
        &mut self,
        index: usize,
        bar: &SimBar,
        close_at: UtcNanos,
        closes_period: bool,
    ) -> Result<(), BacktestError> {
        self.account = self
            .account
            .apply(
                &Input::Mark {
                    instrument: self.input.instrument.ledger_id.clone(),
                    price: bar.close,
                },
                &self.config.fees,
            )?
            .account;
        if matches!(bar.session, Session::Regular) {
            let (bid, _) = self.quote(bar.close)?;
            self.step(
                close_at,
                MarketSession::Regular,
                risk::Input::Mark { bid, sane: true },
            )?;
            let regular_seconds = RegularSeconds {
                calendar: self.calendar,
            }
            .seconds_between(bar.start, close_at)?;
            self.last_close = Some(LastClose {
                close: bar.close,
                at: close_at,
                volume: bar.volume,
                regular_seconds,
            });
        }
        for day in crate::charges_due(bar, &self.dates, &mut self.charged, AssetClass::UsEquity)? {
            self.account = self
                .account
                .apply(
                    &Input::FeesCharged {
                        family: FeeFamily::Equities,
                        day,
                    },
                    &self.config.fees,
                )?
                .account;
        }
        if closes_period {
            let period = u32::try_from(self.equity.len())
                .map_err(|_| NumError::Overflow)?
                .checked_add(1)
                .ok_or(NumError::Overflow)?;
            let equity = self.account.equity()?;
            self.equity.push(Observation {
                period,
                date: bar.trade_date,
                bar: index,
                equity,
            });
            self.prior_close_equity = equity;
            let volume = self
                .input
                .bars
                .iter()
                .filter(|other| {
                    other.trade_date == bar.trade_date && matches!(other.session, Session::Regular)
                })
                .try_fold(Qty::ZERO, |sum, other| sum.checked_add(other.volume))?;
            self.periods.push(ClosedPeriod {
                close: bar.close,
                volume,
            });
        }
        Ok(())
    }

    /// The fills, fees and notionals the strategy's metric block is computed from (DEC-127 items
    /// 10, 11 and 22).
    fn totals(&self) -> Result<Totals, BacktestError> {
        let mut buy_notional = Usd::ZERO;
        let mut sell_notional = Usd::ZERO;
        let mut filled_qty = Qty::ZERO;
        for fill in &self.fills {
            let notional = fill.fill.qty.notional(fill.fill.price)?;
            match fill.execution.side {
                Side::Buy => buy_notional = buy_notional.checked_add(notional)?,
                Side::Sell => sell_notional = sell_notional.checked_add(notional)?,
            }
            filled_qty = filled_qty.checked_add(fill.fill.qty)?;
        }
        let mut submitted_qty = Qty::ZERO;
        for order in &self.orders {
            submitted_qty = submitted_qty.checked_add(order.order.qty)?;
        }
        Ok(Totals {
            buy_notional,
            sell_notional,
            fill_count: u32::try_from(self.fills.len()).map_err(|_| NumError::Overflow)?,
            fees_total: self.account.fees_total()?,
            fees_charged: self.account.fees_charged(),
            fees_accrued: self.account.fees_accrued()?,
            fees_asset: self.account.asset_fees(),
            submitted_qty,
            filled_qty,
        })
    }
}

/// The order index a run's execution names: `o<index>` (DEC-127 item 21).
fn fill_order_index(execution: &Execution) -> Option<u32> {
    execution
        .client_order_id
        .as_deref()
        .and_then(|id| id.strip_prefix('o'))
        .and_then(|digits| digits.parse().ok())
}

/// The fill model's half spread, the one spread the run's quotes and fills share.
pub(crate) fn half_spread(sim: &SimConfig) -> mandate_num::Bps {
    match sim.slippage {
        mandate_sim::Slippage::Fixed {
            half_spread_bps, ..
        }
        | mandate_sim::Slippage::Sqrt {
            half_spread_bps, ..
        } => half_spread_bps,
    }
}

/// `numerator / denominator` as decimal text exact to nine places, truncated: the share of a bar's
/// volume the trailing window holds.
fn ratio_text(numerator: u64, denominator: u64) -> Result<String, BacktestError> {
    let scaled = numerator
        .checked_mul(1_000_000_000)
        .and_then(|scaled| scaled.checked_div(denominator))
        .ok_or(NumError::Overflow)?;
    let whole = scaled
        .checked_div(1_000_000_000)
        .ok_or(NumError::Overflow)?;
    let fraction = scaled
        .checked_rem(1_000_000_000)
        .ok_or(NumError::Overflow)?;
    Ok(format!("{whole}.{fraction:09}"))
}
