//! Research in the runtime process (DEC-2208): the research driver (DEC-2204) behind a deny-all
//! default, its batches journaled on the agent stream with the step's run as `correlation_id` and
//! the core's own artifacts stored first, and its worker the only place research does I/O.

use std::collections::{BTreeSet, VecDeque};

use mandate_canon::{Digest, Object};
use mandate_journal::{AppendOutcome, Environment, StreamId};
use mandate_process::Input as Lifecycled;
use mandate_research::{
    AdmissionFacts, LineageState, PolicyOverlay, ValidatedMandate, WorkingUniverse,
};
use mandate_research_driver::{
    AppendFailure, Driver, DriverError, JournalWriter, RuntimeCore, StepBatch, Stepped, Worker,
};
use mandate_research_run::Symbol;
use mandate_runtime::{
    ActiveThesis, Effect, EventDraft, FoldedEvent, Preconditions, ResearchConfig, ResearchContext,
    ResearchInput, ResearchRequest, ResearchState, RiskClock, RuntimeState, tailed,
};
use mandate_store::Store;
use mandate_store::envelope::{Envelope, Fields, draft_bytes};
use mandate_time::UtcNanos;

use super::{Alert, Refs, RuntimeProcess, VERSION};

/// What a research wiring reads each pass besides the core's state: every member a fold of the
/// journal or a pinned configuration object, as the core's `ResearchContext` names them.
#[derive(Clone)]
pub struct ResearchParts {
    pub preconditions: Preconditions,
    pub mandate: ValidatedMandate,
    pub overlay: PolicyOverlay,
    pub universe: WorkingUniverse,
    pub facts: AdmissionFacts,
    pub lineages: LineageState,
    pub active: Vec<ActiveThesis>,
    pub last_reservation: Option<RiskClock>,
    pub data_symbols: BTreeSet<Symbol>,
}

/// The research context of one pass; `None` runs no research on that pass.
pub trait ResearchSource {
    fn parts(&self, state: &RuntimeState) -> Option<ResearchParts>;
}

/// The research worker: retrieval and the gateway's calls. It stores the artifacts its answers name
/// (retrieved records, prompts, responses) before it hands an answer back.
pub trait ResearchWorker {
    /// Takes one of the core's requests, after the batch that authorises it committed, with the
    /// parts of the pass that made it, which a run's retrieval reads its scope from (DEC-2210).
    fn hand(&mut self, request: &ResearchRequest, parts: &ResearchParts);

    /// The answers ready since the last pass, in the order they arrived, each that carries a risk
    /// clock stamped `at`, the risk clock of the pass it is handed in (DEC-2210 item 3).
    fn answers(&mut self, at: RiskClock) -> Vec<ResearchInput>;
}

/// Where the artifacts the core makes are stored (DEC-2206 item 4): content-addressed, put before
/// the batch that names them. Whether the bytes are stored.
pub trait ArtifactSink {
    fn put(&mut self, bytes: &[u8]) -> bool;

    /// The bytes stored under `digest`, checked against it, or `None` when the sink gives none: a
    /// sink that only stores gives none, so a turn whose transcript it holds is answered nothing
    /// (DEC-2264).
    fn get(&self, _digest: &Digest) -> Option<Vec<u8>> {
        None
    }
}

/// One deployment's research, as its wiring gives it (DEC-2208 item 2).
pub struct ResearchWiring {
    pub config: ResearchConfig,
    pub source: Box<dyn ResearchSource>,
    pub worker: Box<dyn ResearchWorker>,
    pub artifacts: Box<dyn ArtifactSink>,
}

/// The process's research: its wiring, the core's research state, the driver, the worker's
/// answers not yet handed, and of the batch in doubt the event time its retry carries again, so the
/// journal is offered the same bytes (journal spec §5.1), and the context it was built with, so a
/// source with nothing to give on a later pass never holds the retry (DEC-2208 item 4).
pub(super) struct Research {
    wiring: ResearchWiring,
    state: ResearchState,
    driver: Driver,
    inbox: VecDeque<ResearchInput>,
    doubted_at: Option<UtcNanos>,
    doubted_parts: Option<ResearchParts>,
}

impl Research {
    pub(super) fn new(wiring: ResearchWiring) -> Self {
        Self {
            wiring,
            state: ResearchState::new(),
            driver: Driver::new(),
            inbox: VecDeque::new(),
            doubted_at: None,
            doubted_parts: None,
        }
    }
}

/// What one research input did to the process.
enum Outcome {
    /// Handed, refused, or halted: the pass may go on.
    Done,
    /// The batch is in doubt, or the source has no context to hand it with: the input waits, and
    /// nothing else is handed until it is.
    Held,
    /// The process no longer matches the journal, or was fenced: it has stopped.
    Stopped,
}

/// Why an append ended the process rather than the step.
#[derive(Clone, Copy)]
enum Fatal {
    Fenced,
    Failed,
}

/// The driver's worker over the wiring's, with the parts of the pass it is handed in.
struct Hands<'a>(&'a mut dyn ResearchWorker, &'a ResearchParts);

impl Worker for Hands<'_> {
    fn hand(&mut self, request: &ResearchRequest) {
        self.0.hand(request, self.1);
    }
}

/// The agent stream's writer over the process's store (DEC-2208 item 5).
struct Writer<'a, S> {
    store: &'a mut S,
    agent: &'a StreamId,
    head: u64,
    epoch: u64,
    environment: Environment,
    actor_id: &'a str,
    written_at: UtcNanos,
    now: UtcNanos,
    refs: &'a dyn Refs,
    artifacts: &'a mut dyn ArtifactSink,
    /// Whether this append is the retry of a batch in doubt, which the journal may already hold.
    retry: bool,
    fatal: Option<Fatal>,
}

impl<S: Store> JournalWriter for Writer<'_, S> {
    fn append(&mut self, batch: &StepBatch<'_>) -> Result<Vec<FoldedEvent>, AppendFailure> {
        for artifact in batch.artifacts {
            if !self.artifacts.put(artifact) {
                return Err(if self.retry {
                    AppendFailure::Unresolved
                } else {
                    AppendFailure::Refused {
                        code: "artifact_unstored".to_owned(),
                    }
                });
            }
        }
        let envelope = Envelope {
            stream: self.agent.as_str(),
            environment: self.environment,
            actor_kind: "agent",
            actor_id: self.actor_id,
            build: concat!("mandate-daemon/", env!("CARGO_PKG_VERSION")),
            version: VERSION,
            event_time: self.written_at,
        };
        let bytes: Option<Vec<Vec<u8>>> = batch
            .drafts
            .iter()
            .map(|draft| bytes(&envelope, draft, self.refs, batch.correlation_id))
            .collect();
        let Some(bytes) = bytes else {
            return Err(AppendFailure::Refused {
                code: "no_config_refs".to_owned(),
            });
        };
        match self
            .store
            .append(self.agent, self.head, self.epoch, self.now, &bytes)
        {
            AppendOutcome::Committed(stored) | AppendOutcome::AlreadyCommitted(stored) => {
                tailed(&stored).map_err(|_| {
                    self.fatal = Some(Fatal::Failed);
                    AppendFailure::Unresolved
                })
            }
            AppendOutcome::Unavailable | AppendOutcome::Ambiguous => Err(AppendFailure::Unresolved),
            AppendOutcome::Invalid { error, .. } => Err(AppendFailure::Refused {
                code: error.reason.code().to_owned(),
            }),
            AppendOutcome::Fenced { .. } => {
                self.fatal = Some(Fatal::Fenced);
                Err(AppendFailure::Unresolved)
            }
            AppendOutcome::HeadMismatch { .. } | AppendOutcome::IdempotencyConflict { .. } => {
                self.fatal = Some(Fatal::Failed);
                Err(AppendFailure::Unresolved)
            }
        }
    }
}

/// One draft's bytes, with the schema version and references `refs` gives it and the step's run.
fn bytes(
    envelope: &Envelope<'_>,
    draft: &EventDraft,
    refs: &dyn Refs,
    correlation_id: Option<&str>,
) -> Option<Vec<u8>> {
    let (schema_version, config_refs): (u64, Object) = refs.refs(draft)?;
    draft_bytes(
        envelope,
        &Fields {
            event_id: &draft.event_id.0,
            event_type: &draft.event_type,
            schema_version,
            causation_id: draft.causation_id.as_ref().map(|id| id.0.as_str()),
            correlation_id,
            config_refs: &config_refs,
            payload: &draft.payload,
        },
    )
}

impl<S: Store> RuntimeProcess<S> {
    /// The research input in doubt, handed again before anything else (DEC-2208 item 4). Whether
    /// nothing research wrote is in doubt any more.
    pub(super) fn research_retry(&mut self, now: UtcNanos, alerts: &mut Vec<Alert>) -> bool {
        let Some((pending, at)) = self.research.as_ref().and_then(|research| {
            research
                .driver
                .pending()
                .cloned()
                .map(|pending| (pending, research.doubted_at.unwrap_or(now)))
        }) else {
            return true;
        };
        matches!(self.research_hand(now, at, pending, alerts), Outcome::Done)
    }

    /// The research pass (DEC-2208 item 3): `Poll` at the risk clock, then the worker's answers in
    /// arrival order, until one is in doubt.
    pub(super) fn research_pass(&mut self, now: UtcNanos, alerts: &mut Vec<Alert>) {
        let Some(at) = self.state.risk_clock() else {
            return;
        };
        let Some(research) = self.research.as_mut() else {
            return;
        };
        if research.driver.halted() {
            return;
        }
        let answers = research.wiring.worker.answers(at);
        research.inbox.extend(answers);
        if !matches!(
            self.research_hand(now, now, ResearchInput::Poll(at), alerts),
            Outcome::Done
        ) {
            return;
        }
        loop {
            let Some(next) = self
                .research
                .as_mut()
                .and_then(|research| research.inbox.pop_front())
            else {
                return;
            };
            match self.research_hand(now, now, next.clone(), alerts) {
                Outcome::Done => {}
                Outcome::Held => {
                    if let Some(research) = self.research.as_mut()
                        && research.driver.pending() != Some(&next)
                    {
                        research.inbox.push_front(next);
                    }
                    return;
                }
                Outcome::Stopped => return,
            }
        }
    }

    /// One research input through the driver, its drafts written at `written_at`.
    fn research_hand(
        &mut self,
        now: UtcNanos,
        written_at: UtcNanos,
        input: ResearchInput,
        alerts: &mut Vec<Alert>,
    ) -> Outcome {
        let Some(agent) = self.agent.clone() else {
            return Outcome::Stopped;
        };
        let Some(research) = self.research.as_mut() else {
            return Outcome::Done;
        };
        let retry = research.driver.pending().is_some();
        let Some(parts) = research
            .wiring
            .source
            .parts(&self.state)
            .or_else(|| research.doubted_parts.clone().filter(|_| retry))
        else {
            return Outcome::Held;
        };
        let context = ResearchContext {
            config: Some(&research.wiring.config),
            preconditions: &parts.preconditions,
            mandate: &parts.mandate,
            overlay: &parts.overlay,
            universe: &parts.universe,
            facts: &parts.facts,
            lineages: &parts.lineages,
            active_theses: &parts.active,
            last_reservation: parts.last_reservation,
            data_symbols: &parts.data_symbols,
        };
        let ports = self.wiring.ports();
        let head = self.state.head(agent.as_str()).map_or(0, |seq| seq.0);
        let actor_id = self.state.deployment().agent.0.clone();
        let mut writer = Writer {
            store: &mut self.store,
            agent: &agent,
            head,
            epoch: self.epoch,
            environment: self.environment,
            actor_id: &actor_id,
            written_at,
            now,
            refs: self.wiring.refs.as_ref(),
            artifacts: research.wiring.artifacts.as_mut(),
            retry,
            fatal: None,
        };
        let mut core = RuntimeCore {
            state: &mut self.state,
            research: &mut research.state,
            ports: &ports,
            context: &context,
        };
        let mut hands = Hands(research.wiring.worker.as_mut(), &parts);
        let stepped = research
            .driver
            .step(input, &mut core, &mut writer, &mut hands);
        let fatal = writer.fatal;
        research.doubted_at = research.driver.pending().map(|_| written_at);
        research.doubted_parts = research.driver.pending().map(|_| parts.clone());
        match (fatal, stepped) {
            (Some(Fatal::Fenced), _) => {
                self.lifecycle.observe(now, Lifecycled::Fenced);
                Outcome::Stopped
            }
            (Some(Fatal::Failed), _) | (None, Err(DriverError::Diverged { .. })) => {
                self.lifecycle.observe(now, Lifecycled::Failed);
                Outcome::Stopped
            }
            (None, Ok(Stepped::Handed { effects, .. })) => {
                for effect in effects {
                    if let Effect::Notify(notice) = effect {
                        alerts.push(Alert {
                            key: notice.message_key,
                            subject: Some(notice.subject_event.0),
                        });
                    }
                }
                Outcome::Done
            }
            (None, Ok(Stepped::Held)) => Outcome::Held,
            (None, Err(DriverError::Refused { .. })) => {
                alerts.push(Alert {
                    key: "research_halted",
                    subject: None,
                });
                Outcome::Done
            }
            (None, Err(DriverError::Core(_))) => {
                alerts.push(Alert {
                    key: "research_input_refused",
                    subject: None,
                });
                Outcome::Done
            }
            (None, Err(DriverError::Held)) => Outcome::Held,
            (None, Err(DriverError::Halted)) => Outcome::Done,
        }
    }
}
