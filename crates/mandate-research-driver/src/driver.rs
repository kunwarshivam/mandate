//! The step machine around the core's research loop (agent harness spec §5.3, §10.3; DEC-996 item
//! 2; DEC-2204).
//!
//! Each input goes to the core. The step's drafts are appended as one batch and every stored event
//! is folded back into the core, in order. Only then are the worker's requests handed on, in the
//! order the core gave them, and the step's other effects returned for the shell to run, so no
//! request is ever acted on, and no notice names a record, before the record is committed
//! (`AGENTS.md` rule 5, HI-2).
//!
//! An append whose outcome is unknown (a timeout, a lost connection) holds the driver: every other
//! input is refused with [`DriverError::Held`] until the shell retries the same one, which the core
//! answers with the same drafts, so the journal's idempotency settles it (journal spec §5.1). An
//! append the journal refuses is a writer defect that no retry can mend, so the driver halts:
//! research stops in this process until a restart, and trading, which never waits on research,
//! is untouched (HI-7, `AGENTS.md` rule 3).

use mandate_runtime::{
    Effect, EventDraft, FoldedEvent, Ports, ResearchContext, ResearchInput, ResearchRequest,
    ResearchState, ResearchStep, RuntimeError, RuntimeState, fold, handle_research,
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
}

/// The agent stream's writer: one step's drafts as one `append` batch.
pub trait JournalWriter {
    /// The stored events, in order, whether this append committed them or an earlier one with the
    /// same drafts did (journal spec §5.1's `AlreadyCommitted`).
    ///
    /// # Errors
    /// [`AppendFailure`] when the batch is not known to be stored.
    fn append(&mut self, drafts: &[EventDraft]) -> Result<Vec<FoldedEvent>, AppendFailure>;
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
    #[error("research is halted in this process after a refused append")]
    Halted,
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
            match journal.append(&drafts) {
                Ok(stored) => {
                    for event in &stored {
                        if let Err(error) = core.fold(event) {
                            self.halted = true;
                            return Err(DriverError::Core(error));
                        }
                    }
                }
                Err(AppendFailure::Unresolved) => {
                    self.pending = Some(input);
                    return Ok(Stepped::Held);
                }
                Err(AppendFailure::Refused { code }) => {
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
}
