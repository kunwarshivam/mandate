//! The crate's one error enum, with a stable `code()` per variant (ADR-0001 ES-09).

/// Why the worker could not build a value. None of these is a model's fault: a model's bad output
/// is an [`crate::OutputRejection`], and a gateway's failure a [`mandate_gateway::CallRecord`]'s outcome. These say the
/// worker's own inputs were not usable, and the run then makes no call (`AGENTS.md` rule 3).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResearchRunError {
    /// The body of every stub in the tests commit (DEC-77, DEC-990 item 3).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    /// An id or symbol that is empty, too long, or outside its grammar.
    #[error("{what} is not a valid identifier")]
    InvalidId { what: &'static str },
    /// The call plan names a different number of journaled observations than the retrieval holds,
    /// so some item would reach the model without its `ObservationRecorded` (HI-2).
    #[error("the retrieval holds {items} items but {observations} observations were journaled")]
    ObservationsMismatch { items: usize, observations: usize },
    /// A window, a deadline, or an instant outside the representable range.
    #[error("an instant outside the representable range")]
    TimeOutOfRange,
    /// An agentic run's step out of its order: a turn after the run stopped, a reply for a turn
    /// not asked for, results that are not the dispatch's calls, or a second end (§6.9).
    #[error("out of the loop's order: {what}")]
    LoopOrder { what: &'static str },
    /// An agentic loop whose bounds or declarations no registry may pin (DEC-2262 item 5).
    #[error("the agentic loop is not one a registry may pin")]
    UnpinnableLoop,
}

impl ResearchRunError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unimplemented { .. } => "unimplemented",
            Self::InvalidId { .. } => "invalid_id",
            Self::ObservationsMismatch { .. } => "observations_mismatch",
            Self::TimeOutOfRange => "time_out_of_range",
            Self::LoopOrder { .. } => "loop_order",
            Self::UnpinnableLoop => "unpinnable_loop",
        }
    }
}
