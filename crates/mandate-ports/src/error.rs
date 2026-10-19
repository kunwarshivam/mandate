//! Why a port could not answer.

use mandate_risk::GateError;

/// A port's refusal, each mapped by its host to its own error with the same reason.
#[derive(Debug, thiserror::Error)]
pub enum PortError {
    /// A trusted fact the port needs is missing or cannot be read.
    #[error("{what} is absent")]
    Absent { what: &'static str },
    /// The gate refused to evaluate the proposed order.
    #[error(transparent)]
    Gate(#[from] GateError),
}
