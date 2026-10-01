//! The branches the pending tests in `tests/` do not reach (DEC-278): the owner's Stop and a resume
//! after it, a kill switch addressed to a connection or a workspace, answers and commands for a
//! sibling agent, the evidence a request cites, a request for an `increase` of a crypto pair, a
//! `counted` grant, an owner exit's working orders, its retry and its restart, and an `ask` for an
//! exit. Each test drives `handle` and `fold` the way the shell does and pairs every "nothing
//! happened" with the case where the same step acts.

use std::cell::RefCell;
use std::collections::BTreeSet;

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_canon::{Int, Key, Value};
use mandate_journal::Environment;
use mandate_num::{Price, Qty};

use super::*;
use crate::ports::{FlattenPlanner, GateDryRun, IdGen, OrderPlan, Ports};
use crate::state::{RuntimeState, fold};
use crate::step::handle;
use crate::types::{
    ApprovalSettings, Classified, ConnectionId, Deployment, Effect, EventDraft, FlattenPlan,
    FoldedEvent, Input, MandateView, ModelOutput, Seq, SignalInputs, WriterEpoch,
};

type Checked = Result<(), String>;

const AGENT_STREAM: &str = "agent:w:a";
const ACCOUNT_STREAM: &str = "acct:w:x";
const CONTROL_STREAM: &str = "ctl:w";
const OWNER: &str = "owner";
const AT: i64 = 1_000;

fn failed(what: impl std::fmt::Display) -> String {
    what.to_string()
}

fn instrument(name: &str) -> Result<InstrumentId, String> {
    InstrumentId::new(name).map_err(failed)
}

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

fn int(n: u64) -> Result<Value, String> {
    Int::new(n).map(Value::Int).ok_or_else(|| failed(n))
}

fn object(pairs: Vec<(&str, Value)>) -> Result<Value, String> {
    pairs
        .into_iter()
        .map(|(k, v)| Key::new(k).map(|k| (k, v)).map_err(|_| failed(k)))
        .collect::<Result<_, _>>()
        .map(Value::Object)
}

/// ULID-shaped ids from the epoch, the head, and the ordinal, in decimal digits.
struct Ids;

impl IdGen for Ids {
    fn event_id(&self, epoch: WriterEpoch, head: Seq, ordinal: u32) -> EventId {
        EventId(format!("0{:05}{:010}{:010}", epoch.0, head.0, ordinal))
    }
}

struct Allow;

impl GateDryRun for Allow {
    fn check(&self, _proposal: &Proposal) -> DryRunVerdict {
        DryRunVerdict::Allow
    }
}

/// A plan that proposes the next of its proposals at every evaluation, whatever the outputs say.
struct Plan {
    proposals: RefCell<Vec<Proposal>>,
    autonomy: Autonomy,
}

impl Plan {
    fn of(proposals: Vec<Proposal>, autonomy: Autonomy) -> Self {
        Self {
            proposals: RefCell::new(proposals),
            autonomy,
        }
    }
}

impl OrderPlan for Plan {
    fn plan(&self, _view: &MandateView, _inputs: &SignalInputs) -> Option<Proposal> {
        let mut proposals = self.proposals.borrow_mut();
        if proposals.is_empty() {
            None
        } else {
            Some(proposals.remove(0))
        }
    }

    fn classify(&self, _view: &MandateView, _proposal: &Proposal) -> Classified {
        Classified {
            autonomy: self.autonomy,
            decided_by: Some("rule:r".to_owned()),
        }
    }
}

/// Records every request and plans nothing, so a test reads what the runtime asked for.
struct Recording {
    asked: RefCell<Vec<FlattenRequest>>,
}

impl FlattenPlanner for Recording {
    fn plan(&self, request: &FlattenRequest) -> FlattenPlan {
        self.asked.borrow_mut().push(request.clone());
        FlattenPlan {
            cancel_client_order_ids: request.working_orders.clone(),
            sells: Vec::new(),
            purpose: request.initiator.sell_purpose(),
            confirmation: request.confirmation.clone(),
        }
    }
}

fn proposal(name: &str, purpose: Purpose, class: AssetClass) -> Result<Proposal, String> {
    Ok(Proposal {
        instrument: instrument(name)?,
        asset_class: class,
        side: if purpose.adds_risk() {
            Side::Buy
        } else {
            Side::Sell
        },
        qty: Qty::parse("2").map_err(failed)?,
        limit: Price::parse("100").map_err(failed)?,
        purpose,
        combined_score: text("0.5"),
    })
}

fn view() -> Result<MandateView, String> {
    Ok(MandateView {
        version: "v1".to_owned(),
        working_universe: [
            instrument("AAPL")?,
            instrument("MSFT")?,
            instrument("BTCUSD")?,
        ]
        .into(),
        restricted_instruments: BTreeSet::new(),
        approval: ApprovalSettings {
            approvers: [OWNER.to_owned()].into(),
            author: "author".to_owned(),
            timeout_s: 300,
            environment: Environment::Paper,
        },
    })
}

fn deployment() -> Deployment {
    Deployment {
        agent: AgentId("a".to_owned()),
        connection: ConnectionId("c".to_owned()),
        workspace: WorkspaceId("w".to_owned()),
    }
}

/// The shell, reduced to a test's needs: it folds what it is given and every draft it is handed.
struct Rig {
    state: RuntimeState,
    epoch: u64,
    agent: Vec<FoldedEvent>,
    followed: Vec<FoldedEvent>,
}

impl Rig {
    fn started(ports: &Ports<'_>) -> Result<(Self, Vec<Effect>), String> {
        let mut rig = Self {
            state: RuntimeState::new(deployment()),
            epoch: 1,
            agent: Vec::new(),
            followed: Vec::new(),
        };
        rig.account("ReconciliationRun", vec![("result", text("clean"))])?;
        let effects = rig.step(Input::Started(WriterEpoch(1)), ports)?;
        Ok((rig, effects))
    }

    /// A crash and restart: a new epoch, the same journal, folded from seq 1.
    fn restarted(&self, ports: &Ports<'_>) -> Result<(Self, Vec<Effect>), String> {
        let epoch = self.epoch.saturating_add(1);
        let mut rig = Self {
            state: RuntimeState::new(deployment()),
            epoch,
            agent: self.agent.clone(),
            followed: self.followed.clone(),
        };
        for event in self.agent.iter().chain(self.followed.iter()) {
            fold(&mut rig.state, event).map_err(failed)?;
        }
        let effects = rig.step(Input::Started(WriterEpoch(epoch)), ports)?;
        Ok((rig, effects))
    }

    fn next(stream: &str, events: &[FoldedEvent]) -> u64 {
        let count = events.iter().filter(|e| e.stream == stream).count();
        u64::try_from(count).unwrap_or(u64::MAX).saturating_add(1)
    }

    fn external(
        &mut self,
        stream: &str,
        event_type: &str,
        payload: Value,
        id: String,
    ) -> Result<FoldedEvent, String> {
        let event = FoldedEvent {
            stream: stream.to_owned(),
            seq: Seq(Self::next(stream, &self.followed)),
            event_id: EventId(id),
            event_type: event_type.to_owned(),
            causation_id: None,
            actor: mandate_approval::ActorKind::User,
            payload,
        };
        fold(&mut self.state, &event).map_err(failed)?;
        self.followed.push(event.clone());
        Ok(event)
    }

    fn account(
        &mut self,
        event_type: &str,
        mut pairs: Vec<(&str, Value)>,
    ) -> Result<FoldedEvent, String> {
        pairs.push(("risk_clock", int(1_000)?));
        let seq = Self::next(ACCOUNT_STREAM, &self.followed);
        self.external(
            ACCOUNT_STREAM,
            event_type,
            object(pairs)?,
            format!("acct-{seq}"),
        )
    }

    /// A control-stream event from the owner, folded and then handed in, as the tailer does.
    fn control(
        &mut self,
        event_type: &str,
        payload: Value,
        ports: &Ports<'_>,
    ) -> Result<(FoldedEvent, Vec<Effect>), String> {
        let seq = Self::next(CONTROL_STREAM, &self.followed);
        let id = format!("1{seq:025}");
        let event = self.external(CONTROL_STREAM, event_type, payload, id)?;
        let effects = self.step(Input::Journal(event.clone()), ports)?;
        Ok((event, effects))
    }

    /// One step, every draft committed and folded back as the writer's own.
    fn step(&mut self, input: Input, ports: &Ports<'_>) -> Result<Vec<Effect>, String> {
        let effects = handle(&mut self.state, input, ports).map_err(failed)?;
        for effect in &effects {
            if let Effect::Journal(draft) = effect {
                let event = FoldedEvent {
                    stream: AGENT_STREAM.to_owned(),
                    seq: Seq(Self::next(AGENT_STREAM, &self.agent)),
                    event_id: draft.event_id.clone(),
                    event_type: draft.event_type.clone(),
                    causation_id: draft.causation_id.clone(),
                    actor: mandate_approval::ActorKind::Agent,
                    payload: draft.payload.clone(),
                };
                fold(&mut self.state, &event).map_err(failed)?;
                self.agent.push(event);
            }
        }
        Ok(effects)
    }

    /// A fresh model output and a tick at `at`, which is one evaluation.
    fn evaluate(&mut self, at: i64, model: &str, ports: &Ports<'_>) -> Result<Vec<Effect>, String> {
        self.step(
            Input::ModelOutput(ModelOutput {
                model: model.to_owned(),
                version: "1".to_owned(),
                instrument: instrument("AAPL")?,
                as_of: RiskClock::from_secs(at),
                expires_at: RiskClock::from_secs(at.saturating_add(300)),
                content: text("long"),
            }),
            ports,
        )?;
        self.step(Input::Tick(RiskClock::from_secs(at)), ports)
    }
}

fn drafts(effects: &[Effect]) -> Vec<&EventDraft> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Journal(draft) => Some(draft),
            _ => None,
        })
        .collect()
}

fn types(effects: &[Effect]) -> Vec<&str> {
    drafts(effects)
        .iter()
        .map(|d| d.event_type.as_str())
        .collect()
}

fn the<'e>(effects: &'e [Effect], event_type: &str) -> Result<&'e EventDraft, String> {
    let found: Vec<&EventDraft> = drafts(effects)
        .into_iter()
        .filter(|d| d.event_type == event_type)
        .collect();
    match found.as_slice() {
        [one] => Ok(one),
        _ => Err(format!("one {event_type} in {:?}", types(effects))),
    }
}

fn handed(effects: &[Effect]) -> Vec<&IntentHandoff> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Intent(handoff) => Some(handoff),
            _ => None,
        })
        .collect()
}

fn member<'d>(draft: &'d EventDraft, key: &str) -> Option<&'d str> {
    draft.payload.get(key).and_then(Value::as_str)
}

fn evidence(assertion: &str, at: i64) -> Result<Value, String> {
    object(vec![
        ("assertion_id", text(assertion)),
        ("authenticated_at", int(u64::try_from(at).map_err(failed)?)?),
        ("method", text("cli_confirm")),
    ])
}

/// One `OwnerCommandIssued` (DEC-257 item 5).
fn command(
    agent: &str,
    command: &str,
    scope: (&str, &str),
    step_up: Value,
    bid: bool,
) -> Result<Value, String> {
    let given = |v: &str| if bid { text(v) } else { Value::Null };
    object(vec![
        ("agent", text(agent)),
        ("command", text(command)),
        ("scope", text(scope.0)),
        ("subject", text(scope.1)),
        ("bid", given("99")),
        ("bid_size", given("100")),
        ("floor", given("95")),
        ("submitted_at", int(1_000)?),
        ("step_up", step_up),
        ("user", text(OWNER)),
    ])
}

fn mode_of(rig: &Rig) -> Mode {
    rig.state.effective_mode()
}

/// DEC-257 item 8: only an age outside the window is `stale`; evidence that is reused, of a method
/// the environment refuses, or missing is `absent`, and evidence that counts is `valid`.
#[test]
fn step_up_status_names_only_age_as_stale() {
    assert_eq!(step_up_status(None), "valid");
    assert_eq!(step_up_status(Some(StepUpRefusal::Stale)), "stale");
    for absent in [
        StepUpRefusal::Missing,
        StepUpRefusal::Reused,
        StepUpRefusal::Method,
    ] {
        assert_eq!(step_up_status(Some(absent)), "absent", "{absent:?}");
    }
}

/// An exit is never asked: only `open` and `increase` bind (`AGENTS.md` rule 2).
#[test]
fn only_risk_adding_purposes_are_askable() {
    for purpose in [
        Purpose::RiskExit,
        Purpose::OwnerExit,
        Purpose::DiscretionaryExit,
        Purpose::Protective,
        Purpose::Flatten,
    ] {
        assert_eq!(askable(purpose), None, "{purpose:?}");
    }
    assert_eq!(askable(Purpose::Open), Some(AskablePurpose::Open));
    assert_eq!(askable(Purpose::Increase), Some(AskablePurpose::Increase));
    assert_eq!(purpose_of(AskablePurpose::Increase), Purpose::Increase);
}

/// DEC-278 item 1: a classifier's `ask` for an exit cannot hold it. The exit is proposed and handed
/// as an `auto` one is, and nothing is requested; the same `ask` for an opening asks, and its batch
/// carries exactly one notice, the opaque `NotifyApproval`, never an `Effect::Notify` whose key could
/// carry content (the #395 review, minor 2).
#[test]
fn an_ask_for_an_exit_is_proposed_and_an_ask_for_an_opening_asks() -> Checked {
    let (view, flatten) = (
        view()?,
        Recording {
            asked: RefCell::new(Vec::new()),
        },
    );
    let exiting = Plan::of(
        vec![proposal(
            "AAPL",
            Purpose::DiscretionaryExit,
            AssetClass::UsEquity,
        )?],
        Autonomy::Ask,
    );
    let ports = Ports {
        ids: &Ids,
        gate: &Allow,
        plan: &exiting,
        flatten: &flatten,
        view: &view,
    };
    let (mut rig, _) = Rig::started(&ports)?;
    let ran = rig.evaluate(AT, "m1", &ports)?;
    assert_eq!(types(&ran), vec!["DecisionMade", "IntentProposed"]);
    assert_eq!(handed(&ran).len(), 1);

    let opening = Plan::of(
        vec![proposal("AAPL", Purpose::Open, AssetClass::UsEquity)?],
        Autonomy::Ask,
    );
    let ports = Ports {
        plan: &opening,
        ..ports
    };
    let (mut rig, _) = Rig::started(&ports)?;
    let ran = rig.evaluate(AT, "m1", &ports)?;
    assert_eq!(
        types(&ran),
        vec!["DecisionMade", "ApprovalRequested", "ApprovalDelivered"]
    );
    assert!(handed(&ran).is_empty());
    let notices: Vec<&Effect> = ran
        .iter()
        .filter(|e| matches!(e, Effect::Notify(_) | Effect::NotifyApproval(_)))
        .collect();
    assert!(
        matches!(notices.as_slice(), [Effect::NotifyApproval(_)]),
        "an ask notifies once, through the opaque approval notice and no other (rule 6, EI-9): {notices:?}"
    );
    Ok(())
}

/// A request for an `increase` of a crypto pair binds both and folds back to a pending approval
/// with the same bound action, so replay rebuilds what admission reads.
#[test]
fn an_increase_of_a_crypto_pair_round_trips_through_the_fold() -> Checked {
    let (view, flatten) = (
        view()?,
        Recording {
            asked: RefCell::new(Vec::new()),
        },
    );
    let plan = Plan::of(
        vec![proposal("BTCUSD", Purpose::Increase, AssetClass::Crypto)?],
        Autonomy::Ask,
    );
    let ports = Ports {
        ids: &Ids,
        gate: &Allow,
        plan: &plan,
        flatten: &flatten,
        view: &view,
    };
    let (mut rig, _) = Rig::started(&ports)?;
    let ran = rig.evaluate(AT, "m1", &ports)?;
    let request = the(&ran, "ApprovalRequested")?;
    assert_eq!(member(request, "asset_class"), Some("crypto"));
    assert_eq!(member(request, "purpose"), Some("increase"));
    let (restarted, _) = rig.restarted(&ports)?;
    let pending = restarted
        .state
        .pending_approvals()
        .get(&request.event_id)
        .ok_or("the request folds back")?;
    let bound = &pending.request.content.bound;
    assert_eq!(bound.purpose, AskablePurpose::Increase);
    assert_eq!(bound.asset_class, mandate_approval::AssetClass::Crypto);
    assert_eq!(bound.instrument, "BTCUSD");
    assert!(pending.request.delivered);
    Ok(())
}

/// A request cites the `ModelOutputRecorded` of every output still unexpired on its instrument,
/// and not one that has expired.
#[test]
fn a_request_cites_the_unexpired_outputs_behind_it() -> Checked {
    let (view, flatten) = (
        view()?,
        Recording {
            asked: RefCell::new(Vec::new()),
        },
    );
    let plan = Plan::of(
        vec![proposal("AAPL", Purpose::Open, AssetClass::UsEquity)?],
        Autonomy::Ask,
    );
    let ports = Ports {
        ids: &Ids,
        gate: &Allow,
        plan: &plan,
        flatten: &flatten,
        view: &view,
    };
    let (mut rig, _) = Rig::started(&ports)?;
    let early = rig.step(
        Input::ModelOutput(ModelOutput {
            model: "m0".to_owned(),
            version: "1".to_owned(),
            instrument: instrument("AAPL")?,
            as_of: RiskClock::from_secs(AT.saturating_sub(600)),
            expires_at: RiskClock::from_secs(AT.saturating_sub(1)),
            content: text("long"),
        }),
        &ports,
    )?;
    let expired = the(&early, "ModelOutputRecorded")?.event_id.clone();
    let ran = rig.evaluate(AT, "m1", &ports)?;
    let fresh = rig
        .agent
        .iter()
        .rfind(|e| e.event_type == "ModelOutputRecorded")
        .map(|e| e.event_id.clone())
        .ok_or("the fresh output is journaled")?;
    let request = the(&ran, "ApprovalRequested")?;
    let cited: Vec<&str> = request
        .payload
        .get("content")
        .and_then(|c| c.get("evidence"))
        .and_then(|e| e.get("outputs"))
        .and_then(|o| match o {
            Value::Array(items) => Some(items),
            _ => None,
        })
        .ok_or("the content lists its outputs")?
        .iter()
        .filter_map(|o| o.get("event_id").and_then(Value::as_str))
        .collect();
    assert_eq!(cited, vec![fresh.0.as_str()], "not {}", expired.0);
    Ok(())
}

/// A `counted` grant joins the grant set check 7 counts and leaves the approval pending; a
/// `refused` one changes nothing, and an `admitted` one ends it.
#[test]
fn a_counted_grant_joins_the_grant_set_and_the_approval_stays_pending() -> Checked {
    let (view, flatten) = (
        view()?,
        Recording {
            asked: RefCell::new(Vec::new()),
        },
    );
    let plan = Plan::of(
        vec![proposal("AAPL", Purpose::Open, AssetClass::UsEquity)?],
        Autonomy::Ask,
    );
    let ports = Ports {
        ids: &Ids,
        gate: &Allow,
        plan: &plan,
        flatten: &flatten,
        view: &view,
    };
    let (mut rig, _) = Rig::started(&ports)?;
    let ran = rig.evaluate(AT, "m1", &ports)?;
    let approval = the(&ran, "ApprovalRequested")?.event_id.clone();
    for (n, result, responder) in [
        (1u64, "refused", "someone"),
        (2, "counted", OWNER),
        (3, "admitted", "second"),
    ] {
        let event = FoldedEvent {
            stream: AGENT_STREAM.to_owned(),
            seq: Seq(Rig::next(AGENT_STREAM, &rig.agent)),
            event_id: EventId(format!("2{n:025}")),
            event_type: "ApprovalResponded".to_owned(),
            causation_id: None,
            actor: mandate_approval::ActorKind::Agent,
            payload: object(vec![
                ("approval", text(&approval.0)),
                ("verdict", text("approved")),
                ("responder", text(responder)),
                ("result", text(result)),
            ])?,
        };
        fold(&mut rig.state, &event).map_err(failed)?;
        rig.agent.push(event);
        let pending = rig.state.pending_approvals().get(&approval);
        match result {
            "refused" => assert!(pending.is_some_and(|p| p.request.grants.is_empty())),
            "counted" => assert_eq!(
                pending.map(|p| p.request.grants.clone()),
                Some([OpaqueUser(OWNER.to_owned())].into())
            ),
            _ => assert!(pending.is_none(), "an admitted answer ends it"),
        }
    }
    Ok(())
}

/// An answer or a command for a sibling agent is that sibling's to copy, so it is inert here; the
/// same command for this agent applies.
#[test]
fn owner_input_for_another_agent_is_inert() -> Checked {
    let (view, flatten) = (
        view()?,
        Recording {
            asked: RefCell::new(Vec::new()),
        },
    );
    let plan = Plan::of(
        vec![proposal("AAPL", Purpose::Open, AssetClass::UsEquity)?],
        Autonomy::Ask,
    );
    let ports = Ports {
        ids: &Ids,
        gate: &Allow,
        plan: &plan,
        flatten: &flatten,
        view: &view,
    };
    let (mut rig, _) = Rig::started(&ports)?;
    let ran = rig.evaluate(AT, "m1", &ports)?;
    let approval = the(&ran, "ApprovalRequested")?.event_id.clone();
    let skip = |agent: &str| {
        object(vec![
            ("agent", text(agent)),
            ("approval", text(&approval.0)),
            ("verdict", text("skipped")),
            ("content_hash", text("sha256:00")),
            ("submitted_at", int(1_000)?),
            ("step_up", Value::Null),
            ("responder", text(OWNER)),
            ("role", text("approver")),
        ])
    };
    let (_, ran) = rig.control(RESPONSE_SUBMITTED, skip("b")?, &ports)?;
    assert!(ran.is_empty(), "{:?}", types(&ran));
    let (_, ran) = rig.control(
        COMMAND_ISSUED,
        command("b", "pause", ("agent", "b"), Value::Null, false)?,
        &ports,
    )?;
    assert!(ran.is_empty(), "{:?}", types(&ran));
    assert_eq!(mode_of(&rig), Mode::Normal);

    let (_, ran) = rig.control(RESPONSE_SUBMITTED, skip("a")?, &ports)?;
    assert_eq!(types(&ran), vec!["ApprovalResponded"]);
    let (_, ran) = rig.control(
        COMMAND_ISSUED,
        command("a", "pause", ("agent", "a"), Value::Null, false)?,
        &ports,
    )?;
    assert_eq!(types(&ran), vec!["AgentModeChanged", "ApprovalCanceled"]);
    assert_eq!(mode_of(&rig), Mode::Paused);
    Ok(())
}

/// Mandate spec §6.1, PX-4: the owner's Stop needs step-up fresh when the runtime processes it.
/// Stale evidence changes no mode and leaves the approval pending, and its one copy is its
/// `OwnerCommandRefused` (journal spec §9, rule 16; DEC-291); fresh evidence stops the agent and
/// cancels the approval; and a resume after it, even with fresh evidence, never lifts a Stop.
#[test]
fn a_stop_needs_fresh_step_up_and_no_resume_lifts_it() -> Checked {
    let (view, flatten) = (
        view()?,
        Recording {
            asked: RefCell::new(Vec::new()),
        },
    );
    let plan = Plan::of(
        vec![proposal("AAPL", Purpose::Open, AssetClass::UsEquity)?],
        Autonomy::Ask,
    );
    let ports = Ports {
        ids: &Ids,
        gate: &Allow,
        plan: &plan,
        flatten: &flatten,
        view: &view,
    };
    let (mut rig, _) = Rig::started(&ports)?;
    let ran = rig.evaluate(AT, "m1", &ports)?;
    let approval = the(&ran, "ApprovalRequested")?.event_id.clone();

    let stale = evidence("stale", AT.saturating_sub(301))?;
    let (refused, ran) = rig.control(
        COMMAND_ISSUED,
        command("a", "stop", ("agent", "a"), stale, false)?,
        &ports,
    )?;
    assert_eq!(types(&ran), vec!["OwnerCommandRefused"]);
    let record = the(&ran, "OwnerCommandRefused")?;
    assert_eq!(member(record, "reason"), Some("step_up_stale"));
    assert_eq!(record.causation_id.as_ref(), Some(&refused.event_id));
    assert!(rig.state.pending_approvals().contains_key(&approval));

    let (stop, ran) = rig.control(
        COMMAND_ISSUED,
        command("a", "stop", ("agent", "a"), evidence("fresh", AT)?, false)?,
        &ports,
    )?;
    let changed = the(&ran, "AgentModeChanged")?;
    assert_eq!(member(changed, "reason"), Some("owner_stop"));
    assert_eq!(member(changed, "to"), Some("stopped"));
    assert_eq!(changed.causation_id.as_ref(), Some(&stop.event_id));
    assert_eq!(
        member(the(&ran, "ApprovalCanceled")?, "reason"),
        Some("owner_stop")
    );

    let (_, ran) = rig.control(
        COMMAND_ISSUED,
        command("a", "resume", ("agent", "a"), evidence("again", AT)?, false)?,
        &ports,
    )?;
    assert!(ran.is_empty(), "{:?}", types(&ran));
    assert_eq!(mode_of(&rig), Mode::Stopped);
    Ok(())
}

/// Trading-domain spec §5.5: a control-stream kill switch reaches this deployment through its
/// connection or its workspace, and not through another connection's.
#[test]
fn a_kill_switch_reaches_its_connection_and_workspace_only() -> Checked {
    let (view, flatten) = (
        view()?,
        Recording {
            asked: RefCell::new(Vec::new()),
        },
    );
    let plan = Plan::of(Vec::new(), Autonomy::Auto);
    let ports = Ports {
        ids: &Ids,
        gate: &Allow,
        plan: &plan,
        flatten: &flatten,
        view: &view,
    };
    for (scope, reaches) in [
        (("connection", "other"), false),
        (("connection", "c"), true),
        (("workspace", "w"), true),
    ] {
        let (mut rig, _) = Rig::started(&ports)?;
        let (_, ran) = rig.control(
            COMMAND_ISSUED,
            command("a", "kill_switch", scope, Value::Null, false)?,
            &ports,
        )?;
        assert_eq!(
            drafts(&ran)
                .iter()
                .any(|d| d.event_type == "KillSwitchActivated"),
            reaches,
            "{scope:?}: {:?}",
            types(&ran)
        );
        assert_eq!(handed(&ran).len(), usize::from(reaches), "{scope:?}");
    }
    Ok(())
}

/// Journal spec §9.1: an owner's control-stream kill switch records its scope, its subject, the
/// bid as given, and the step-up as judged at commit, with the evidence itself when it is valid.
#[test]
fn an_owner_kill_switch_records_its_scope_and_step_up() -> Checked {
    let (view, flatten) = (
        view()?,
        Recording {
            asked: RefCell::new(Vec::new()),
        },
    );
    let plan = Plan::of(Vec::new(), Autonomy::Auto);
    let ports = Ports {
        ids: &Ids,
        gate: &Allow,
        plan: &plan,
        flatten: &flatten,
        view: &view,
    };
    let (mut rig, _) = Rig::started(&ports)?;
    let (kill, ran) = rig.control(
        COMMAND_ISSUED,
        command("a", "kill_switch", ("agent", "a"), evidence("k", AT)?, true)?,
        &ports,
    )?;
    let shown = the(&ran, "OwnerExitRequested")?;
    assert_eq!(shown.causation_id.as_ref(), Some(&kill.event_id));
    assert_eq!(member(shown, "scope"), Some("agent"));
    assert_eq!(member(shown, "subject"), Some("a"));
    assert_eq!(member(shown, "bid"), Some("99"));
    assert_eq!(member(shown, "step_up_status"), Some("valid"));
    assert_eq!(
        shown
            .payload
            .get("step_up")
            .and_then(|e| e.get("assertion_id")),
        Some(&text("k"))
    );
    assert_eq!(shown.payload.get("confirmed"), Some(&Value::Bool(true)));
    Ok(())
}

/// An owner's exit of one instrument asks the planner to cancel only that instrument's working
/// orders, never another's (mandate spec §6.1, trading-domain spec §5.5).
#[test]
fn an_owner_exit_names_only_its_instruments_orders() -> Checked {
    let (view, flatten) = (
        view()?,
        Recording {
            asked: RefCell::new(Vec::new()),
        },
    );
    let plan = Plan::of(
        vec![
            proposal("AAPL", Purpose::Open, AssetClass::UsEquity)?,
            proposal("MSFT", Purpose::Open, AssetClass::UsEquity)?,
        ],
        Autonomy::Auto,
    );
    let ports = Ports {
        ids: &Ids,
        gate: &Allow,
        plan: &plan,
        flatten: &flatten,
        view: &view,
    };
    let (mut rig, _) = Rig::started(&ports)?;
    let first = rig.evaluate(AT, "m1", &ports)?;
    let aapl = the(&first, "IntentProposed")?.event_id.clone();
    let second = rig.evaluate(AT.saturating_add(1), "m1", &ports)?;
    assert_eq!(
        member(the(&second, "IntentProposed")?, "instrument"),
        Some("MSFT")
    );
    rig.control(
        COMMAND_ISSUED,
        command(
            "a",
            "owner_exit",
            ("instrument", "AAPL"),
            Value::Null,
            false,
        )?,
        &ports,
    )?;
    let asked = flatten.asked.borrow();
    let request = asked.last().ok_or("the planner was asked")?;
    assert_eq!(request.instrument, Some(instrument("AAPL")?));
    assert_eq!(request.working_orders, vec![aapl.0]);
    Ok(())
}

/// Journal spec §5.1, rule 13: an owner exit whose append went unanswered is retried with the same
/// draft and hands its flatten again; and a restart hands again an owner exit the executor has not
/// taken, but not one an `IntentReceived` shows it took.
#[test]
fn an_owner_exit_is_handed_again_on_a_retry_and_a_restart_until_taken() -> Checked {
    let (view, flatten) = (
        view()?,
        Recording {
            asked: RefCell::new(Vec::new()),
        },
    );
    let plan = Plan::of(Vec::new(), Autonomy::Auto);
    let ports = Ports {
        ids: &Ids,
        gate: &Allow,
        plan: &plan,
        flatten: &flatten,
        view: &view,
    };
    let (mut rig, _) = Rig::started(&ports)?;
    let exit = rig.external(
        CONTROL_STREAM,
        COMMAND_ISSUED,
        command(
            "a",
            "owner_exit",
            ("instrument", "AAPL"),
            Value::Null,
            false,
        )?,
        format!("1{:025}", 1),
    )?;
    let first = handle(&mut rig.state, Input::Journal(exit.clone()), &ports).map_err(failed)?;
    let retry = handle(&mut rig.state, Input::Journal(exit), &ports).map_err(failed)?;
    assert_eq!(
        retry, first,
        "the retry re-emits the drafts and their handoff"
    );
    let copy = the(&retry, "OwnerExitRequested")?.event_id.clone();
    assert!(
        matches!(handed(&retry).as_slice(), [one] if one.intent_id == copy),
        "{retry:?}"
    );
    rig.step(Input::Tick(RiskClock::from_secs(AT)), &ports)
        .err()
        .ok_or("the batch is still in doubt")?;
    for effect in &retry {
        if let Effect::Journal(draft) = effect {
            let event = FoldedEvent {
                stream: AGENT_STREAM.to_owned(),
                seq: Seq(Rig::next(AGENT_STREAM, &rig.agent)),
                event_id: draft.event_id.clone(),
                event_type: draft.event_type.clone(),
                causation_id: draft.causation_id.clone(),
                actor: mandate_approval::ActorKind::Agent,
                payload: draft.payload.clone(),
            };
            fold(&mut rig.state, &event).map_err(failed)?;
            rig.agent.push(event);
        }
    }

    let (mut restarted, recovered) = rig.restarted(&ports)?;
    assert!(
        handed(&recovered).iter().any(|h| h.intent_id == copy
            && matches!(&h.body, IntentBody::Flatten(plan) if plan.purpose == Purpose::OwnerExit)),
        "an untaken owner exit is handed again: {recovered:?}"
    );
    assert_eq!(
        flatten.asked.borrow().last().map(|r| r.instrument.clone()),
        Some(Some(instrument("AAPL")?))
    );
    restarted.account("IntentReceived", vec![("intent_id", text(&copy.0))])?;
    let (_, recovered) = restarted.restarted(&ports)?;
    assert!(
        handed(&recovered).iter().all(|h| h.intent_id != copy),
        "a taken one is not: {recovered:?}"
    );
    Ok(())
}
