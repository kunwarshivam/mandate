//! Family E's `lifecycle` op (MC-E01 to MC-E24, MC-E29, MC-E31; mandate spec §6.1, §6.4; DEC-280,
//! DEC-292 item 3, DEC-317): each case's script driven through `mandate-runtime`'s own [`handle`]
//! and [`fold`], so every draft is the runtime's. Nothing here decides an admission, a
//! re-validation, a timeout or a cancellation.
//!
//! **The shell.** One deployment (`a` on connection `c` in workspace `w`) starts from a clean
//! reconciliation folded at the case's `start`, so no startup hold is taken, and every draft a step
//! returns is folded back as the agent stream's next event, as the shell does.
//!
//! **`ask`** is a tick at the folded clock whose order plan proposes the bound order once, classified
//! `ask` by its `decided_by`, the dry run allowing it. Its reference mark is folded first as the
//! account stream's `MarkUpdated` at its `seq`, the seqs before it repeating its price. The step's
//! `DecisionMade` records the decision the script starts from, and is set aside. The other steps land
//! in DEC-317's later slices and fail naming theirs until then.
//!
//! **The map** (DEC-317 item 4). A draft's `clock` is the folded risk clock after its step;
//! instants are whole seconds; `order_type` disambiguates the intent payload's `type` from the
//! expected draft's event type; every other member compares under the closed journal name. The
//! case's approval and content-hash names are bound one to one to the runtime's request id and hash:
//! the reference model hashes its request as a stand-in for the content object
//! (`reference/mandate/ref.py`, `escalation_step`), so a case's hash is a name, and the runtime's
//! `content_hash` must be the digest of its own inline `content`. Every member of a draft's payload
//! is one the case states or one checked here, and so is its cause. The one reading of an unstated
//! member is journal spec §9.7's (DEC-533 item 3): an `ApprovalResponded` writes `quorum`,
//! `separation_of_duties` and `delegation` always, `null` where nothing was judged, so a case that
//! leaves one unstated reads it as absent or `null`, never as a value; and a step whose grant check 7
//! judged (rule 48) must state its `quorum`. A re-validation's `decided_by_now` is §9.7's (DEC-533
//! item 4), or until E8-3's writer lands the superseded value the runtime writes (DEC-830). The
//! step's other effects, the
//! deadline timer and the opaque notification, are `mandate-runtime`'s own tests' to pin (DEC-317
//! item 5).
//!
//! **Every member is read (DEC-85).** The case, the context, each step, the bound order, each
//! expectation and each expected draft are swept, and a member or value this module does not know
//! fails the case naming it.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_canon::{Digest, Int, Key, Value, to_canonical};
use mandate_journal::Environment;
use mandate_num::{Price, Qty};
use mandate_runtime::{
    ActorKind, AgentId, ApprovalSettings, Autonomy, Classified, ConnectionId, Deployment,
    DryRunVerdict, Effect, EventDraft, EventId, FlattenPlan, FlattenPlanner, FlattenRequest,
    FoldedEvent, GateDryRun, IdGen, Input, IntentBody, MandateView, Mode, OrderPlan, Ports,
    Proposal, Purpose, RiskClock, RuntimeState, Seq, SignalInputs, WorkspaceId, WriterEpoch, fold,
    handle,
};
use mandate_time::NewYorkTime;

use crate::mandate::{instant, unknown_members};
use crate::{Json, at, ensure, list_at, str_at, to_canon, u64_at};

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
/// An expected request's members the map reads plainly, by the case's name, the runtime's, and how.
/// A bound order has the same members but the last, the deadline.
const REQUESTED: &[(&str, &str, As)] = &[
    ("instrument", "instrument", As::Canonical),
    ("asset_class", "asset_class", As::Canonical),
    ("side", "side", As::Canonical),
    ("qty", "qty", As::Canonical),
    ("limit_price", "limit", As::Canonical),
    ("purpose", "purpose", As::Canonical),
    ("mandate_version", "mandate_version", As::Canonical),
    ("decided_by", "decided_by", As::Canonical),
    ("combined_score", "combined_score", As::Canonical),
    ("reference_mark", "reference_mark", As::Canonical),
    ("approvers_required", "approvers_required", As::Canonical),
    (
        "independent_required",
        "independent_required",
        As::Canonical,
    ),
    ("timeout_s", "timeout_s", As::Canonical),
    ("deadline", "deadline", As::Seconds),
];
const RESPONDED: &[(&str, &str, As)] = &[
    ("responder", "responder", As::Canonical),
    ("verdict", "verdict", As::Canonical),
    ("step_up", "step_up", As::StepUp),
    ("result", "result", As::Canonical),
    ("reason", "reason", As::Canonical),
    ("effective_at", "effective_at", As::Seconds),
    ("quorum", "quorum", As::Quorum),
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
const REVALIDATED: &[(&str, &str, As)] = &[
    ("result", "result", As::Canonical),
    ("reason", "reason", As::Canonical),
];
const INTENDED: &[(&str, &str, As)] = &[
    ("instrument_id", "instrument_id", As::Canonical),
    ("side", "side", As::Canonical),
    ("order_type", "type", As::Canonical),
    ("tif", "tif", As::Canonical),
    ("qty", "qty", As::Canonical),
    ("limit_price", "limit_price", As::Canonical),
    ("purpose", "purpose", As::Canonical),
];
/// The one member the runtime writes that no draft compares: the request's `content`, checked only
/// as its hash's preimage (DEC-317 item 5). Any other member supplied without a value fails.
const UNCOMPARED: &[&str] = &["content"];
/// The `ApprovalResponded` members a case may leave unstated, each then read as absent or `null`
/// (journal spec §9.7, DEC-533 item 3): `quorum` is `null` wherever check 7 was not judged, and the
/// other two are `null` at this version. A case written before §9.7 states none of them.
const NULL_UNLESS_STATED: &[&str] = &["quorum", "separation_of_duties", "delegation"];
const TIMED_OUT: &[(&str, &str, As)] = &[("on_timeout", "on_timeout", As::Canonical)];
const CANCELED: &[(&str, &str, As)] = &[("reason", "reason", As::Canonical)];
const DELIVERED: &[(&str, &str, As)] = &[
    ("channel", "channel", As::Canonical),
    ("status", "status", As::Canonical),
];

/// How a case member is read off the runtime's draft (DEC-317 item 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum As {
    /// As the case writes it, which is the runtime's canonical form: a decimal in its canonical
    /// text, a reference mark as `{price, seq}`, `null` as `null`.
    Canonical,
    /// An instant, written by the runtime as its whole second.
    Seconds,
    /// `{assertion, authenticated_at, method}` or `null`, as the control stream writes it.
    StepUp,
    /// Check 7's `{required, independent}`, which the runtime does not record yet (E8-3).
    Quorum,
}

/// ULID-shaped agent-stream ids from the epoch, the head and the ordinal in decimal digits, which
/// are Crockford characters.
struct Ids;

impl IdGen for Ids {
    fn event_id(&self, epoch: WriterEpoch, head: Seq, ordinal: u32) -> EventId {
        EventId(format!("0{:05}{:010}{:010}", epoch.0, head.0, ordinal))
    }
}

/// The order plan of one step: the bound order once at an ask, and the step's classification.
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

/// What one script step did: the drafts the case states, after the records it does not, every
/// effect, the folded clock after it, the ask it made by the case's approval name and bound order,
/// and the `now` its responses were judged against.
struct Ran {
    drafts: Vec<EventDraft>,
    effects: Vec<Effect>,
    clock: i64,
    asked: Option<(String, Json)>,
    now: Option<Json>,
    /// The `DecisionMade` an ask set aside, which its request names as its cause.
    decision: Option<EventId>,
}

/// An approval the runtime asked for, by the case's name: the request's `event_id`, the mandate
/// version the runtime bound, and the case's bound order.
struct Asked {
    id: String,
    version: String,
    bound: Json,
}

/// The shell around one runtime, and the names bound so far.
struct Shell {
    state: RuntimeState,
    agent_seq: u64,
    account_seq: u64,
    view: MandateView,
    classified: Classified,
    verdict: DryRunVerdict,
    control_seq: u64,
    /// The mandate's author, who commands a pause, a Stop or a kill switch.
    author: String,
    asked: BTreeMap<String, Asked>,
    /// The last folded mark of each instrument, in its canonical text.
    marks: BTreeMap<String, String>,
    /// The instrument of the last ask, which `now` reads.
    instrument: Option<String>,
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
    ensure(!script.is_empty() && script.len() == expected.len(), || {
        format!("{} steps and {} expectations", script.len(), expected.len())
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
    if let window @ Json::Object(_) = at(context, "quiet_hours")? {
        unknown_members(window, &["start", "end"])
            .map_err(|unknown| format!("`quiet_hours` members not interpreted: {unknown}"))?;
        for key in ["start", "end"] {
            NewYorkTime::parse(str_at(window, key)?).map_err(|e| format!("`{key}`: {e}"))?;
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
            state: RuntimeState::new(Deployment {
                agent: AgentId("a".to_owned()),
                connection: ConnectionId("c".to_owned()),
                workspace: WorkspaceId("w".to_owned()),
            }),
            agent_seq: 0,
            account_seq: 0,
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
            control_seq: 0,
            author,
            asked: BTreeMap::new(),
            marks: BTreeMap::new(),
            instrument: None,
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
        let mut ran = Ran {
            drafts: Vec::new(),
            effects: Vec::new(),
            clock: 0,
            asked: None,
            now: step.get("now").cloned(),
            decision: None,
        };
        let records: &[(&str, &str)] = match kind {
            "ask" => {
                let bound = at(step, "bound")?;
                ran.effects = self.ask(bound)?;
                ran.asked = Some((str_at(step, "approval")?.to_owned(), bound.clone()));
                &[("DecisionMade", "")]
            }
            "response" => {
                let moved = self.apply_now(at(step, "now")?)?;
                ran.effects = self.respond(at(step, "response")?)?;
                if moved {
                    cause_records("mode_tightened")
                } else {
                    &[]
                }
            }
            "tick" => {
                let at = second(at(step, "at")?, "at")?;
                ran.effects = self.step(Input::Tick(RiskClock::from_secs(at)), None)?;
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
                self.apply_now(at(step, "now")?)?;
                ensure(self.state.effective_mode() < Mode::ExitsOnly, || {
                    "an exits-only restriction tightens nothing in a mode already as strict"
                        .to_owned()
                })?;
                let clock = self.clock()?;
                self.account("AgentModeApplied", vec![("to", text("exits_only"))], clock)?;
                for response in list_at(step, "responses")? {
                    ran.effects.extend(self.respond(response)?);
                }
                cause_records(reason)
            }
        };
        ran.drafts = ran
            .effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Journal(draft) => Some(draft.clone()),
                _ => None,
            })
            .collect();
        ran.decision = strip(&mut ran.drafts, records)?
            .into_iter()
            .find(|d| d.event_type == "DecisionMade")
            .map(|d| d.event_id);
        ran.clock = self.clock()?;
        Ok(ran)
    }

    /// `fold`: a `MarkUpdated` of the asked instrument at its unchanged price, carrying the event's
    /// clock as `risk_clock`, handed to nothing (DEC-280 item 2).
    fn fold_mark(&mut self, event: &Json) -> Result<(), String> {
        unknown_members(event, &["type", "clock"])
            .map_err(|unknown| format!("`event` members not interpreted: {unknown}"))?;
        let event_type = str_at(event, "type")?;
        ensure(event_type == "MarkUpdated", || {
            format!("a `fold` of `{event_type}` is not one the shell knows")
        })?;
        let instrument = self.instrument.clone().unwrap_or_default();
        let price = self
            .marks
            .get(&instrument)
            .cloned()
            .ok_or_else(|| "a `fold` of a mark before any mark was folded".to_owned())?;
        self.mark(&instrument, &price, second(at(event, "clock")?, "clock")?)
    }

    /// `cancel`: the input that cancels for `reason`, folded and handed in. The owner's commands
    /// carry fresh `cli_confirm` evidence, so a Stop is refused only where `cli_confirm` is
    /// (DEC-155 item 4).
    fn cancel(&mut self, reason: &str) -> Result<Vec<Effect>, String> {
        let clock = self.clock()?;
        let event = match reason {
            "version_applied" => self.account("MandateVersionApplied", Vec::new(), clock)?,
            "mode_tightened" => {
                self.account("AgentModeApplied", vec![("to", text("exits_only"))], clock)?
            }
            "owner_pause" => self.command("pause", clock)?,
            "owner_stop" => self.command("stop", clock)?,
            "kill_switch" => self.command("kill_switch", clock)?,
            other => return Err(format!("`{other}` is not a cancellation reason")),
        };
        self.step(Input::Journal(event), None)
    }

    /// The owner's agent-scoped command, with fresh `cli_confirm` evidence, folded on the control
    /// stream.
    fn command(&mut self, command: &str, clock: i64) -> Result<FoldedEvent, String> {
        let evidence = object(vec![
            (
                "assertion_id",
                text(&format!("cancel-{}", self.control_seq)),
            ),
            ("authenticated_at", seconds(clock)?),
            ("method", text("cli_confirm")),
        ])?;
        let payload = object(vec![
            ("agent", text("a")),
            ("command", text(command)),
            ("scope", text("agent")),
            ("subject", text("a")),
            ("submitted_at", seconds(clock)?),
            ("step_up", evidence),
            ("user", text(&self.author)),
        ])?;
        self.control("OwnerCommandIssued", payload, ActorKind::User)
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
                let event = FoldedEvent {
                    stream: AGENT_STREAM.to_owned(),
                    seq: Seq(self.agent_seq),
                    event_id: draft.event_id.clone(),
                    event_type: draft.event_type.clone(),
                    causation_id: draft.causation_id.clone(),
                    actor: ActorKind::Agent,
                    payload: draft.payload.clone(),
                };
                fold(&mut self.state, &event)
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

    fn mark(&mut self, instrument: &str, price: &str, risk_clock: i64) -> Result<(), String> {
        let members = vec![("instrument", text(instrument)), ("price", text(price))];
        self.account("MarkUpdated", members, risk_clock)?;
        self.marks.insert(instrument.to_owned(), price.to_owned());
        Ok(())
    }

    /// `ask`: the bound order proposed once at a tick of the folded clock, classified `ask` by its
    /// `decided_by`, its reference mark folded at its `seq` first.
    fn ask(&mut self, bound: &Json) -> Result<Vec<Effect>, String> {
        let members = REQUESTED
            .split_last()
            .map_or(REQUESTED, |(_deadline, bound)| bound);
        let known: Vec<&str> = members.iter().map(|(case, _, _)| *case).collect();
        unknown_members(bound, &known)
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
                let (price, seq) = (str_at(mark, "price")?, u64_at(mark, "seq")?);
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
            purpose: match str_at(bound, "purpose")? {
                "open" => Purpose::Open,
                "increase" => Purpose::Increase,
                other => return Err(format!("`{other}` is not a purpose an ask binds")),
            },
            exit_origin: None,
            exit_conviction: None,
            buy_conviction: None,
            combined_score: text(str_at(bound, "combined_score")?),
            outputs_used: BTreeSet::new(),
            model_weights: BTreeMap::new(),
            clips_applied: Vec::new(),
            execution: None,
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

    /// Each expected draft against the runtime's, in order.
    fn compare(&mut self, ran: &Ran, expected: &[Json]) -> Result<(), String> {
        let types: Vec<&str> = ran.drafts.iter().map(|d| d.event_type.as_str()).collect();
        ensure(ran.drafts.len() == expected.len(), || {
            format!(
                "{} drafts expected, the runtime wrote {types:?}",
                expected.len()
            )
        })?;
        let mut faults = Vec::new();
        for (index, (want, draft)) in expected.iter().zip(&ran.drafts).enumerate() {
            let number = index.saturating_add(1);
            for fault in self.compare_draft(want, draft, ran) {
                faults.push(format!("draft {number} ({}): {fault}", draft.event_type));
            }
        }
        faults.extend(handoffs(&ran.effects));
        ensure(faults.is_empty(), || faults.join("; "))
    }

    /// One expected draft: its type, its clock, each member the map reads plainly, the members only
    /// the runtime writes, and the names bound; any other member on either side fails.
    fn compare_draft(&mut self, want: &Json, draft: &EventDraft, ran: &Ran) -> Vec<String> {
        let event_type = match str_at(want, "type") {
            Ok(event_type) => event_type,
            Err(e) => return vec![e],
        };
        let (plain, special, extras): (_, &[&str], Vec<(&str, Option<Value>)>) = match event_type {
            "ApprovalRequested" => (
                REQUESTED,
                &["approval", "content_hash"],
                vec![("on_timeout", Some(text("skip"))), ("content", None)],
            ),
            "ApprovalDelivered" => (
                DELIVERED,
                &["approval"],
                vec![("message_id", Some(Value::Null))],
            ),
            "ApprovalResponded" => (
                RESPONDED,
                &["approval", "source"],
                vec![("role", Some(text("approver")))],
            ),
            "ApprovalRevalidated" => match self.revalidated_values(draft, ran) {
                Ok(values) => (REVALIDATED, &["approval"], values),
                Err(e) => return vec![e],
            },
            "IntentProposed" => (INTENDED, &["approval", "mandate_version"], Vec::new()),
            "ApprovalTimedOut" => (TIMED_OUT, &["approval"], Vec::new()),
            "ApprovalCanceled" => (CANCELED, &["approval"], Vec::new()),
            other => return vec![format!("`{other}` is not a draft type the map knows")],
        };
        let mut faults = Vec::new();
        let known: Vec<&str> = ["type", "clock"]
            .into_iter()
            .chain(plain.iter().map(|(case, _, _)| *case))
            .chain(special.iter().copied())
            .collect();
        if let Err(unknown) = unknown_members(want, &known) {
            faults.push(format!("members not interpreted: {unknown}"));
        }
        if draft.event_type != event_type {
            return vec![format!("expected a {event_type}")];
        }
        match at(want, "clock").and_then(|clock| second(clock, "clock")) {
            Ok(clock) if clock == ran.clock => {}
            Ok(clock) => faults.push(format!(
                "clock: expected {clock}, the step is at {}",
                ran.clock
            )),
            Err(e) => faults.push(e),
        }
        let mut written: BTreeSet<&str> = special.iter().copied().collect();
        for (case, runtime, how) in plain {
            if let Some(value) = want.get(*case) {
                written.insert(runtime);
                if let Err(e) = member(*how, value, draft.payload.get(runtime)) {
                    faults.push(format!("`{case}`: {e}"));
                }
            }
        }
        for (name, value) in extras {
            written.insert(name);
            faults.extend(extra_fault(name, value.as_ref(), draft.payload.get(name)));
        }
        faults.extend(match event_type {
            "ApprovalRequested" => self.requested(want, draft, ran),
            "ApprovalResponded" => self.responded(want, draft),
            "IntentProposed" => self.intended(want, draft, ran),
            _ => self.names_approval(want, draft),
        });
        let cause = draft.causation_id.as_ref().map(|cause| cause.0.as_str());
        let approval = draft.payload.get("approval").and_then(Value::as_str);
        if event_type == "ApprovalDelivered" && cause != approval {
            faults.push(format!("its cause is {cause:?}, not its request"));
        }
        for (key, got) in draft.payload.as_object().into_iter().flatten() {
            if !written.contains(key.as_str()) {
                faults.extend(unstated_fault(event_type, key.as_str(), got));
            }
        }
        faults
    }

    /// An `ApprovalRequested`: the ask's approval name bound to its `event_id`, and its hash name to
    /// the runtime's, which must be the digest of the runtime's own `content`; and the bound order
    /// read against what the runtime bound.
    fn requested(&mut self, want: &Json, draft: &EventDraft, ran: &Ran) -> Vec<String> {
        let Some((approval, bound)) = &ran.asked else {
            return vec!["a request outside an ask".to_owned()];
        };
        let mut faults = Vec::new();
        if draft.causation_id != ran.decision {
            faults.push(format!(
                "its cause is {:?}, not the ask's `DecisionMade`",
                draft.causation_id
            ));
        }
        if want.get("approval").and_then(Json::as_str) != Some(approval.as_str()) {
            faults.push(format!("`approval`: expected the ask's `{approval}`"));
        }
        faults.extend(
            bind(
                &mut self.approval_ids,
                Some(approval),
                Some(&draft.event_id.0),
            )
            .err(),
        );
        let hash = draft.payload.get("content_hash").and_then(Value::as_str);
        let digest = draft
            .payload
            .get("content")
            .map(|content| format!("sha256:{}", Digest::of(&to_canonical(content))));
        let name = want.get("content_hash").and_then(Json::as_str);
        faults.extend(
            bind(
                &mut self.hashes,
                name,
                hash.filter(|h| Some(*h) == digest.as_deref()),
            )
            .err(),
        );
        for (case, runtime, how) in REQUESTED {
            if let Some(value) = bound.get(*case)
                && let Err(e) = member(*how, value, draft.payload.get(runtime))
            {
                faults.push(format!("bound `{case}`: {e}"));
            }
        }
        let version = draft.payload.get("mandate_version").and_then(Value::as_str);
        let asked = Asked {
            id: draft.event_id.0.clone(),
            version: version.unwrap_or_default().to_owned(),
            bound: bound.clone(),
        };
        self.asked.insert(approval.clone(), asked);
        faults
    }

    /// An `IntentProposed`: caused by this step's `ApprovalRevalidated` of the case's approval, whose
    /// request bound the mandate version the case states. Journal spec §9.1's closed
    /// `IntentProposed` carries neither, so both are read through its cause.
    fn intended(&self, want: &Json, draft: &EventDraft, ran: &Ran) -> Vec<String> {
        let cause = ran
            .drafts
            .iter()
            .find(|d| Some(&d.event_id) == draft.causation_id.as_ref());
        let Some(revalidation) = cause.filter(|d| d.event_type == "ApprovalRevalidated") else {
            return vec![format!(
                "its cause is no re-validation of this step: {cause:?}"
            )];
        };
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

    /// What an `ApprovalRevalidated` records it compared (journal spec §9): the bound order's version,
    /// trigger and reference mark, `now`'s version, mode, restriction, trigger, dry run and mark, and
    /// the asset class's drift band (§6.4: 100 bp for `us_equity`, 200 bp for `crypto`). A trigger the
    /// classifier does not name is recorded as `""`.
    fn revalidated_values(
        &self,
        draft: &EventDraft,
        ran: &Ran,
    ) -> Result<Vec<(&'static str, Option<Value>)>, String> {
        let approval = draft.payload.get("approval").and_then(Value::as_str);
        let bound = &self
            .asked
            .values()
            .find(|asked| Some(asked.id.as_str()) == approval)
            .ok_or_else(|| format!("re-validation of {approval:?}, which no ask requested"))?
            .bound;
        let now = ran
            .now
            .as_ref()
            .ok_or_else(|| "a re-validation outside a response".to_owned())?;
        let canon = |value: &Json, path: &str| at(value, path).and_then(to_canon).map(Some);
        let label = decided_by_now(
            at(now, "classification")?,
            draft.payload.get("decided_by_now"),
        )?;
        let m_req = match at(bound, "reference_mark")? {
            Json::Null => Value::Null,
            mark => to_canon(at(mark, "price")?)?,
        };
        let band = match str_at(bound, "asset_class")? {
            "crypto" => 200,
            _ => 100,
        };
        Ok(vec![
            ("mandate_version_bound", canon(bound, "mandate_version")?),
            ("mandate_version_now", canon(now, "mandate_version")?),
            ("mode", canon(now, "mode")?),
            (
                "instrument_restricted",
                canon(now, "instrument_restricted")?,
            ),
            ("decided_by_bound", canon(bound, "decided_by")?),
            ("decided_by_now", Some(label)),
            ("dry_run", canon(now, "dry_run.verdict")?),
            ("dry_run_reason", canon(now, "dry_run.reason")?),
            ("m_req", Some(m_req)),
            ("m_now", canon(now, "mark")?),
            ("band_bp", Some(seconds(band)?)),
        ])
    }

    /// The draft's `approval` is the runtime's id for the case's name, or the name as written when
    /// none is bound.
    fn names_approval(&self, want: &Json, draft: &EventDraft) -> Vec<String> {
        let wanted = want.get("approval").and_then(Json::as_str);
        let id = wanted.map(|name| translate(&self.approval_ids, name));
        match (id, draft.payload.get("approval").and_then(Value::as_str)) {
            (Some(Ok(id)), Some(got)) if id == got => Vec::new(),
            (id, got) => vec![format!("`approval`: expected {id:?}, got {got:?}")],
        }
    }

    /// An `ApprovalResponded`: its approval, the control-stream event it copies, and, where check 7
    /// judged the grant, the case's own `quorum`.
    fn responded(&self, want: &Json, draft: &EventDraft) -> Vec<String> {
        let mut faults = self.names_approval(want, draft);
        faults.extend(unstated_quorum(want));
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

    /// `now`: the view, the classification and the dry run as stated, the mark folded as the
    /// account stream's latest `MarkUpdated`, and the mode folded where it differs from the
    /// runtime's, which must then read as stated. True whenever it folded a new mode, cancelling or
    /// not: the next step then records the fold as its first draft, `AgentModeChanged`
    /// (`restriction_changed`), which belongs to the fold rather than to any cancellation, so a
    /// loosening is set aside the same way (the #550 review, minor 2).
    fn apply_now(&mut self, now: &Json) -> Result<bool, String> {
        unknown_members(now, NOW_KEYS)
            .map_err(|unknown| format!("`now` members not interpreted: {unknown}"))?;
        let instrument = self
            .instrument
            .clone()
            .ok_or_else(|| "a `now` before any ask".to_owned())?;
        let id = InstrumentId::new(&instrument).map_err(|e| format!("`instrument`: {e:?}"))?;
        let held = |key: &str| -> Result<BTreeSet<InstrumentId>, String> {
            let flag = at(now, key)?
                .as_bool()
                .ok_or_else(|| format!("`{key}` is a boolean"))?;
            Ok(if flag {
                [id.clone()].into()
            } else {
                BTreeSet::new()
            })
        };
        self.view.restricted_instruments = held("instrument_restricted")?;
        self.view.working_universe = held("in_working_universe")?;
        self.view.version = str_at(now, "mandate_version")?.to_owned();
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
        self.verdict = match (
            str_at(dry_run, "verdict")?,
            optional_text(dry_run, "reason")?,
        ) {
            ("allow", None) => DryRunVerdict::Allow,
            ("deny", Some(reason_code)) => DryRunVerdict::Deny { reason_code },
            (verdict, reason) => {
                return Err(format!(
                    "a dry run `{verdict}` with reason {reason:?} is not one the gate gives"
                ));
            }
        };
        let clock = self.clock()?;
        match optional_text(now, "mark")? {
            None => ensure(!self.marks.contains_key(&instrument), || {
                "`now` has no mark, after a mark was folded".to_owned()
            })?,
            Some(price) => self.mark(&instrument, &price, clock)?,
        }
        let mode = str_at(now, "mode")?;
        let wanted = match mode {
            "normal" => Mode::Normal,
            "exits_only" => Mode::ExitsOnly,
            "paused" => Mode::Paused,
            "stopped" => Mode::Stopped,
            other => return Err(format!("`{other}` is not a mode")),
        };
        let moved = self.state.effective_mode() != wanted;
        if moved {
            self.account("AgentModeApplied", vec![("to", text(mode))], clock)?;
        }
        ensure(self.state.effective_mode() == wanted, || {
            format!(
                "`now` states mode `{mode}`, and the runtime's effective mode is {:?}",
                self.state.effective_mode()
            )
        })?;
        Ok(moved)
    }

    /// A response: a new `source` is folded on the control stream and handed in; a re-tailed one
    /// hands the event already folded again, which must be the same response. The submission's
    /// `role` is what the CLI writes; the runtime's copy writes its own `approver` and never reads
    /// this one.
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
        let approval = translate(&self.approval_ids, str_at(response, "approval")?)?;
        let hash = translate(&self.hashes, str_at(response, "content_hash")?)?;
        let submitted = seconds(second(at(response, "submitted_at")?, "submitted_at")?)?;
        let payload = object(vec![
            ("agent", text("a")),
            ("approval", text(&approval)),
            ("verdict", text(verdict)),
            ("content_hash", text(&hash)),
            ("submitted_at", submitted),
            ("step_up", step_up(at(response, "step_up")?)?),
            ("responder", text(str_at(response, "responder")?)),
            ("role", text("approver")),
        ])?;
        let event = self.control("ApprovalResponseSubmitted", payload, actor)?;
        self.sources
            .insert(source.to_owned(), (event.clone(), response.clone()));
        self.step(Input::Journal(event), None)
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

/// A member only the runtime writes: it must equal the value supplied, and only `UNCOMPARED`
/// members may be supplied with none.
fn extra_fault(name: &str, want: Option<&Value>, got: Option<&Value>) -> Option<String> {
    match want {
        Some(want) if got != Some(want) => Some(format!("`{name}`: got {got:?}")),
        Some(_) => None,
        None if UNCOMPARED.contains(&name) => None,
        None => Some(format!("`{name}` is supplied with no value to compare")),
    }
}

/// A member the runtime wrote that the case does not state. Only an `ApprovalResponded`'s
/// `NULL_UNLESS_STATED` member written `null` passes; a value there is one the case never stated.
fn unstated_fault(event_type: &str, name: &str, got: &Value) -> Option<String> {
    let null_unless_stated =
        event_type == "ApprovalResponded" && NULL_UNLESS_STATED.contains(&name);
    match got {
        Value::Null if null_unless_stated => None,
        _ => Some(format!(
            "the runtime wrote `{name}`, which the case does not state, as {got:?}"
        )),
    }
}

/// The `decided_by_now` a re-validation records for the step's re-classification: journal spec
/// §9.7's (DEC-533 item 4), the `ask`'s own label, or `null` when it is not an `ask` or its label is
/// empty. Until E8-3's writer lands, the runtime writes the superseded value instead, the label the
/// re-classification gave whatever its decision, `""` with none, and that one value also passes
/// (DEC-830 item 2); no third value does.
fn decided_by_now(classification: &Json, got: Option<&Value>) -> Result<Value, String> {
    let by = optional_text(classification, "by")?.unwrap_or_default();
    let superseded = text(&by);
    if got == Some(&superseded) {
        return Ok(superseded);
    }
    Ok(match str_at(classification, "decision")? {
        "ask" if !by.is_empty() => superseded,
        _ => Value::Null,
    })
}

/// A case's `ApprovalResponded` whose grant check 7 judged (journal spec §9.7 rule 48: `approved`,
/// and `admitted` or `counted`, or refused as `duplicate_approver` or `not_independent`) and that
/// does not state the `quorum` it applied: a defect in the case, never read as `null`.
fn unstated_quorum(want: &Json) -> Option<String> {
    let text_at = |key: &str| want.get(key).and_then(Json::as_str);
    let judged = text_at("verdict") == Some("approved")
        && (matches!(text_at("result"), Some("admitted" | "counted"))
            || matches!(
                text_at("reason"),
                Some("duplicate_approver" | "not_independent")
            ));
    (judged && want.get("quorum").is_none()).then(|| {
        "the case does not state the `quorum` check 7 judged (journal spec §9.7 rule 48)".to_owned()
    })
}

/// Sets aside the step's first drafts as `records` names them, by type and, where given, reason,
/// and returns them.
fn strip(
    drafts: &mut Vec<EventDraft>,
    records: &[(&str, &str)],
) -> Result<Vec<EventDraft>, String> {
    let mut removed = Vec::new();
    for (event_type, reason) in records {
        let Some(first) = drafts.first() else {
            return Ok(removed);
        };
        let fits = first.event_type == *event_type
            && (reason.is_empty() || first.payload.get("reason") == Some(&text(reason)));
        ensure(fits, || {
            format!(
                "the step's records begin {} {:?}, not the {event_type} the map sets aside",
                first.event_type, first.payload
            )
        })?;
        removed.push(drafts.remove(0));
    }
    Ok(removed)
}

/// Every handoff follows the draft that records it in the same list: an order its `IntentProposed`,
/// a flatten its `KillSwitchActivated` or `OwnerExitRequested`; and every `IntentProposed` is handed
/// exactly once (`AGENTS.md` rule 5).
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
    let wanted = match how {
        As::Seconds => seconds(second(want, "instant")?)?,
        As::Canonical => to_canon(want)?,
        As::StepUp => step_up(want)?,
        As::Quorum => {
            ensure(got.is_some(), || {
                "the runtime records no quorum: `ApprovalResponded` owes the approver count and \
                 independence check 7 applied (journal spec §9, mandate spec §6.4), which E8-3's \
                 runtime does not write yet"
                    .to_owned()
            })?;
            to_canon(want)?
        }
    };
    ensure(got == Some(&wanted), || {
        format!("expected {want}, got {got:?}")
    })
}

/// A step-up as the control stream writes it: `{assertion_id, authenticated_at, method}`.
fn step_up(evidence: &Json) -> Result<Value, String> {
    if evidence.is_null() {
        return Ok(Value::Null);
    }
    unknown_members(evidence, &["assertion", "authenticated_at", "method"])
        .map_err(|unknown| format!("`step_up` members not interpreted: {unknown}"))?;
    let authenticated = second(at(evidence, "authenticated_at")?, "authenticated_at")?;
    object(vec![
        ("assertion_id", text(str_at(evidence, "assertion")?)),
        ("authenticated_at", seconds(authenticated)?),
        ("method", text(str_at(evidence, "method")?)),
    ])
}

fn optional_text(value: &Json, key: &str) -> Result<Option<String>, String> {
    match at(value, key)? {
        Json::Null => Ok(None),
        Json::String(s) => Ok(Some(s.clone())),
        _ => Err(format!("`{key}` is a string or null")),
    }
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

/// Binds a case name to a runtime one, one to one; each must be present.
fn bind(
    bound: &mut BTreeMap<String, String>,
    name: Option<&str>,
    runtime: Option<&str>,
) -> Result<(), String> {
    let (Some(name), Some(runtime)) = (name, runtime) else {
        return Err(format!(
            "{name:?} binds to no runtime name or digest {runtime:?}"
        ));
    };
    let clash = bound
        .iter()
        .any(|(other, held)| (other == name) != (held == runtime));
    ensure(!clash, || {
        format!("`{name}` and `{runtime}` are each bound to another name")
    })?;
    bound.insert(name.to_owned(), runtime.to_owned());
    Ok(())
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

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

fn seconds(secs: i64) -> Result<Value, String> {
    let secs = u64::try_from(secs).map_err(|_| format!("{secs} is before the epoch"))?;
    Int::new(secs)
        .map(Value::Int)
        .ok_or_else(|| format!("{secs} is not a canonical integer"))
}

fn object(members: Vec<(&str, Value)>) -> Result<Value, String> {
    let members = members.into_iter().map(|(key, value)| {
        Key::new(key)
            .map(|key| (key, value))
            .map_err(|_| format!("`{key}` is not a canonical key"))
    });
    Ok(Value::Object(members.collect::<Result<_, _>>()?))
}

#[cfg(test)]
mod tests;
