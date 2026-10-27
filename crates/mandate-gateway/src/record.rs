//! A call's record (inference spec §3.2, §3.6): what the caller journals as
//! `ModelInvocationRecorded` (journal spec §9.19 on the agent stream, §9.9 for the compiler on the
//! control stream) before it uses the output, and the artifacts it names.

use mandate_canon::{Digest, Value};
use mandate_time::UtcNanos;

use crate::guard::GuardRule;
use crate::kinds::{Outcome, Purpose};
use crate::money::{Usage, Usd6};
use crate::registry::{ModelRef, int, object, sampling};

/// What a candidate in a research response is (journal spec rule 151).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CandidateKind {
    Thesis,
    Invalidation,
}

impl CandidateKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::Thesis => "thesis",
            Self::Invalidation => "invalidation",
        }
    }
}

/// A harness check's verdict on one candidate (journal spec rule 151): the harness lists the
/// theses first, then the invalidations, each in response order, with a result that fits the kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verdict {
    pub kind: CandidateKind,
    pub result: VerdictResult,
}

/// A harness check's result on one candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerdictResult {
    Judged,
    UnknownInstrument,
    UnknownEvidence,
    DuplicateInstrument,
    ForbiddenLanguage,
    RevisionsNotEnabled,
    UnknownThesis,
}

impl VerdictResult {
    /// Whether this result can be a `kind`'s (journal spec rule 151).
    pub fn fits(self, kind: CandidateKind) -> bool {
        match self {
            Self::Judged | Self::ForbiddenLanguage => true,
            Self::UnknownThesis => kind == CandidateKind::Invalidation,
            Self::UnknownInstrument
            | Self::UnknownEvidence
            | Self::DuplicateInstrument
            | Self::RevisionsNotEnabled => kind == CandidateKind::Thesis,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Judged => "judged",
            Self::UnknownInstrument => "unknown_instrument",
            Self::UnknownEvidence => "unknown_evidence",
            Self::DuplicateInstrument => "duplicate_instrument",
            Self::ForbiddenLanguage => "forbidden_language",
            Self::RevisionsNotEnabled => "revisions_not_enabled",
            Self::UnknownThesis => "unknown_thesis",
        }
    }
}

/// One call's record. `output` is present only for `Ok`: a validated output of the pinned model
/// (inference spec §1.2); every other outcome has none, which the order builder treats as missing
/// (INF-4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallRecord {
    pub call_id: String,
    pub agent_id: Option<String>,
    pub purpose: Purpose,
    pub model: ModelRef,
    pub endpoint: String,
    /// The canonical request's digest; `None` exactly when the guard refused the prompt (journal
    /// spec rule 148).
    pub request_digest: Option<Digest>,
    /// The rule a refused prompt broke; `Some` exactly for `input_rejected`.
    pub guard_rule: Option<GuardRule>,
    pub seed: Option<u64>,
    pub prompt_ref: Option<Digest>,
    pub response_ref: Option<Digest>,
    pub reported_identity: Option<String>,
    pub provider_request_id: Option<String>,
    pub outcome: Outcome,
    pub attempts: u32,
    pub tokens: Usage,
    pub cost: Usd6,
    pub price_table_ref: Digest,
    pub cache_hit: bool,
    pub deadline: UtcNanos,
    pub completed_at: UtcNanos,
    pub reservation: Option<String>,
    pub observations: Vec<String>,
    pub output: Option<Value>,
    /// For an `ok` tool-call reply, its validated tool calls in reply order, and no `output`
    /// (inference spec §3.2); empty otherwise. The caller journals one `ToolCallRecorded` for each.
    pub tool_calls: Vec<crate::tools::ToolCall>,
    /// The bytes `prompt_ref` names, for the caller to store as an artifact before it journals the
    /// record; `None` exactly when the guard refused the prompt (journal spec rule 148).
    pub prompt: Option<Vec<u8>>,
    /// The bytes `response_ref` names, when the response is kept.
    pub response: Option<Vec<u8>>,
}

impl CallRecord {
    /// The agent-stream payload of journal spec §9.19, with the harness's verdicts, one per
    /// candidate in response order (rule 151). `observations` are sorted and de-duplicated
    /// (rule 150).
    pub fn agent_payload(&self, verdicts: &[Verdict]) -> Result<Value, PayloadError> {
        self.check(verdicts)?;
        let mut observations = self.observations.clone();
        observations.sort_unstable();
        observations.dedup();
        let verdicts = verdicts
            .iter()
            .enumerate()
            .map(|(i, v)| {
                object([
                    ("candidate", int(u64::try_from(i).unwrap_or(u64::MAX))),
                    ("kind", Value::Str(v.kind.code().to_owned())),
                    ("result", Value::Str(v.result.code().to_owned())),
                ])
            })
            .collect();
        Ok(object([
            ("attempts", int(u64::from(self.attempts))),
            ("cache_hit", Value::Bool(self.cache_hit)),
            ("call_id", Value::Str(self.call_id.clone())),
            ("completed_at", Value::Str(self.completed_at.to_string())),
            ("cost_usd", Value::Str(self.cost.to_string())),
            ("deadline", Value::Str(self.deadline.to_string())),
            ("endpoint", Value::Str(self.endpoint.clone())),
            (
                "guard_rule",
                self.guard_rule
                    .map_or(Value::Null, |r| Value::Str(r.code().to_owned())),
            ),
            ("model", model_value(&self.model)),
            (
                "observations",
                Value::Array(observations.into_iter().map(Value::Str).collect()),
            ),
            ("outcome", Value::Str(self.outcome.code().to_owned())),
            (
                "price_table_ref",
                Value::Str(reference(self.price_table_ref)),
            ),
            (
                "prompt_ref",
                self.prompt_ref
                    .map_or(Value::Null, |d| Value::Str(reference(d))),
            ),
            (
                "provider_request_id",
                text(self.provider_request_id.as_deref()),
            ),
            ("purpose", Value::Str(self.purpose.code().to_owned())),
            ("reported_identity", text(self.reported_identity.as_deref())),
            (
                "request_digest",
                self.request_digest
                    .map_or(Value::Null, |d| Value::Str(reference(d))),
            ),
            ("reservation", text(self.reservation.as_deref())),
            (
                "response_ref",
                self.response_ref
                    .map_or(Value::Null, |d| Value::Str(reference(d))),
            ),
            ("sampling", sampling(self.seed)),
            ("tokens", tokens_value(self.tokens)),
            ("verdicts", Value::Array(verdicts)),
        ]))
    }
}

impl CallRecord {
    /// Journal spec rules 148 and 151, and the agent stream's purposes, as the caller would hit
    /// them at append: checked here so a payload that could not be journaled is never built.
    fn check(&self, verdicts: &[Verdict]) -> Result<(), PayloadError> {
        if self.purpose == Purpose::Compiler {
            return Err(PayloadError::CompilerOnAgentStream);
        }
        if !self.purpose.names_agent() {
            return Err(PayloadError::NoAgentOnAgentStream);
        }
        let rejected = self.outcome == Outcome::InputRejected;
        if self.prompt_ref.is_none() != rejected
            || self.request_digest.is_none() != rejected
            || self.guard_rule.is_some() != rejected
        {
            return Err(PayloadError::GuardMembers);
        }
        let judged = self.purpose == Purpose::Research && self.outcome == Outcome::Ok;
        if !judged && !verdicts.is_empty() {
            return Err(PayloadError::Verdicts);
        }
        let kinds_in_order = verdicts
            .windows(2)
            .all(|w| matches!(w, [a, b] if a.kind <= b.kind));
        if !kinds_in_order || !verdicts.iter().all(|v| v.result.fits(v.kind)) {
            return Err(PayloadError::Verdicts);
        }
        Ok(())
    }
}

/// Why a record has no agent-stream payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PayloadError {
    #[error("the compiler's record is on the control stream (journal spec §9.9)")]
    CompilerOnAgentStream,
    #[error("an evaluation names no agent, so its record is on no agent stream (DEC-2216)")]
    NoAgentOnAgentStream,
    #[error(
        "prompt_ref, request_digest and guard_rule disagree with the outcome (journal spec rule 148)"
    )]
    GuardMembers,
    #[error(
        "verdicts only for a research ok, theses first, each result fitting its kind (rule 151)"
    )]
    Verdicts,
}

fn text(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |t| Value::Str(t.to_owned()))
}

/// A digest as the journal's `sha256:` reference.
pub fn reference(digest: Digest) -> String {
    format!("sha256:{digest}")
}

pub(crate) fn model_value(model: &ModelRef) -> Value {
    object([
        ("content_hash", Value::Str(reference(model.content_hash))),
        ("model_id", Value::Str(model.model_id.clone())),
        ("model_version", Value::Str(model.model_version.clone())),
    ])
}

pub(crate) fn tokens_value(tokens: Usage) -> Value {
    object([
        ("cached", int(tokens.cached)),
        ("input", int(tokens.input)),
        ("output", int(tokens.output)),
    ])
}
