//! The cell's research, wired only when asked for (DEC-2210 item 5, DEC-2215): the recorded bundle
//! at the path the process is given, the recorded worker over it, and the cell's source, for a
//! paper deployment read from the journal. Without the opt-in the process runs deny-all
//! (DEC-2208 item 1).

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;
use std::rc::Rc;

use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{Digest, Key, Object, Value};
use mandate_gateway::meter::{MeterStore, SharedMeter};
use mandate_gateway::money::Usd6;
use mandate_gateway::provider::SequentialIds;
use mandate_journal::{Environment, StreamId};
use mandate_num::Usd;
use mandate_research::PolicyOverlay;
use mandate_research_run::agentic::{OutputMode, Tool, ToolClass};
use mandate_runtime::{AgenticConfig, EventDraft, ResearchConfig};
use mandate_time::UtcNanos;

use crate::deployment;
use crate::meter::CellMeter;
use crate::recorded::{Bundle, BundleError};
use crate::runtime::{Refs, ResearchWiring};
use crate::source::{CellSource, Rows, SourceFor};
use crate::worker::{RecordedWorker, StoreSink, WorkerError, WorkerFor};

/// The deployment research is asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellDeployment {
    pub workspace: String,
    pub agent: String,
    pub environment: Environment,
    /// Whether the process read the deployment from the journal (DEC-1016), rather than being
    /// handed it by flags.
    pub resolved: bool,
}

/// Why research could not be wired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellError {
    /// Research runs only for a paper deployment.
    NotPaper,
    /// Research runs only for a deployment read from the journal.
    Unresolved,
    /// The artifact store at the given root cannot be opened.
    ArtifactsUnavailable,
    /// The recorded bundle refused to load.
    Bundle(BundleError),
    /// The deployment or its mandate document could not be read, with `deployment`'s code.
    Mandate(&'static str),
    /// The recorded worker could not be built.
    Worker(WorkerError),
}

impl CellError {
    /// The code the binary stops the start with.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotPaper => "research_needs_paper",
            Self::Unresolved => "research_needs_resolved_deployment",
            Self::ArtifactsUnavailable => "artifact_store_unavailable",
            Self::Bundle(_) => "research_bundle_unreadable",
            Self::Mandate(code) => code,
            Self::Worker(_) => "research_worker_unbuilt",
        }
    }
}

impl fmt::Display for CellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for CellError {}

/// The research a process is wired with, and the refs its records carry.
pub struct CellResearch {
    pub wiring: ResearchWiring,
    pub refs: Box<dyn Refs>,
}

/// The references every record of a researching deployment carries: its mandate version and the
/// research entry's model version, at the draft's own [`EventDraft::schema_version`].
struct ResearchRefs {
    mandate_version: String,
    model_version: Digest,
}

impl Refs for ResearchRefs {
    fn refs(&self, draft: &EventDraft) -> Option<(u64, Object)> {
        let mut refs = Object::new();
        refs.insert(
            Key::new("mandate_version").ok()?,
            Value::Str(self.mandate_version.clone()),
        );
        refs.insert(
            Key::new("model_version").ok()?,
            Value::Str(format!("sha256:{}", self.model_version.to_hex())),
        );
        Some((draft.schema_version(), refs))
    }
}

/// Research asked for over a recorded bundle, for a resolved paper deployment: made only by
/// [`opt_in`], so no research is built for any other deployment.
pub struct OptIn {
    bundle: Bundle,
    deployment: CellDeployment,
    agentic: Option<AgenticConfig>,
}

/// The opt-in for research over the recorded bundle at `dir`, for `deployment`. It reads no journal
/// and no artifact store, so a refused opt-in stops the start before the process connects to either.
///
/// # Errors
/// [`CellError::NotPaper`] or [`CellError::Unresolved`] for any but a resolved paper deployment, and
/// [`CellError::Bundle`] when the bundle does not load.
pub fn opt_in(dir: &Path, deployment: &CellDeployment) -> Result<OptIn, CellError> {
    if deployment.environment != Environment::Paper {
        return Err(CellError::NotPaper);
    }
    if !deployment.resolved {
        return Err(CellError::Unresolved);
    }
    let bundle = Bundle::load(dir).map_err(CellError::Bundle)?;
    let agentic = bundle
        .entry
        .agentic
        .map(|_| agentic_config(&bundle))
        .transpose()?;
    Ok(OptIn {
        bundle,
        deployment: deployment.clone(),
        agentic,
    })
}

/// An agentic bundle's loop as the core runs it (DEC-2268): `final_output`, the entry's pinned
/// bounds, both menus the entry's declared tools with any action left out (each is on both kinds'
/// menus, DEC-2262 item 2), and the recorded run cap as the run's cost ceiling. An agentic bundle that records no run cap is refused, so no agentic run
/// starts without one (agent harness spec §6.11).
///
/// # Errors
/// [`CellError::Bundle`] with `run_usd` malformed when the bundle records no run cap.
pub fn agentic_config(bundle: &Bundle) -> Result<AgenticConfig, CellError> {
    let malformed = |member| {
        CellError::Bundle(BundleError::Malformed {
            file: "research.json",
            member,
        })
    };
    let (Some(bounds), Some(cost_ceiling)) = (bundle.entry.agentic, bundle.run_cap) else {
        return Err(malformed("run_usd"));
    };
    let content = bundle.entry.content();
    let declared = content.agentic().ok_or_else(|| malformed("agentic"))?;
    let menu: BTreeSet<Tool> = declared
        .tools
        .iter()
        .filter_map(|tool| Tool::from_name(&tool.name))
        .filter(|tool| tool.class() != ToolClass::Action)
        .collect();
    Ok(AgenticConfig {
        output: OutputMode::FinalOutput,
        bounds,
        ideate_menu: menu.clone(),
        monitor_menu: menu,
        cost_ceiling,
    })
}

/// The agent's daily research cap (DEC-2254 items 1 to 3): the mandate document's
/// `behavior.research.cost_cap_usd_per_day` tightened by the research profile, the value and overlay
/// the source hands the core, and zero for a mandate with no `behavior.research`.
///
/// # Errors
/// [`CellError::Mandate`] with `mandate_document_unavailable` when the document gives no cap the
/// gateway can hold.
pub fn research_daily_cap(document: &Value) -> Result<Usd6, CellError> {
    let unavailable = || CellError::Mandate("mandate_document_unavailable");
    let research = document
        .get("behavior")
        .and_then(|behavior| behavior.get("research"))
        .ok_or_else(unavailable)?;
    if *research == Value::Null {
        return Ok(Usd6::ZERO);
    }
    let cap = research
        .get("cost_cap_usd_per_day")
        .and_then(Value::as_str)
        .and_then(|text| Usd::parse(text).ok())
        .ok_or_else(unavailable)?;
    let cap = PolicyOverlay::internal_research_profile().effective_cost_cap(cap);
    Usd6::parse(&cap.to_string()).map_err(|_| unavailable())
}

/// The ids a workspace's meter writer opens its stream with, its `StreamOpened` the one record it
/// gives an id of its own (DEC-2258 item 2): drawn from the stream's own name, so no two
/// workspaces' streams open with one id, as the journal's event ids must not repeat across streams.
pub fn meter_opened_ids(workspace: &str) -> SequentialIds {
    let digest = Digest::of(format!("meter:{workspace}").as_bytes());
    let (high, _) = digest.as_bytes().split_at(16);
    SequentialIds::starting_at(u128::from_be_bytes(high.try_into().unwrap_or([0; 16])))
}

/// As [`research`], with the worker's gateway and the research source sharing `meter`, the
/// workspace's journaled meter stream in a process given a journal (DEC-2259 item 1).
///
/// # Errors
/// As [`research`].
pub fn research_metered(
    opted: OptIn,
    rows: Box<dyn Rows>,
    meter: Box<dyn MeterStore>,
    artifacts: &Path,
    now: Box<dyn FnMut() -> UtcNanos>,
) -> Result<CellResearch, CellError> {
    wired(opted, rows, meter, artifacts, now)
}

/// The research `opted` asks for, reading the streams through `rows` and the mandate documents and
/// artifacts from the store at `artifacts`, its meter in memory, for fixtures and tests (DEC-2259
/// item 1).
///
/// # Errors
/// [`CellError`] when the artifact store, the deployment, its mandate document, or the worker cannot
/// be had.
pub fn research(
    opted: OptIn,
    rows: Box<dyn Rows>,
    artifacts: &Path,
    now: Box<dyn FnMut() -> UtcNanos>,
) -> Result<CellResearch, CellError> {
    wired(opted, rows, Box::new(SharedMeter::new()), artifacts, now)
}

/// The research `opted` asks for, its worker's gateway and source sharing `meter`.
fn wired(
    opted: OptIn,
    rows: Box<dyn Rows>,
    meter: Box<dyn MeterStore>,
    artifacts: &Path,
    now: Box<dyn FnMut() -> UtcNanos>,
) -> Result<CellResearch, CellError> {
    let OptIn {
        bundle,
        deployment,
        agentic,
    } = opted;
    let rows: Rc<dyn Rows> = Rc::from(rows);
    let now = Rc::new(RefCell::new(now));
    let clock = {
        let now = Rc::clone(&now);
        move || (now.borrow_mut())()
    };
    let who = SourceFor {
        workspace: deployment.workspace.clone(),
        agent: deployment.agent.clone(),
        environment: deployment.environment,
    };
    let meter = CellMeter::over(meter, Rc::clone(&rows), who.clone(), Rc::new(clock.clone()));
    let open =
        || FsArtifactStore::open_existing(artifacts).map_err(|_| CellError::ArtifactsUnavailable);
    let control = StreamId::parse(&format!("ctl:{}", deployment.workspace))
        .ok_or(CellError::Mandate("deployment_missing"))?;
    let rows_now = rows
        .rows(&control)
        .ok_or(CellError::Mandate("journal_unavailable"))?;
    let (version, document) = deployment::document(
        &rows_now,
        &deployment.agent,
        deployment.environment,
        &open()?,
    )
    .map_err(CellError::Mandate)?;
    let description = document
        .get("behavior")
        .and_then(|behavior| behavior.get("description"))
        .and_then(Value::as_str)
        .ok_or(CellError::Mandate("mandate_document_unavailable"))?
        .to_owned();
    let pin = bundle.entry.content().model_ref();
    let deadline_ms = u32::try_from(bundle.entry.deadline_ms).map_err(|_| {
        CellError::Bundle(BundleError::Malformed {
            file: "research.json",
            member: "deadline_ms",
        })
    })?;
    let daily = research_daily_cap(&document)?;
    let worker = RecordedWorker::capped(
        &bundle,
        WorkerFor {
            workspace: deployment.workspace.clone(),
            agent: deployment.agent.clone(),
            description: description.clone(),
        },
        Box::new(StoreSink(open()?)),
        Box::new(clock),
        meter.clone(),
        daily,
    )
    .map_err(CellError::Worker)?;
    let source = CellSource::metered(&bundle, who, rows, Box::new(open()?), meter);
    let refs = ResearchRefs {
        mandate_version: version,
        model_version: pin.content_hash,
    };
    Ok(CellResearch {
        wiring: ResearchWiring {
            config: ResearchConfig {
                pin,
                deadline_ms,
                max_output_tokens: bundle.entry.max_output_tokens,
                plan: bundle.entry.plan,
                bounds: bundle.entry.bounds,
                description,
                revisions_enabled: false,
                agentic,
            },
            source: Box::new(source),
            worker: Box::new(worker),
            artifacts: Box::new(StoreSink(open()?)),
        },
        refs: Box::new(refs),
    })
}
