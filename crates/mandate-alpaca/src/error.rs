//! The crate's error enums, each with a stable reason `code()` (ADR-0001 ES-09).
//!
//! No variant carries a URL, a header, a request body, or a response body, so no credential and
//! no broker account number can reach an error message (`AGENTS.md` rule 7, ES-09).

use mandate_num::NumError;
use mandate_time::TimeError;

/// Why a request produced no response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    #[error("the request timed out")]
    Timeout,
    #[error("the connection failed")]
    Connect,
    #[error("the request failed")]
    Request,
    #[error("the path is not a paper trading endpoint; nothing was sent")]
    RefusedPath,
}

impl TransportError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Connect => "connect",
            Self::Request => "request",
            Self::RefusedPath => "refused_path",
        }
    }

    /// Whether the outcome is unknown rather than settled. A refused path never left the process,
    /// so it is the one transport failure that is **not** an unknown broker outcome.
    pub fn is_unknown_outcome(self) -> bool {
        !matches!(self, Self::RefusedPath)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CredentialsError {
    #[error("{variable} is not set or empty")]
    Missing { variable: &'static str },
}

impl CredentialsError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Missing { .. } => "missing",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HttpSetupError {
    #[error("building the HTTPS client failed")]
    Client,
}

impl HttpSetupError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Client => "client",
        }
    }
}

/// Why a response body could not be turned into a broker fact.
///
/// Broker numbers never pass through a float (ES-23): every quantity, price, and amount arrives
/// as raw text and is parsed by `mandate-num`. A value in exponent form, one with more places
/// than the instrument's increment, or one the broker sent as a JSON number that has already been
/// through a float is a typed error here, never a rounded value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    /// The body of every stub in the tests PR (DEC-77, DEC-83).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    #[error("the response is not JSON")]
    NotJson,
    #[error("field {field} is missing")]
    MissingField { field: &'static str },
    #[error("field {field} has the wrong type")]
    WrongType { field: &'static str },
    #[error("field {field} is a JSON number, which has already been through a float")]
    FloatNumber { field: &'static str },
    #[error("field {field} is in exponent form, which is not canonical decimal text")]
    ExponentForm { field: &'static str },
    #[error("field {field} has more places than the instrument's increment allows")]
    TooManyPlaces { field: &'static str },
    #[error("order status {status} is outside the section 5.7 table")]
    UnknownStatus { status: String },
    #[error("field {field} is not interpreted until {story}")]
    NotInterpreted {
        field: &'static str,
        story: &'static str,
    },
    #[error(transparent)]
    Num(#[from] NumError),
    #[error(transparent)]
    Time(#[from] TimeError),
}

impl WireError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unimplemented { .. } => "unimplemented",
            Self::NotJson => "not_json",
            Self::MissingField { .. } => "missing_field",
            Self::WrongType { .. } => "wrong_type",
            Self::FloatNumber { .. } => "float_number",
            Self::ExponentForm { .. } => "exponent_form",
            Self::TooManyPlaces { .. } => "too_many_places",
            Self::UnknownStatus { .. } => "unknown_status",
            Self::NotInterpreted { .. } => "not_interpreted",
            Self::Num(_) => "arithmetic",
            Self::Time(_) => "time",
        }
    }

    /// Every code this crate's wire layer can answer with, in the order [`Self::code`] matches
    /// them. The set is closed, which `hand::every_error_code_is_stable_and_unique` checks.
    pub const CODES: [&'static str; 11] = [
        "unimplemented",
        "not_json",
        "missing_field",
        "wrong_type",
        "float_number",
        "exponent_form",
        "too_many_places",
        "unknown_status",
        "not_interpreted",
        "arithmetic",
        "time",
    ];
}

/// Why one client call did not produce a broker fact.
///
/// A parse failure is deliberately **not** an unknown broker outcome: the broker answered, and
/// an answer this crate cannot read is a loud refusal (DEC-85), not a reason for the executor to
/// go on querying. Only [`Self::Unknown`] means "the outcome is unknown and recovery must query"
/// (task brief interpretation 10).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClientError {
    /// The body of every stub in the tests PR (DEC-77, DEC-83).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    /// The outcome is unknown. This is the only variant [`crate::TradingClient`] passes to the
    /// executor as a [`mandate_executor::BrokerUnknown`].
    #[error(transparent)]
    Unknown(#[from] mandate_executor::BrokerUnknown),
    #[error(transparent)]
    Wire(#[from] WireError),
    #[error(transparent)]
    Transport(#[from] TransportError),
}

impl ClientError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unimplemented { .. } => "unimplemented",
            Self::Unknown(_) => "unknown_outcome",
            Self::Wire(error) => error.code(),
            Self::Transport(error) => error.code(),
        }
    }

    /// The unknown outcome to hand the executor, or `None` when the broker did answer and the
    /// failure is ours to fail loudly on.
    pub fn as_unknown(&self) -> Option<mandate_executor::BrokerUnknown> {
        match self {
            Self::Unknown(unknown) => Some(*unknown),
            Self::Transport(error) if error.is_unknown_outcome() => Some(match error {
                TransportError::Timeout => mandate_executor::BrokerUnknown::Timeout,
                _ => mandate_executor::BrokerUnknown::Transport,
            }),
            _ => None,
        }
    }
}
