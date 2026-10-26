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
//! so the same tick over the same write-once partitions gives the same answer. Every refusal, a
//! closes read the trust check refuses, a `Flat` or `Undecided` signal, and a pin the registry or
//! the host disagrees with, is no output, which can only shrink a buy (MI-10). No `llm.` model is
//! ever evaluated here: LLM opinions reach the core through the research pass.
//!
//! The runtime mounts the artifact store read-only, so [`Held`] stores nothing: it only confirms
//! the data process already holds the closes object an observation will name (DEC-2385 item 5).

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use mandate_accounting::InstrumentId;
use mandate_bars::{closes_object, daily_dir, trusted_daily_closes};
use mandate_canon::{Value, parse};
use mandate_daemon::models::{Evaluated, Models};
use mandate_daemon::{ArtifactSink, ModelsWiring};
use mandate_domain::AssetClass;
use mandate_journal::{ArtifactRef, ArtifactSource, Environment, StoredEvent, get_artifact};
use mandate_modelhost::{DailyCloses, Evaluation, Pin, evaluate};
use mandate_runtime::RiskClock;
use mandate_spec::context::JournaledFact;
use mandate_spec::document::{Mandate, ModelId};
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

/// The deployment's pinned quant models over the stored daily closes.
pub struct PinnedModels {
    pinned: Vec<Pinned>,
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
            pinned,
            registry,
            calendar,
            bars: bars.to_path_buf(),
            source,
        })
    }

    /// `pinned`'s output at `now`, or none for any refusal.
    fn one(&self, pinned: &Pinned, now: UtcNanos) -> Option<Evaluated> {
        let dir = daily_dir(&self.bars, &pinned.symbol).ok()?;
        let daily = DailyCloses {
            instrument_id: pinned.pin.instrument_id.clone(),
            closes: trusted_daily_closes(&dir, &pinned.symbol, now).ok()?,
        };
        let Evaluation::Long(output) =
            evaluate(&pinned.pin, &self.registry, &self.calendar, &daily, now).ok()?
        else {
            return None;
        };
        let closes = closes_object(&daily.closes, pinned.pin.instrument_id.as_str()).ok()?;
        Some(Evaluated {
            output: *output,
            closes,
            source: self.source.to_owned(),
        })
    }
}

impl Models for PinnedModels {
    /// Each pin's output at the tick `at`, in order; a pin that gives none is left out.
    fn evaluate(&self, at: RiskClock) -> Vec<Evaluated> {
        let Ok(now) = UtcNanos::from_parts(at.secs(), 0) else {
            return Vec::new();
        };
        self.pinned
            .iter()
            .filter_map(|pinned| self.one(pinned, now))
            .collect()
    }
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
pub fn wiring<A: ArtifactSource + 'static>(
    control: &[StoredEvent],
    environment: Environment,
    document: &Value,
    bars: Option<&str>,
    artifacts: A,
) -> Option<ModelsWiring> {
    let bars = Path::new(bars.filter(|root| !root.is_empty())?);
    let models = PinnedModels::new(document, registry(control, environment)?, bars)?;
    Some(ModelsWiring {
        models: Box::new(models),
        artifacts: Box::new(Held(artifacts)),
    })
}
