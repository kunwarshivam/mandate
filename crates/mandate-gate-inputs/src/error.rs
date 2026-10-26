//! Why a reviewed input cannot be read.

use mandate_executor::ExecutorError;
use mandate_gate_context::RuleSetError;
use mandate_num::NumError;

/// A reviewed input the gate cannot use: each refuses the run rather than guessing.
#[derive(Debug, thiserror::Error)]
pub enum InputError {
    /// A member is missing, has the wrong type, or holds a value the reviewed rules refuse.
    #[error("missing or unusable: {what}")]
    Missing { what: &'static str },
    #[error(transparent)]
    Num(#[from] NumError),
    #[error(transparent)]
    Executor(#[from] ExecutorError),
}

impl From<RuleSetError> for InputError {
    fn from(error: RuleSetError) -> Self {
        match error {
            RuleSetError::Missing { what } => Self::Missing { what },
            RuleSetError::Num(error) => Self::Num(error),
        }
    }
}

pub(crate) fn missing(what: &'static str) -> InputError {
    InputError::Missing { what }
}
