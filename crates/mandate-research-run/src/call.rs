//! The one gateway call a run makes (§6.2 step 3, §10.1; inference spec §3), and how the harness
//! treats each way it can fail (§7.3).

use std::collections::BTreeMap;

use mandate_canon::{Int, Key, Object, Value, parse, to_canonical};
use mandate_domain::AssetClass;
use mandate_gateway::meter::MeterStore;
use mandate_gateway::provider::{Clock, Transport};
use mandate_gateway::registry::ModelRef as ModelPin;
use mandate_gateway::{CallRecord, CallRequest, Gateway, KeyOwner, Outcome, Purpose, RequestError};
use mandate_time::UtcNanos;

use crate::error::ResearchRunError;
use crate::item::{ItemId, ItemKind, SourceId, Symbol};
use crate::retrieve::Retrieval;

/// What the runtime core hands the worker once the run's observations are journaled: the ids and
/// limits of the call, and the `ObservationRecorded` event id of each retrieved item, in the
/// retrieval's order (§6.2 steps 2 and 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallPlan {
    pub run_id: String,
    pub call_id: String,
    pub workspace_id: String,
    pub agent_id: String,
    pub model: ModelPin,
    pub as_of: UtcNanos,
    pub deadline_ms: u32,
    pub max_output_tokens: u32,
    pub observations: Vec<String>,
}

/// One active thesis of this agent, as the model may see it: ids and the instrument, no figure
/// (§6.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryThesis {
    pub thesis_id: String,
    pub symbol: Symbol,
    pub expires_at: UtcNanos,
}

/// One retrieved item as the model sees it: marked untrusted, with the id a thesis cites, its
/// source, and the `ObservationRecorded` that journaled it first (§6.4, HI-2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UntrustedItem {
    pub item_id: ItemId,
    pub source: SourceId,
    pub kind: ItemKind,
    pub observation: String,
    pub body: Value,
}

/// §6.4's typed inputs: everything that may go in, and no field for anything that may not. There
/// is no allocation, limit, equity, P&L, credential, approval, or other agent's data here (HI-11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallInputs {
    pub items: Vec<UntrustedItem>,
    /// `behavior.description`, which construction checked against the prompt guard (§5.1 step 5).
    pub description: String,
    pub cut_off: UtcNanos,
    pub data_universe: Vec<Symbol>,
    pub asset_classes: Vec<AssetClass>,
    pub max_instruments: u32,
    pub working_universe: Vec<Symbol>,
    pub held: Vec<Symbol>,
    pub theses: Vec<MemoryThesis>,
}

/// One research call (§10.1's request table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchCall {
    pub call_id: String,
    pub run_id: String,
    pub workspace_id: String,
    pub agent_id: String,
    pub model: ModelPin,
    pub inputs: CallInputs,
    pub as_of: UtcNanos,
    /// The dispatch instant plus the entry's `deadline_ms`, on the wall clock (§6.2 step 3,
    /// DEC-431 item 24).
    pub deadline: UtcNanos,
    pub max_output_tokens: u32,
}

/// What the run tells the model about itself besides the retrieved items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallContext {
    pub description: String,
    pub data_universe: Vec<Symbol>,
    pub asset_classes: Vec<AssetClass>,
    pub max_instruments: u32,
    pub working_universe: Vec<Symbol>,
    pub held: Vec<Symbol>,
    pub theses: Vec<MemoryThesis>,
}

impl ResearchCall {
    /// The call for `plan` over `retrieval`, dispatched at `dispatched_at`. Each item is paired
    /// with its journaled observation, so an item that was not journaled has no way in (HI-2).
    ///
    /// # Errors
    /// [`ResearchRunError::ObservationsMismatch`] when the counts differ, and
    /// [`ResearchRunError::TimeOutOfRange`] for a deadline past the representable range.
    pub fn build(
        plan: &CallPlan,
        retrieval: &Retrieval,
        context: CallContext,
        dispatched_at: UtcNanos,
    ) -> Result<Self, ResearchRunError> {
        if plan.observations.len() != retrieval.items.len() {
            return Err(ResearchRunError::ObservationsMismatch {
                items: retrieval.items.len(),
                observations: plan.observations.len(),
            });
        }
        let items = retrieval
            .items
            .iter()
            .zip(&plan.observations)
            .map(|(item, observation)| {
                let record = item.record()?;
                let body =
                    parse(&record).map_err(|_| ResearchRunError::InvalidId { what: "record" })?;
                Ok(UntrustedItem {
                    item_id: item.item_id()?,
                    source: item.source()?,
                    kind: item.kind(),
                    observation: observation.clone(),
                    body,
                })
            })
            .collect::<Result<Vec<_>, ResearchRunError>>()?;
        Ok(Self {
            call_id: plan.call_id.clone(),
            run_id: plan.run_id.clone(),
            workspace_id: plan.workspace_id.clone(),
            agent_id: plan.agent_id.clone(),
            model: plan.model.clone(),
            inputs: CallInputs {
                items,
                description: context.description,
                cut_off: plan.as_of,
                data_universe: context.data_universe,
                asset_classes: context.asset_classes,
                max_instruments: context.max_instruments,
                working_universe: context.working_universe,
                held: context.held,
                theses: context.theses,
            },
            as_of: plan.as_of,
            deadline: after_ms(dispatched_at, plan.deadline_ms)?,
            max_output_tokens: plan.max_output_tokens,
        })
    }
}

impl ResearchCall {
    /// The gateway's request for this call (inference spec §3.1): the typed inputs rendered as one
    /// canonical JSON text per template slot ([`crate::RESEARCH_INPUTS`]), never prompt text, and
    /// the journaled observations as the retrieved context. The prompt itself is the gateway's to
    /// render from the pinned template, inside data sections (§6.4).
    ///
    /// # Errors
    /// [`ResearchRunError::InvalidId`] when an input cannot be written as canonical text.
    pub fn request(&self) -> Result<CallRequest, ResearchRunError> {
        Ok(CallRequest {
            call_id: self.call_id.clone(),
            workspace_id: self.workspace_id.clone(),
            agent_id: Some(self.agent_id.clone()),
            purpose: Purpose::Research,
            model: self.model.clone(),
            inputs: render_inputs(&self.inputs)?,
            as_of: self.as_of,
            deadline: self.deadline,
            observations: self
                .inputs
                .items
                .iter()
                .map(|item| item.observation.clone())
                .collect(),
            key_owner: KeyOwner::Platform,
            run_kind: None,
            turns: Vec::new(),
            tools: Vec::new(),
            spend_ceiling: None,
        })
    }
}

/// The typed inputs as the template's slots: `context`, `items`, and `memory`, each canonical JSON.
///
/// # Errors
/// [`ResearchRunError::InvalidId`] when an input cannot be written as canonical text.
pub fn render_inputs(inputs: &CallInputs) -> Result<BTreeMap<String, String>, ResearchRunError> {
    let symbols = |list: &[Symbol]| {
        Value::Array(
            list.iter()
                .map(|symbol| Value::Str(symbol.as_str().to_owned()))
                .collect(),
        )
    };
    let context = object(vec![
        (
            "asset_classes",
            Value::Array(
                inputs
                    .asset_classes
                    .iter()
                    .map(|class| Value::Str(class.as_str().to_owned()))
                    .collect(),
            ),
        ),
        ("cut_off", Value::Str(inputs.cut_off.to_string())),
        ("data_universe", symbols(&inputs.data_universe)),
        ("description", Value::Str(inputs.description.clone())),
        ("held", symbols(&inputs.held)),
        (
            "max_instruments",
            Value::Int(
                Int::new(u64::from(inputs.max_instruments))
                    .ok_or(ResearchRunError::InvalidId { what: "inputs" })?,
            ),
        ),
        ("working_universe", symbols(&inputs.working_universe)),
    ])?;
    let items = inputs
        .items
        .iter()
        .map(|item| {
            object(vec![
                ("body", item.body.clone()),
                ("item_id", Value::Str(item.item_id.as_str().to_owned())),
                ("kind", Value::Str(item.kind.as_str().to_owned())),
                ("observation", Value::Str(item.observation.clone())),
                ("source", Value::Str(item.source.as_str().to_owned())),
            ])
        })
        .collect::<Result<Vec<_>, _>>()?;
    let memory = inputs
        .theses
        .iter()
        .map(|thesis| {
            object(vec![
                ("expires_at", Value::Str(thesis.expires_at.to_string())),
                ("symbol", Value::Str(thesis.symbol.as_str().to_owned())),
                ("thesis_id", Value::Str(thesis.thesis_id.clone())),
            ])
        })
        .collect::<Result<Vec<_>, _>>()?;
    [
        ("context", context),
        ("items", Value::Array(items)),
        ("memory", Value::Array(memory)),
    ]
    .into_iter()
    .map(|(name, value)| {
        String::from_utf8(to_canonical(&value))
            .map(|text| (name.to_owned(), text))
            .map_err(|_| ResearchRunError::InvalidId { what: "inputs" })
    })
    .collect()
}

/// A canonical object of `members`.
fn object(members: Vec<(&str, Value)>) -> Result<Value, ResearchRunError> {
    let mut out = Object::new();
    for (name, value) in members {
        let key = Key::new(name).map_err(|_| ResearchRunError::InvalidId { what: "inputs" })?;
        out.insert(key, value);
    }
    Ok(Value::Object(out))
}

/// What the harness does about one call besides treating a failed call's output as missing, which
/// it always does (HI-6, §7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Treatment {
    /// Alert the operator with opaque text (rule 6).
    pub alert_operator: bool,
    /// Disable research for this pin until a new version (`model_withdrawn`).
    pub disable_research: bool,
    /// Counts toward the three-in-a-row `deadline_exceeded` alert.
    pub deadline_streak: bool,
}

/// §7.3's table over the gateway's outcomes. `ok` has no treatment. No outcome is ever retried
/// within the run: the gateway has already retried what it may, and sampling again until something
/// parses would select outputs (DEC-432 item 3).
pub fn treatment(outcome: Outcome) -> Treatment {
    Treatment {
        alert_operator: matches!(
            outcome,
            Outcome::PolicyDenied
                | Outcome::CredentialInvalid
                | Outcome::MeterUnavailable
                | Outcome::InputRejected
        ),
        disable_research: outcome == Outcome::ModelWithdrawn,
        deadline_streak: outcome == Outcome::DeadlineExceeded,
    }
}

/// The gateway's answer to one research call: its record, which the core journals whatever the
/// outcome, or a request the gateway could not make at all, which has no record (inference spec
/// §3.1).
#[allow(
    clippy::large_enum_variant,
    reason = "one answer per run, handed straight to the core; boxing the record buys nothing"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Recorded(CallRecord),
    Refused(RequestError),
}

/// The worker's call (§6.2 step 3): builds the call for `plan` over `retrieval` at the gateway's
/// own clock, so the deadline is the dispatch instant plus `deadline_ms` (DEC-431 item 24), and
/// sends it through the gateway, which renders, guards, reserves, calls, and validates.
///
/// # Errors
/// [`ResearchRunError`] when the call cannot be built; nothing is then sent.
pub fn call<T: Transport, S: MeterStore, C: Clock>(
    gateway: &mut Gateway<T, S, C>,
    plan: &CallPlan,
    retrieval: &Retrieval,
    context: CallContext,
) -> Result<(ResearchCall, Answer), ResearchRunError> {
    let dispatched_at = gateway.clock_mut().now();
    let built = ResearchCall::build(plan, retrieval, context, dispatched_at)?;
    let request = built.request()?;
    let answer = match gateway.call(&request) {
        Ok(record) => Answer::Recorded(record),
        Err(error) => Answer::Refused(error),
    };
    Ok((built, answer))
}

/// `at` plus `ms` milliseconds, exactly.
pub(crate) fn after_ms(at: UtcNanos, ms: u32) -> Result<UtcNanos, ResearchRunError> {
    let nanos = u64::from(at.nanos())
        .checked_add(u64::from(ms % 1_000).saturating_mul(1_000_000))
        .ok_or(ResearchRunError::TimeOutOfRange)?;
    let carry =
        i64::try_from(nanos / 1_000_000_000).map_err(|_| ResearchRunError::TimeOutOfRange)?;
    let secs = at
        .secs()
        .checked_add(i64::from(ms / 1_000))
        .and_then(|secs| secs.checked_add(carry))
        .ok_or(ResearchRunError::TimeOutOfRange)?;
    let nanos =
        u32::try_from(nanos % 1_000_000_000).map_err(|_| ResearchRunError::TimeOutOfRange)?;
    UtcNanos::from_parts(secs, nanos).map_err(|_| ResearchRunError::TimeOutOfRange)
}
