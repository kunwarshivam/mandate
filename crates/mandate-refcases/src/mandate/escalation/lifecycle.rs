//! Family E's `lifecycle` op (MC-E01 to MC-E24, MC-E29, MC-E31; mandate spec §6.1, §6.4; DEC-280,
//! DEC-292 item 3, DEC-317): each case's script driven through `mandate-runtime`'s own [`handle`]
//! and [`fold`], so every admission, re-validation, timeout and cancellation in a draft is
//! `mandate-runtime`'s and `mandate-approval`'s. Nothing here decides one.
//!
//! **The shell.** One deployment (`a` on connection `c` in workspace `w`) starts from a clean
//! reconciliation folded at the case's `start`, so no startup hold is taken. Every draft a step
//! returns is folded back as the agent stream's next event, as the shell does. The owner's answers
//! are control-stream `ApprovalResponseSubmitted` events, folded and then handed in, as the tailer
//! does. A re-tailed `source` hands the event it already folded again and folds nothing.
//!
//! **The steps.** `ask` is a tick at the folded clock whose order plan proposes the bound order
//! once and whose classifier answers `ask` by the bound `decided_by`; the dry run allows it.
//! `tick` is a tick. `fold` folds a `MarkUpdated` of the last asked instrument at its unchanged
//! price carrying the event's `clock` as `risk_clock`, and steps nothing. `cancel` hands the input
//! that cancels for its reason: a `MandateVersionApplied` or an exits-only `AgentModeApplied` on
//! the account stream, or the owner's pause, Stop or agent kill switch on the control stream.
//! `batch` folds an exits-only `AgentModeApplied` without handing it, then hands each response, so
//! the first response's own step applies the tightening and cancels before it judges (DEC-131
//! item 25(j)). Only `mode_tightened` can be batched: every other cancellation is applied by the
//! step that hands its own input.
//!
//! **The ports.** A step's `now` is the state its responses are judged against. The mandate version,
//! the instrument restriction and the working universe are the view's; the classification and the
//! dry run are the order plan's and the gate's answers. The mark is folded as a `MarkUpdated` when
//! it differs from the last one folded, and the mode as an `AgentModeApplied` when it differs from
//! the effective mode, which must then read as `now` states it. A request's reference mark is
//! folded as the account stream's `MarkUpdated` at its `seq`, the seqs before it repeating its price.
//!
//! **The map** (DEC-317). A draft's `clock` is the folded risk clock after its step. The case's
//! approval, source and content-hash names are bound one to one to the runtime's ids and hash when
//! the runtime first writes them: the reference model hashes its request as a stand-in for the
//! content object (`reference/mandate/ref.py`, `escalation_step`), so a case's hash is a name, and
//! the runtime's own `content_hash` is checked against its inline `content`. Instants are whole
//! seconds, `limit_price` is `limit`, a step-up's `assertion` is `assertion_id`, and decimals compare
//! by value. An `IntentProposed` names its approval through the `ApprovalRevalidated` it is caused
//! by, and its mandate version through that approval's `ApprovalRequested`. The records a script
//! does not state are checked and set aside: an ask's `DecisionMade`, and a cancellation's own
//! `AgentModeChanged`, `OwnerExitRequested` and `KillSwitchActivated`. Every other member the runtime
//! writes is one the case states or one checked here.
//!
//! **Every member is read (DEC-85).** The case, the context, each step by its kind, the bound order,
//! each response and its step-up, each `now`, each expectation and each expected draft by its type
//! are swept, and a member or a value this module does not know fails the case naming it.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_canon::{Digest, Int, Key, Value, to_canonical};
use mandate_journal::Environment;
use mandate_num::{Price, Qty, Signed};
use mandate_runtime::{
    ActorKind, AgentId, ApprovalSettings, Autonomy, Classified, ConnectionId, Deployment,
    DryRunVerdict, Effect, EventDraft, EventId, FlattenPlan, FlattenPlanner, FlattenRequest,
    FoldedEvent, GateDryRun, IdGen, Input, IntentBody, MandateView, Mode, OrderPlan, Ports,
    Proposal, Purpose, RiskClock, RuntimeState, Seq, SignalInputs, WorkspaceId, WriterEpoch, fold,
    handle,
};
use mandate_time::NewYorkTime;

use crate::mandate::{instant, unknown_members};
use crate::{Json, at, ensure, list_at, str_at, u64_at};

const AGENT: &str = "a";
const AGENT_STREAM: &str = "agent:w:a";
const ACCOUNT_STREAM: &str = "acct:w:c";
const CONTROL_STREAM: &str = "ctl:w";

const CASE_KEYS: &[&str] = &[
    "id", "kind", "op", "title", "context", "start", "script", "expect",
];
const CONTEXT_KEYS: &[&str] = &[
    "approvers",
    "author",
    "environment",
    "timeout_s",
    "inbox",
    "push_channels",
    "quiet_hours",
];
const BOUND_KEYS: &[&str] = &[
    "instrument",
    "asset_class",
    "side",
    "qty",
    "limit_price",
    "purpose",
    "mandate_version",
    "decided_by",
    "combined_score",
    "reference_mark",
    "approvers_required",
    "independent_required",
    "timeout_s",
];
const RESPONSE_KEYS: &[&str] = &[
    "source",
    "approval",
    "actor_kind",
    "responder",
    "verdict",
    "content_hash",
    "submitted_at",
    "step_up",
];
const NOW_KEYS: &[&str] = &[
    "mandate_version",
    "mode",
    "instrument_restricted",
    "in_working_universe",
    "classification",
    "dry_run",
    "mark",
];

/// How a case member is read off the runtime's draft (DEC-317's map).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum As {
    /// A string or `null`, compared as written.
    Text,
    /// A decimal string, compared by value.
    Decimal,
    /// An instant, written by the runtime as its whole second.
    Seconds,
    /// A count.
    Count,
    /// A boolean.
    Flag,
    /// `{price, seq}` or `null`.
    Mark,
    /// `{assertion, authenticated_at, method}` or `null`, written `{assertion_id, …}`.
    StepUp,
    /// Check 7's `{required, independent}`.
    Quorum,
}

/// The members of one expected draft type the map reads plainly: the case's name, the runtime's
/// name, and how. `type`, `clock`, `approval`, `content_hash`, `source`, and an intent's
/// `mandate_version` are read by [`Shell::compare_draft`] itself.
fn plain_members(event_type: &str) -> Option<&'static [(&'static str, &'static str, As)]> {
    let members: &'static [(&'static str, &'static str, As)] = match event_type {
        "ApprovalRequested" => &[
            ("instrument", "instrument", As::Text),
            ("asset_class", "asset_class", As::Text),
            ("side", "side", As::Text),
            ("qty", "qty", As::Decimal),
            ("limit_price", "limit", As::Decimal),
            ("purpose", "purpose", As::Text),
            ("mandate_version", "mandate_version", As::Text),
            ("decided_by", "decided_by", As::Text),
            ("combined_score", "combined_score", As::Decimal),
            ("reference_mark", "reference_mark", As::Mark),
            ("approvers_required", "approvers_required", As::Count),
            ("independent_required", "independent_required", As::Flag),
            ("timeout_s", "timeout_s", As::Count),
            ("deadline", "deadline", As::Seconds),
        ],
        "ApprovalDelivered" => &[
            ("channel", "channel", As::Text),
            ("status", "status", As::Text),
        ],
        "ApprovalResponded" => &[
            ("responder", "responder", As::Text),
            ("verdict", "verdict", As::Text),
            ("step_up", "step_up", As::StepUp),
            ("result", "result", As::Text),
            ("reason", "reason", As::Text),
            ("effective_at", "effective_at", As::Seconds),
            ("quorum", "quorum", As::Quorum),
        ],
        "ApprovalRevalidated" => &[
            ("result", "result", As::Text),
            ("reason", "reason", As::Text),
        ],
        "IntentProposed" => &[
            ("instrument", "instrument", As::Text),
            ("side", "side", As::Text),
            ("qty", "qty", As::Decimal),
            ("limit_price", "limit", As::Decimal),
            ("purpose", "purpose", As::Text),
        ],
        "ApprovalTimedOut" => &[("on_timeout", "on_timeout", As::Text)],
        "ApprovalCanceled" => &[("reason", "reason", As::Text)],
        _ => return None,
    };
    Some(members)
}

/// The members of one draft type read by [`Shell::compare_draft`] itself, beside `type` and
/// `clock`: the case's name and the runtime's.
fn special_members(event_type: &str) -> &'static [(&'static str, &'static str)] {
    match event_type {
        "ApprovalRequested" => &[("approval", ""), ("content_hash", "content_hash")],
        "ApprovalResponded" => &[("approval", "approval"), ("source", "")],
        "IntentProposed" => &[("approval", ""), ("mandate_version", "")],
        _ => &[("approval", "approval")],
    }
}

/// The deployment the shell runs, by opaque id.
fn deployment() -> Deployment {
    Deployment {
        agent: AgentId(AGENT.to_owned()),
        connection: ConnectionId("c".to_owned()),
        workspace: WorkspaceId("w".to_owned()),
    }
}

/// ULID-shaped ids from the epoch, the head and the ordinal in decimal digits, which are Crockford
/// characters; an agent-stream id starts `0`, a control-stream id `1`, an account-stream id `2`.
struct Ids;

impl IdGen for Ids {
    fn event_id(&self, epoch: WriterEpoch, head: Seq, ordinal: u32) -> EventId {
        EventId(format!("0{:05}{:010}{:010}", epoch.0, head.0, ordinal))
    }
}

/// The order plan of one step: the bound order once at an ask, and the classification `now` states.
struct Plan {
    proposal: Cell<Option<Proposal>>,
    classified: Classified,
}

impl OrderPlan for Plan {
    fn plan(&self, _view: &MandateView, _inputs: &SignalInputs) -> Option<Proposal> {
        self.proposal.take()
    }

    fn classify(&self, _view: &MandateView, _proposal: &Proposal) -> Classified {
        self.classified.clone()
    }
}

struct Gate(DryRunVerdict);

impl GateDryRun for Gate {
    fn check(&self, _proposal: &Proposal) -> DryRunVerdict {
        self.0.clone()
    }
}

/// A kill switch's flatten names the working orders and sells nothing: what a plan holds is family
/// F's, and no lifecycle case reads it.
struct Flatten;

impl FlattenPlanner for Flatten {
    fn plan(&self, request: &FlattenRequest) -> FlattenPlan {
        FlattenPlan {
            cancel_client_order_ids: request.working_orders.clone(),
            sells: Vec::new(),
            purpose: request.initiator.sell_purpose(),
            confirmation: request.confirmation.clone(),
        }
    }
}

/// An approval the runtime asked for, by the case's name.
struct Asked {
    /// The `ApprovalRequested`'s `event_id`.
    id: String,
    /// The mandate version the runtime's request bound.
    version: String,
    /// The case's bound order, which re-validation's recorded values are read against.
    bound: Json,
}

/// What one script step did: the drafts the case states, after the records it does not, every
/// effect, the folded clock after it, and what an expectation reads besides.
struct Ran {
    drafts: Vec<EventDraft>,
    effects: Vec<Effect>,
    clock: i64,
    asked: Option<(String, Json)>,
    now: Option<Json>,
}

/// The shell around one runtime, and the names bound so far.
struct Shell {
    state: RuntimeState,
    agent_seq: u64,
    account_seq: u64,
    control_seq: u64,
    view: MandateView,
    classified: Classified,
    verdict: DryRunVerdict,
    author: String,
    /// The last folded mark of each instrument, as its price and account-stream `seq`.
    marks: BTreeMap<String, (Signed, u64)>,
    /// The instrument of the last ask, which a `fold`, a `now` and a mark read.
    instrument: Option<String>,
    asked: BTreeMap<String, Asked>,
    approval_ids: BTreeMap<String, String>,
    hashes: BTreeMap<String, String>,
    /// Each `source` handed so far: its control-stream event and the response as the case gave it.
    sources: BTreeMap<String, (FoldedEvent, Json)>,
}

pub(super) fn lifecycle_case(case: &Json) -> Result<(), String> {
    unknown_members(case, CASE_KEYS)
        .map_err(|unknown| format!("case keys not interpreted: {unknown}"))?;
    let settings = context(at(case, "context")?)?;
    let start = second(at(case, "start")?, "start")?;
    let script = list_at(case, "script")?;
    let expected = list_at(case, "expect")?;
    ensure(!script.is_empty(), || {
        "a case states at least one step".to_owned()
    })?;
    ensure(script.len() == expected.len(), || {
        format!("{} steps but {} expectations", script.len(), expected.len())
    })?;
    let mut shell = Shell::started(settings, start)?;
    for (index, (step, expect)) in script.iter().zip(expected).enumerate() {
        let number = index.saturating_add(1);
        unknown_members(expect, &["drafts"]).map_err(|unknown| {
            format!("expectation {number}: members not interpreted: {unknown}")
        })?;
        let ran = shell.run(step).map_err(|e| format!("step {number}: {e}"))?;
        shell
            .compare(&ran, list_at(expect, "drafts")?)
            .map_err(|e| format!("step {number}: {e}"))?;
    }
    Ok(())
}

/// The context's approval settings. One `cli_inbox` and no push channel is all the runtime
/// delivers on (push is E8-4), and quiet hours suppress push alone (§6.4), so they are parsed and
/// cannot change a draft.
fn context(context: &Json) -> Result<ApprovalSettings, String> {
    unknown_members(context, CONTEXT_KEYS)
        .map_err(|unknown| format!("context members not interpreted: {unknown}"))?;
    let approvers = list_at(context, "approvers")?
        .iter()
        .map(|user| {
            user.as_str()
                .map(str::to_owned)
                .ok_or_else(|| "an approver is an opaque user id".to_owned())
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let environment = match str_at(context, "environment")? {
        "paper" => Environment::Paper,
        "live" => Environment::Live,
        other => return Err(format!("`{other}` is not an environment")),
    };
    ensure(u64_at(context, "inbox")? == 1, || {
        "the runtime delivers on exactly one `cli_inbox`".to_owned()
    })?;
    ensure(list_at(context, "push_channels")?.is_empty(), || {
        "push channels are E8-4's, and the runtime delivers on none".to_owned()
    })?;
    match at(context, "quiet_hours")? {
        Json::Null => {}
        window => {
            unknown_members(window, &["start", "end"])
                .map_err(|unknown| format!("`quiet_hours` members not interpreted: {unknown}"))?;
            for key in ["start", "end"] {
                NewYorkTime::parse(str_at(window, key)?).map_err(|e| format!("`{key}`: {e}"))?;
            }
        }
    }
    Ok(ApprovalSettings {
        approvers,
        author: str_at(context, "author")?.to_owned(),
        timeout_s: i64::try_from(u64_at(context, "timeout_s")?)
            .map_err(|_| "`timeout_s` does not fit an i64".to_owned())?,
        environment,
    })
}

impl Shell {
    /// A started runtime: a clean reconciliation folded at `start`, then `Input::Started`, which
    /// must journal nothing.
    fn started(approval: ApprovalSettings, start: i64) -> Result<Self, String> {
        let author = approval.author.clone();
        let mut shell = Self {
            state: RuntimeState::new(deployment()),
            agent_seq: 0,
            account_seq: 0,
            control_seq: 0,
            view: MandateView {
                version: String::new(),
                working_universe: BTreeSet::new(),
                restricted_instruments: BTreeSet::new(),
                approval,
            },
            classified: Classified {
                autonomy: Autonomy::Deny,
                decided_by: None,
            },
            verdict: DryRunVerdict::Allow,
            author,
            marks: BTreeMap::new(),
            instrument: None,
            asked: BTreeMap::new(),
            approval_ids: BTreeMap::new(),
            hashes: BTreeMap::new(),
            sources: BTreeMap::new(),
        };
        shell.account("ReconciliationRun", vec![("result", text("clean"))], start)?;
        let effects = shell.step(Input::Started(WriterEpoch(1)), None)?;
        ensure(effects.is_empty(), || {
            format!("the start took effect: {effects:?}")
        })?;
        Ok(shell)
    }

    fn clock(&self) -> Result<i64, String> {
        self.state
            .risk_clock()
            .map(RiskClock::secs)
            .ok_or_else(|| "the runtime folded no risk clock".to_owned())
    }

    fn run(&mut self, step: &Json) -> Result<Ran, String> {
        let mut ran = Ran {
            drafts: Vec::new(),
            effects: Vec::new(),
            clock: 0,
            asked: None,
            now: None,
        };
        let kind = str_at(step, "kind")?;
        let known: &[&str] = match kind {
            "ask" => &["kind", "approval", "bound"],
            "response" => &["kind", "response", "now"],
            "tick" => &["kind", "at"],
            "fold" => &["kind", "event"],
            "cancel" => &["kind", "reason"],
            "batch" => &["kind", "reason", "now", "responses"],
            other => return Err(format!("`{other}` is not a lifecycle step")),
        };
        unknown_members(step, known)
            .map_err(|unknown| format!("`{kind}` members not interpreted: {unknown}"))?;
        let records: &[(&str, &str)] = match kind {
            "ask" => {
                let bound = at(step, "bound")?;
                ran.effects = self.ask(bound)?;
                ran.asked = Some((str_at(step, "approval")?.to_owned(), bound.clone()));
                &[("DecisionMade", "")]
            }
            "response" => {
                let now = at(step, "now")?;
                self.apply_now(now)?;
                ran.now = Some(now.clone());
                ran.effects = self.respond(at(step, "response")?)?;
                &[]
            }
            "tick" => {
                ran.effects = self.step(
                    Input::Tick(RiskClock::from_secs(second(at(step, "at")?, "at")?)),
                    None,
                )?;
                &[]
            }
            "fold" => {
                self.fold_mark(at(step, "event")?)?;
                &[]
            }
            "cancel" => {
                let reason = str_at(step, "reason")?;
                ran.effects = self.cancel(reason)?;
                cause_records(reason)
            }
            _ => {
                let reason = str_at(step, "reason")?;
                ensure(reason == "mode_tightened", || {
                    format!(
                        "a `{reason}` cancellation is applied by its own input's step, never batched with a response"
                    )
                })?;
                let now = at(step, "now")?;
                self.apply_now(now)?;
                ensure(self.state.effective_mode() < Mode::ExitsOnly, || {
                    "an exits-only restriction tightens nothing in a mode already as strict"
                        .to_owned()
                })?;
                ran.now = Some(now.clone());
                let clock = self.clock()?;
                self.account("AgentModeApplied", vec![("to", text("exits_only"))], clock)?;
                for response in list_at(step, "responses")? {
                    ran.effects.extend(self.respond(response)?);
                }
                cause_records(reason)
            }
        };
        ran.clock = self.clock()?;
        ran.drafts = ran
            .effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Journal(draft) => Some(draft.clone()),
                _ => None,
            })
            .collect();
        strip(&mut ran.drafts, records)?;
        Ok(ran)
    }

    /// One `handle`, every draft folded back as the agent stream's next event.
    fn step(&mut self, input: Input, proposal: Option<Proposal>) -> Result<Vec<Effect>, String> {
        let (ids, gate, flatten) = (Ids, Gate(self.verdict.clone()), Flatten);
        let plan = Plan {
            proposal: Cell::new(proposal),
            classified: self.classified.clone(),
        };
        let ports = Ports {
            ids: &ids,
            gate: &gate,
            plan: &plan,
            flatten: &flatten,
            view: &self.view,
        };
        let effects =
            handle(&mut self.state, input, &ports).map_err(|e| format!("the runtime: {e}"))?;
        for effect in &effects {
            if let Effect::Journal(draft) = effect {
                self.agent_seq = self.agent_seq.saturating_add(1);
                fold(
                    &mut self.state,
                    &FoldedEvent {
                        stream: AGENT_STREAM.to_owned(),
                        seq: Seq(self.agent_seq),
                        event_id: draft.event_id.clone(),
                        event_type: draft.event_type.clone(),
                        causation_id: draft.causation_id.clone(),
                        actor: ActorKind::Agent,
                        payload: draft.payload.clone(),
                    },
                )
                .map_err(|e| format!("folding {}: {e}", draft.event_type))?;
            }
        }
        Ok(effects)
    }

    /// One account-stream event folded at the next `seq`, carrying `risk_clock`.
    fn account(
        &mut self,
        event_type: &str,
        mut members: Vec<(&str, Value)>,
        risk_clock: i64,
    ) -> Result<FoldedEvent, String> {
        members.push(("risk_clock", seconds(risk_clock)?));
        self.account_seq = self.account_seq.saturating_add(1);
        let event = FoldedEvent {
            stream: ACCOUNT_STREAM.to_owned(),
            seq: Seq(self.account_seq),
            event_id: EventId(format!("2{:025}", self.account_seq)),
            event_type: event_type.to_owned(),
            causation_id: None,
            actor: ActorKind::System,
            payload: object(members)?,
        };
        fold(&mut self.state, &event).map_err(|e| format!("folding {event_type}: {e}"))?;
        Ok(event)
    }

    /// One control-stream event from `actor`, folded at the next `seq` as the tailer folds it.
    fn control(
        &mut self,
        event_type: &str,
        payload: Value,
        actor: ActorKind,
    ) -> Result<FoldedEvent, String> {
        self.control_seq = self.control_seq.saturating_add(1);
        let event = FoldedEvent {
            stream: CONTROL_STREAM.to_owned(),
            seq: Seq(self.control_seq),
            event_id: EventId(format!("1{:025}", self.control_seq)),
            event_type: event_type.to_owned(),
            causation_id: None,
            actor,
            payload,
        };
        fold(&mut self.state, &event).map_err(|e| format!("folding {event_type}: {e}"))?;
        Ok(event)
    }

    fn mark(&mut self, instrument: &str, price: &str, risk_clock: i64) -> Result<(), String> {
        self.account(
            "MarkUpdated",
            vec![("instrument", text(instrument)), ("price", text(price))],
            risk_clock,
        )?;
        self.marks
            .insert(instrument.to_owned(), (decimal(price)?, self.account_seq));
        Ok(())
    }

    /// `ask`: the bound order proposed once at a tick of the folded clock, classified `ask` by its
    /// `decided_by`, its reference mark folded at its `seq` first.
    fn ask(&mut self, bound: &Json) -> Result<Vec<Effect>, String> {
        unknown_members(bound, BOUND_KEYS)
            .map_err(|unknown| format!("`bound` members not interpreted: {unknown}"))?;
        let instrument = str_at(bound, "instrument")?;
        let clock = self.clock()?;
        match at(bound, "reference_mark")? {
            Json::Null => ensure(!self.marks.contains_key(instrument), || {
                "a request with no reference mark, after a mark was folded".to_owned()
            })?,
            mark => {
                unknown_members(mark, &["price", "seq"]).map_err(|unknown| {
                    format!("`reference_mark` members not interpreted: {unknown}")
                })?;
                let price = str_at(mark, "price")?;
                let seq = u64_at(mark, "seq")?;
                ensure(seq > self.account_seq, || {
                    format!("a reference mark at seq {seq} is not after the account stream's head")
                })?;
                for _ in self.account_seq..seq {
                    self.mark(instrument, price, clock)?;
                }
            }
        }
        let id = InstrumentId::new(instrument).map_err(|e| format!("`instrument`: {e:?}"))?;
        let proposal = Proposal {
            instrument: id.clone(),
            asset_class: match str_at(bound, "asset_class")? {
                "us_equity" => AssetClass::UsEquity,
                "crypto" => AssetClass::Crypto,
                other => return Err(format!("`{other}` is not an asset class")),
            },
            side: match str_at(bound, "side")? {
                "buy" => Side::Buy,
                other => return Err(format!("`{other}` is not the side an ask binds")),
            },
            qty: Qty::parse(str_at(bound, "qty")?).map_err(|e| format!("`qty`: {e}"))?,
            limit: Price::parse(str_at(bound, "limit_price")?)
                .map_err(|e| format!("`limit_price`: {e}"))?,
            purpose: purpose(str_at(bound, "purpose")?)?,
            combined_score: text(str_at(bound, "combined_score")?),
        };
        self.view.version = str_at(bound, "mandate_version")?.to_owned();
        self.view.working_universe = [id].into();
        self.view.restricted_instruments = BTreeSet::new();
        self.classified = Classified {
            autonomy: Autonomy::Ask,
            decided_by: Some(str_at(bound, "decided_by")?.to_owned()),
        };
        self.verdict = DryRunVerdict::Allow;
        self.instrument = Some(instrument.to_owned());
        self.step(Input::Tick(RiskClock::from_secs(clock)), Some(proposal))
    }

    fn asked_instrument(&self) -> Result<String, String> {
        self.instrument
            .clone()
            .ok_or_else(|| "no instrument has been asked for yet".to_owned())
    }

    /// `fold`: a `MarkUpdated` of the asked instrument at its unchanged price, carrying the event's
    /// clock, handed to nothing.
    fn fold_mark(&mut self, event: &Json) -> Result<(), String> {
        unknown_members(event, &["type", "clock"])
            .map_err(|unknown| format!("`event` members not interpreted: {unknown}"))?;
        let event_type = str_at(event, "type")?;
        ensure(event_type == "MarkUpdated", || {
            format!("a `fold` of `{event_type}` is not one the shell knows")
        })?;
        let instrument = self.asked_instrument()?;
        let (price, _) = self
            .marks
            .get(&instrument)
            .copied()
            .ok_or_else(|| "a `fold` of a mark before any mark was folded".to_owned())?;
        self.mark(
            &instrument,
            &price.to_string(),
            second(at(event, "clock")?, "clock")?,
        )
    }

    /// `now`: the view, the classification and the dry run as stated, then the mark and the mode
    /// folded where they differ.
    fn apply_now(&mut self, now: &Json) -> Result<(), String> {
        unknown_members(now, NOW_KEYS)
            .map_err(|unknown| format!("`now` members not interpreted: {unknown}"))?;
        let instrument = self.asked_instrument()?;
        let id = InstrumentId::new(&instrument).map_err(|e| format!("`instrument`: {e:?}"))?;
        self.view.version = str_at(now, "mandate_version")?.to_owned();
        self.view.restricted_instruments = if flag(now, "instrument_restricted")? {
            [id.clone()].into()
        } else {
            BTreeSet::new()
        };
        self.view.working_universe = if flag(now, "in_working_universe")? {
            [id].into()
        } else {
            BTreeSet::new()
        };
        let classification = at(now, "classification")?;
        unknown_members(classification, &["decision", "by"])
            .map_err(|unknown| format!("`classification` members not interpreted: {unknown}"))?;
        self.classified = Classified {
            autonomy: match str_at(classification, "decision")? {
                "auto" => Autonomy::Auto,
                "ask" => Autonomy::Ask,
                "deny" => Autonomy::Deny,
                other => return Err(format!("`{other}` is not a classification")),
            },
            decided_by: optional_text(classification, "by")?,
        };
        let dry_run = at(now, "dry_run")?;
        unknown_members(dry_run, &["verdict", "reason"])
            .map_err(|unknown| format!("`dry_run` members not interpreted: {unknown}"))?;
        let reason = optional_text(dry_run, "reason")?;
        self.verdict = match (str_at(dry_run, "verdict")?, reason) {
            ("allow", None) => DryRunVerdict::Allow,
            ("deny", Some(reason_code)) => DryRunVerdict::Deny { reason_code },
            (verdict, reason) => {
                return Err(format!(
                    "a dry run `{verdict}` with reason {reason:?} is not one the gate gives"
                ));
            }
        };
        let clock = self.clock()?;
        match at(now, "mark")? {
            Json::Null => ensure(!self.marks.contains_key(&instrument), || {
                "`now` has no mark, after a mark was folded".to_owned()
            })?,
            mark => {
                let price = mark
                    .as_str()
                    .ok_or_else(|| "`mark` is a decimal string".to_owned())?;
                if self.marks.get(&instrument).map(|(held, _)| *held) != Some(decimal(price)?) {
                    self.mark(&instrument, price, clock)?;
                }
            }
        }
        let mode = str_at(now, "mode")?;
        let wanted = mode_named(mode)?;
        if self.state.effective_mode() != wanted {
            self.account("AgentModeApplied", vec![("to", text(mode))], clock)?;
        }
        ensure(self.state.effective_mode() == wanted, || {
            format!(
                "`now` states mode `{mode}`, and the runtime's effective mode is {:?}",
                self.state.effective_mode()
            )
        })
    }

    /// A response: a new `source` is folded on the control stream and handed in; a re-tailed one
    /// hands the event already folded again, which must be the same response.
    fn respond(&mut self, response: &Json) -> Result<Vec<Effect>, String> {
        unknown_members(response, RESPONSE_KEYS)
            .map_err(|unknown| format!("`response` members not interpreted: {unknown}"))?;
        let source = str_at(response, "source")?;
        if let Some((event, first)) = self.sources.get(source) {
            ensure(first == response, || {
                format!("`{source}` is re-tailed with other members than it was first handed with")
            })?;
            let event = event.clone();
            return self.step(Input::Journal(event), None);
        }
        let verdict = str_at(response, "verdict")?;
        ensure(matches!(verdict, "approved" | "skipped"), || {
            format!("`{verdict}` is not a verdict")
        })?;
        let actor = match str_at(response, "actor_kind")? {
            "user" => ActorKind::User,
            "agent" => ActorKind::Agent,
            "system" => ActorKind::System,
            "broker" => ActorKind::Broker,
            "platform_operator" => ActorKind::PlatformOperator,
            other => return Err(format!("`{other}` is not an actor kind")),
        };
        let payload = object(vec![
            ("agent", text(AGENT)),
            (
                "approval",
                text(&translate(
                    &self.approval_ids,
                    str_at(response, "approval")?,
                )?),
            ),
            ("verdict", text(verdict)),
            (
                "content_hash",
                text(&translate(&self.hashes, str_at(response, "content_hash")?)?),
            ),
            (
                "submitted_at",
                seconds(second(at(response, "submitted_at")?, "submitted_at")?)?,
            ),
            ("step_up", step_up(at(response, "step_up")?)?),
            ("responder", text(str_at(response, "responder")?)),
            ("role", text("approver")),
        ])?;
        let event = self.control("ApprovalResponseSubmitted", payload, actor)?;
        self.sources
            .insert(source.to_owned(), (event.clone(), response.clone()));
        self.step(Input::Journal(event), None)
    }

    /// `cancel`: the input that cancels for `reason`, folded and handed in.
    fn cancel(&mut self, reason: &str) -> Result<Vec<Effect>, String> {
        let clock = self.clock()?;
        let event = match reason {
            "version_applied" => self.account("MandateVersionApplied", Vec::new(), clock)?,
            "mode_tightened" => {
                self.account("AgentModeApplied", vec![("to", text("exits_only"))], clock)?
            }
            "owner_pause" | "owner_stop" | "kill_switch" => {
                let command = match reason {
                    "owner_pause" => "pause",
                    "owner_stop" => "stop",
                    _ => "kill_switch",
                };
                let assertion = format!("cancel-{}", self.control_seq.saturating_add(1));
                let payload = object(vec![
                    ("agent", text(AGENT)),
                    ("command", text(command)),
                    ("scope", text("agent")),
                    ("subject", text(AGENT)),
                    ("bid", Value::Null),
                    ("bid_size", Value::Null),
                    ("floor", Value::Null),
                    ("submitted_at", seconds(clock)?),
                    (
                        "step_up",
                        object(vec![
                            ("assertion_id", text(&assertion)),
                            ("authenticated_at", seconds(clock)?),
                            ("method", text("cli_confirm")),
                        ])?,
                    ),
                    ("user", text(&self.author)),
                ])?;
                self.control("OwnerCommandIssued", payload, ActorKind::User)?
            }
            other => return Err(format!("`{other}` is not a cancellation reason")),
        };
        self.step(Input::Journal(event), None)
    }

    /// Each expected draft against the runtime's, in order, and every handoff after the record that
    /// authorises it (`AGENTS.md` rule 5).
    fn compare(&mut self, ran: &Ran, expected: &[Json]) -> Result<(), String> {
        let types = |drafts: &[EventDraft]| -> Vec<String> {
            drafts.iter().map(|d| d.event_type.clone()).collect()
        };
        ensure(ran.drafts.len() == expected.len(), || {
            format!(
                "{} drafts expected, the runtime wrote {:?}",
                expected.len(),
                types(&ran.drafts)
            )
        })?;
        let mut faults = Vec::new();
        for (index, (want, draft)) in expected.iter().zip(&ran.drafts).enumerate() {
            let number = index.saturating_add(1);
            faults.extend(
                self.compare_draft(want, draft, ran)
                    .into_iter()
                    .map(|fault| format!("draft {number} ({}): {fault}", draft.event_type)),
            );
        }
        faults.extend(handoffs(&ran.effects));
        ensure(faults.is_empty(), || faults.join("; "))
    }

    fn compare_draft(&mut self, want: &Json, draft: &EventDraft, ran: &Ran) -> Vec<String> {
        let mut faults = Vec::new();
        let event_type = match str_at(want, "type") {
            Ok(event_type) => event_type,
            Err(e) => return vec![e],
        };
        let Some(plain) = plain_members(event_type) else {
            return vec![format!("`{event_type}` is not a draft type the map knows")];
        };
        let special = special_members(event_type);
        let known: Vec<&str> = ["type", "clock"]
            .into_iter()
            .chain(plain.iter().map(|(case, _, _)| *case))
            .chain(special.iter().map(|(case, _)| *case))
            .collect();
        if let Err(unknown) = unknown_members(want, &known) {
            faults.push(format!("members not interpreted: {unknown}"));
        }
        if draft.event_type != event_type {
            faults.push(format!("expected a {event_type}"));
            return faults;
        }
        match second_of(want, "clock") {
            Ok(clock) if clock == ran.clock => {}
            Ok(clock) => faults.push(format!(
                "clock: expected {clock}, the step is at {}",
                ran.clock
            )),
            Err(e) => faults.push(e),
        }
        let mut written: BTreeSet<&str> = BTreeSet::new();
        for (case, runtime, how) in plain {
            if let Some(value) = want.get(*case) {
                written.insert(runtime);
                if let Err(e) = member(*how, value, draft.payload.get(runtime)) {
                    faults.push(format!("`{case}`: {e}"));
                }
            }
        }
        for (_, runtime) in special {
            written.insert(runtime);
        }
        let special_faults = match event_type {
            "ApprovalRequested" => self.requested(want, draft, ran),
            "ApprovalResponded" => self.responded(want, draft),
            "IntentProposed" => self.intended(want, draft, ran),
            _ => self.names_approval(want, draft),
        };
        faults.extend(special_faults);
        for (name, value) in runtime_only(event_type, draft, ran, &self.asked) {
            written.insert(name);
            faults.extend(extra_fault(draft, name, value));
        }
        for key in draft.payload.as_object().into_iter().flat_map(|o| o.keys()) {
            let name = key.as_str();
            if !written.contains(name) {
                faults.push(format!(
                    "the runtime wrote `{name}`, which the case does not state"
                ));
            }
        }
        faults
    }

    /// An `ApprovalRequested`: the ask's approval name bound to its `event_id` and its hash to the
    /// runtime's, which must be the digest of the runtime's own `content`; and the bound order's
    /// members read against what the runtime bound.
    fn requested(&mut self, want: &Json, draft: &EventDraft, ran: &Ran) -> Vec<String> {
        let mut faults = Vec::new();
        let Some((approval, bound)) = &ran.asked else {
            return vec!["a request outside an ask".to_owned()];
        };
        if want.get("approval").and_then(Json::as_str) != Some(approval.as_str()) {
            faults.push(format!("`approval`: expected the ask's `{approval}`"));
        }
        if let Err(e) = bind(&mut self.approval_ids, approval, &draft.event_id.0) {
            faults.push(e);
        }
        let hash = stated_hash(draft);
        if let Err(e) = &hash {
            faults.push(e.clone());
        }
        match (want.get("content_hash").and_then(Json::as_str), hash) {
            (Some(name), Ok(hash)) => {
                if let Err(e) = bind(&mut self.hashes, name, hash) {
                    faults.push(e);
                }
            }
            _ => faults.push("`content_hash`: the case and the runtime each state one".to_owned()),
        }
        for (case, runtime, how) in plain_members("ApprovalRequested").unwrap_or_default() {
            if let Some(value) = bound.get(*case)
                && let Err(e) = member(*how, value, draft.payload.get(runtime))
            {
                faults.push(format!("bound `{case}`: {e}"));
            }
        }
        self.asked.insert(
            approval.clone(),
            Asked {
                id: draft.event_id.0.clone(),
                version: draft
                    .payload
                    .get("mandate_version")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                bound: bound.clone(),
            },
        );
        faults
    }

    /// The draft's `approval` is the runtime's id for the case's name.
    fn names_approval(&self, want: &Json, draft: &EventDraft) -> Vec<String> {
        let wanted = want
            .get("approval")
            .and_then(Json::as_str)
            .map(|name| translate(&self.approval_ids, name));
        match (
            wanted,
            draft.payload.get("approval").and_then(Value::as_str),
        ) {
            (Some(Ok(id)), Some(got)) if id == got => Vec::new(),
            (wanted, got) => vec![format!("`approval`: expected {wanted:?}, got {got:?}")],
        }
    }

    /// An `ApprovalResponded`: its approval, and the control-stream event it copies.
    fn responded(&self, want: &Json, draft: &EventDraft) -> Vec<String> {
        let mut faults = self.names_approval(want, draft);
        let source = want
            .get("source")
            .and_then(Json::as_str)
            .and_then(|name| self.sources.get(name))
            .map(|(event, _)| &event.event_id);
        if source.is_none() || draft.causation_id.as_ref() != source {
            faults.push(format!(
                "`source`: expected the copy of {source:?}, its cause is {:?}",
                draft.causation_id
            ));
        }
        faults
    }

    /// An `IntentProposed`: caused by this step's `ApprovalRevalidated` of the case's approval, whose
    /// request bound the mandate version the case states.
    fn intended(&self, want: &Json, draft: &EventDraft, ran: &Ran) -> Vec<String> {
        let cause = ran
            .drafts
            .iter()
            .find(|d| Some(&d.event_id) == draft.causation_id.as_ref());
        let Some(revalidation) = cause else {
            return vec!["its cause is no draft of this step".to_owned()];
        };
        if revalidation.event_type != "ApprovalRevalidated" {
            return vec![format!("its cause is a {}", revalidation.event_type)];
        }
        let mut faults = self.names_approval(want, revalidation);
        let named = want
            .get("approval")
            .and_then(Json::as_str)
            .unwrap_or_default();
        let version = self.asked.get(named).map(|asked| asked.version.as_str());
        if version.is_none() || want.get("mandate_version").and_then(Json::as_str) != version {
            faults.push(format!(
                "`mandate_version`: the approval's request bound {version:?}"
            ));
        }
        faults
    }
}

/// The members the runtime writes that no case states, each with what it must hold, or `None` for
/// one [`Shell::requested`] checks: `on_timeout` is always `skip`, `cli_inbox` has no message id, a
/// response's role is `approver`, and a re-validation records the values it compared.
fn runtime_only(
    event_type: &str,
    draft: &EventDraft,
    ran: &Ran,
    asked: &BTreeMap<String, Asked>,
) -> Vec<(&'static str, Option<Result<Value, String>>)> {
    match event_type {
        "ApprovalRequested" => vec![("on_timeout", Some(Ok(text("skip")))), ("content", None)],
        "ApprovalDelivered" => vec![("message_id", Some(Ok(Value::Null)))],
        "ApprovalResponded" => vec![("role", Some(Ok(text("approver"))))],
        "ApprovalRevalidated" => revalidated_values(draft, ran, asked)
            .into_iter()
            .map(|(name, value)| (name, Some(value)))
            .collect(),
        _ => Vec::new(),
    }
}

/// One runtime-only member against what it must hold, or nothing when another check reads it.
fn extra_fault(
    draft: &EventDraft,
    name: &str,
    value: Option<Result<Value, String>>,
) -> Option<String> {
    match value {
        Some(Ok(value)) if draft.payload.get(name) == Some(&value) => None,
        Some(Ok(value)) => Some(format!(
            "`{name}`: expected {value:?}, got {:?}",
            draft.payload.get(name)
        )),
        Some(Err(e)) => Some(format!("`{name}`: {e}")),
        None => None,
    }
}

/// What an `ApprovalRevalidated` records it compared (journal spec §9): the bound order's version,
/// trigger and reference mark, `now`'s version, mode, restriction, trigger, dry run and mark, and
/// the asset class's drift band (§6.4: 100 bp for `us_equity`, 200 bp for `crypto`).
fn revalidated_values(
    draft: &EventDraft,
    ran: &Ran,
    asked: &BTreeMap<String, Asked>,
) -> Vec<(&'static str, Result<Value, String>)> {
    let bound = asked
        .values()
        .find(|asked| draft.payload.get("approval").and_then(Value::as_str) == Some(&asked.id))
        .map(|asked| &asked.bound)
        .ok_or_else(|| "the approval is not one the case asked for".to_owned());
    let now = ran
        .now
        .as_ref()
        .ok_or_else(|| "a re-validation outside a response".to_owned());
    vec![
        (
            "mandate_version_bound",
            read(&bound, "mandate_version", Recorded::Text),
        ),
        (
            "mandate_version_now",
            read(&now, "mandate_version", Recorded::Text),
        ),
        ("mode", read(&now, "mode", Recorded::Text)),
        (
            "instrument_restricted",
            read(&now, "instrument_restricted", Recorded::Flag),
        ),
        (
            "decided_by_bound",
            read(&bound, "decided_by", Recorded::Text),
        ),
        (
            "decided_by_now",
            read(&now, "classification.by", Recorded::Label),
        ),
        ("dry_run", read(&now, "dry_run.verdict", Recorded::Text)),
        (
            "dry_run_reason",
            read(&now, "dry_run.reason", Recorded::Text),
        ),
        ("m_req", reference_price(&bound)),
        ("m_now", read(&now, "mark", Recorded::Decimal)),
        ("band_bp", band(&bound)),
    ]
}

fn read(source: &Result<&Json, String>, path: &str, how: Recorded) -> Result<Value, String> {
    recorded(how, at(source.clone()?, path)?)
}

fn reference_price(bound: &Result<&Json, String>) -> Result<Value, String> {
    match at(bound.clone()?, "reference_mark")? {
        Json::Null => Ok(Value::Null),
        mark => recorded(Recorded::Decimal, at(mark, "price")?),
    }
}

fn band(bound: &Result<&Json, String>) -> Result<Value, String> {
    match str_at(bound.clone()?, "asset_class")? {
        "us_equity" => int(100),
        "crypto" => int(200),
        other => Err(format!("`{other}` is not an asset class")),
    }
}

/// How a value the runtime records was read from the case.
#[derive(Debug, Clone, Copy)]
enum Recorded {
    Text,
    Flag,
    /// A `decided_by` label, which the runtime records as `""` when the classifier names none.
    Label,
    /// A decimal string or `null`, recorded in the runtime's own canonical text.
    Decimal,
}

fn recorded(how: Recorded, value: &Json) -> Result<Value, String> {
    match (how, value) {
        (Recorded::Text | Recorded::Decimal, Json::Null) => Ok(Value::Null),
        (Recorded::Label, Json::Null) => Ok(text("")),
        (Recorded::Text | Recorded::Label, Json::String(s)) => Ok(text(s)),
        (Recorded::Decimal, Json::String(s)) => Price::parse(s)
            .map(|price| text(&price.to_string()))
            .map_err(|e| format!("`{s}`: {e}")),
        (Recorded::Flag, Json::Bool(b)) => Ok(Value::Bool(*b)),
        (how, value) => Err(format!("{value} is not a {how:?} value")),
    }
}

/// The runtime's `content_hash`, which must be the digest of the canonical form of its own inline
/// `content` (journal spec §4, mandate spec §6.4).
fn stated_hash(draft: &EventDraft) -> Result<&str, String> {
    let hash = draft.payload.get("content_hash").and_then(Value::as_str);
    let digest = draft
        .payload
        .get("content")
        .map(|content| format!("sha256:{}", Digest::of(&to_canonical(content))));
    match hash {
        Some(hash) if Some(hash) == digest.as_deref() => Ok(hash),
        _ => Err("the runtime's `content_hash` is not the digest of its `content`".to_owned()),
    }
}

/// The records a cancellation writes before its `ApprovalCanceled`, which no script states: the
/// mode it applied, and for a kill switch the owner's instruction and the switch itself.
fn cause_records(reason: &str) -> &'static [(&'static str, &'static str)] {
    match reason {
        "mode_tightened" => &[("AgentModeChanged", "restriction_changed")],
        "owner_pause" => &[("AgentModeChanged", "owner_pause")],
        "owner_stop" => &[("AgentModeChanged", "owner_stop")],
        "kill_switch" => &[
            ("AgentModeChanged", "kill_switch"),
            ("OwnerExitRequested", ""),
            ("KillSwitchActivated", ""),
        ],
        _ => &[],
    }
}

/// Sets aside the step's first drafts as `records` names them, by type and, where given, reason.
fn strip(drafts: &mut Vec<EventDraft>, records: &[(&str, &str)]) -> Result<(), String> {
    for (event_type, reason) in records {
        let Some(first) = drafts.first() else {
            return Ok(());
        };
        let fits = first.event_type == *event_type
            && (reason.is_empty() || first.payload.get("reason") == Some(&text(reason)));
        ensure(fits, || {
            format!(
                "the step's records begin {} {:?}, not the {event_type} the map sets aside",
                first.event_type, first.payload
            )
        })?;
        drafts.remove(0);
    }
    Ok(())
}

/// Every handoff follows the draft that records it in the same list: an order its `IntentProposed`,
/// a flatten its `KillSwitchActivated` or `OwnerExitRequested`; and every `IntentProposed` is
/// handed exactly once.
fn handoffs(effects: &[Effect]) -> Vec<String> {
    let mut faults = Vec::new();
    let mut recorded: BTreeMap<&EventId, &str> = BTreeMap::new();
    let mut handed: BTreeMap<&EventId, usize> = BTreeMap::new();
    for effect in effects {
        match effect {
            Effect::Journal(draft) => {
                recorded.insert(&draft.event_id, &draft.event_type);
            }
            Effect::Intent(handoff) => {
                let by = recorded.get(&handoff.intent_id).copied();
                let fits = match &handoff.body {
                    IntentBody::Order { .. } => by == Some("IntentProposed"),
                    IntentBody::Flatten(_) => {
                        matches!(by, Some("KillSwitchActivated" | "OwnerExitRequested"))
                    }
                };
                if !fits {
                    faults.push(format!(
                        "a handoff of {:?} follows no record of it",
                        handoff.intent_id
                    ));
                }
                let count = handed.entry(&handoff.intent_id).or_default();
                *count = count.saturating_add(1);
            }
            _ => {}
        }
    }
    for (id, event_type) in &recorded {
        if *event_type == "IntentProposed" && handed.get(id).copied() != Some(1) {
            faults.push(format!("the intent {id:?} is not handed exactly once"));
        }
    }
    faults
}

/// One case member against the runtime's, under `how`.
fn member(how: As, want: &Json, got: Option<&Value>) -> Result<(), String> {
    let fault = || format!("expected {want}, got {got:?}");
    let ok = match (how, want, got) {
        (As::Text, Json::Null, Some(Value::Null)) => true,
        (As::Text, Json::String(s), Some(got)) => got.as_str() == Some(s.as_str()),
        (As::Decimal, Json::String(s), Some(got)) => {
            let got = got.as_str().ok_or_else(fault)?;
            decimal(s)? == decimal(got)?
        }
        (As::Seconds, Json::String(_), Some(got)) => {
            let secs = u64::try_from(second(want, "instant")?).map_err(|_| fault())?;
            got.as_int() == Some(secs)
        }
        (As::Count, Json::Number(n), Some(got)) => {
            n.as_u64().is_some() && got.as_int() == n.as_u64()
        }
        (As::Flag, Json::Bool(b), Some(Value::Bool(got))) => b == got,
        (As::Mark | As::StepUp, Json::Null, Some(Value::Null)) => true,
        (As::Mark, Json::Object(_), Some(got)) => {
            unknown_members(want, &["price", "seq"])?;
            fields_are(got, &["price", "seq"])?;
            member(As::Decimal, at(want, "price")?, got.get("price")).is_ok()
                && member(As::Count, at(want, "seq")?, got.get("seq")).is_ok()
        }
        (As::StepUp, Json::Object(_), Some(got)) => {
            unknown_members(want, &["assertion", "authenticated_at", "method"])?;
            fields_are(got, &["assertion_id", "authenticated_at", "method"])?;
            member(As::Text, at(want, "assertion")?, got.get("assertion_id")).is_ok()
                && member(
                    As::Seconds,
                    at(want, "authenticated_at")?,
                    got.get("authenticated_at"),
                )
                .is_ok()
                && member(As::Text, at(want, "method")?, got.get("method")).is_ok()
        }
        (As::Quorum, Json::Object(_), None) => {
            return Err(
                "the runtime records no quorum: `ApprovalResponded` owes the approver count and \
                 independence check 7 applied (journal spec §9, mandate spec §6.4), which E8-3's \
                 runtime does not write yet"
                    .to_owned(),
            );
        }
        (As::Quorum, Json::Object(_), Some(got)) => {
            unknown_members(want, &["required", "independent"])?;
            fields_are(got, &["required", "independent"])?;
            member(As::Count, at(want, "required")?, got.get("required")).is_ok()
                && member(As::Flag, at(want, "independent")?, got.get("independent")).is_ok()
        }
        _ => false,
    };
    ensure(ok, fault)
}

/// A runtime object with exactly these members.
fn fields_are(got: &Value, names: &[&str]) -> Result<(), String> {
    let keys: Vec<&str> = got
        .as_object()
        .map(|o| o.keys().map(Key::as_str).collect())
        .unwrap_or_default();
    let mut wanted = names.to_vec();
    wanted.sort_unstable();
    ensure(keys == wanted, || {
        format!("the runtime's members are {keys:?}, not {wanted:?}")
    })
}

/// The runtime's name for the case's, or the case's own when none is bound, which must not be a
/// runtime name bound to another (a wrong hash stays wrong).
fn translate(bound: &BTreeMap<String, String>, name: &str) -> Result<String, String> {
    match bound.get(name) {
        Some(runtime) => Ok(runtime.clone()),
        None => {
            ensure(!bound.values().any(|runtime| runtime == name), || {
                format!("`{name}` is a runtime name bound to another case name")
            })?;
            Ok(name.to_owned())
        }
    }
}

/// Binds a case name to a runtime one, one to one.
fn bind(bound: &mut BTreeMap<String, String>, name: &str, runtime: &str) -> Result<(), String> {
    let clash = bound
        .iter()
        .any(|(other, held)| (other == name) != (held == runtime));
    ensure(!clash, || {
        format!("`{name}` and `{runtime}` are each bound to another name")
    })?;
    bound.insert(name.to_owned(), runtime.to_owned());
    Ok(())
}

/// What an ask binds: `open` or `increase`, since an exit is never asked (`AGENTS.md` rule 2).
fn purpose(name: &str) -> Result<Purpose, String> {
    match name {
        "open" => Ok(Purpose::Open),
        "increase" => Ok(Purpose::Increase),
        other => Err(format!("`{other}` is not a purpose an ask binds")),
    }
}

fn mode_named(name: &str) -> Result<Mode, String> {
    Ok(match name {
        "normal" => Mode::Normal,
        "exits_only" => Mode::ExitsOnly,
        "paused" => Mode::Paused,
        "stopped" => Mode::Stopped,
        other => return Err(format!("`{other}` is not a mode")),
    })
}

/// A step-up as the control stream writes it: `{assertion_id, authenticated_at, method}`.
fn step_up(evidence: &Json) -> Result<Value, String> {
    if evidence.is_null() {
        return Ok(Value::Null);
    }
    unknown_members(evidence, &["assertion", "authenticated_at", "method"])
        .map_err(|unknown| format!("`step_up` members not interpreted: {unknown}"))?;
    object(vec![
        ("assertion_id", text(str_at(evidence, "assertion")?)),
        (
            "authenticated_at",
            seconds(second(
                at(evidence, "authenticated_at")?,
                "authenticated_at",
            )?)?,
        ),
        ("method", text(str_at(evidence, "method")?)),
    ])
}

fn flag(value: &Json, key: &str) -> Result<bool, String> {
    at(value, key)?
        .as_bool()
        .ok_or_else(|| format!("`{key}` is a boolean"))
}

fn optional_text(value: &Json, key: &str) -> Result<Option<String>, String> {
    match at(value, key)? {
        Json::Null => Ok(None),
        Json::String(s) => Ok(Some(s.clone())),
        _ => Err(format!("`{key}` is a string or null")),
    }
}

fn decimal(text: &str) -> Result<Signed, String> {
    Signed::parse(text).map_err(|e| format!("`{text}`: {e}"))
}

/// The whole risk-clock second an instant names; a fraction fails rather than being dropped
/// (mandate spec §5.2, DEC-292 item 2).
fn second(value: &Json, what: &str) -> Result<i64, String> {
    let when = instant(value, what)?;
    ensure(when.nanos() == 0, || {
        format!("`{what}` is not a whole second")
    })?;
    Ok(when.secs())
}

fn second_of(value: &Json, key: &str) -> Result<i64, String> {
    second(at(value, key)?, key)
}

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

fn int(n: u64) -> Result<Value, String> {
    Int::new(n)
        .map(Value::Int)
        .ok_or_else(|| format!("{n} is not a canonical integer"))
}

fn seconds(secs: i64) -> Result<Value, String> {
    int(u64::try_from(secs).map_err(|_| format!("{secs} is before the epoch"))?)
}

fn object(members: Vec<(&str, Value)>) -> Result<Value, String> {
    Ok(Value::Object(
        members
            .into_iter()
            .map(|(key, value)| {
                Key::new(key)
                    .map(|key| (key, value))
                    .map_err(|_| format!("`{key}` is not a canonical key"))
            })
            .collect::<Result<_, _>>()?,
    ))
}

#[cfg(test)]
mod tests;
