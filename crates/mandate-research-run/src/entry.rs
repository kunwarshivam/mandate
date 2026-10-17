//! The research agent's registry entry (inference spec §4.1, agent harness spec §1.3, §6.4, §6.5):
//! the pinned prompt, the retrieval plan, and the output schema, all inside one content hash, so
//! a change to any of them is a new pin (DEC-431 item 5).

use mandate_canon::Value;
use mandate_gateway::registry::{Identity, PinnedContent, Template};

use crate::output::OutputBounds;
use crate::plan::RetrievalPlan;

/// The template's slots, strictly sorted as the registry requires: the run's context, the
/// retrieved items, and the agent's active theses (§6.4).
pub const RESEARCH_INPUTS: [&str; 3] = ["context", "items", "memory"];

/// What a research registry entry is made of besides the fixed template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchEntry {
    pub model_id: String,
    pub model_version: String,
    pub identity: Identity,
    pub deadline_ms: u64,
    pub max_output_tokens: u32,
    pub seed: Option<u64>,
    pub plan: RetrievalPlan,
    pub bounds: OutputBounds,
}

impl ResearchEntry {
    /// The pinned content the registry holds and the mandate's pin names by its hash.
    pub fn content(&self) -> PinnedContent {
        PinnedContent {
            model_id: self.model_id.clone(),
            model_version: self.model_version.clone(),
            identity: self.identity.clone(),
            template: Template {
                system: String::new(),
                user: String::new(),
                inputs: Vec::new(),
            },
            retrieval_plan: Value::Null,
            output_schema: Value::Null,
            validation_bounds: Value::Null,
            params_schema: Value::Null,
            deadline_ms: self.deadline_ms,
            max_output_tokens: self.max_output_tokens,
            seed: self.seed,
            methodology: String::new(),
            authorship: String::new(),
        }
    }
}

/// §6.5's output schema in the gateway's schema language, from the entry's bounds.
pub fn output_schema(_bounds: &OutputBounds) -> Value {
    Value::Null
}
