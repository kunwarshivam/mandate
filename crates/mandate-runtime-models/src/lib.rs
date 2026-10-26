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
//! The deployed runtime's model composer ([DEC-2383](../../../docs/project/decisions/DEC-2383.md)
//! item 7, [DEC-503](../../../docs/project/decisions/DEC-503.md) item 2): `mandate-modelhost`
//! behind `mandate-daemon`'s [`Models`] port, as `mandate-paper` composes the host for the
//! founder's paper run. It sits above both, so the daemon never runs a model and the host never
//! reaches the journal; the `mandate-daemon` binary in this crate links it.
//!
//! [`PinnedModels`] answers from data already held, with no network call (DEC-2383 item 5): the
//! confirmed mandate's pins of its `quant.` models on its pinned US equities, the registry folded
//! from the control stream's model registrations ([`registry`]), `ExchangeCalendar::us_equities()`,
//! and the daily closes `mandate_bars::trusted_daily_closes` admits at the tick, the one trusted
//! reader of the stored bars (DEC-2385, DEC-2274). The tick's own risk clock is the host's `now`,
//! so the same tick over the same write-once partitions gives the same answer. A version's `Long`
//! output and its exit output (`quant.ma_crossover` 1.1.0's `Flat`, conviction −1, DEC-2390) are
//! both handed on. Every refusal, a closes read the trust check refuses, a signal the pinned
//! version answers with no output, and a pin the registry or the host disagrees with, is no
//! output, which can only shrink a buy (MI-10). No `llm.` model is ever evaluated here: LLM
//! opinions reach the core through the research pass.
//!
//! An unpinned research mandate pins its `quant.` models on its working universe instead
//! ([DEC-2827](../../../docs/project/decisions/DEC-2827.md)), read at each tick from the agent
//! stream's thesis records with the executor's own replay: a `Long` only on the one instrument a
//! surviving thesis holds, an exit on any instrument a record admitted, each instrument's symbol its
//! own registered snapshot's (DEC-2819).
//!
//! The runtime mounts the artifact store read-only, so [`Held`] stores nothing: it only confirms
//! the data process already holds the closes object an observation will name (DEC-2385 item 5).

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use mandate_accounting::InstrumentId;
use mandate_bars::{closes_object, daily_dir, trusted_daily_closes};
use mandate_canon::{Value, parse};
use mandate_daemon::models::{Evaluated, Models, Streams};
use mandate_daemon::{ArtifactSink, ModelsWiring};
use mandate_domain::AssetClass;
use mandate_journal::{ArtifactRef, ArtifactSource, Environment, StoredEvent, get_artifact};
use mandate_modelhost::{DailyCloses, Evaluation, Pin, evaluate};
use mandate_research::{ReplayError, ThesisRecord, replay_theses};
use mandate_runtime::RiskClock;
use mandate_spec::context::JournaledFact;
use mandate_spec::document::{Mandate, ModelId, SignalModel};
use mandate_spec::validate::RegisteredModel;
use mandate_time::{ExchangeCalendar, UtcNanos};

/// The last component of the bars root the cell's data process writes its recording under
/// (`mandate-datad`'s `RECORDED_ROOT`, DEC-2385 item 4).
pub const RECORDING: &str = "cell-recorded";

/// The observation `source` of the recording's closes (DEC-2385 item 4).
pub const RECORDED_SOURCE: &str = "cell_recorded_daily_bars";

/// The registered signal models by id, as V-007 and the host read them.
pub type Registry = BTreeMap<ModelId, RegisteredModel>;

/// The model registrations of `environment` on the workspace's `control` rows, folded in journal
/// order as the validation context folds them (`JournaledFact::ModelRegistered`): the latest
/// registration of an id is its terms. `None` when any registration's row does not read, which is
/// never skipped for an older one, so no model is evaluated against terms it may no longer have.
pub fn registry(control: &[StoredEvent], environment: Environment) -> Option<Registry> {
    let mut registry = Registry::new();
    for row in control.iter().filter(|row| {
        row.environment == environment.as_str() && row.event_type == "ConfigSnapshotRegistered"
    }) {
        let body = parse(&row.body).ok()?;
        let payload = body.get("payload")?;
        let fact = JournaledFact::from_record(&row.event_type, payload, &|_| None, None).ok()?;
        if let Some(JournaledFact::ModelRegistered { id, model }) = fact {
            registry.insert(id, model);
        }
    }
    Some(registry)
}

/// The observation source of the closes stored under `bars`: only the cell's recording is named
/// (DEC-2385 item 7), so any other root gives none.
pub fn source(bars: &Path) -> Option<&'static str> {
    (bars.file_name()? == OsStr::new(RECORDING)).then_some(RECORDED_SOURCE)
}

/// One pin the composer evaluates, with the symbol its daily dataset is stored under.
struct Pinned {
    pin: Pin,
    symbol: String,
}

/// What the composer pins its quant models on.
enum Pins {
    /// A pinned mandate's instruments, fixed by its confirmed version.
    Pinned(Vec<Pinned>),
    /// An unpinned mandate's working universe, read at each tick (DEC-2827).
    Working(Working),
}

/// A research deployment's quant models, and the store its instruments' registered snapshots are
/// read from.
struct Working {
    models: Vec<SignalModel>,
    artifacts: Box<dyn ArtifactSource>,
}

/// The deployment's pinned quant models over the stored daily closes.
pub struct PinnedModels {
    pins: Pins,
    registry: Registry,
    calendar: ExchangeCalendar,
    bars: PathBuf,
    source: &'static str,
}

impl PinnedModels {
    /// Every `quant.` model of the confirmed mandate `document` on each of its pinned US
    /// equities, in the document's order, against `registry` and the closes under `bars`. `None`
    /// when the document does not parse, `bars` is not a root [`source`] names, the calendar is
    /// unavailable, or nothing is pinned to evaluate.
    pub fn new(document: &Value, registry: Registry, bars: &Path) -> Option<Self> {
        let mandate = Mandate::parse(document).ok()?;
        let source = source(bars)?;
        let calendar = ExchangeCalendar::us_equities().ok()?;
        let pinned: Vec<Pinned> = mandate
            .behavior
            .signal_models
            .iter()
            .filter(|model| model.id.as_str().starts_with("quant."))
            .flat_map(|model| {
                mandate
                    .universe
                    .pinned_instruments
                    .iter()
                    .filter(|instrument| instrument.asset_class == AssetClass::UsEquity)
                    .filter_map(move |instrument| {
                        Some(Pinned {
                            pin: Pin {
                                model: model.clone(),
                                instrument_id: InstrumentId::new(instrument.asset_id.as_str())
                                    .ok()?,
                            },
                            symbol: instrument.symbol.clone(),
                        })
                    })
            })
            .collect();
        (!pinned.is_empty()).then(|| Self {
            pins: Pins::Pinned(pinned),
            registry,
            calendar,
            bars: bars.to_path_buf(),
            source,
        })
    }

    /// Every `quant.` model of the confirmed unpinned mandate `document`, pinned at each tick on
    /// its working universe as the executor reads it (DEC-2827): a `Long` only on the one instrument
    /// a surviving thesis holds, an exit on any instrument a thesis record admitted, each with the
    /// symbol of its own latest snapshot registered on the control stream and read from
    /// `artifacts` (DEC-2819). `None` for a pinned mandate, one with no `quant.` model or no model that admits
    /// instruments, or as [`Self::new`].
    pub fn working(
        document: &Value,
        registry: Registry,
        bars: &Path,
        artifacts: Box<dyn ArtifactSource>,
    ) -> Option<Self> {
        let mandate = Mandate::parse(document).ok()?;
        if mandate.universe.pinned {
            return None;
        }
        let source = source(bars)?;
        let calendar = ExchangeCalendar::us_equities().ok()?;
        let signal_models = &mandate.behavior.signal_models;
        if !signal_models.iter().any(|model| model.admits_instruments) {
            return None;
        }
        let models: Vec<SignalModel> = signal_models
            .iter()
            .filter(|model| model.id.as_str().starts_with("quant."))
            .cloned()
            .collect();
        (!models.is_empty()).then(|| Self {
            pins: Pins::Working(Working { models, artifacts }),
            registry,
            calendar,
            bars: bars.to_path_buf(),
            source,
        })
    }

    /// `pinned`'s output at `now`, or none for any refusal.
    fn one(&self, pinned: &Pinned, now: UtcNanos) -> Option<Evaluated> {
        self.said(pinned, now).map(|(evaluated, _)| evaluated)
    }

    /// `pinned`'s output at `now` and whether it is an exit, or none for any refusal.
    fn said(&self, pinned: &Pinned, now: UtcNanos) -> Option<(Evaluated, bool)> {
        let dir = daily_dir(&self.bars, &pinned.symbol).ok()?;
        let daily = DailyCloses {
            instrument_id: pinned.pin.instrument_id.clone(),
            closes: trusted_daily_closes(&dir, &pinned.symbol, now).ok()?,
        };
        let (output, exit) =
            match evaluate(&pinned.pin, &self.registry, &self.calendar, &daily, now).ok()? {
                Evaluation::Long(output) => (output, false),
                Evaluation::Exit(output) => (output, true),
                Evaluation::NoOutput(_) => return None,
            };
        let closes = closes_object(&daily.closes, pinned.pin.instrument_id.as_str()).ok()?;
        Some((
            Evaluated {
                output: *output,
                closes,
                source: self.source.to_owned(),
            },
            exit,
        ))
    }
}

impl Models for PinnedModels {
    /// Each pin's output at the tick `at`, in order; a pin that gives none is left out. A research
    /// deployment's pins are read from `streams` at `at`, and only the outputs its working universe
    /// lets through are kept.
    fn evaluate(&self, at: RiskClock, streams: Streams<'_>) -> Vec<Evaluated> {
        let Ok(now) = UtcNanos::from_parts(at.secs(), 0) else {
            return Vec::new();
        };
        match &self.pins {
            Pins::Pinned(pinned) => pinned
                .iter()
                .filter_map(|pinned| self.one(pinned, now))
                .collect(),
            Pins::Working(working) => self.working_outputs(working, streams, now),
        }
    }
}

impl PinnedModels {
    /// The research deployment's outputs at `now`: each quant model on each instrument its thesis
    /// records ever admitted, in instrument order, its `Long` kept only on the instrument it may
    /// open in and its exit kept on any. Control rows that do not read give none.
    fn working_outputs(
        &self,
        working: &Working,
        streams: Streams<'_>,
        now: UtcNanos,
    ) -> Vec<Evaluated> {
        let Some(control) = streams.control else {
            return Vec::new();
        };
        let Some(symbols) = symbols(control, working.artifacts.as_ref()) else {
            return Vec::new();
        };
        let open = if every_row_reads(control) {
            openable(streams.agent, now)
        } else {
            None
        };
        let mut outputs = Vec::new();
        for instrument in ever_admitted(streams.agent) {
            let (Some(symbol), Ok(instrument_id)) =
                (symbols.get(&instrument), InstrumentId::new(&instrument))
            else {
                continue;
            };
            let may_open = open.as_deref() == Some(instrument.as_str());
            for model in &working.models {
                let pinned = Pinned {
                    pin: Pin {
                        model: model.clone(),
                        instrument_id: instrument_id.clone(),
                    },
                    symbol: symbol.clone(),
                };
                if let Some((evaluated, exit)) = self.said(&pinned, now)
                    && (exit || may_open)
                {
                    outputs.push(evaluated);
                }
            }
        }
        outputs
    }
}

/// Whether every control row reads as a record with a payload: one that does not holds the
/// executor's openings, so it holds every `Long` here too.
fn every_row_reads(control: &[StoredEvent]) -> bool {
    control.iter().all(|row| {
        parse(&row.body)
            .ok()
            .is_some_and(|body| body.get("payload").is_some())
    })
}

/// The symbol of each instrument by asset id, from the latest `instrument_snapshot` registered on
/// the control rows that names it, its object read from `artifacts` (DEC-523, DEC-2819 item 3).
/// `None` when a snapshot's registration or object does not read, or names no instrument or
/// symbol, which is never passed over for an earlier one. The snapshot's other DEC-523 members are
/// the executor's to check before any opening.
fn symbols(
    control: &[StoredEvent],
    artifacts: &dyn ArtifactSource,
) -> Option<BTreeMap<String, String>> {
    let mut symbols = BTreeMap::new();
    for row in control
        .iter()
        .filter(|row| row.event_type == "ConfigSnapshotRegistered")
    {
        let body = parse(&row.body).ok()?;
        let payload = body.get("payload")?;
        if payload.get("kind").and_then(Value::as_str) != Some("instrument_snapshot") {
            continue;
        }
        let reference = ArtifactRef::parse(payload.get("content_hash")?.as_str()?)?;
        let object = parse(&get_artifact(artifacts, &reference).ok()?).ok()?;
        let text = |member: &str| Some(object.get(member)?.as_str()?.to_owned());
        symbols.insert(text("instrument_id")?, text("symbol")?);
    }
    Some(symbols)
}

/// The thesis records of the agent stream's `rows`, in stream order, each one that cannot be read
/// an `Err` the replay stops at.
fn thesis_records(
    rows: &[StoredEvent],
) -> impl Iterator<Item = Result<ThesisRecord<'_>, ReplayError>> {
    rows.iter()
        .filter(|row| {
            matches!(
                row.event_type.as_str(),
                "ThesisProposed" | "ThesisRevised" | "ThesisInvalidated"
            )
        })
        .map(|row| {
            let payload = parse(&row.body)
                .ok()
                .and_then(|body| body.get("payload").cloned())
                .ok_or(ReplayError::Unreadable { member: "payload" })?;
            Ok(ThesisRecord {
                event_type: row.event_type.as_str(),
                payload,
            })
        })
}

/// The instrument a research agent may open in at `now`, as the executor reads it: the one a
/// surviving thesis holds, when exactly one does and every record replays; else none.
fn openable(rows: &[StoredEvent], now: UtcNanos) -> Option<String> {
    let replayed = replay_theses(thesis_records(rows), now).ok()?;
    let [entry] = replayed.active.as_slice() else {
        return None;
    };
    Some(entry.instrument.as_str().to_owned())
}

/// Every instrument a thesis record on the agent stream's `rows` admitted, as the executor maps a
/// held position's exits: each `ThesisProposed` or `ThesisRevised` with `admitted` true, whatever
/// any other record says, so a record that cannot be read takes no instrument from an exit
/// (`AGENTS.md` rule 13).
fn ever_admitted(rows: &[StoredEvent]) -> BTreeSet<String> {
    rows.iter()
        .filter(|row| matches!(row.event_type.as_str(), "ThesisProposed" | "ThesisRevised"))
        .filter_map(|row| {
            let body = parse(&row.body).ok()?;
            let payload = body.get("payload")?;
            if payload.get("admitted") != Some(&Value::Bool(true)) {
                return None;
            }
            Some(payload.get("instrument_id")?.as_str()?.to_owned())
        })
        .collect()
}

/// The artifact store as the read-only runtime sees it: a put stores nothing and succeeds only when
/// the store already holds those exact bytes, checked against their digest (DEC-2385 item 5).
pub struct Held<A>(pub A);

impl<A: ArtifactSource> ArtifactSink for Held<A> {
    fn put(&mut self, bytes: &[u8]) -> bool {
        get_artifact(&self.0, &ArtifactRef::of(bytes)).is_ok()
    }
}

/// The models wiring of a resolved deployment: [`PinnedModels`] for the confirmed mandate
/// `document` against the `environment`'s registrations on `control`, the closes under `bars`
/// (the process's `MANDATE_BARS`), and [`Held`] over `artifacts`. `None`, which evaluates no
/// model, when `bars` is unset or empty, or when [`registry`] or [`PinnedModels::new`] gives none.
pub fn wiring<A: ArtifactSource + Clone + 'static>(
    control: &[StoredEvent],
    environment: Environment,
    document: &Value,
    bars: Option<&str>,
    artifacts: A,
) -> Option<ModelsWiring> {
    let bars = Path::new(bars.filter(|root| !root.is_empty())?);
    let registry = registry(control, environment)?;
    let models = match PinnedModels::working(
        document,
        registry.clone(),
        bars,
        Box::new(artifacts.clone()),
    ) {
        Some(working) => working,
        None => PinnedModels::new(document, registry, bars)?,
    };
    Some(ModelsWiring {
        models: Box::new(models),
        artifacts: Box::new(Held(artifacts)),
    })
}
