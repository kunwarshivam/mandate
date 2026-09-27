//! The production adapters: one per [`Stage`](crate::Stage), each binding one crate's public API.
//!
//! The implementation lands one slice at a time (DEC-77 stage 3, DEC-166). **Live** in slice 1:
//! [`StoredBars`], [`MovingAverage`] and [`AlpacaConnector`], whose upstreams are fully implemented
//! and whose every input the shell holds. **Still refusing**, each with [`Cause::Unimplemented`]:
//! every other adapter, because its upstream is a stub or no production source exists for one of
//! its inputs, and the shell never invents one (DEC-166 item 2). The first of them, [`RiskExitPath`],
//! is also the first stage [`crate::run`] reaches, so a run over [`production`] still refuses at its
//! first probe and places no order.
//!
//! What each will bind, per the task brief's step table:
//!
//! | Adapter | Binds |
//! |---|---|
//! | [`RiskExitPath`] | `mandate_risk::agent_flatten` (E6-3) |
//! | [`ExecutorProtection`] | `mandate_executor::handle` on a synthetic protective input (E7-4) |
//! | [`SpecMandate`] | `mandate_spec::validate`, then `ValidatedMandate::new` (stream F) |
//! | [`StoredBars`] | `mandate_marketdata::dataset::read_manifest` and `dataset::read` (E2-1, E2-2) |
//! | [`MovingAverage`] | `mandate_backtest::Strategy::MovingAverageCrossover` (E4-2) |
//! | [`BuilderPlan`] | `mandate-builder`'s sizing and classification (stream H) |
//! | [`RiskGate`] | `mandate_risk::evaluate` as a dry run (E6-3, E6-6 to E6-8) |
//! | [`StoreJournal`] | `mandate_journal::MemoryJournal` in CI, `mandate-journal-pg` for the manual run |
//! | [`ExecutorSink`] | the two `IntentHandoff` types, joined with the deployment's `AgentId` |
//! | [`CoreExecutor`] | `mandate_executor::handle` and `fold` (stream K) |
//! | [`AlpacaConnector`] | `mandate_alpaca::TradingClient` over a `TradingTransport` (stream K) |
//! | [`ExecutorReconciler`] | `mandate_executor::reconcile` on the connector's snapshot (E7-3) |

use std::path::PathBuf;

use mandate_alpaca::{RetryPolicy, TokioPause, TradingClient, TradingTransport};
use mandate_backtest::{Signal, Strategy, StrategyConfig};
use mandate_executor::{BrokerConnector, BrokerOutcome, BrokerRequest, ConnectorError};
use mandate_journal::{AppendOutcome, StoredEvent};
use mandate_marketdata::dataset;
use mandate_marketdata::inspect::{self, ActionsReport, GapClass, Inspection};
use mandate_marketdata::model::{Kind, Records, TimeUnit, Timeframe};
use mandate_num::{Bps, Price, Usd};
use mandate_risk::Decision;
use mandate_runtime::{
    AgentId, Autonomy, FlattenPlan, FlattenRequest, IntentHandoff, MandateView, Proposal,
    SignalInputs,
};

use crate::error::Cause;
use crate::stages::{
    Admitted, Bars, Classifier, Connector, Executor, ExitPath, Gate, JournalWriter, MandateSource,
    ModelRef, Protection, Reconciled, Reconciler, SignalModel, Sink, Sizing, Stages,
};

/// `mandate_risk::agent_flatten`, probed once against a synthetic request before anything starts.
pub struct RiskExitPath;

impl ExitPath for RiskExitPath {
    fn probe(&self) -> Result<(), Cause> {
        Err(Cause::Unimplemented { story: "E7-7" })
    }

    fn plan(&self, request: &FlattenRequest) -> Result<FlattenPlan, Cause> {
        let _ = request;
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// The executor's protective sequence, probed through `mandate_executor::handle`, since
/// `mod protection` is private (task brief, Decisions needed 4).
pub struct ExecutorProtection;

impl Protection for ExecutorProtection {
    fn probe(&self) -> Result<(), Cause> {
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// The mandate document at `path`, validated with no violation.
pub struct SpecMandate {
    pub path: PathBuf,
}

impl MandateSource for SpecMandate {
    fn admitted(&self) -> Result<Admitted, Cause> {
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// The stored daily bars in the dataset directory `dir`, read through `mandate-marketdata`'s own
/// `inspect` and `dataset::read`. The closes are returned exactly as stored: the shell adjusts,
/// filters and repairs nothing, so coverage it cannot trust is a refusal, never a guess (PB-15).
pub struct StoredBars {
    pub dir: PathBuf,
}

impl Bars for StoredBars {
    fn closes(&self, symbol: &str) -> Result<Vec<Price>, Cause> {
        let inspection = inspect::inspect(&self.dir)?;
        trusted(&inspection, symbol)?;
        let (_, days) = dataset::read_manifest(&self.dir)?;
        let mut closes = Vec::new();
        for listed in days.iter().filter(|listed| listed.file.is_some()) {
            let path = self.dir.join(dataset::partition_name(listed.day));
            match dataset::read(&path, inspection.dataset.kind())? {
                Records::Bars(bars) => {
                    for bar in bars {
                        closes.push(Price::parse(bar.close.as_str())?);
                    }
                }
                Records::Trades(_) | Records::Quotes(_) => {
                    return Err(untrusted("the dataset holds no bars"));
                }
            }
        }
        Ok(closes)
    }
}

/// Whether `inspect` found the dataset fit to decide on: the pinned symbol's daily bars, every
/// partition intact, no trading day without its bar, and no split inside the span, whose prices
/// the shell may not adjust (DEC-138 item 3). `inspect`'s quality warnings are the vendor's
/// records as stored (DEC-89) and are not refusals.
fn trusted(inspection: &Inspection, symbol: &str) -> Result<(), Cause> {
    let daily = Kind::Bars(Timeframe::new(1, TimeUnit::Day)?);
    let refusal = if inspection.dataset.symbol().as_str() != symbol {
        Some("the dataset is another instrument's")
    } else if inspection.dataset.kind() != daily {
        Some("the dataset is not daily bars")
    } else if !inspection.problems.is_empty() {
        Some("a partition cannot be trusted")
    } else if !inspection.duplicates.is_empty() {
        Some("two bars share a start")
    } else if !inspection.coverage.empty.is_empty() {
        Some("a trading day has no bar")
    } else if inspection.gaps.iter().any(|gap| {
        gap.stretches
            .iter()
            .any(|stretch| matches!(stretch.class, GapClass::TrueGap | GapClass::Unclassified))
    }) {
        Some("a trading day was never fetched")
    } else if split_inside(inspection) {
        Some("a split lies inside the span, or the corporate actions do not cover it")
    } else {
        None
    };
    match refusal {
        Some(what) => Err(untrusted(what)),
        None => Ok(()),
    }
}

/// Whether a split could have moved a close inside the span. Actions recorded for only part of the
/// span say nothing about the rest, so they count as one.
fn split_inside(inspection: &Inspection) -> bool {
    match &inspection.corporate_actions {
        ActionsReport::NotApplicable | ActionsReport::NotRecorded => false,
        ActionsReport::Incomplete(_) => true,
        ActionsReport::Applied { recorded, .. } => match inspection.coverage.span {
            Some(span) => recorded
                .actions
                .splits
                .iter()
                .any(|split| split.ex_date > span.first() && split.ex_date <= span.last()),
            None => false,
        },
    }
}

fn untrusted(what: &'static str) -> Cause {
    Cause::Untrusted { what }
}

/// E4-2's moving-average baseline, the one model this adapter implements, with the windows the
/// mandate's envelope gives it (`AGENTS.md` rule 11): a model the envelope names differently, or
/// a window it does not state, is a refusal rather than a default.
pub struct MovingAverage;

/// The id the envelope gives E4-2's baseline (DEC-157 item 4).
pub const MOVING_AVERAGE: &str = "quant.ma_crossover";

impl SignalModel for MovingAverage {
    fn signal(&self, model: &ModelRef, closes: &[Price]) -> Result<Signal, Cause> {
        if model.id != MOVING_AVERAGE {
            return Err(Cause::Absent {
                what: "the adapter implements only quant.ma_crossover",
            });
        }
        let strategy = Strategy::MovingAverageCrossover(StrategyConfig {
            fast_periods: window(model, "fast_periods")?,
            slow_periods: window(model, "slow_periods")?,
            collar: Bps::ZERO,
            target_notional: Usd::ZERO,
        });
        Ok(strategy.signal(closes)?)
    }
}

/// One window, as the envelope states it. `Strategy::signal` reads only the two windows; the
/// collar and the target notional above price and size a backtest's orders, which the tracer
/// takes from the order builder instead.
fn window(model: &ModelRef, key: &'static str) -> Result<u32, Cause> {
    match model.params.get(key).map(|value| value.parse::<u32>()) {
        Some(Ok(periods)) => Ok(periods),
        Some(Err(_)) | None => Err(Cause::Absent {
            what: "the model's params do not state its windows",
        }),
    }
}

/// `mandate-builder`'s `conviction_linear` sizing and §6 classification.
pub struct BuilderPlan;

impl Sizing for BuilderPlan {
    fn size(&self, view: &MandateView, inputs: &SignalInputs) -> Result<Option<Proposal>, Cause> {
        let _ = (view, inputs);
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

impl Classifier for BuilderPlan {
    fn classify(&self, view: &MandateView, proposal: &Proposal) -> Result<Autonomy, Cause> {
        let _ = (view, proposal);
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// `mandate_risk::evaluate`, as the runtime's advisory pass. The binding call stays inside
/// `mandate-executor` (`AGENTS.md` rules 1 and 12).
pub struct RiskGate;

impl Gate for RiskGate {
    fn evaluate(&self, proposal: &Proposal) -> Result<Decision, Cause> {
        let _ = proposal;
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// The journal store: `mandate_journal::MemoryJournal` for CI and a scratch run,
/// `mandate-journal-pg` at the DSN for the manual paper run.
pub struct StoreJournal {
    pub dsn: Option<String>,
}

impl JournalWriter for StoreJournal {
    fn take_ownership(&mut self, stream: &str) -> Result<u64, Cause> {
        let _ = stream;
        Err(Cause::Unimplemented { story: "E7-7" })
    }

    fn read(&self, stream: &str) -> Result<Vec<StoredEvent>, Cause> {
        let _ = stream;
        Err(Cause::Unimplemented { story: "E7-7" })
    }

    fn append(
        &mut self,
        stream: &str,
        expected_head: u64,
        writer_epoch: u64,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        let _ = (stream, expected_head, writer_epoch, drafts);
        AppendOutcome::Unavailable
    }
}

/// The runtime's `IntentSink`, handing to the executor with this deployment's `AgentId`.
pub struct ExecutorSink {
    pub agent: AgentId,
}

impl Sink for ExecutorSink {
    fn hand(&mut self, handoff: &IntentHandoff) -> Result<mandate_executor::IntentHandoff, Cause> {
        let _ = handoff;
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// `mandate_executor::handle` and `fold` over the account stream's state.
pub struct CoreExecutor;

impl Executor for CoreExecutor {
    fn step(
        &mut self,
        input: mandate_executor::Input,
    ) -> Result<Vec<mandate_executor::Effect>, Cause> {
        let _ = input;
        Err(Cause::Unimplemented { story: "E7-7" })
    }

    fn committed(&mut self, event: &mandate_executor::FoldedEvent) -> Result<(), Cause> {
        let _ = event;
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// `mandate_alpaca::TradingClient` over `transport`: the scripted one in CI, `AlpacaPaperHttp` for
/// the manual run. The shell names no host and builds no URL (TI-5): every request is one of the
/// client's own, sent through the transport it is given.
///
/// `BrokerConnector::call` is async and the stage is not, so the shell owns the runtime that drives
/// it (task brief step 13): one current-thread tokio runtime per call, which holds nothing between
/// calls, as the client holds nothing a restart would lose.
pub struct AlpacaConnector<T> {
    pub transport: T,
}

impl<T: TradingTransport + Clone> Connector for AlpacaConnector<T> {
    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .map_err(|_| ConnectorError::NotSent {
                code: "runtime_unavailable",
            })?;
        let mut client =
            TradingClient::new(self.transport.clone(), TokioPause, RetryPolicy::default());
        runtime.block_on(client.call(request))
    }
}

/// The connector of a process that holds no transport: every request is refused before it leaves
/// the process. The binary uses it until the slice that makes the exit probes real, since every run
/// refuses at the first probe before any request (DEC-166 item 3).
pub struct Disconnected;

impl Connector for Disconnected {
    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        let _ = request;
        Err(ConnectorError::NotSent {
            code: "no_transport",
        })
    }
}

/// `mandate_executor::reconcile` on the broker snapshot the connector reads at startup.
pub struct ExecutorReconciler;

impl Reconciler for ExecutorReconciler {
    fn reconcile(&mut self) -> Result<Reconciled, Cause> {
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// Where the production stages read from.
pub struct Sources<T> {
    pub mandate: PathBuf,
    pub dataset: PathBuf,
    /// `None` is a scratch in-memory journal: a run that places nothing keeps nothing, so a later
    /// start cannot re-hand an intent a planning run proposed (DEC-157 item 6).
    pub journal: Option<String>,
    pub agent: AgentId,
    pub transport: T,
}

/// The production stages, with the Alpaca paper client over `sources.transport`.
pub fn production<T: TradingTransport + Clone + 'static>(sources: Sources<T>) -> Stages {
    let Sources {
        mandate,
        dataset,
        journal,
        agent,
        transport,
    } = sources;
    over(Sources {
        mandate,
        dataset,
        journal,
        agent,
        transport: Box::new(AlpacaConnector { transport }),
    })
}

/// The production stages over the given connector: the binary's [`Disconnected`] one, or
/// [`production`]'s.
pub fn over(sources: Sources<Box<dyn Connector>>) -> Stages {
    Stages {
        exit: Box::new(RiskExitPath),
        protection: Box::new(ExecutorProtection),
        mandate: Box::new(SpecMandate {
            path: sources.mandate,
        }),
        bars: Box::new(StoredBars {
            dir: sources.dataset,
        }),
        signal: Box::new(MovingAverage),
        reconciler: Box::new(ExecutorReconciler),
        sizing: Box::new(BuilderPlan),
        classifier: Box::new(BuilderPlan),
        gate: Box::new(RiskGate),
        journal: Box::new(StoreJournal {
            dsn: sources.journal,
        }),
        sink: Box::new(ExecutorSink {
            agent: sources.agent,
        }),
        executor: Box::new(CoreExecutor),
        connector: sources.transport,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    use mandate_backtest::{BacktestError, Signal};
    use mandate_canon::DecStr;
    use mandate_marketdata::actions::{RecordedActions, write_actions};
    use mandate_marketdata::dataset::{Store, partition_name};
    use mandate_marketdata::inspect::{self, ActionsReport};
    use mandate_marketdata::model::{
        AssetClass, Bar, CorporateActions, DatasetId, DayRange, Feed, Kind, Records, Split,
        SplitRatio, Symbol,
    };
    use mandate_num::Price;
    use mandate_time::{Date, UtcNanos};

    use super::{MovingAverage, StoredBars, split_inside, trusted};
    use crate::error::Cause;
    use crate::stages::{Bars, ModelRef, SignalModel};

    /// A scratch directory, removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Result<Scratch, String> {
            let path = std::env::temp_dir().join(format!(
                "mandate-shell-adapters-{name}-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).map_err(|e| e.to_string())?;
            Ok(Scratch(path))
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn day(text: &str) -> Result<Date, String> {
        Date::parse(text).map_err(|e| e.to_string())
    }

    fn dataset(symbol: &str, timeframe: &str) -> Result<DatasetId, String> {
        DatasetId::new(
            AssetClass::UsEquity,
            Feed::Iex,
            Kind::Bars(timeframe.parse().map_err(|e| format!("{e:?}"))?),
            Symbol::parse(symbol).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }

    fn bar(on: Date, close: &str) -> Result<Bar, String> {
        let dec = |text: &str| DecStr::parse(text).map_err(|e| e.to_string());
        Ok(Bar {
            start: UtcNanos::parse_rfc3339(&format!("{on}T04:00:00Z"))
                .map_err(|e| e.to_string())?,
            open: dec(close)?,
            high: dec(close)?,
            low: dec(close)?,
            close: dec(close)?,
            volume: dec("50000000")?,
            vwap: dec(close)?,
            trade_count: 400_000,
        })
    }

    /// One daily bar per weekday from 2026-08-24, a Monday, with these closes; `skip` names
    /// weekdays given no partition at all, and `empty` weekdays listed with no bar.
    fn write(
        root: &Path,
        id: &DatasetId,
        closes: &[&str],
        skip: &[&str],
        empty: &[&str],
    ) -> Result<PathBuf, String> {
        let store = Store::new(root);
        let mut on = day("2026-08-24")?;
        for close in closes {
            while on.is_weekend()
                || skip.contains(&on.to_string().as_str())
                || empty.contains(&on.to_string().as_str())
            {
                if empty.contains(&on.to_string().as_str()) {
                    store
                        .put_day(id, on, &Records::Bars(Vec::new()))
                        .map_err(|e| e.to_string())?;
                }
                on = on.next().map_err(|e| e.to_string())?;
            }
            store
                .put_day(id, on, &Records::Bars(vec![bar(on, close)?]))
                .map_err(|e| e.to_string())?;
            on = on.next().map_err(|e| e.to_string())?;
        }
        Ok(store.dataset_dir(id))
    }

    fn rising() -> Vec<String> {
        (231..256).map(|whole| format!("{whole}.20")).collect()
    }

    fn aapl(root: &Path, skip: &[&str], empty: &[&str]) -> Result<PathBuf, String> {
        let closes = rising();
        let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
        write(root, &dataset("AAPL", "1Day")?, &closes, skip, empty)
    }

    fn untrusted(answer: Result<Vec<Price>, Cause>) -> Result<&'static str, String> {
        match answer {
            Err(Cause::Untrusted { what }) => Ok(what),
            other => Err(format!("expected an untrusted refusal, got {other:?}")),
        }
    }

    #[test]
    fn stored_closes_are_read_oldest_first_exactly_as_stored() -> Result<(), String> {
        let scratch = Scratch::new("read")?;
        let dir = aapl(&scratch.0, &[], &[])?;
        let saturday = day("2026-08-29")?;
        Store::new(&scratch.0)
            .put_day(
                &dataset("AAPL", "1Day")?,
                saturday,
                &Records::Bars(Vec::new()),
            )
            .map_err(|e| e.to_string())?;
        let closes = StoredBars { dir }
            .closes("AAPL")
            .map_err(|e| e.to_string())?;
        let expected: Vec<String> = rising()
            .iter()
            .map(|c| c.trim_end_matches('0').to_owned())
            .collect();
        let read: Vec<String> = closes.iter().map(ToString::to_string).collect();
        assert_eq!(read, expected);
        Ok(())
    }

    #[test]
    fn another_instruments_bars_are_refused() -> Result<(), String> {
        let scratch = Scratch::new("symbol")?;
        let dir = aapl(&scratch.0, &[], &[])?;
        let what = untrusted(StoredBars { dir }.closes("MSFT"))?;
        assert_eq!(what, "the dataset is another instrument's");
        Ok(())
    }

    #[test]
    fn bars_that_are_not_daily_are_refused() -> Result<(), String> {
        let scratch = Scratch::new("hourly")?;
        let dir = write(
            &scratch.0,
            &dataset("AAPL", "1Hour")?,
            &["231.20"],
            &[],
            &[],
        )?;
        let what = untrusted(StoredBars { dir }.closes("AAPL"))?;
        assert_eq!(what, "the dataset is not daily bars");
        Ok(())
    }

    #[test]
    fn an_altered_partition_is_refused() -> Result<(), String> {
        let scratch = Scratch::new("altered")?;
        let dir = aapl(&scratch.0, &[], &[])?;
        let path = dir.join(partition_name(day("2026-08-26")?));
        let mut bytes = fs::read(&path).map_err(|e| e.to_string())?;
        bytes.push(0);
        fs::write(&path, bytes).map_err(|e| e.to_string())?;
        let what = untrusted(StoredBars { dir }.closes("AAPL"))?;
        assert_eq!(what, "a partition cannot be trusted");
        Ok(())
    }

    #[test]
    fn two_bars_with_one_start_are_refused() -> Result<(), String> {
        let scratch = Scratch::new("duplicate")?;
        let dir = aapl(&scratch.0, &["2026-08-26"], &[])?;
        let on = day("2026-08-26")?;
        Store::new(&scratch.0)
            .put_day(
                &dataset("AAPL", "1Day")?,
                on,
                &Records::Bars(vec![bar(on, "233.20")?, bar(on, "233.30")?]),
            )
            .map_err(|e| e.to_string())?;
        let what = untrusted(StoredBars { dir }.closes("AAPL"))?;
        assert_eq!(what, "two bars share a start");
        Ok(())
    }

    #[test]
    fn a_trading_day_listed_without_a_bar_is_refused() -> Result<(), String> {
        let scratch = Scratch::new("empty")?;
        let dir = aapl(&scratch.0, &[], &["2026-08-26"])?;
        let what = untrusted(StoredBars { dir }.closes("AAPL"))?;
        assert_eq!(what, "a trading day has no bar");
        Ok(())
    }

    #[test]
    fn a_trading_day_never_fetched_is_refused() -> Result<(), String> {
        let scratch = Scratch::new("gap")?;
        let dir = aapl(&scratch.0, &["2026-08-26"], &[])?;
        let what = untrusted(StoredBars { dir }.closes("AAPL"))?;
        assert_eq!(what, "a trading day was never fetched");
        Ok(())
    }

    #[test]
    fn a_split_inside_the_span_or_actions_short_of_it_are_refused() -> Result<(), String> {
        let scratch = Scratch::new("split")?;
        let dir = aapl(&scratch.0, &[], &[])?;
        let mut inspection = inspect::inspect(&dir).map_err(|e| e.to_string())?;
        assert!(trusted(&inspection, "AAPL").is_ok());
        let span = inspection.coverage.span.ok_or("no span")?;
        let symbol = Symbol::parse("AAPL").map_err(|e| e.to_string())?;
        let split = |ex_date: Date| -> Result<Split, String> {
            Ok(Split {
                id: "split".to_owned(),
                ex_date,
                ratio: SplitRatio::new(4, 1).map_err(|e| e.to_string())?,
            })
        };
        let with = |splits: Vec<Split>| RecordedActions {
            range: span,
            actions: CorporateActions {
                splits,
                ..CorporateActions::none(symbol.clone())
            },
        };
        let cases = [
            (span.first(), false),
            (span.first().next().map_err(|e| e.to_string())?, true),
            (span.last(), true),
            (span.last().next().map_err(|e| e.to_string())?, false),
        ];
        for (ex_date, refused) in cases {
            inspection.corporate_actions = ActionsReport::Applied {
                recorded: with(vec![split(ex_date)?]),
                as_of: span.last(),
            };
            assert_eq!(split_inside(&inspection), refused, "{ex_date}");
        }
        inspection.corporate_actions = ActionsReport::Applied {
            recorded: with(Vec::new()),
            as_of: span.last(),
        };
        assert!(!split_inside(&inspection));
        inspection.corporate_actions = ActionsReport::Incomplete(with(Vec::new()));
        assert!(split_inside(&inspection));
        assert!(matches!(
            trusted(&inspection, "AAPL"),
            Err(Cause::Untrusted {
                what: "a split lies inside the span, or the corporate actions do not cover it"
            })
        ));
        inspection.corporate_actions = ActionsReport::Applied {
            recorded: with(vec![split(span.last())?]),
            as_of: span.last(),
        };
        inspection.coverage.span = None;
        assert!(!split_inside(&inspection));
        Ok(())
    }

    #[test]
    fn stored_actions_covering_the_span_with_no_split_are_trusted() -> Result<(), String> {
        let scratch = Scratch::new("actions")?;
        let dir = aapl(&scratch.0, &[], &[])?;
        let span =
            DayRange::new(day("2026-08-24")?, day("2026-09-25")?).map_err(|e| e.to_string())?;
        let symbol = Symbol::parse("AAPL").map_err(|e| e.to_string())?;
        write_actions(
            &dir,
            &RecordedActions {
                range: span,
                actions: CorporateActions::none(symbol),
            },
        )
        .map_err(|e| e.to_string())?;
        let closes = StoredBars { dir }
            .closes("AAPL")
            .map_err(|e| e.to_string())?;
        assert_eq!(closes.len(), 25);
        Ok(())
    }

    fn model(id: &str, params: &[(&str, &str)]) -> ModelRef {
        ModelRef {
            id: id.to_owned(),
            version: "1.0.0".to_owned(),
            max_output_age_s: 86_400,
            params: params
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    /// The closes as prices, in canonical text (`Price` refuses a trailing zero).
    fn prices(closes: &[String]) -> Result<Vec<Price>, String> {
        closes
            .iter()
            .map(|close| Price::parse(close.trim_end_matches('0')).map_err(|e| e.to_string()))
            .collect()
    }

    #[test]
    fn the_crossover_reads_the_windows_the_envelope_states() -> Result<(), String> {
        let windows = [("fast_periods", "5"), ("slow_periods", "20")];
        let crossover = model("quant.ma_crossover", &windows);
        let up = prices(&rising())?;
        let mut down = up.clone();
        down.reverse();
        let answer = |closes: &[Price]| MovingAverage.signal(&crossover, closes);
        assert!(matches!(answer(&up), Ok(Signal::Long)), "{:?}", answer(&up));
        assert!(matches!(answer(&down), Ok(Signal::Flat)));
        let short = up.get(..19).ok_or("closes")?;
        assert!(matches!(answer(short), Ok(Signal::Undecided)));
        let exactly = up.get(..20).ok_or("closes")?;
        assert!(matches!(answer(exactly), Ok(Signal::Long)));
        let crossed = model(
            "quant.ma_crossover",
            &[("fast_periods", "20"), ("slow_periods", "5")],
        );
        assert!(matches!(
            MovingAverage.signal(&crossed, &up),
            Err(Cause::Backtest(BacktestError::StrategyWindowsCrossed))
        ));
        Ok(())
    }

    #[test]
    fn another_model_or_an_unstated_window_is_refused() -> Result<(), String> {
        let up = prices(&rising())?;
        let refused = |model: ModelRef| {
            matches!(MovingAverage.signal(&model, &up), Err(Cause::Absent { .. }))
        };
        let windows = [("fast_periods", "5"), ("slow_periods", "20")];
        assert!(refused(model("quant.other", &windows)));
        assert!(refused(model(
            "quant.ma_crossover",
            &[("slow_periods", "20")]
        )));
        assert!(refused(model(
            "quant.ma_crossover",
            &[("fast_periods", "5")]
        )));
        assert!(refused(model(
            "quant.ma_crossover",
            &[("fast_periods", "five"), ("slow_periods", "20")]
        )));
        Ok(())
    }
}
