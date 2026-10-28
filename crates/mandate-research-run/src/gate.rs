//! The §4.3 gate's probe set (DEC-2217 item 3, inference spec §4.3): five `ideate`, five
//! `monitor` and five prompt-injection runs, each over a slice of a fixture corpus, built into the
//! research runtime's and the model-gateway service's images from one file, and named by the
//! SHA-256 of its canonical bytes. The gate-mode runtime drives these runs, and the service
//! recomputes each run's inputs and tool results from the same slices, so the two cannot drift.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_domain::AssetClass;
use mandate_gateway::money::Usd6;
use mandate_time::UtcNanos;

use crate::agentic::RunKind;
use crate::plane::{PlaneError, RecordedPlane};
use crate::{SourceId, Symbol};

/// The probe set, as the repository holds it and both images build it in.
pub const PROBE_SET: &str = include_str!("../../../fixtures/gate/research-probe-set.json");

/// The built-in probe set's digest: SHA-256 over its canonical bytes, or `None` when the built-in
/// file does not read as a probe set, so no session can open on a set that does not exist.
pub fn probe_set_digest() -> Option<Digest> {
    let value = parse(PROBE_SET.as_bytes()).ok()?;
    ProbeSet::read(&value).ok()?;
    Some(Digest::of(&to_canonical(&value)))
}

/// Why a probe set did not read: the member missing, empty, or out of its range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeError {
    Member(&'static str),
}

/// One active thesis a monitoring run starts with. Its id is the runtime's own once journaled, so
/// the service holds the runtime to the symbol and the expiry, never to the id (DEC-2218).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeThesis {
    pub thesis_id: String,
    pub symbol: Symbol,
    pub direction: String,
    pub invalidation: String,
    pub expires_at: UtcNanos,
}

/// The envelope every probe run is researched under: the description the model reads, the asset
/// classes and instrument ceiling of its context, and the cost ceiling of one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeContext {
    pub description: String,
    pub asset_classes: Vec<AssetClass>,
    pub max_instruments: u32,
    pub run_usd: Usd6,
}

/// One run of the probe set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeRun {
    pub kind: RunKind,
    /// Whether the run's corpus slice carries prompt-injection text, which §4.3's injection row
    /// judges.
    pub injection: bool,
    /// The corpus slice the run's tools read, by name.
    pub corpus: String,
    pub cut_off: UtcNanos,
    pub universe: BTreeSet<Symbol>,
    /// The theses a monitoring run starts with; empty for every other run.
    pub theses: Vec<ProbeThesis>,
}

/// The probe set: its runs in order, its corpus slices by name, and the source allowlist the
/// slices' items were written under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeSet {
    pub context: ProbeContext,
    pub runs: Vec<ProbeRun>,
    pub allowlist_version: u32,
    pub sources: BTreeSet<SourceId>,
    corpus: BTreeMap<String, RecordedPlane>,
}

impl ProbeSet {
    /// The probe set `value` holds, read strictly.
    ///
    /// # Errors
    /// [`ProbeError::Member`] naming the first member that is missing, empty, or out of its range:
    /// a run naming a slice the set lacks, a monitoring run with no thesis or another run with one,
    /// a thesis on a symbol outside its run's universe, a news item from a source off the set's
    /// allowlist, or a slice that is not a plane (named by its file, `bars.json` and the like).
    pub fn read(value: &Value) -> Result<Self, ProbeError> {
        let member = |name: &'static str| value.get(name).ok_or(ProbeError::Member(name));
        let context = probe_context(member("context")?)?;
        let allowlist = member("allowlist")?;
        let allowlist_version = allowlist
            .get("version")
            .and_then(Value::as_int)
            .and_then(|version| u32::try_from(version).ok())
            .ok_or(ProbeError::Member("allowlist"))?;
        let sources = allowlist
            .get("sources")
            .and_then(Value::as_array)
            .ok_or(ProbeError::Member("allowlist"))?
            .iter()
            .map(|source| {
                source
                    .as_str()
                    .and_then(|text| SourceId::new(text).ok())
                    .ok_or(ProbeError::Member("allowlist"))
            })
            .collect::<Result<_, _>>()?;
        let corpus = member("corpus")?
            .as_object()
            .ok_or(ProbeError::Member("corpus"))?
            .iter()
            .map(|(name, slice)| {
                let file = |name: &'static str| slice.get(name).ok_or(ProbeError::Member(name));
                let plane = RecordedPlane::read(
                    file("bars")?,
                    file("news")?,
                    file("filings")?,
                    file("instruments")?,
                    allowlist_version,
                )
                .map_err(|PlaneError { file, .. }| ProbeError::Member(file))?;
                Ok((name.as_str().to_owned(), plane))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let sources: BTreeSet<SourceId> = sources;
        if corpus
            .values()
            .flat_map(|plane| plane.news.iter().map(|item| &item.source))
            .any(|source| !sources.contains(source))
        {
            return Err(ProbeError::Member("source"));
        }
        let runs = member("runs")?
            .as_array()
            .filter(|runs| !runs.is_empty())
            .ok_or(ProbeError::Member("runs"))?
            .iter()
            .map(|run| probe_run(run, &corpus))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            context,
            runs,
            allowlist_version,
            sources,
            corpus,
        })
    }

    /// The built-in probe set.
    ///
    /// # Errors
    /// [`ProbeError::Member`] when the built-in file does not read, which its own test refuses.
    pub fn built_in() -> Result<Self, ProbeError> {
        let value = parse(PROBE_SET.as_bytes()).map_err(|_| ProbeError::Member(""))?;
        Self::read(&value)
    }

    /// The corpus slice `run`'s tools read.
    pub fn plane(&self, run: &ProbeRun) -> Option<&RecordedPlane> {
        self.corpus.get(&run.corpus)
    }
}

fn text<'a>(value: &'a Value, member: &'static str) -> Result<&'a str, ProbeError> {
    value
        .get(member)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .ok_or(ProbeError::Member(member))
}

fn probe_run(
    run: &Value,
    corpus: &BTreeMap<String, RecordedPlane>,
) -> Result<ProbeRun, ProbeError> {
    let kind = match text(run, "kind")? {
        "ideate" => RunKind::Ideate,
        "monitor" => RunKind::Monitor,
        _ => return Err(ProbeError::Member("kind")),
    };
    let slice = text(run, "corpus")?;
    if !corpus.contains_key(slice) {
        return Err(ProbeError::Member("corpus"));
    }
    let injection = match run.get("injection") {
        Some(Value::Bool(flag)) => *flag,
        _ => return Err(ProbeError::Member("injection")),
    };
    let cut_off =
        UtcNanos::parse(text(run, "cut_off")?).map_err(|_| ProbeError::Member("cut_off"))?;
    let universe = run
        .get("universe")
        .and_then(Value::as_array)
        .filter(|symbols| !symbols.is_empty())
        .ok_or(ProbeError::Member("universe"))?
        .iter()
        .map(|symbol| {
            symbol
                .as_str()
                .and_then(|text| Symbol::new(text).ok())
                .ok_or(ProbeError::Member("universe"))
        })
        .collect::<Result<_, _>>()?;
    let universe: BTreeSet<Symbol> = universe;
    let theses: Vec<ProbeThesis> = run
        .get("theses")
        .and_then(Value::as_array)
        .ok_or(ProbeError::Member("theses"))?
        .iter()
        .map(|thesis| {
            Ok(ProbeThesis {
                thesis_id: text(thesis, "thesis_id")?.to_owned(),
                symbol: Symbol::new(text(thesis, "symbol")?)
                    .map_err(|_| ProbeError::Member("symbol"))?,
                direction: text(thesis, "direction")?.to_owned(),
                invalidation: text(thesis, "invalidation")?.to_owned(),
                expires_at: UtcNanos::parse(text(thesis, "expires_at")?)
                    .map_err(|_| ProbeError::Member("expires_at"))?,
            })
        })
        .collect::<Result<_, _>>()?;
    if theses.is_empty() == (kind == RunKind::Monitor)
        || theses
            .iter()
            .any(|thesis| !universe.contains(&thesis.symbol))
    {
        return Err(ProbeError::Member("theses"));
    }
    Ok(ProbeRun {
        kind,
        injection,
        corpus: slice.to_owned(),
        cut_off,
        universe,
        theses,
    })
}

fn probe_context(context: &Value) -> Result<ProbeContext, ProbeError> {
    let asset_classes = context
        .get("asset_classes")
        .and_then(Value::as_array)
        .filter(|classes| !classes.is_empty())
        .ok_or(ProbeError::Member("asset_classes"))?
        .iter()
        .map(|class| {
            class
                .as_str()
                .and_then(|text| AssetClass::parse(text).ok())
                .ok_or(ProbeError::Member("asset_classes"))
        })
        .collect::<Result<_, _>>()?;
    let max_instruments = context
        .get("max_instruments")
        .and_then(Value::as_int)
        .and_then(|max| u32::try_from(max).ok())
        .filter(|max| *max > 0)
        .ok_or(ProbeError::Member("max_instruments"))?;
    Ok(ProbeContext {
        description: text(context, "description")?.to_owned(),
        asset_classes,
        max_instruments,
        run_usd: Usd6::parse(text(context, "run_usd")?)
            .map_err(|_| ProbeError::Member("run_usd"))?,
    })
}
