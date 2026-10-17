//! Every refusal this crate makes, each with a stable reason code (ADR-0001 ES-09). No variant
//! carries vendor text, a URL, or a credential, so an error is safe to log.

/// Why a load, a fetch, an ingestion, or a read was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
    #[error("the allowlist file is malformed")]
    AllowlistMalformed,
    #[error("the allowlist names a source twice")]
    DuplicateSource,
    #[error("an allowlist endpoint is not an https host and path prefix")]
    BadEndpoint,
    #[error("the source is not on the allowlist in effect")]
    SourceNotAllowlisted,
    #[error("the URL is outside every endpoint of its source")]
    EndpointNotAllowlisted,
    #[error("the query names an allowlist version that is not in effect")]
    AllowlistVersionMismatch,
    #[error("the source's kind does not match the parser, store, or read")]
    WrongKind,
    #[error("the response is larger than its source's cap")]
    ResponseTooLarge,
    #[error("the vendor payload is malformed")]
    Malformed,
    #[error("a structured vendor field is outside its allowed form")]
    BadField,
    #[error("the item names more instruments than the cap")]
    TooManyTags,
    #[error("the item's source time is later than its receipt")]
    FutureDated,
    #[error("a stored item would be revised in place")]
    RevisedInPlace,
    #[error("the request comes before its source's minimum interval")]
    TooSoon,
    #[error("the source answered with a status other than 200")]
    Status,
    #[error("the transport failed")]
    Transport,
    #[error("a time computation is out of range")]
    Time,
}

impl SourceError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(self) -> &'static str {
        match self {
            Self::AllowlistMalformed => "allowlist_malformed",
            Self::DuplicateSource => "duplicate_source",
            Self::BadEndpoint => "bad_endpoint",
            Self::SourceNotAllowlisted => "source_not_allowlisted",
            Self::EndpointNotAllowlisted => "endpoint_not_allowlisted",
            Self::AllowlistVersionMismatch => "allowlist_version_mismatch",
            Self::WrongKind => "wrong_kind",
            Self::ResponseTooLarge => "response_too_large",
            Self::Malformed => "malformed",
            Self::BadField => "bad_field",
            Self::TooManyTags => "too_many_tags",
            Self::FutureDated => "future_dated",
            Self::RevisedInPlace => "revised_in_place",
            Self::TooSoon => "too_soon",
            Self::Status => "status",
            Self::Transport => "transport",
            Self::Time => "time",
        }
    }
}
