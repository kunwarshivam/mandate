//! The cell's research worker (DEC-2210 item 3): retrieval over the recorded plane, and the one
//! call through the gateway over its recorded fake, the only transport this type can name. It
//! stores every artifact an answer names before it keeps the answer, and stamps each answer with
//! the risk clock of the pass it is handed in.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use mandate_domain::AssetClass;
use mandate_gateway::fake::{Recorded, RecordedTransport};
use mandate_gateway::guard::GuardContext;
use mandate_gateway::meter::{Caps, DeploymentMeter, MemoryMeter, Meter, SharedMeter};
use mandate_gateway::money::Usd6;
use mandate_gateway::openrouter::OpenRouter;
use mandate_gateway::policy::EndpointPolicy;
use mandate_gateway::provider::{Clock, Delivery, ManualClock, SequentialIds};
use mandate_gateway::registry::Locality;
use mandate_gateway::{CallRecord, Gateway, RateCaps, RequestError, Settings};
use mandate_journal::{ArtifactRef, ArtifactStore, Environment};
use mandate_research::{AssetClass as EnvelopeClass, WorkingUniverse};
use mandate_research_run::{
    Allowlist, Answer, CallContext, CallPlan, MemoryThesis, Readers, RecordedKey, Retrieval,
    RetrievalPlan, RetrievedItem, Scope, SourceId, call, recorded_reply, retrieve,
};
use mandate_runtime::{ResearchInput, ResearchRequest, RetrievedObservation, RiskClock};
use mandate_store::MemoryStore;
use mandate_time::UtcNanos;

use crate::meter::CellMeter;
use crate::recorded::{Bundle, BundleError, RecordedPlane, RecordedReply};
use crate::runtime::{ArtifactSink, ResearchParts, ResearchWorker};
use crate::source::SourceFor;

/// Why the worker could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerError {
    /// The bundle's entry or endpoint did not register.
    Bundle(BundleError),
    /// The daily research cap is above the workspace's monthly budget (DEC-2254 item 3).
    DailyCapAboveWorkspace,
}

/// Who the worker reads and calls for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerFor {
    pub workspace: String,
    pub agent: String,
    /// The mandate's `behavior.description`, which the call shows the model.
    pub description: String,
}

/// The gateway's request rate for the cell: a one-shot run makes one call per research interval,
/// so a small fixed cap never binds it and bounds a fault that would call again.
const RATE: RateCaps = RateCaps {
    per_workspace: 6,
    per_agent: 6,
};

/// An answer kept until the core is handed it, without the risk clock it is handed at.
enum Ready {
    Retrieved {
        run_id: String,
        items: Vec<RetrievedObservation>,
    },
    Called {
        run_id: String,
        record: Box<CallRecord>,
    },
    Refused {
        run_id: String,
        error: RequestError,
    },
    Cancelled {
        run_id: String,
        dispatched: bool,
    },
}

/// What a run's call reads, kept from its start.
struct Run {
    retrieval: Retrieval,
    context: CallContext,
    dispatched: bool,
}

/// The cell's worker.
pub struct RecordedWorker {
    gateway: Gateway<RecordedTransport, CellMeter, ManualClock>,
    plane: RecordedPlane,
    plan: RetrievalPlan,
    reply: RecordedReply,
    slug: String,
    provider: String,
    workspace: String,
    agent: String,
    description: String,
    sink: Box<dyn ArtifactSink>,
    now: Box<dyn FnMut() -> UtcNanos>,
    runs: BTreeMap<String, Run>,
    ready: Vec<Ready>,
}

impl RecordedWorker {
    /// The worker over `bundle`, storing through `sink`, its gateway's clock moved to `now()` before
    /// each call, and its meter its own and never held: it reads an empty agent stream, so no call
    /// record asks it for a reservation it does not hold. The rest as [`RecordedWorker::metered`].
    ///
    /// # Errors
    /// [`WorkerError::Bundle`] when the bundle's entry or endpoint does not register.
    pub fn new(
        bundle: &Bundle,
        who: WorkerFor,
        sink: Box<dyn ArtifactSink>,
        now: Box<dyn FnMut() -> UtcNanos>,
    ) -> Result<Self, WorkerError> {
        let meter = CellMeter::new(
            SharedMeter::new(),
            Rc::new(MemoryStore::default()),
            SourceFor {
                workspace: who.workspace.clone(),
                agent: who.agent.clone(),
                environment: Environment::Paper,
            },
            Rc::new(|| UtcNanos::EPOCH),
        );
        Self::metered(bundle, who, sink, now, meter)
    }

    /// As [`RecordedWorker::new`], with the gateway reserving and settling through `meter`, the
    /// store the research source reads the agent's spend from (DEC-2249 item 3): while `meter` is
    /// held, every call is refused `meter_unavailable` and nothing is sent.
    ///
    /// # Errors
    /// As [`RecordedWorker::new`].
    pub fn metered(
        bundle: &Bundle,
        who: WorkerFor,
        sink: Box<dyn ArtifactSink>,
        now: Box<dyn FnMut() -> UtcNanos>,
        meter: CellMeter,
    ) -> Result<Self, WorkerError> {
        let caps = Caps::new(bundle.workspace_cap);
        Self::built(bundle, who, sink, now, meter, caps)
    }

    /// The worker whose gateway meters through `meter` under `caps` and the bundle's deployment cap.
    fn built(
        bundle: &Bundle,
        who: WorkerFor,
        sink: Box<dyn ArtifactSink>,
        mut now: Box<dyn FnMut() -> UtcNanos>,
        meter: CellMeter,
        caps: Caps,
    ) -> Result<Self, WorkerError> {
        let settings = Settings {
            workspace_id: who.workspace.clone(),
            policy: EndpointPolicy::new(
                BTreeSet::from([Locality::Hosted]),
                Some(BTreeSet::from([bundle.endpoint.route.upstream.clone()])),
                None,
            ),
            guard: GuardContext::default(),
            rate: RATE,
        };
        let start = now();
        let ids = |offset: u128| {
            let base = u128::try_from(start.secs())
                .unwrap_or(0)
                .saturating_mul(1 << 64);
            Box::new(SequentialIds::starting_at(base.saturating_add(offset)))
        };
        let meter = Meter::new(
            meter,
            caps,
            DeploymentMeter::new(
                Box::new(MemoryMeter::new()),
                bundle.deployment_cap,
                ids(1 << 40),
            ),
        );
        let gateway = Gateway::new(
            settings,
            bundle.registry().map_err(WorkerError::Bundle)?,
            Box::new(OpenRouter),
            RecordedTransport::new(),
            Box::new(RecordedKey),
            meter,
            ManualClock::at(start),
            ids(0),
        );
        Ok(Self {
            gateway,
            plane: bundle.plane.clone(),
            plan: bundle.entry.plan,
            reply: bundle.reply.clone(),
            slug: bundle.entry.identity.model_name.clone(),
            provider: bundle.endpoint.route.reported_provider.clone(),
            workspace: who.workspace,
            agent: who.agent,
            description: who.description,
            sink,
            now,
            runs: BTreeMap::new(),
            ready: Vec::new(),
        })
    }

    /// As [`RecordedWorker::metered`], with the gateway refusing `budget_exhausted` any research call
    /// whose reservation would take the agent's spend for the risk day past `daily` (DEC-2254 item
    /// 4): nothing is reserved and nothing is sent.
    ///
    /// # Errors
    /// As [`RecordedWorker::new`], and [`WorkerError::DailyCapAboveWorkspace`] when `daily` is above
    /// the bundle's workspace budget.
    pub fn capped(
        bundle: &Bundle,
        who: WorkerFor,
        sink: Box<dyn ArtifactSink>,
        now: Box<dyn FnMut() -> UtcNanos>,
        meter: CellMeter,
        daily: Usd6,
    ) -> Result<Self, WorkerError> {
        let caps = Caps::new(bundle.workspace_cap)
            .with_research_daily(&who.agent, daily)
            .map_err(|_| WorkerError::DailyCapAboveWorkspace)?;
        Self::built(bundle, who, sink, now, meter, caps)
    }

    fn start(&mut self, run_id: &str, cut_off: RiskClock, parts: &ResearchParts) {
        let Ok(at) = UtcNanos::from_parts(cut_off.secs(), 0) else {
            return;
        };
        let allowlist = Allowlist {
            version: parts.facts.allowlist.version.0,
            sources: parts
                .facts
                .allowlist
                .sources
                .iter()
                .filter_map(|source| SourceId::new(source.as_str()).ok())
                .collect(),
        };
        let scope = Scope {
            workspace: self.workspace.clone(),
            agent: self.agent.clone(),
            cut_off: at,
            data_universe: parts.data_symbols.clone(),
            held: BTreeSet::new(),
            allowlist,
        };
        let readers = Readers {
            bars: &self.plane,
            news: &self.plane,
            filings: &self.plane,
            instruments: &self.plane,
        };
        let Ok(retrieval) = retrieve(&self.plan, &scope, readers) else {
            return;
        };
        let mut kept = Vec::new();
        let mut items = Vec::new();
        for item in retrieval.items {
            let Ok((observation, record)) = RetrievedObservation::capture(&item, cut_off) else {
                continue;
            };
            if self.sink.put(&record) {
                kept.push(item);
                items.push(observation);
            }
        }
        let context = self.context(parts, &kept);
        self.runs.insert(
            run_id.to_owned(),
            Run {
                retrieval: Retrieval {
                    cut_off: retrieval.cut_off,
                    items: kept,
                    drops: retrieval.drops,
                },
                context,
                dispatched: false,
            },
        );
        self.ready.push(Ready::Retrieved {
            run_id: run_id.to_owned(),
            items,
        });
    }
}

impl RecordedWorker {
    /// What the call tells the model besides the items: the data universe, the envelope's asset
    /// classes and instrument cap, the working universe and active theses by symbol, and nothing
    /// held (no position read reaches the cell's research).
    fn context(&self, parts: &ResearchParts, kept: &[RetrievedItem]) -> CallContext {
        let symbol_of: BTreeMap<_, _> = kept
            .iter()
            .filter_map(|item| match item {
                RetrievedItem::Instrument(fact) => {
                    Some((fact.asset_id.as_str().to_owned(), fact.symbol.clone()))
                }
                _ => None,
            })
            .collect();
        let envelope = parts.mandate.envelope();
        CallContext {
            description: self.description.clone(),
            data_universe: parts.data_symbols.iter().cloned().collect(),
            asset_classes: envelope
                .asset_classes
                .iter()
                .map(|class| match class {
                    EnvelopeClass::UsEquity => AssetClass::UsEquity,
                    EnvelopeClass::Crypto => AssetClass::Crypto,
                })
                .collect(),
            max_instruments: envelope.max_instruments,
            working_universe: match &parts.universe {
                WorkingUniverse::Known { instruments, .. } => instruments
                    .iter()
                    .filter_map(|id| symbol_of.get(id.as_str()).cloned())
                    .collect(),
                WorkingUniverse::Unavailable => Vec::new(),
            },
            held: Vec::new(),
            theses: parts
                .active
                .iter()
                .filter_map(|thesis| {
                    Some(MemoryThesis {
                        thesis_id: thesis.thesis_id.clone(),
                        symbol: thesis.symbol.clone(),
                        expires_at: UtcNanos::from_parts(thesis.expires_at.secs(), 0).ok()?,
                    })
                })
                .collect(),
        }
    }

    fn call(&mut self, plan: &CallPlan) {
        let Some(run) = self.runs.get_mut(&plan.run_id) else {
            return;
        };
        let now = (self.now)();
        self.gateway.clock_mut().wait_until(now);
        self.gateway.transport_mut().script(vec![Recorded {
            latency_ms: self.reply.latency_ms,
            delivery: Delivery::Response {
                status: 200,
                body: recorded_reply(
                    &self.reply.output,
                    &self.slug,
                    &self.provider,
                    self.reply.usage,
                ),
                retry_after_ms: None,
            },
        }]);
        let Ok((_, answer)) = call(&mut self.gateway, plan, &run.retrieval, run.context.clone())
        else {
            return;
        };
        run.dispatched = true;
        let run_id = plan.run_id.clone();
        match answer {
            Answer::Recorded(record) => {
                let stored = [&record.prompt, &record.response]
                    .into_iter()
                    .flatten()
                    .all(|bytes| self.sink.put(bytes));
                if stored {
                    self.ready.push(Ready::Called {
                        run_id,
                        record: Box::new(record),
                    });
                }
            }
            Answer::Refused(error) => self.ready.push(Ready::Refused { run_id, error }),
        }
    }

    fn cancel(&mut self, run_id: &str) {
        let dispatched = self.runs.remove(run_id).is_some_and(|run| run.dispatched);
        self.ready
            .retain(|ready| !matches!(ready, Ready::Retrieved { run_id: id, .. } if id == run_id));
        self.ready.push(Ready::Cancelled {
            run_id: run_id.to_owned(),
            dispatched,
        });
    }
}

impl ResearchWorker for RecordedWorker {
    fn hand(&mut self, request: &ResearchRequest, parts: &ResearchParts) {
        match request {
            ResearchRequest::Start { run_id, cut_off } => self.start(run_id, *cut_off, parts),
            ResearchRequest::Call(plan) => self.call(plan),
            ResearchRequest::Cancel { run_id } => self.cancel(run_id),
            ResearchRequest::Turn(_) | ResearchRequest::Dispatch(_) => {}
        }
    }

    fn answers(&mut self, at: RiskClock) -> Vec<ResearchInput> {
        self.ready
            .drain(..)
            .map(|ready| match ready {
                Ready::Retrieved { run_id, items } => ResearchInput::Retrieved { run_id, items },
                Ready::Called { run_id, record } => ResearchInput::Called { run_id, at, record },
                Ready::Refused { run_id, error } => ResearchInput::Refused { run_id, at, error },
                Ready::Cancelled { run_id, dispatched } => {
                    ResearchInput::Cancelled { run_id, dispatched }
                }
            })
            .collect()
    }
}

/// The artifact store as a research sink: each put is content-addressed, and stored only when the
/// store gives back the reference of the bytes it was handed.
pub struct StoreSink<A>(pub A);

impl<A: ArtifactStore> ArtifactSink for StoreSink<A> {
    fn put(&mut self, bytes: &[u8]) -> bool {
        self.0
            .put_artifact(bytes)
            .is_ok_and(|reference| reference == ArtifactRef::of(bytes))
    }
}
