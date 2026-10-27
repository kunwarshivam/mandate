//! The model registry (inference spec §4, E15-7). An entry has pinned content, immutable once
//! registered and covered by its content hash, and routing and status outside the hash, so a
//! routing change never invalidates an owner's pin (DEC-432 item 17).

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Digest, Int, Key, Object, Value, to_canonical};
use mandate_time::Date;

use crate::money::PriceTable;
use crate::schema;
use crate::tools::Agentic;

/// Inference spec §3.4: a fast model's deadline is at most 300 ms, a slow model's at most 600 s.
pub const FAST_DEADLINE_MS: u64 = 300;
pub const SLOW_DEADLINE_MS: u64 = 600_000;
/// The model-id prefix of a fast decision model (mandate spec §8.1).
const FAST_PREFIX: &str = "fast.";
/// The one provider whose dateless model ids are pinned snapshots (DEC-2282 item 7).
const ANTHROPIC: &str = "anthropic";
/// Words that name a moving target rather than one model (INF-1, DEC-997 item 2).
const FLOATING: [&str; 4] = ["latest", "auto", "default", "stable"];

/// What served a call: an immutable dated snapshot or a weights digest (INF-1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Snapshot {
    Dated(String),
    Weights(Digest),
}

/// A model's identity (inference spec §4.1). For OpenRouter, `model_name` is the slug it is
/// requested and reported by, and `snapshot` names the upstream's dated snapshot the slug serves
/// (DEC-997 item 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub provider: String,
    pub model_name: String,
    pub snapshot: Snapshot,
    pub quantization: Option<String>,
}

/// The pinned prompt: fixed system text and user text with `{{name}}` slots, one per input the
/// contract lists. Callers pass typed inputs, never prompt text (inference spec §3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub system: String,
    pub user: String,
    pub inputs: Vec<String>,
}

/// How an entry is called (inference spec §4.1, DEC-2203): one call, or an agentic research run's
/// turns over its pinned tools.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    SingleCall,
    Agentic(Agentic),
}

/// `top_p` in millionths, from 1 to 1,000,000 (inference spec §4.1): a decimal, never a float.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TopP(u32);

impl TopP {
    pub const SCALE: u32 = 1_000_000;

    /// `millionths` as a `top_p`, or `None` outside 1 to [`TopP::SCALE`].
    pub fn from_millionths(millionths: u32) -> Option<Self> {
        (1..=Self::SCALE)
            .contains(&millionths)
            .then_some(Self(millionths))
    }

    pub fn millionths(self) -> u32 {
        self.0
    }

    /// The journal's decimal text: `1`, or `0.` and the significant digits, no trailing zero.
    pub fn text(self) -> String {
        if self.0 == Self::SCALE {
            return "1".to_owned();
        }
        let digits = format!("{:06}", self.0);
        format!("0.{}", digits.trim_end_matches('0'))
    }
}

/// How much a reasoning model thinks before it answers (inference spec §4.1, DEC-2203 item 22).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Effort {
    Low,
    Medium,
    High,
}

impl Effort {
    pub fn code(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

/// The pinned reasoning mode: off, or on at one effort.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reasoning {
    Off,
    On(Effort),
}

impl Reasoning {
    pub fn canonical(self) -> Value {
        match self {
            Self::Off => object([("enabled", Value::Bool(false))]),
            Self::On(effort) => object([("effort", Value::Str(effort.code().to_owned()))]),
        }
    }
}

/// An entry's pinned content (inference spec §4.1), every member inside the content hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedContent {
    pub model_id: String,
    pub model_version: String,
    pub identity: Identity,
    pub template: Template,
    pub retrieval_plan: Value,
    pub output_schema: Value,
    pub validation_bounds: Value,
    pub params_schema: Value,
    pub deadline_ms: u64,
    pub max_output_tokens: u32,
    pub seed: Option<u64>,
    pub top_p: Option<TopP>,
    pub reasoning: Option<Reasoning>,
    pub mode: Mode,
    pub methodology: String,
    pub authorship: String,
}

impl PinnedContent {
    /// The canonical object the content hash covers. Sampling is temperature 0 always (inference
    /// spec §3.4) and the seed is pinned. `endpoints` and `status` are not in it. A `single_call`
    /// entry leaves out `mode`, `tools`, `loop_bounds`, and `output_schemas`, and an entry with no
    /// `top_p` or reasoning leaves those out of its sampling, so every hash from before inference
    /// spec v0.4 is unchanged.
    pub fn canonical(&self) -> Value {
        let identity = self.identity.canonical();
        let template = object([
            (
                "inputs",
                Value::Array(
                    self.template
                        .inputs
                        .iter()
                        .cloned()
                        .map(Value::Str)
                        .collect(),
                ),
            ),
            ("system", Value::Str(self.template.system.clone())),
            ("user", Value::Str(self.template.user.clone())),
        ]);
        let mut canonical = object([
            ("authorship", Value::Str(self.authorship.clone())),
            ("deadline_ms", int(self.deadline_ms)),
            ("identity", identity),
            ("max_output_tokens", int(u64::from(self.max_output_tokens))),
            ("methodology", Value::Str(self.methodology.clone())),
            ("model_id", Value::Str(self.model_id.clone())),
            ("model_version", Value::Str(self.model_version.clone())),
            ("output_schema", self.output_schema.clone()),
            ("params_schema", self.params_schema.clone()),
            ("retrieval_plan", self.retrieval_plan.clone()),
            ("sampling", self.sampling()),
            ("template", template),
            ("validation_bounds", self.validation_bounds.clone()),
        ]);
        if let (Mode::Agentic(agentic), Value::Object(members)) = (&self.mode, &mut canonical) {
            let schemas = agentic
                .output_schemas
                .iter()
                .filter_map(|(kind, s)| Key::new(kind).ok().map(|k| (k, s.clone())))
                .collect::<Object>();
            for (name, value) in [
                ("loop_bounds", agentic.loop_bounds.canonical()),
                ("mode", Value::Str("agentic".to_owned())),
                ("output", Value::Str(agentic.output.code().to_owned())),
                ("output_schemas", Value::Object(schemas)),
                (
                    "tools",
                    Value::Array(agentic.tools.iter().map(|t| t.canonical()).collect()),
                ),
            ] {
                if let Ok(key) = Key::new(name) {
                    members.insert(key, value);
                }
            }
        }
        canonical
    }

    /// The pinned sampling: temperature 0 and the seed, with `top_p` and the reasoning mode only
    /// where the entry pins them (inference spec §3.1, §4.1).
    pub fn sampling(&self) -> Value {
        let mut sampling = sampling(self.seed);
        if let Value::Object(members) = &mut sampling {
            if let (Some(top_p), Ok(key)) = (self.top_p, Key::new("top_p")) {
                members.insert(key, Value::Str(top_p.text()));
            }
            if let (Some(reasoning), Ok(key)) = (self.reasoning, Key::new("reasoning")) {
                members.insert(key, reasoning.canonical());
            }
        }
        sampling
    }

    /// The agentic loop, or `None` for a `single_call` entry.
    pub fn agentic(&self) -> Option<&Agentic> {
        match &self.mode {
            Mode::SingleCall => None,
            Mode::Agentic(agentic) => Some(agentic),
        }
    }

    /// SHA-256 over the canonical JSON of the pinned content (inference spec §4.1).
    pub fn content_hash(&self) -> Digest {
        Digest::of(&to_canonical(&self.canonical()))
    }

    /// The run kinds an agentic entry answers, which name its role, or `None` for a `single_call`
    /// entry, which has no role (DEC-2282).
    pub fn role(&self) -> Option<BTreeSet<&str>> {
        self.agentic()
            .map(|a| a.output_schemas.keys().map(String::as_str).collect())
    }

    /// The harness content a role's offered entries share (inference spec §4.2 step 5, DEC-2282):
    /// the canonical pinned content without the model's own id and version, identity, sampling,
    /// deadline, and token limit, the members in which one model's entry may differ from another's.
    pub fn harness(&self) -> Value {
        let mut canonical = self.canonical();
        if let Value::Object(members) = &mut canonical {
            for name in [
                "deadline_ms",
                "identity",
                "max_output_tokens",
                "model_id",
                "model_version",
                "sampling",
            ] {
                if let Ok(key) = Key::new(name) {
                    members.remove(&key);
                }
            }
        }
        canonical
    }

    pub fn model_ref(&self) -> ModelRef {
        ModelRef {
            model_id: self.model_id.clone(),
            model_version: self.model_version.clone(),
            content_hash: self.content_hash(),
        }
    }
}

impl Identity {
    pub fn canonical(&self) -> Value {
        let snapshot = match &self.snapshot {
            Snapshot::Dated(text) => object([("dated", Value::Str(text.clone()))]),
            Snapshot::Weights(digest) => {
                object([("weights", Value::Str(format!("sha256:{digest}")))])
            }
        };
        object([
            ("model_name", Value::Str(self.model_name.clone())),
            ("provider", Value::Str(self.provider.clone())),
            (
                "quantization",
                self.quantization.clone().map_or(Value::Null, Value::Str),
            ),
            ("snapshot", snapshot),
        ])
    }
}

/// The temperature and seed every request carries: temperature 0 as the journal's decimal text
/// (journal spec §9.19's `sampling`), and the pinned seed.
pub(crate) fn sampling(seed: Option<u64>) -> Value {
    object([
        ("seed", seed.map_or(Value::Null, int)),
        ("temperature", Value::Str("0".to_owned())),
    ])
}

pub(crate) fn int(n: u64) -> Value {
    Int::new(n).map_or(Value::Null, Value::Int)
}

pub(crate) fn object<const N: usize>(members: [(&str, Value); N]) -> Value {
    let mut out = Object::new();
    for (name, value) in members {
        if let Ok(key) = Key::new(name) {
            out.insert(key, value);
        }
    }
    Value::Object(out)
}

/// A mandate's pin of a model (mandate spec §8.1): id, version, and content hash.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModelRef {
    pub model_id: String,
    pub model_version: String,
    pub content_hash: Digest,
}

/// Where an endpoint runs (INF-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Locality {
    Hosted,
    Local,
}

/// How a request is routed to exactly one upstream, and the provider name its response reports
/// (DEC-997 item 2). `upstream` is the routing slug (`provider.order`, `provider.only`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub upstream: String,
    pub reported_provider: String,
}

/// An endpoint attested to serve exactly an entry's identity (inference spec §4.1), outside the
/// content hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub id: String,
    pub host: String,
    pub region: String,
    pub locality: Locality,
    pub route: Route,
    pub prices: PriceTable,
}

/// What a provider says served a call (inference spec §3.2). Either part may be missing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reported {
    pub model: Option<String>,
    pub provider: Option<String>,
}

impl Reported {
    /// The record's `reported_identity` text: `model@provider`, or `None` when neither is known.
    pub fn text(&self) -> Option<String> {
        match (&self.model, &self.provider) {
            (None, None) => None,
            (m, p) => Some(format!(
                "{}@{}",
                m.as_deref().unwrap_or(""),
                p.as_deref().unwrap_or("")
            )),
        }
    }
}

/// An entry's status, outside the hash (inference spec §4.1, §4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Evaluating,
    Offered,
    Deprecated { retires_on: Date },
    Withdrawn,
}

/// One registered model: pinned content, its hash, its endpoints, and its status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    content: PinnedContent,
    content_hash: Digest,
    endpoints: Vec<EndpointState>,
    status: Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EndpointState {
    endpoint: Endpoint,
    healthy: bool,
}

impl Entry {
    pub fn content(&self) -> &PinnedContent {
        &self.content
    }

    pub fn content_hash(&self) -> Digest {
        self.content_hash
    }

    pub fn status(&self) -> Status {
        self.status
    }

    /// The endpoints in registration order, each with whether it is healthy.
    pub fn endpoints(&self) -> impl Iterator<Item = (&Endpoint, bool)> {
        self.endpoints.iter().map(|s| (&s.endpoint, s.healthy))
    }

    /// INF-2: a response serves this entry only if its reported model and provider are both
    /// present and equal, byte for byte, to the pin's model name and the endpoint's provider.
    pub fn serves(&self, endpoint: &Endpoint, reported: &Reported) -> bool {
        reported.model.as_deref() == Some(self.content.identity.model_name.as_str())
            && reported.provider.as_deref() == Some(endpoint.route.reported_provider.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    #[error("the model name or snapshot is a floating alias, a variant, or empty (INF-1)")]
    FloatingAlias,
    #[error("open weights name no quantization, so the routing lock could not pin one (INF-2)")]
    Quantization,
    #[error("the deadline is zero or above the tier's bound (inference spec §3.4)")]
    Deadline,
    #[error("max_output_tokens is zero")]
    NoOutputBound,
    #[error("the output schema is not in the gateway's schema language")]
    OutputSchema,
    #[error("the template's slots and its input contract differ")]
    Template,
    #[error("the agentic loop's tools, bounds, or output schemas are not ones a registry may pin")]
    Agentic,
    #[error("this id and version are registered with other content; pinned content is immutable")]
    Immutable,
    #[error("no entry for this id and version")]
    Unknown,
    #[error("the content hash differs from the registered entry's")]
    HashMismatch,
    #[error("the identity probe was empty or reported another identity (inference spec §4.3)")]
    ProbeFailed,
    #[error("an endpoint with this id is already listed")]
    DuplicateEndpoint,
    #[error("a withdrawn entry never changes status again (INF-15)")]
    Withdrawn,
    #[error("an offered entry of the same run kinds pins other harness content (DEC-2282)")]
    HarnessMismatch,
}

/// Why a call's model resolves to nothing callable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unresolved {
    Unknown,
    HashMismatch,
    Withdrawn,
}

/// The workspace deployment's registry.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    entries: BTreeMap<(String, String), Entry>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers pinned content with status `evaluating` and no endpoint. Registering the same
    /// content again is a no-op; other content under the same id and version is refused.
    pub fn register(&mut self, content: PinnedContent) -> Result<Digest, RegistryError> {
        check_content(&content)?;
        let hash = content.content_hash();
        let key = (content.model_id.clone(), content.model_version.clone());
        if let Some(existing) = self.entries.get(&key) {
            return if existing.content_hash == hash {
                Ok(hash)
            } else {
                Err(RegistryError::Immutable)
            };
        }
        self.entries.insert(
            key,
            Entry {
                content,
                content_hash: hash,
                endpoints: Vec::new(),
                status: Status::Evaluating,
            },
        );
        Ok(hash)
    }

    /// Adds an endpoint only after the identity probe (inference spec §4.3): at least one probe
    /// response, every one reporting exactly this entry's identity through this endpoint.
    pub fn add_endpoint(
        &mut self,
        model: &ModelRef,
        endpoint: Endpoint,
        probe: &[Reported],
    ) -> Result<(), RegistryError> {
        let entry = self.entry_mut(model)?;
        if entry.status == Status::Withdrawn {
            return Err(RegistryError::Withdrawn);
        }
        if probe.is_empty() || !probe.iter().all(|r| entry.serves(&endpoint, r)) {
            return Err(RegistryError::ProbeFailed);
        }
        if entry.endpoints.iter().any(|s| s.endpoint.id == endpoint.id) {
            return Err(RegistryError::DuplicateEndpoint);
        }
        entry.endpoints.push(EndpointState {
            endpoint,
            healthy: true,
        });
        Ok(())
    }

    /// Sets the status; `withdrawn` is final (INF-15). An agentic entry is offered only when its
    /// harness content is that of every other offered entry answering any of its run kinds, so an
    /// owner's choice among them changes the model, never what the agent may read or do
    /// (DEC-2282).
    pub fn set_status(&mut self, model: &ModelRef, status: Status) -> Result<(), RegistryError> {
        if self.entry_mut(model)?.status == Status::Withdrawn {
            return Err(RegistryError::Withdrawn);
        }
        if status == Status::Offered {
            self.check_role(model)?;
        }
        self.entry_mut(model)?.status = status;
        Ok(())
    }

    /// Marks an endpoint unhealthy after an identity mismatch (inference spec §9); only an
    /// operator restores it.
    pub fn set_healthy(
        &mut self,
        model: &ModelRef,
        endpoint_id: &str,
        healthy: bool,
    ) -> Result<(), RegistryError> {
        let entry = self.entry_mut(model)?;
        entry
            .endpoints
            .iter_mut()
            .filter(|s| s.endpoint.id == endpoint_id)
            .for_each(|s| s.healthy = healthy);
        Ok(())
    }

    /// The entry a pin names, if it is registered with that hash and not withdrawn.
    pub fn resolve(&self, model: &ModelRef) -> Result<&Entry, Unresolved> {
        let entry = self
            .entries
            .get(&(model.model_id.clone(), model.model_version.clone()))
            .ok_or(Unresolved::Unknown)?;
        if entry.content_hash != model.content_hash {
            return Err(Unresolved::HashMismatch);
        }
        if entry.status == Status::Withdrawn {
            return Err(Unresolved::Withdrawn);
        }
        Ok(entry)
    }

    /// The entry a pin names, whatever its status, if it is registered with that hash. The call path
    /// renders a withdrawn model's prompt before it refuses, so its record names the prompt.
    pub fn entry(&self, model: &ModelRef) -> Result<&Entry, Unresolved> {
        let entry = self
            .entries
            .get(&(model.model_id.clone(), model.model_version.clone()))
            .ok_or(Unresolved::Unknown)?;
        if entry.content_hash != model.content_hash {
            return Err(Unresolved::HashMismatch);
        }
        Ok(entry)
    }

    /// Every entry, for the egress allowlist.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values()
    }

    fn check_role(&mut self, model: &ModelRef) -> Result<(), RegistryError> {
        let entry = self.entry_mut(model)?;
        let content = entry.content.clone();
        let Some(role) = content.role() else {
            return Ok(());
        };
        let harness = content.harness();
        let differs = self.entries.values().any(|other| {
            other.status == Status::Offered
                && other.content_hash != model.content_hash
                && other
                    .content
                    .role()
                    .is_some_and(|kinds| !kinds.is_disjoint(&role))
                && other.content.harness() != harness
        });
        if differs {
            Err(RegistryError::HarnessMismatch)
        } else {
            Ok(())
        }
    }

    fn entry_mut(&mut self, model: &ModelRef) -> Result<&mut Entry, RegistryError> {
        let entry = self
            .entries
            .get_mut(&(model.model_id.clone(), model.model_version.clone()))
            .ok_or(RegistryError::Unknown)?;
        if entry.content_hash != model.content_hash {
            return Err(RegistryError::HashMismatch);
        }
        Ok(entry)
    }
}

fn check_content(content: &PinnedContent) -> Result<(), RegistryError> {
    if floating(&content.identity) {
        return Err(RegistryError::FloatingAlias);
    }
    if matches!(content.identity.snapshot, Snapshot::Weights(_))
        && content.identity.quantization.is_none()
    {
        return Err(RegistryError::Quantization);
    }
    let bound = if content.model_id.starts_with(FAST_PREFIX) {
        FAST_DEADLINE_MS
    } else {
        SLOW_DEADLINE_MS
    };
    if content.deadline_ms == 0 || content.deadline_ms > bound {
        return Err(RegistryError::Deadline);
    }
    if content.max_output_tokens == 0 {
        return Err(RegistryError::NoOutputBound);
    }
    if !schema::well_formed(&content.output_schema) {
        return Err(RegistryError::OutputSchema);
    }
    if !template_matches(&content.template) {
        return Err(RegistryError::Template);
    }
    if content.agentic().is_some_and(|a| !a.well_formed()) {
        return Err(RegistryError::Agentic);
    }
    Ok(())
}

/// INF-1 and DEC-997 item 2: an empty name, a variant suffix (`:nitro`, `:online`, …), a
/// floating word as any segment of the name, or a dated snapshot with no date in it.
fn floating(identity: &Identity) -> bool {
    let name = identity.model_name.as_str();
    let segments = |text: &str| -> bool {
        text.split(['/', '-', '.', '_', ':', '@', '~'])
            .any(|s| FLOATING.contains(&s.to_ascii_lowercase().as_str()))
    };
    let undated = match &identity.snapshot {
        Snapshot::Dated(text) => segments(text) || !(has_date(text) || vendor_pinned(identity)),
        Snapshot::Weights(_) => false,
    };
    name.is_empty()
        || identity.provider.is_empty()
        || name.contains(':')
        || name.starts_with('~')
        || segments(name)
        || undated
}

/// Whether the snapshot is a dateless model id its vendor states is a pinned snapshot: Anthropic
/// states this of every model id, and the founder accepted that reading of DEC-997 item 2 with
/// DEC-2282 item 7. Only provider `anthropic`, a model name under `anthropic/claude-`, and a
/// `claude-` snapshot of lowercase letters, digits, and hyphens qualify; every other identity's
/// snapshot needs a date (INF-1).
fn vendor_pinned(identity: &Identity) -> bool {
    let id_like = |rest: &str| {
        !rest.is_empty()
            && rest
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    let Snapshot::Dated(snapshot) = &identity.snapshot else {
        return false;
    };
    identity.provider == ANTHROPIC
        && identity.model_name.starts_with("anthropic/claude-")
        && snapshot.strip_prefix("claude-").is_some_and(id_like)
}

/// Whether `text` holds eight digits as `YYYYMMDD` or `YYYY-MM-DD`, a plausible calendar date.
fn has_date(text: &str) -> bool {
    let digits: Vec<char> = text.chars().collect();
    let date_at = |window: &[char]| -> bool {
        let plain: String = window.iter().filter(|c| **c != '-').collect();
        if !plain.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        let year = plain.get(0..4).and_then(|y| y.parse::<u16>().ok());
        let month = plain.get(4..6).and_then(|m| m.parse::<u8>().ok());
        let day = plain.get(6..8).and_then(|d| d.parse::<u8>().ok());
        match (year, month, day) {
            (Some(y), Some(m), Some(d)) => Date::new(y, m, d).is_ok(),
            _ => false,
        }
    };
    digits
        .windows(8)
        .any(|w| w.iter().all(char::is_ascii_digit) && date_at(w))
        || digits
            .windows(10)
            .any(|w| w.get(4) == Some(&'-') && w.get(7) == Some(&'-') && date_at(w))
}

/// Every `{{name}}` slot of the user text is a listed input, every input has a slot, and the
/// inputs are strictly sorted.
fn template_matches(template: &Template) -> bool {
    let slots = slots(&template.user);
    let sorted = template
        .inputs
        .windows(2)
        .all(|w| matches!(w, [a, b] if a < b));
    let listed: Vec<&str> = template.inputs.iter().map(String::as_str).collect();
    sorted && !template.system.contains("{{") && slots == listed
}

/// The distinct slot names of `text`, sorted.
pub(crate) fn slots(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let after = rest.get(start.saturating_add(2)..).unwrap_or("");
        let Some(end) = after.find("}}") else { break };
        if let Some(name) = after.get(..end) {
            out.push(name);
        }
        rest = after.get(end.saturating_add(2)..).unwrap_or("");
    }
    out.sort_unstable();
    out.dedup();
    out
}
