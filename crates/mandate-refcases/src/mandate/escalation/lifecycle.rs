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
//! instants are whole seconds; `limit_price` is `limit`; every other member compares as the
//! runtime's canonical value, which is how the case writes it. The case's approval and content-hash
//! names are bound one to one to the runtime's request id and hash: the reference model hashes its
//! request as a stand-in for the content object (`reference/mandate/ref.py`, `escalation_step`), so
//! a case's hash is a name, and the runtime's `content_hash` must be the digest of its own inline
//! `content`. Every member of a draft's payload is one the case states or one checked here, and so
//! is its cause. The step's other effects, the deadline timer and the opaque notification, are
//! `mandate-runtime`'s own tests' to pin (DEC-317 item 5).
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
    FoldedEvent, GateDryRun, IdGen, Input, MandateView, OrderPlan, Ports, Proposal, Purpose,
    RiskClock, RuntimeState, Seq, SignalInputs, WorkspaceId, WriterEpoch, fold, handle,
};
use mandate_time::NewYorkTime;

use crate::mandate::{instant, not_implemented, unknown_members};
use crate::{Json, at, ensure, list_at, str_at, to_canon, u64_at};

const AGENT_STREAM: &str = "agent:w:a";
const ACCOUNT_STREAM: &str = "acct:w:c";

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

/// What one script step did: the drafts the case states, after the records it does not, the folded
/// clock after it, and the ask it made, by the case's approval name and bound order.
struct Ran {
    drafts: Vec<EventDraft>,
    clock: i64,
    asked: Option<(String, Json)>,
    /// The `DecisionMade` an ask set aside, which its request names as its cause.
    decision: Option<EventId>,
}

/// The shell around one runtime, and the names bound so far.
struct Shell {
    state: RuntimeState,
    agent_seq: u64,
    account_seq: u64,
    view: MandateView,
    classified: Classified,
    verdict: DryRunVerdict,
    /// The instruments a mark has been folded for.
    marks: BTreeSet<String>,
    approval_ids: BTreeMap<String, String>,
    hashes: BTreeMap<String, String>,
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
            marks: BTreeSet::new(),
            approval_ids: BTreeMap::new(),
            hashes: BTreeMap::new(),
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
        let slice = match kind {
            "ask" => 1,
            "response" => 2,
            "tick" | "fold" | "cancel" | "batch" => 3,
            other => return Err(format!("`{other}` is not a lifecycle step")),
        };
        ensure(slice == 1, || {
            not_implemented(&format!("the `{kind}` step, DEC-317's slice {slice},"))
        })?;
        unknown_members(step, &["kind", "approval", "bound"])
            .map_err(|unknown| format!("`ask` members not interpreted: {unknown}"))?;
        let bound = at(step, "bound")?;
        let mut drafts: Vec<EventDraft> = self
            .ask(bound)?
            .into_iter()
            .filter_map(|effect| match effect {
                Effect::Journal(draft) => Some(draft),
                _ => None,
            })
            .collect();
        let decision = drafts
            .first()
            .filter(|d| d.event_type == "DecisionMade")
            .map(|d| d.event_id.clone());
        if decision.is_some() {
            drafts.remove(0);
        }
        Ok(Ran {
            drafts,
            clock: self.clock()?,
            asked: Some((str_at(step, "approval")?.to_owned(), bound.clone())),
            decision,
        })
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
    ) -> Result<(), String> {
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
        fold(&mut self.state, &event).map_err(|e| format!("folding {event_type}: {e}"))
    }

    fn mark(&mut self, instrument: &str, price: &str, risk_clock: i64) -> Result<(), String> {
        let members = vec![("instrument", text(instrument)), ("price", text(price))];
        self.account("MarkUpdated", members, risk_clock)?;
        self.marks.insert(instrument.to_owned());
        Ok(())
    }

    /// `ask`: the bound order proposed once at a tick of the folded clock, classified `ask` by its
    /// `decided_by`, its reference mark folded at its `seq` first.
    fn ask(&mut self, bound: &Json) -> Result<Vec<Effect>, String> {
        let known: Vec<&str> = REQUESTED.iter().map(|(case, _, _)| *case).collect();
        unknown_members(bound, known.get(..13).unwrap_or_default())
            .map_err(|unknown| format!("`bound` members not interpreted: {unknown}"))?;
        let instrument = str_at(bound, "instrument")?;
        let clock = self.clock()?;
        match at(bound, "reference_mark")? {
            Json::Null => ensure(!self.marks.contains(instrument), || {
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
            if value.is_some_and(|value| draft.payload.get(name) != Some(&value)) {
                faults.push(format!("`{name}`: got {:?}", draft.payload.get(name)));
            }
        }
        faults.extend(match event_type {
            "ApprovalRequested" => self.requested(want, draft, ran),
            _ => self.delivered(want, draft),
        });
        for key in draft.payload.as_object().into_iter().flat_map(|o| o.keys()) {
            if !written.contains(key.as_str()) {
                faults.push(format!(
                    "the runtime wrote `{}`, which the case does not state",
                    key.as_str()
                ));
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
        if ran.decision.is_none() || draft.causation_id != ran.decision {
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
        faults
    }

    /// An `ApprovalDelivered`: its approval, which is also its cause.
    fn delivered(&self, want: &Json, draft: &EventDraft) -> Vec<String> {
        let mut faults = self.names_approval(want, draft);
        let cause = draft.causation_id.as_ref().map(|cause| cause.0.as_str());
        if cause.is_none() || cause != draft.payload.get("approval").and_then(Value::as_str) {
            faults.push(format!("its cause is {cause:?}, not its request"));
        }
        faults
    }

    /// The draft's `approval` is the runtime's id for the case's name.
    fn names_approval(&self, want: &Json, draft: &EventDraft) -> Vec<String> {
        let wanted = want.get("approval").and_then(Json::as_str);
        let id = wanted.and_then(|name| self.approval_ids.get(name));
        match draft.payload.get("approval").and_then(Value::as_str) {
            Some(got) if id.map(String::as_str) == Some(got) => Vec::new(),
            got => vec![format!(
                "`approval`: expected the id of {wanted:?}, got {got:?}"
            )],
        }
    }
}

/// One case member against the runtime's, under `how`.
fn member(how: As, want: &Json, got: Option<&Value>) -> Result<(), String> {
    let wanted = match how {
        As::Seconds => seconds(second(want, "instant")?)?,
        As::Canonical => to_canon(want)?,
    };
    ensure(got == Some(&wanted), || {
        format!("expected {want}, got {got:?}")
    })
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
