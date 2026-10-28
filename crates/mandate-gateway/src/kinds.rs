//! The closed vocabularies of a call (inference spec §3.1 to §3.3), each with the stable code the
//! journal records (journal spec §9.19).

/// Why a call is made (inference spec §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Purpose {
    Research,
    LlmSignal,
    FastSignal,
    Compiler,
    /// The platform's call that evaluates a registry entry: the §4.3 gate and the live start's
    /// identity probe (DEC-2216). It names no agent and never crosses the wire.
    Evaluation,
}

impl Purpose {
    pub fn code(self) -> &'static str {
        match self {
            Self::Research => "research",
            Self::LlmSignal => "llm_signal",
            Self::FastSignal => "fast_signal",
            Self::Compiler => "compiler",
            Self::Evaluation => "evaluation",
        }
    }

    /// Whether a call of this purpose names an agent: every purpose's but the compiler's and an
    /// evaluation's (rule 156).
    pub fn names_agent(self) -> bool {
        !matches!(self, Self::Compiler | Self::Evaluation)
    }
}

/// Whose provider key pays (inference spec §7.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyOwner {
    Platform,
    Customer,
}

impl KeyOwner {
    pub fn code(self) -> &'static str {
        match self {
            Self::Platform => "platform",
            Self::Customer => "customer",
        }
    }
}

/// How a call ended (inference spec §3.3): `Ok` or one error kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Outcome {
    Ok,
    PolicyDenied,
    BudgetExhausted,
    RateLimitedLocal,
    MeterUnavailable,
    InputRejected,
    ModelWithdrawn,
    DeadlineExceeded,
    ProviderUnavailable,
    RateLimitedProvider,
    CredentialInvalid,
    ContentRefused,
    SchemaInvalid,
    IdentityMismatch,
}

impl Outcome {
    pub const ALL: [Self; 14] = [
        Self::Ok,
        Self::PolicyDenied,
        Self::BudgetExhausted,
        Self::RateLimitedLocal,
        Self::MeterUnavailable,
        Self::InputRejected,
        Self::ModelWithdrawn,
        Self::DeadlineExceeded,
        Self::ProviderUnavailable,
        Self::RateLimitedProvider,
        Self::CredentialInvalid,
        Self::ContentRefused,
        Self::SchemaInvalid,
        Self::IdentityMismatch,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::PolicyDenied => "policy_denied",
            Self::BudgetExhausted => "budget_exhausted",
            Self::RateLimitedLocal => "rate_limited_local",
            Self::MeterUnavailable => "meter_unavailable",
            Self::InputRejected => "input_rejected",
            Self::ModelWithdrawn => "model_withdrawn",
            Self::DeadlineExceeded => "deadline_exceeded",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::RateLimitedProvider => "rate_limited_provider",
            Self::CredentialInvalid => "credential_invalid",
            Self::ContentRefused => "content_refused",
            Self::SchemaInvalid => "schema_invalid",
            Self::IdentityMismatch => "identity_mismatch",
        }
    }

    /// The six refusals: nothing leaves the deployment, nothing is reserved, nothing costs
    /// (inference spec §3.3; journal spec rules 73 and 149).
    pub fn is_refusal(self) -> bool {
        matches!(
            self,
            Self::PolicyDenied
                | Self::BudgetExhausted
                | Self::RateLimitedLocal
                | Self::MeterUnavailable
                | Self::InputRejected
                | Self::ModelWithdrawn
        )
    }
}
