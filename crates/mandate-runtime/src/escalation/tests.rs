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
use mandate_time::UtcNanos;

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

/// The §4.7 timestamp of a whole second (journal spec §4.7), the form §9.2 types the refusal's
/// `effective_at` (DEC-261 item 7, DEC-308), computed here from the judged second itself rather
/// than from the writer, so the pins hold an oracle of their own. It is derived twice: by civil
/// calendar arithmetic over the epoch day (days from 1970-01-01 to a proleptic Gregorian date),
/// and as the canonical instant `UtcNanos` prints, and the two must agree, so a defect in the
/// printer a correct stamp calls cannot pass the pins unseen.
fn timestamp_of(secs: i64) -> Result<String, String> {
    let (days, of_day) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let shifted = days.saturating_add(719_468);
    let (era, day_of_era) = (shifted.div_euclid(146_097), shifted.rem_euclid(146_097));
    let year_of_era = day_of_era
        .saturating_sub(day_of_era.div_euclid(1_460))
        .saturating_add(day_of_era.div_euclid(36_524))
        .saturating_sub(day_of_era.div_euclid(146_096))
        .div_euclid(365);
    let day_of_year = day_of_era.saturating_sub(
        year_of_era
            .saturating_mul(365)
            .saturating_add(year_of_era.div_euclid(4))
            .saturating_sub(year_of_era.div_euclid(100)),
    );
    let month_from_march = day_of_year
        .saturating_mul(5)
        .saturating_add(2)
        .div_euclid(153);
    let day = day_of_year
        .saturating_sub(
            month_from_march
                .saturating_mul(153)
                .saturating_add(2)
                .div_euclid(5),
        )
        .saturating_add(1);
    let month = if month_from_march < 10 {
        month_from_march.saturating_add(3)
    } else {
        month_from_march.saturating_sub(9)
    };
    let year = year_of_era
        .saturating_add(era.saturating_mul(400))
        .saturating_add(i64::from(month <= 2));
    let civil = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.000000000Z",
        of_day.div_euclid(3_600),
        of_day.rem_euclid(3_600).div_euclid(60),
        of_day.rem_euclid(60)
    );
    let printed = UtcNanos::from_parts(secs, 0).map_err(failed)?.to_string();
    assert_eq!(
        civil, printed,
        "the civil derivation and `UtcNanos` agree on the §4.7 timestamp of {secs} seconds"
    );
    Ok(civil)
}

/// The refusal's `effective_at` (DEC-308): the writer stamps the §4.7 timestamp of `judged`
/// through `payload::stamp`, and the journaled record carries that value. It fails at the
/// assertion messages when the writer stamps anything but the judged second's canonical timestamp
/// (DEC-77).
fn stamps_the_judged_second(record: &EventDraft, judged: i64) -> Result<(), String> {
    let expected = timestamp_of(judged)?;
    let stamped = payload::stamp(RiskClock::from_secs(judged), "effective_at").map_err(failed)?;
    assert_eq!(
        stamped,
        Value::Str(expected.clone()),
        "effective_at is the §4.7 timestamp {expected} of the judged second {judged}, never \
         integer risk-clock seconds (journal spec §9.2, DEC-261 item 7, DEC-308)"
    );
    assert_eq!(
        record.payload.get("effective_at"),
        Some(&stamped),
        "the refusal carries the stamp of the judged second {judged}, never its \
         `submitted_at` (DEC-308)"
    );
    Ok(())
}

/// The committed vectors' `refused_stop` draft (`fixtures/refcases/journal.json`, generated from
/// `docs/specs/reference-cases/journal.yaml`): the §4.7 `effective_at` the reference validator
/// accepts, and the integer seconds its `refused_at_risk_clock_seconds` draft refuses, whose one
/// change is checked to be `payload.effective_at`. Both halves are read through the crate's own
/// strict JSON reader (`mandate_canon::parse`), never transcribed, so a vector change re-reads
/// rather than passing silently, and the crate gains no dependency.
fn refused_stop_vector() -> Result<(String, i64), String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/refcases/journal.json"
    );
    let bytes = std::fs::read(path).map_err(failed)?;
    let fixture = mandate_canon::parse(&bytes).map_err(failed)?;
    let control = fixture
        .get("control_stream")
        .ok_or_else(|| failed("the vectors carry a control stream"))?;
    let stamp = control
        .get("drafts")
        .and_then(|drafts| drafts.get("refused_stop"))
        .and_then(|draft| draft.get("payload"))
        .and_then(|payload| payload.get("effective_at"))
        .and_then(Value::as_str)
        .ok_or_else(|| failed("the vectors' refused_stop carries no effective_at"))?
        .to_owned();
    let change = control
        .get("invalid_drafts")
        .and_then(Value::as_array)
        .ok_or_else(|| failed("the vectors list their invalid drafts"))?
        .iter()
        .find(|draft| {
            draft.get("name").and_then(Value::as_str) == Some("refused_at_risk_clock_seconds")
        })
        .ok_or_else(|| failed("the vectors refuse integer seconds for effective_at"))?
        .get("changes")
        .and_then(Value::as_array)
        .and_then(<[Value]>::first)
        .ok_or_else(|| failed("the refused draft names its change"))?;
    assert_eq!(
        change.get("path").and_then(Value::as_str),
        Some("payload.effective_at"),
        "the refused draft's first change is the refusal's effective_at"
    );
    let seconds = change
        .get("value")
        .and_then(Value::as_int)
        .and_then(|secs| i64::try_from(secs).ok())
        .ok_or_else(|| failed("the refused seconds are an integer"))?;
    Ok((stamp, seconds))
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

/// Mandate spec §6.1, journal spec §9, DEC-291 (the #413 review, minor 2): a Stop is judged when
/// the runtime processes it. One submitted at 1 000 s with evidence authenticated then, read once
/// the folded clock reached 1 400 s, is stale, and its `OwnerCommandRefused` records the folded
/// second as `effective_at`, never its `submitted_at`.
///
/// The coordinator named this test's change for the §9.2 writer follow-up (DEC-261 item 7,
/// DEC-308): the assertion DEC-291's writer shipped, that `effective_at` is the integer of the
/// folded second, now holds the §4.7 timestamp of that same instant, through the stamp the writer
/// takes (#469).
#[test]
fn a_stop_processed_late_is_refused_at_the_folded_second() -> Checked {
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
    let processed = AT.saturating_add(400);
    rig.step(Input::Tick(RiskClock::from_secs(processed)), &ports)?;
    let (stop, ran) = rig.control(
        COMMAND_ISSUED,
        command("a", "stop", ("agent", "a"), evidence("late", AT)?, false)?,
        &ports,
    )?;
    assert_eq!(types(&ran), vec!["OwnerCommandRefused"]);
    let record = the(&ran, "OwnerCommandRefused")?;
    assert_eq!(member(record, "reason"), Some("step_up_stale"));
    assert_eq!(record.causation_id.as_ref(), Some(&stop.event_id));
    stamps_the_judged_second(record, processed)?;
    Ok(())
}

/// Journal spec §9.2 (DEC-261 item 7, DEC-308): a refused resume records `effective_at` as the
/// §4.7 timestamp of the second it was judged at. One submitted at 1 000 s and read once the
/// folded clock reached 1 400 s is refused at the folded second, and the journaled record carries
/// the stamp the writer writes, never its `submitted_at` and never integer seconds.
#[test]
fn a_refused_resume_stamps_effective_at_the_judged_second() -> Checked {
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
    let (_, ran) = rig.control(
        COMMAND_ISSUED,
        command("a", "pause", ("agent", "a"), Value::Null, false)?,
        &ports,
    )?;
    assert_eq!(types(&ran), vec!["AgentModeChanged"]);
    let judged = AT.saturating_add(400);
    rig.step(Input::Tick(RiskClock::from_secs(judged)), &ports)?;
    let (_, ran) = rig.control(
        COMMAND_ISSUED,
        command(
            "a",
            "resume",
            ("agent", "a"),
            evidence("late-resume", AT.saturating_sub(301))?,
            false,
        )?,
        &ports,
    )?;
    assert_eq!(types(&ran), vec!["OwnerCommandRefused"]);
    stamps_the_judged_second(the(&ran, "OwnerCommandRefused")?, judged)?;
    Ok(())
}

/// Journal spec §9.2 (DEC-261 item 7, DEC-308): a refused Stop records `effective_at` the same
/// way. A Stop submitted at 1 000 s with evidence authenticated then, read once the folded clock
/// reached 1 400 s, is stale, and its refusal carries the folded second's §4.7 timestamp.
#[test]
fn a_refused_stop_stamps_effective_at_the_judged_second() -> Checked {
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
    let judged = AT.saturating_add(400);
    rig.step(Input::Tick(RiskClock::from_secs(judged)), &ports)?;
    let (_, ran) = rig.control(
        COMMAND_ISSUED,
        command(
            "a",
            "stop",
            ("agent", "a"),
            evidence("late-stop", AT.saturating_sub(301))?,
            false,
        )?,
        &ports,
    )?;
    assert_eq!(types(&ran), vec!["OwnerCommandRefused"]);
    stamps_the_judged_second(the(&ran, "OwnerCommandRefused")?, judged)?;
    Ok(())
}

/// Journal spec §9.2 (DEC-261 item 7, DEC-308): every reason a resume or Stop can be refused
/// carries the same §4.7 stamp of the judged second — `step_up_missing` and `step_up_stale` and
/// `step_up_reused` on one paper view, and `step_up_method` on a backtest one, where `cli_confirm`
/// does not count (mandate spec §6.1, DEC-155 item 4).
#[test]
fn a_refusal_stamps_effective_at_for_each_of_its_four_reasons() -> Checked {
    let mut backtest_view = view()?;
    backtest_view.approval.environment = Environment::Backtest;
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
    let judged = AT.saturating_add(400);
    rig.step(Input::Tick(RiskClock::from_secs(judged)), &ports)?;
    for (step_up, reason) in [
        (Value::Null, "step_up_missing"),
        (
            evidence("stale-four", AT.saturating_sub(301))?,
            "step_up_stale",
        ),
    ] {
        let (_, ran) = rig.control(
            COMMAND_ISSUED,
            command("a", "stop", ("agent", "a"), step_up, false)?,
            &ports,
        )?;
        assert_eq!(types(&ran), vec!["OwnerCommandRefused"]);
        let record = the(&ran, "OwnerCommandRefused")?;
        assert_eq!(member(record, "reason"), Some(reason));
        stamps_the_judged_second(record, judged)?;
    }
    let (_, ran) = rig.control(
        COMMAND_ISSUED,
        command(
            "a",
            "pause",
            ("agent", "a"),
            evidence("shared-four", judged.saturating_sub(1))?,
            false,
        )?,
        &ports,
    )?;
    assert_eq!(types(&ran), vec!["AgentModeChanged"]);
    let (_, ran) = rig.control(
        COMMAND_ISSUED,
        command(
            "a",
            "stop",
            ("agent", "a"),
            evidence("shared-four", judged.saturating_sub(1))?,
            false,
        )?,
        &ports,
    )?;
    assert_eq!(types(&ran), vec!["OwnerCommandRefused"]);
    let record = the(&ran, "OwnerCommandRefused")?;
    assert_eq!(member(record, "reason"), Some("step_up_reused"));
    stamps_the_judged_second(record, judged)?;

    let backtest_ports = Ports {
        ids: &Ids,
        gate: &Allow,
        plan: &plan,
        flatten: &flatten,
        view: &backtest_view,
    };
    let (mut rig, _) = Rig::started(&backtest_ports)?;
    let (_, ran) = rig.control(
        COMMAND_ISSUED,
        command(
            "a",
            "stop",
            ("agent", "a"),
            evidence("method-four", AT)?,
            false,
        )?,
        &backtest_ports,
    )?;
    assert_eq!(types(&ran), vec!["OwnerCommandRefused"]);
    let record = the(&ran, "OwnerCommandRefused")?;
    assert_eq!(member(record, "reason"), Some("step_up_method"));
    stamps_the_judged_second(record, AT)?;
    Ok(())
}

/// Journal spec §9.2 and its test vectors (DEC-261 item 7, DEC-308): the refusal's `effective_at`
/// is the committed `refused_stop` draft's value — the §4.7 timestamp the reference validator
/// accepts — for the instant of the integer seconds its `refused_at_risk_clock_seconds` draft
/// refuses. Both halves are read from `fixtures/refcases/journal.json`, the two are held to name
/// one instant, the string is held to the test's own second derivation of that instant's §4.7
/// timestamp, and a Stop judged at that instant carries the vector's string member for member.
#[test]
fn the_refusals_effective_at_is_the_vectors_refused_stop_timestamp() -> Checked {
    let (vector_stamp, seconds) = refused_stop_vector()?;
    let parsed = UtcNanos::parse(&vector_stamp).map_err(failed)?;
    assert_eq!(
        parsed.secs(),
        seconds,
        "the vector's {vector_stamp} and its refused {seconds} seconds name one instant"
    );
    assert_eq!(
        vector_stamp,
        timestamp_of(seconds)?,
        "the vector's {vector_stamp} is the §4.7 timestamp of {seconds} seconds by the test's own \
         derivation, independent of the reference generator"
    );
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
    rig.step(Input::Tick(RiskClock::from_secs(seconds)), &ports)?;
    let (_, ran) = rig.control(
        COMMAND_ISSUED,
        command(
            "a",
            "stop",
            ("agent", "a"),
            evidence("vector", seconds.saturating_sub(301))?,
            false,
        )?,
        &ports,
    )?;
    assert_eq!(types(&ran), vec!["OwnerCommandRefused"]);
    let record = the(&ran, "OwnerCommandRefused")?;
    assert_eq!(member(record, "command"), Some("stop"));
    assert_eq!(member(record, "reason"), Some("step_up_stale"));
    let stamped = payload::stamp(RiskClock::from_secs(seconds), "effective_at").map_err(failed)?;
    assert_eq!(
        stamped,
        Value::Str(vector_stamp.clone()),
        "the stamp is the vector's §4.7 form {vector_stamp}, never the integer seconds the \
         vectors refuse (journal spec §9.2, DEC-261 item 7, DEC-308)"
    );
    assert_eq!(
        record.payload.get("effective_at"),
        Some(&stamped),
        "the refusal carries the vector's timestamp"
    );
    Ok(())
}

/// Journal spec §4.7 (DEC-308; the #469 review, minor 1): a §4.7 timestamp holds the seconds from
/// `1970-01-01T00:00:00Z` to `9999-12-31T23:59:59Z`. `payload::stamp` writes both ends, and refuses
/// a second beyond either one as `NonCanonicalPayload` naming the field, never clamping it to the
/// nearer end or writing it in another form.
#[test]
fn a_second_outside_the_timestamp_range_is_refused_naming_the_field() -> Checked {
    const LAST_SECOND: i64 = 253_402_300_799;
    for inside in [0, 1, LAST_SECOND] {
        assert_eq!(
            payload::stamp(RiskClock::from_secs(inside), "effective_at"),
            Ok(Value::Str(timestamp_of(inside)?)),
            "{inside} seconds lies inside the §4.7 range and is stamped as its own instant"
        );
    }
    for outside in [-1, i64::MIN, LAST_SECOND.saturating_add(1), i64::MAX] {
        assert_eq!(
            payload::stamp(RiskClock::from_secs(outside), "effective_at"),
            Err(RuntimeError::NonCanonicalPayload {
                field: "effective_at".to_owned(),
            }),
            "{outside} seconds lies outside the §4.7 range and is refused naming the field"
        );
    }
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
