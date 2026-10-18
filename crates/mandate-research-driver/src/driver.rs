//! The step machine around the core's research loop (agent harness spec §5.3, §10.3; DEC-996 item
//! 2; DEC-2204).
//!
//! Each input goes to the core. The step's drafts are appended as one batch and every stored event
//! is folded back into the core, in order. Only then are the worker's requests handed on, in the
//! order the core gave them, and the step's other effects returned for the shell to run, so no
//! request is ever acted on, and no notice names a record, before the record is committed
//! (`AGENTS.md` rule 5, HI-2).
//!
//! Three outcomes are not a plain commit:
//!
//! - **An append whose outcome is unknown** (a timeout, a lost connection) holds the driver. Every
//!   other research input is refused with [`DriverError::Held`] until the shell retries the same
//!   one, which the core answers with the same drafts, so the journal's idempotency settles it
//!   (journal spec §5.1). The agent stream has one writer, so the core holds every trading input
//!   behind that batch too, exactly as it does behind a trading batch in doubt: the shell retries
//!   [`Driver::pending`] before it hands the core any other input.
//! - **An append the journal refused** stored nothing. The core forgets the batch
//!   ([`Core::abandon`]), so trading inputs, the kill switch among them, are never held behind a
//!   batch that can never commit, and research halts in this process until a restart
//!   (`AGENTS.md` rules 3 and 13).
//! - **A reply that is not this batch** (other events, fewer, or out of order), or a committed event
//!   the core cannot fold back: the core no longer matches the journal. The driver halts with
//!   [`DriverError::Diverged`], and the shell restarts the agent process, which rebuilds the core
//!   from the journal (§5.4).

use mandate_runtime::{
    Effect, EventDraft, FoldedEvent, Ports, ResearchContext, ResearchInput, ResearchRequest,
    ResearchState, ResearchStep, RuntimeError, RuntimeState, abandon_refused_research, fold,
    handle_research,
};

/// The core as the driver sees it. [`RuntimeCore`] is the one production implementation.
pub trait Core {
    /// One research input's step.
    ///
    /// # Errors
    /// The core's refusal, whatever it is.
    fn step(&mut self, input: ResearchInput) -> Result<ResearchStep, RuntimeError>;

    /// One stored event, folded back as the shell's tailer folds it.
    ///
    /// # Errors
    /// The core's refusal to fold its own event.
    fn fold(&mut self, event: &FoldedEvent) -> Result<(), RuntimeError>;

    /// The journal refused the step's batch and stored none of it: forget the batch, so no other
    /// input waits behind it. Returns whether a research batch was in doubt.
    fn abandon(&mut self) -> bool;
}

/// One step's batch as the journal writer takes it (DEC-2206 items 4 and 8): the drafts, the
/// artifacts the core made for them, which the writer stores before it appends, and the run every
/// draft belongs to, which the writer puts on each envelope as its `correlation_id`.
#[derive(Debug, Clone, Copy)]
pub struct StepBatch<'a> {
    pub drafts: &'a [EventDraft],
    pub artifacts: &'a [Vec<u8>],
    pub correlation_id: Option<&'a str>,
}

/// The agent stream's writer: one step's drafts as one `append` batch.
pub trait JournalWriter {
    /// Stores the batch's artifacts, then appends its drafts. Returns the stored events, in order,
    /// whether this append committed them or an earlier one with the same drafts did (journal spec
    /// §5.1's `AlreadyCommitted`).
    ///
    /// # Errors
    /// [`AppendFailure`] when the batch is not known to be stored.
    fn append(&mut self, batch: &StepBatch<'_>) -> Result<Vec<FoldedEvent>, AppendFailure>;
}

/// The research worker: retrieval, the gateway call, and cancellation.
pub trait Worker {
    fn hand(&mut self, request: &ResearchRequest);
}

/// Why an append did not report the batch stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppendFailure {
    /// The outcome is unknown: the batch may or may not be stored.
    Unresolved,
    /// The journal refused the batch with this reason code, and stored nothing.
    Refused { code: String },
}

/// What one step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stepped {
    /// The step's drafts are committed and folded back; these requests went to the worker, and
    /// these other effects are the shell's to run now, in order.
    Handed {
        requests: Vec<ResearchRequest>,
        effects: Vec<Effect>,
    },
    /// The append's outcome is unknown; retry the same input before any other.
    Held,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DriverError {
    #[error("the core refused the input: {0}")]
    Core(RuntimeError),
    #[error("an append is unresolved; retry its input first")]
    Held,
    #[error("the journal refused the research batch ({code}); research is halted in this process")]
    Refused { code: String },
    #[error("research is halted in this process")]
    Halted,
    #[error("the core no longer matches the journal ({what}); restart the agent process")]
    Diverged { what: String },
}

/// The driver's own state: the input whose append is in doubt, and whether research is halted.
#[derive(Debug, Clone, Default)]
pub struct Driver {
    pending: Option<ResearchInput>,
    halted: bool,
}

impl Driver {
    pub fn new() -> Self {
        Self::default()
    }

    /// The input whose append is unresolved, which the shell must retry before any other.
    pub fn pending(&self) -> Option<&ResearchInput> {
        self.pending.as_ref()
    }

    pub fn halted(&self) -> bool {
        self.halted
    }

    /// One input through the core, the journal, and the worker, in that order.
    ///
    /// # Errors
    /// [`DriverError`]: the core's refusal, another input while one is held, the journal's refusal,
    /// or any input once halted.
    pub fn step(
        &mut self,
        input: ResearchInput,
        core: &mut dyn Core,
        journal: &mut dyn JournalWriter,
        worker: &mut dyn Worker,
    ) -> Result<Stepped, DriverError> {
        if self.halted {
            return Err(DriverError::Halted);
        }
        if self.pending.as_ref().is_some_and(|held| *held != input) {
            return Err(DriverError::Held);
        }
        let step = core.step(input.clone()).map_err(DriverError::Core)?;
        let mut drafts = Vec::new();
        let mut effects = Vec::new();
        for effect in step.effects {
            match effect {
                Effect::Journal(draft) => drafts.push(draft),
                other => effects.push(other),
            }
        }
        if !drafts.is_empty() {
            let batch = StepBatch {
                drafts: &drafts,
                artifacts: &step.artifacts,
                correlation_id: step.correlation_id.as_deref(),
            };
            match journal.append(&batch) {
                Ok(stored) => {
                    let same = stored.len() == drafts.len()
                        && stored
                            .iter()
                            .zip(&drafts)
                            .all(|(row, draft)| row.event_id == draft.event_id);
                    if !same {
                        self.halted = true;
                        return Err(DriverError::Diverged {
                            what: "the journal's rows are not the step's drafts".to_owned(),
                        });
                    }
                    for event in &stored {
                        if let Err(error) = core.fold(event) {
                            self.halted = true;
                            return Err(DriverError::Diverged {
                                what: format!(
                                    "a committed {} did not fold: {error}",
                                    event.event_type
                                ),
                            });
                        }
                    }
                }
                Err(AppendFailure::Unresolved) => {
                    self.pending = Some(input);
                    return Ok(Stepped::Held);
                }
                Err(AppendFailure::Refused { code }) => {
                    core.abandon();
                    self.pending = None;
                    self.halted = true;
                    return Err(DriverError::Refused { code });
                }
            }
        }
        self.pending = None;
        for request in &step.requests {
            worker.hand(request);
        }
        Ok(Stepped::Handed {
            requests: step.requests,
            effects,
        })
    }
}

/// The production core: the runtime's state, the research loop's state, and what a step reads.
pub struct RuntimeCore<'a> {
    pub state: &'a mut RuntimeState,
    pub research: &'a mut ResearchState,
    pub ports: &'a Ports<'a>,
    pub context: &'a ResearchContext<'a>,
}

impl Core for RuntimeCore<'_> {
    fn step(&mut self, input: ResearchInput) -> Result<ResearchStep, RuntimeError> {
        handle_research(self.state, self.research, input, self.ports, self.context)
    }

    fn fold(&mut self, event: &FoldedEvent) -> Result<(), RuntimeError> {
        fold(self.state, event)?;
        self.research.fold(event)
    }

    fn abandon(&mut self) -> bool {
        abandon_refused_research(self.state, self.research)
    }
}
