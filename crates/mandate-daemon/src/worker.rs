//! The cell's research worker (DEC-2210 item 3): retrieval over the recorded plane, and the one
//! call through the gateway over its recorded fake, the only transport a constructor outside this
//! crate's tests can name (DEC-2269). It
//! stores every artifact an answer names before it keeps the answer, and stamps each answer with
//! the risk clock of the pass it is handed in.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use mandate_canon::Digest;
use mandate_domain::AssetClass;
use mandate_gateway::fake::{Recorded, RecordedTransport, openrouter_reply};
use mandate_gateway::guard::GuardContext;
use mandate_gateway::meter::{Caps, DeploymentMeter, MemoryMeter, Meter, SharedMeter};
use mandate_gateway::money::Usd6;
use mandate_gateway::openrouter::OpenRouter;
use mandate_gateway::policy::EndpointPolicy;
use mandate_gateway::provider::{Clock, Delivery, ManualClock, SequentialIds, Transport};
use mandate_gateway::registry::{Locality, Reported};
use mandate_gateway::{CallRecord, Gateway, RateCaps, RequestError, Settings};
use mandate_journal::{ArtifactRef, ArtifactStore, Environment, get_artifact};
use mandate_research::{AssetClass as EnvelopeClass, WorkingUniverse};
use mandate_research_run::agentic::{LoopBounds, RunKind};
use mandate_research_run::recorded::shown_thesis;
use mandate_research_run::{
    Allowlist, Answer, CallContext, CallPlan, DispatchScope, EarlierTurn, MemoryThesis, Readers,
    RecordedKey, ResearchRunError, Retrieval, RetrievalPlan, RetrievedItem, Scope, SourceId,
    TurnParts, call, call_turn, dispatch, recorded_reply, retrieve,
};
use mandate_runtime::{
    DispatchRequest, DispatchedResult, ResearchInput, ResearchRequest, RetrievedObservation,
    RiskClock, TurnRequest,
};
use mandate_store::MemoryStore;
use mandate_time::UtcNanos;

use crate::meter::CellMeter;
use crate::recorded::{Bundle, BundleError, RecordedPlane, RecordedReply, ScriptedTurn};
use crate::remote::RemoteGateway;
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

/// Why a dispatch was answered with nothing (DEC-2260 item 6); the core's run bound then ends the
/// run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchError {
    /// The worker was not given the loop's bounds (DEC-2260 item 1).
    NoLoop,
    /// The worker did not start the run, or it was cancelled (item 2).
    NotStarted,
    /// The pass's allowlist is not the version the run froze, or the cut-off is not an instant
    /// (item 3).
    ScopeMoved,
    /// The harness refused the dispatch: an action among the calls, or a result it could not
    /// write (item 6).
    Refused(ResearchRunError),
    /// An item's record could not be captured (item 6).
    Uncaptured,
    /// A content was longer than `u32` bytes, or the sink refused a store (items 5 and 6).
    Unstored,
}

/// Why a turn was answered with nothing (DEC-2264); the core's run bound then ends the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnError {
    /// The worker's bundle names no agentic loop (DEC-2263 item 5).
    NoLoop,
    /// The worker did not start the run, or it was cancelled.
    NotStarted,
    /// An artifact of the transcript the sink cannot give back (item 1).
    Missing,
    /// The turn could not be built, so nothing was sent (DEC-2256 item 4).
    Unbuilt(ResearchRunError),
    /// The sink refused the record's prompt or response.
    Unstored,
}

/// Who the worker reads and calls for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerFor {
    pub workspace: String,
    pub agent: String,
    /// The mandate's `behavior.description`, which the call shows the model.
    pub description: String,
}

/// The cell gateway's calls a minute for the whole workspace, fixed whatever the entry pins: it
/// bounds a fault that would call again however many turns an entry's loop allows (DEC-2316).
const WORKSPACE_RATE: u32 = 12;

/// The calls a minute an agent may make beyond the turns one run of its entry may take, so a run
/// that uses every turn its loop allows is never cut by the rate cap (DEC-2316).
const RATE_MARGIN: u32 = 2;

/// The gateway's request rate for the cell's agent (DEC-2316): the turns one run may take, the
/// agentic loop's pinned `max_turns` or one for a one-shot entry, and [`RATE_MARGIN`] beyond them,
/// under the fixed [`WORKSPACE_RATE`]. A run's own bounds and cost ceiling still bind it; the cap
/// bounds only a fault that would call again.
fn rate(bounds: Option<&LoopBounds>) -> RateCaps {
    let turns = bounds.map_or(1, |bounds| bounds.max_turns);
    RateCaps {
        per_workspace: WORKSPACE_RATE,
        per_agent: turns.saturating_add(RATE_MARGIN),
    }
}

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
    Dispatched {
        run_id: String,
        results: Vec<DispatchedResult>,
    },
}

/// What a run's call reads, kept from its start.
struct Run {
    retrieval: Retrieval,
    context: CallContext,
    dispatched: bool,
}

/// The cell's worker, over the recorded fake. A worker over another transport is built only by
/// this crate's tests (DEC-2269).
pub struct RecordedWorker<T: Transport = RecordedTransport> {
    gateway: Gateway<T, CellMeter, ManualClock>,
    /// Hands the transport the recorded reply for the next call alone; a model double that writes
    /// its own replies ignores it.
    prime: fn(&mut T, Recorded),
    plane: RecordedPlane,
    plan: RetrievalPlan,
    reply: RecordedReply,
    turns: Vec<ScriptedTurn>,
    monitor_reply: Option<RecordedReply>,
    slug: String,
    provider: String,
    workspace: String,
    agent: String,
    description: String,
    sink: Box<dyn ArtifactSink>,
    now: Box<dyn FnMut() -> UtcNanos>,
    runs: BTreeMap<String, Run>,
    ready: Vec<Ready>,
    bounds: Option<LoopBounds>,
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
        let meter = own_meter(&who);
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
        Self::built(bundle, who, sink, now, meter, caps, recorded())
    }

    /// The worker over `bundle`, storing through `sink` and stamping at `now()`, that calls models
    /// through the model-gateway service at `remote` (DEC-2397 items 1 and 6). It builds no gateway
    /// and holds no meter: the service reserves and settles every call on its own journaled stream,
    /// and holds each research call to its agent's deployed cap.
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn remote(
        bundle: &Bundle,
        who: WorkerFor,
        sink: Box<dyn ArtifactSink>,
        now: Box<dyn FnMut() -> UtcNanos>,
        remote: RemoteGateway,
    ) -> Self {
        let _ = (bundle, who, sink, now, remote);
        todo!()
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
        Self::built(bundle, who, sink, now, meter, caps, recorded())
    }
}

impl<T: Transport> RecordedWorker<T> {
    /// The worker whose gateway sends through `transport`, primed by `prime`, and meters through
    /// `meter` under `caps` and the bundle's deployment cap.
    fn built(
        bundle: &Bundle,
        who: WorkerFor,
        sink: Box<dyn ArtifactSink>,
        mut now: Box<dyn FnMut() -> UtcNanos>,
        meter: CellMeter,
        caps: Caps,
        (transport, prime): (T, fn(&mut T, Recorded)),
    ) -> Result<Self, WorkerError> {
        let settings = Settings {
            workspace_id: who.workspace.clone(),
            policy: EndpointPolicy::new(
                BTreeSet::from([Locality::Hosted]),
                Some(BTreeSet::from([bundle.endpoint.route.upstream.clone()])),
                None,
            ),
            guard: GuardContext::default(),
            rate: rate(bundle.entry.agentic.as_ref()),
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
            transport,
            Box::new(RecordedKey),
            meter,
            ManualClock::at(start),
            ids(0),
        );
        Ok(Self {
            gateway,
            prime,
            plane: bundle.plane.clone(),
            plan: bundle.entry.plan,
            reply: bundle.reply.clone(),
            turns: bundle.turns.clone(),
            monitor_reply: bundle.monitor_reply.clone(),
            slug: bundle.entry.identity.model_name.clone(),
            provider: bundle.endpoint.route.reported_provider.clone(),
            workspace: who.workspace,
            agent: who.agent,
            description: who.description,
            sink,
            now,
            runs: BTreeMap::new(),
            ready: Vec::new(),
            bounds: bundle.entry.agentic,
        })
    }

    /// The worker, running a dispatch within `bounds`, the agentic entry's pinned `loop_bounds`
    /// (DEC-2260 item 1). Without them it runs no dispatch.
    #[must_use]
    pub fn with_loop(mut self, bounds: LoopBounds) -> Self {
        self.bounds = Some(bounds);
        self
    }

    /// Runs the core's dispatch of a reply's reads and thinks (DEC-2260): within the run's frozen
    /// scope and the loop's bounds, over the recorded plane, each result's content and each of its
    /// items' records stored before `ResearchInput::Dispatched` is kept for the next pass.
    ///
    /// # Errors
    /// [`DispatchError`] when nothing is answered, and why.
    pub fn dispatch(
        &mut self,
        request: &DispatchRequest,
        parts: &ResearchParts,
    ) -> Result<(), DispatchError> {
        let Some(bounds) = &self.bounds else {
            return Err(DispatchError::NoLoop);
        };
        if !self.runs.contains_key(&request.run_id) {
            return Err(DispatchError::NotStarted);
        }
        let scope = self
            .scope(request.cut_off, parts)
            .filter(|scope| scope.allowlist.version == request.allowlist_version)
            .ok_or(DispatchError::ScopeMoved)?;
        let within = DispatchScope {
            scope: &scope,
            bounds,
            result_bytes_left: request.result_bytes_left,
        };
        let ran = dispatch(request.turn, &request.calls, within, self.readers())
            .map_err(DispatchError::Refused)?;
        let mut results = Vec::with_capacity(ran.results.len());
        for result in ran.results {
            let bytes = u32::try_from(result.content.len()).map_err(|_| DispatchError::Unstored)?;
            if !self.sink.put(&result.content) {
                return Err(DispatchError::Unstored);
            }
            let mut items = Vec::with_capacity(result.items.len());
            for item in &result.items {
                let (observation, record) = RetrievedObservation::capture(item, request.cut_off)
                    .map_err(|_| DispatchError::Uncaptured)?;
                if !self.sink.put(&record) {
                    return Err(DispatchError::Unstored);
                }
                items.push(observation);
            }
            results.push(DispatchedResult {
                call: result.call,
                outcome: result.outcome,
                code: result.code,
                content: Digest::of(&result.content),
                bytes,
                truncated: result.truncated,
                untrusted: result.untrusted,
                items,
            });
        }
        self.ready.push(Ready::Dispatched {
            run_id: request.run_id.clone(),
            results,
        });
        Ok(())
    }

    /// The run's read scope at `cut_off`, from the pass's allowlist and data universe, with nothing
    /// held; `None` when the cut-off is not an instant.
    fn scope(&self, cut_off: RiskClock, parts: &ResearchParts) -> Option<Scope> {
        let at = UtcNanos::from_parts(cut_off.secs(), 0).ok()?;
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
        Some(Scope {
            workspace: self.workspace.clone(),
            agent: self.agent.clone(),
            cut_off: at,
            data_universe: parts.data_symbols.clone(),
            held: BTreeSet::new(),
            allowlist,
        })
    }

    /// The recorded plane as every reader.
    fn readers(&self) -> Readers<'_> {
        Readers {
            bars: &self.plane,
            news: &self.plane,
            filings: &self.plane,
            instruments: &self.plane,
        }
    }

    /// Sends the core's turn (DEC-2264): the transcript's artifacts loaded through the sink, the
    /// turn built over the run's kept seed and context and sent through the gateway over the recorded
    /// reply, and the record's prompt and response stored before `Called`, or the gateway's refusal
    /// answered `Refused`.
    ///
    /// # Errors
    /// [`TurnError`] when nothing is answered, and why.
    pub fn turn(&mut self, request: &TurnRequest) -> Result<(), TurnError> {
        if self.bounds.is_none() {
            return Err(TurnError::NoLoop);
        }
        let run_id = request.call.run_id.clone();
        if !self.runs.contains_key(&run_id) {
            return Err(TurnError::NotStarted);
        }
        let transcript = request
            .transcript
            .iter()
            .map(|earlier| {
                Some(EarlierTurn {
                    response: self.sink.get(&earlier.response)?,
                    results: earlier
                        .results
                        .iter()
                        .map(|result| self.sink.get(result))
                        .collect::<Option<_>>()?,
                })
            })
            .collect::<Option<Vec<_>>>()
            .ok_or(TurnError::Missing)?;
        let shown: Vec<String> = self
            .runs
            .get(&run_id)
            .map(|run| {
                run.context
                    .theses
                    .iter()
                    .map(|thesis| thesis.thesis_id.clone())
                    .collect()
            })
            .unwrap_or_default();
        self.script_reply(Some((request.plan.turn, request.kind)), &shown);
        let Some(run) = self.runs.get_mut(&run_id) else {
            return Err(TurnError::NotStarted);
        };
        let parts = TurnParts {
            call: &request.call,
            plan: &request.plan,
            kind: request.kind,
            seed: &run.retrieval,
            context: run.context.clone(),
            transcript,
        };
        let (_, answer) = call_turn(&mut self.gateway, parts).map_err(TurnError::Unbuilt)?;
        run.dispatched = true;
        match answer {
            Answer::Recorded(record) => {
                let stored = [&record.prompt, &record.response]
                    .into_iter()
                    .flatten()
                    .all(|bytes| self.sink.put(bytes));
                if !stored {
                    return Err(TurnError::Unstored);
                }
                self.ready.push(Ready::Called {
                    run_id,
                    record: Box::new(record),
                });
            }
            Answer::Refused(error) => self.ready.push(Ready::Refused { run_id, error }),
        }
        Ok(())
    }

    fn start(&mut self, run_id: &str, cut_off: RiskClock, parts: &ResearchParts) {
        let Some(scope) = self.scope(cut_off, parts) else {
            return;
        };
        let Ok(retrieval) = retrieve(&self.plan, &scope, self.readers()) else {
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

impl<T: Transport> RecordedWorker<T> {
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
        if !self.runs.contains_key(&plan.run_id) {
            return;
        }
        self.script_reply(None, &[]);
        let Some(run) = self.runs.get_mut(&plan.run_id) else {
            return;
        };
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

    /// Moves the gateway's clock to now and scripts the recorded reply for the next call alone: a
    /// `monitor` run's turn the bundle's monitor reply, when it has one, naming the least of the
    /// `shown` theses the run's context showed the model, with no scripted turn
    /// (agent harness spec §6.9); else an agentic run's turn n its scripted turn n, and the single
    /// call and every turn past the script the final reply (DEC-2266). A reply left by a call that
    /// was never sent (one not built, or refused before sending) is cleared first.
    fn script_reply(&mut self, turn: Option<(u32, RunKind)>, shown: &[String]) {
        let now = (self.now)();
        self.gateway.clock_mut().wait_until(now);
        let monitor = self
            .monitor_reply
            .clone()
            .filter(|_| matches!(turn, Some((_, RunKind::Monitor))));
        let scripted = turn
            .filter(|_| monitor.is_none())
            .and_then(|(turn, _)| turn.checked_sub(1))
            .and_then(|index| self.turns.get(usize::try_from(index).ok()?));
        let shown: Vec<&str> = shown.iter().map(String::as_str).collect();
        let last = match monitor {
            Some(reply) => RecordedReply {
                output: shown_thesis(&reply.output, &shown),
                ..reply
            },
            None => self.reply.clone(),
        };
        let (latency_ms, body) = match scripted {
            Some(scripted) => {
                let calls: Vec<(&str, &str)> = scripted
                    .calls
                    .iter()
                    .map(|(name, arguments)| (name.as_str(), arguments.as_str()))
                    .collect();
                let reported = Reported {
                    model: Some(self.slug.clone()),
                    provider: Some(self.provider.clone()),
                };
                (
                    scripted.latency_ms,
                    openrouter_reply(None, &calls, &reported, scripted.usage),
                )
            }
            None => (
                last.latency_ms,
                recorded_reply(&last.output, &self.slug, &self.provider, last.usage),
            ),
        };
        (self.prime)(
            self.gateway.transport_mut(),
            Recorded {
                latency_ms,
                delivery: Delivery::Response {
                    status: 200,
                    body,
                    retry_after_ms: None,
                },
            },
        );
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

impl<T: Transport> ResearchWorker for RecordedWorker<T> {
    fn hand(&mut self, request: &ResearchRequest, parts: &ResearchParts) {
        match request {
            ResearchRequest::Start { run_id, cut_off } => self.start(run_id, *cut_off, parts),
            ResearchRequest::Call(plan) => self.call(plan),
            ResearchRequest::Cancel { run_id } => self.cancel(run_id),
            ResearchRequest::Dispatch(request) => {
                let _unanswered = self.dispatch(request, parts);
            }
            ResearchRequest::Turn(request) => {
                let _unanswered = self.turn(request);
            }
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
                Ready::Dispatched { run_id, results } => ResearchInput::Dispatched {
                    run_id,
                    at,
                    results,
                },
            })
            .collect()
    }
}

#[cfg(test)]
impl<T: Transport> RecordedWorker<T> {
    /// The worker over `bundle` sending every call to `model`, a double that writes its own replies
    /// (DEC-2269): it is never handed the bundle's recorded reply or script. Only this crate's tests
    /// can build one, so no caller outside them names a model transport.
    pub(crate) fn with_model(
        bundle: &Bundle,
        who: WorkerFor,
        sink: Box<dyn ArtifactSink>,
        now: Box<dyn FnMut() -> UtcNanos>,
        model: T,
    ) -> Result<Self, WorkerError> {
        let meter = own_meter(&who);
        Self::with_model_metered(bundle, who, sink, now, model, meter)
    }

    /// As [`RecordedWorker::with_model`], metering through `meter`, whose records a test reads.
    pub(crate) fn with_model_metered(
        bundle: &Bundle,
        who: WorkerFor,
        sink: Box<dyn ArtifactSink>,
        now: Box<dyn FnMut() -> UtcNanos>,
        model: T,
        meter: CellMeter,
    ) -> Result<Self, WorkerError> {
        let caps = Caps::new(bundle.workspace_cap);
        Self::built(bundle, who, sink, now, meter, caps, (model, |_, _| {}))
    }

    pub(crate) fn transport(&self) -> &T {
        self.gateway.transport()
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "ADR-0001 ES-09 excepts tests from the safety-critical denies"
)]
mod model_tests;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "ADR-0001 ES-09 excepts tests from the safety-critical denies"
)]
mod injection_tests;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "ADR-0001 ES-09 excepts tests from the safety-critical denies"
)]
mod bench_tests;

/// The recorded fake, primed by replacing any reply a call never sent took with `reply`.
fn recorded() -> (RecordedTransport, fn(&mut RecordedTransport, Recorded)) {
    (RecordedTransport::new(), |transport, reply| {
        transport.clear_script();
        transport.script(vec![reply]);
    })
}

/// A meter of the worker's own, never held, over an empty agent stream.
fn own_meter(who: &WorkerFor) -> CellMeter {
    CellMeter::new(
        SharedMeter::new(),
        Rc::new(MemoryStore::default()),
        SourceFor {
            workspace: who.workspace.clone(),
            agent: who.agent.clone(),
            environment: Environment::Paper,
        },
        Rc::new(|| UtcNanos::EPOCH),
    )
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

    /// The store's checked read: bytes that do not hash to `digest` are none.
    fn get(&self, digest: &Digest) -> Option<Vec<u8>> {
        get_artifact(&self.0, &ArtifactRef::from_digest(*digest)).ok()
    }
}
