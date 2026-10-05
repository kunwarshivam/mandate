//! Fixtures and an in-memory shell, shared by the hand cases and the properties.
//!
//! The shell is the thin process shell of the task brief, reduced to what a test needs: it folds a
//! journal, runs an effect list in order, and can crash and restart. It holds no clock, no
//! randomness, and no I/O, so a test is as deterministic as the core it drives.

#![allow(dead_code, reason = "each test binary uses a different subset")]

pub mod escalation;

use std::collections::BTreeMap;

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_approval::Notification;
use mandate_canon::{Int, Key, Value};
use mandate_journal::Environment;
use mandate_num::{Price, Qty};
use mandate_runtime::{
    ActorKind, AgentId, ApprovalSettings, Autonomy, Classified, ConnectionId, Deployment,
    DryRunVerdict, Effect, EventDraft, EventId, FlattenLeg, FlattenPlan, FlattenPlanner,
    FlattenRequest, FoldedEvent, GateDryRun, IdGen, Initiator, IntentHandoff, MandateView,
    ModelOutput, OrderExecution, OrderPlan, Ports, Proposal, Purpose, RiskClock, RuntimeError,
    RuntimeState, Seq, SignalInputs, TimeInForce, TimerId, TimerRequest, WorkspaceId, WriterEpoch,
    fold, handle,
};

pub const AGENT_STREAM: &str = "agent:ws1:agent-a";
pub const ACCOUNT_STREAM: &str = "acct:ws1:acct-1";
pub const CONTROL_STREAM: &str = "ctl:ws1";
pub const CLOCK_STREAM: &str = "clock:ws1";
pub const OTHER_AGENT_STREAM: &str = "agent:ws1:agent-b";
pub const VERSION: &str = "v1";
pub const AGENT: &str = "agent-a";
pub const CONNECTION: &str = "conn-1";
pub const WORKSPACE: &str = "ws1";
/// The fixture mandate's one approver, and its author, who is not an approver.
pub const OWNER: &str = "user-owner";
pub const AUTHOR: &str = "user-author";
/// The `DecidedBy` label `FixedPlan` reports for its classification.
pub const ASK_RULE: &str = "rule:big-order";

/// The deployment every fixture is for: the ids in `AGENT_STREAM`, `ACCOUNT_STREAM`, and the rest.
pub fn deployment() -> Deployment {
    Deployment {
        agent: AgentId(AGENT.to_owned()),
        connection: ConnectionId(CONNECTION.to_owned()),
        workspace: WorkspaceId(WORKSPACE.to_owned()),
    }
}

pub fn instrument(name: &str) -> InstrumentId {
    InstrumentId::new(name).unwrap_or_else(|e| panic!("instrument {name}: {e}"))
}

pub fn qty(text: &str) -> Qty {
    Qty::parse(text).unwrap_or_else(|e| panic!("qty {text}: {e}"))
}

pub fn price(text: &str) -> Price {
    Price::parse(text).unwrap_or_else(|e| panic!("price {text}: {e}"))
}

pub fn key(name: &str) -> Key {
    Key::new(name).unwrap_or_else(|e| panic!("key {name}: {e}"))
}

pub fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

pub fn int(n: u64) -> Value {
    Value::Int(Int::new(n).unwrap_or_else(|| panic!("int {n} out of range")))
}

pub fn object(pairs: &[(&str, Value)]) -> Value {
    Value::Object(
        pairs
            .iter()
            .map(|(k, v)| (key(k), v.clone()))
            .collect::<BTreeMap<_, _>>(),
    )
}

pub fn clock(secs: i64) -> RiskClock {
    RiskClock::from_secs(secs)
}

/// One event as the journal would hand it back: already validated and sequenced.
pub fn event(stream: &str, seq: u64, event_type: &str, payload: Value) -> FoldedEvent {
    FoldedEvent {
        stream: stream.to_owned(),
        seq: Seq(seq),
        event_id: EventId(format!("{stream}-{seq}")),
        event_type: event_type.to_owned(),
        causation_id: None,
        actor: ActorKind::System,
        payload,
    }
}

/// The same event, carrying the `causation_id` a copied cross-stream fact needs (journal spec §2).
pub fn copied(
    stream: &str,
    seq: u64,
    event_type: &str,
    payload: Value,
    origin: &EventId,
) -> FoldedEvent {
    FoldedEvent {
        causation_id: Some(origin.clone()),
        ..event(stream, seq, event_type, payload)
    }
}

/// A risk input carries `risk_clock` (mandate spec §5.2), so most fixtures need it.
pub fn with_clock(pairs: &[(&str, Value)], at: i64) -> Value {
    let mut all = pairs.to_vec();
    let secs = u64::try_from(at).unwrap_or_else(|_| panic!("negative risk clock {at}"));
    all.push(("risk_clock", int(secs)));
    object(&all)
}

/// The oracle's own derivation of an event id, written separately from the crate's so that the two
/// agreeing means something (DEC-131 item 6). It is ULID-shaped, as every journal event id is
/// (journal spec §3), because an approval's reference is built only from one
/// (`mandate_approval::ApprovalRef`, DEC-165 item 13): `0`, then the epoch in 5 places, the head in
/// 10, and the ordinal in 10, each in base 20 over Crockford's letters, so no id holds a digit a
/// sentinel scan could mistake for a quantity or a price.
pub fn derived_id(epoch: WriterEpoch, head: Seq, ordinal: u32) -> EventId {
    EventId(format!(
        "0{}{}{}",
        letters(epoch.0, 5),
        letters(head.0, 10),
        letters(u64::from(ordinal), 10)
    ))
}

fn letters(mut n: u64, width: usize) -> String {
    const ALPHABET: &[u8; 20] = b"ABCDEFGHJKMNPQRSTVWX";
    let mut out = vec![b'A'; width];
    for slot in out.iter_mut().rev() {
        *slot = ALPHABET[usize::try_from(n % 20).unwrap_or(0)];
        n /= 20;
    }
    assert_eq!(n, 0, "an id part too large for its width");
    String::from_utf8(out).unwrap_or_default()
}

pub struct TestIds;

impl IdGen for TestIds {
    fn event_id(&self, epoch: WriterEpoch, head: Seq, ordinal: u32) -> EventId {
        derived_id(epoch, head, ordinal)
    }
}

pub struct AllowGate;

impl GateDryRun for AllowGate {
    fn check(&self, _proposal: &Proposal) -> DryRunVerdict {
        DryRunVerdict::Allow
    }
}

pub struct DenyGate(pub &'static str);

impl GateDryRun for DenyGate {
    fn check(&self, _proposal: &Proposal) -> DryRunVerdict {
        DryRunVerdict::Deny {
            reason_code: self.0.to_owned(),
        }
    }
}

/// A plan that always proposes the same thing, so a test's subject is the runtime and never the
/// builder. `requires_fresh` mirrors the real builder's freshness rule: with no unexpired output for
/// the instrument, it proposes nothing (mandate spec §8.1).
pub struct FixedPlan {
    pub proposal: Option<Proposal>,
    pub autonomy: Autonomy,
    pub requires_fresh: bool,
    /// The `DecidedBy` label `classify` reports, which an approval request binds (check 10).
    pub decided_by: &'static str,
}

impl FixedPlan {
    pub fn opening(autonomy: Autonomy) -> Self {
        Self {
            proposal: Some(Proposal {
                instrument: instrument("AAPL"),
                asset_class: AssetClass::UsEquity,
                side: Side::Buy,
                qty: qty("10"),
                limit: price("155"),
                purpose: Purpose::Open,
                combined_score: text("0.5"),
                execution: Some(OrderExecution {
                    asset_class: AssetClass::UsEquity,
                    tif: TimeInForce::Day,
                    protection_required: false,
                    protection: None,
                }),
            }),
            autonomy,
            requires_fresh: true,
            decided_by: ASK_RULE,
        }
    }

    pub fn exiting(autonomy: Autonomy) -> Self {
        Self {
            proposal: Some(Proposal {
                instrument: instrument("AAPL"),
                asset_class: AssetClass::UsEquity,
                side: Side::Sell,
                qty: qty("10"),
                limit: price("149"),
                purpose: Purpose::DiscretionaryExit,
                combined_score: text("-0.5"),
                execution: Some(OrderExecution {
                    asset_class: AssetClass::UsEquity,
                    tif: TimeInForce::Day,
                    protection_required: false,
                    protection: None,
                }),
            }),
            autonomy,
            requires_fresh: true,
            decided_by: ASK_RULE,
        }
    }

    pub fn silent() -> Self {
        Self {
            proposal: None,
            autonomy: Autonomy::Deny,
            requires_fresh: false,
            decided_by: ASK_RULE,
        }
    }
}

impl OrderPlan for FixedPlan {
    fn plan(&self, view: &MandateView, inputs: &SignalInputs) -> Option<Proposal> {
        let proposal = self.proposal.clone()?;
        if !view.working_universe.contains(&proposal.instrument) && proposal.purpose.adds_risk() {
            return None;
        }
        if self.requires_fresh {
            let fresh = inputs.outputs.values().any(|by_instrument| {
                by_instrument
                    .get(&proposal.instrument)
                    .is_some_and(|output| output.expires_at >= inputs.now)
            });
            if !fresh {
                return None;
            }
        }
        Some(proposal)
    }

    fn classify(&self, _view: &MandateView, _proposal: &Proposal) -> Classified {
        Classified {
            autonomy: self.autonomy,
            decided_by: Some(self.decided_by.to_owned()),
        }
    }
}

/// A planner that returns the plan stream G would compute for this fixture: the agent's working
/// orders by id, one equity leg, and the session deferral the initiator implies.
pub struct FixedFlatten {
    pub sells: Vec<(&'static str, AssetClass, &'static str)>,
}

impl FixedFlatten {
    pub fn one_equity() -> Self {
        Self {
            sells: vec![("AAPL", AssetClass::UsEquity, "10")],
        }
    }

    pub fn nothing_held() -> Self {
        Self { sells: Vec::new() }
    }
}

impl FlattenPlanner for FixedFlatten {
    fn plan(&self, request: &FlattenRequest) -> FlattenPlan {
        let deferred = request.confirmation.is_none() && request.initiator != Initiator::Owner;
        FlattenPlan {
            cancel_client_order_ids: request.working_orders.clone(),
            sells: self
                .sells
                .iter()
                .map(|(name, asset_class, quantity)| FlattenLeg {
                    instrument: instrument(name),
                    asset_class: *asset_class,
                    qty: qty(quantity),
                    deferred_to_regular_session: deferred,
                })
                .collect(),
            purpose: request.initiator.sell_purpose(),
            confirmation: request.confirmation.clone(),
        }
    }
}

pub fn universe(instruments: &[&str]) -> MandateView {
    MandateView {
        version: VERSION.to_owned(),
        working_universe: instruments.iter().map(|n| instrument(n)).collect(),
        restricted_instruments: Default::default(),
        approval: ApprovalSettings {
            approvers: [OWNER.to_owned()].into(),
            author: AUTHOR.to_owned(),
            timeout_s: 300,
            environment: Environment::Paper,
        },
    }
}

pub fn view_with_restriction(instruments: &[&str], restricted: &[&str]) -> MandateView {
    MandateView {
        restricted_instruments: restricted.iter().map(|n| instrument(n)).collect(),
        ..universe(instruments)
    }
}

/// A fresh, unexpired model output for `AAPL`, which is what makes `FixedPlan` propose.
pub fn fresh_output(at: i64) -> ModelOutput {
    ModelOutput {
        model: "ma_cross".to_owned(),
        version: "1".to_owned(),
        instrument: instrument("AAPL"),
        as_of: clock(at),
        expires_at: clock(at.saturating_add(300)),
        content: text("long"),
    }
}

pub fn stale_output(at: i64) -> ModelOutput {
    ModelOutput {
        expires_at: clock(at.saturating_sub(1)),
        ..fresh_output(at.saturating_sub(600))
    }
}

/// What the shell did with one effect list, so a test can assert on order as well as content.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Ran {
    pub drafts: Vec<EventDraft>,
    pub handed: Vec<IntentHandoff>,
    pub timers: Vec<TimerRequest>,
    pub notifications: Vec<&'static str>,
    pub approval_notifications: Vec<Notification>,
    pub effects: Vec<Effect>,
}

impl Ran {
    pub fn draft_types(&self) -> Vec<&str> {
        self.drafts.iter().map(|d| d.event_type.as_str()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }
}

/// How an append answered, so a test can put a batch in doubt (journal spec §5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendOutcome {
    Committed,
    Unresolved,
    Fenced,
}

/// The in-memory shell: one fenced writer over the agent stream, plus the journal it reads back.
pub struct Shell {
    pub state: RuntimeState,
    pub epoch: WriterEpoch,
    pub agent_journal: Vec<FoldedEvent>,
    pub followed: Vec<FoldedEvent>,
    pub armed: BTreeMap<TimerId, RiskClock>,
    pub next_append: AppendOutcome,
    pub stopped: bool,
}

impl Shell {
    pub fn new(epoch: u64) -> Self {
        Self {
            state: RuntimeState::new(deployment()),
            epoch: WriterEpoch(epoch),
            agent_journal: Vec::new(),
            followed: Vec::new(),
            armed: BTreeMap::new(),
            next_append: AppendOutcome::Committed,
            stopped: false,
        }
    }

    pub fn head(&self) -> Seq {
        let len = u64::try_from(self.agent_journal.len()).unwrap_or(u64::MAX);
        Seq(len)
    }

    /// Folds one event of a followed stream, exactly as the tailer would.
    pub fn fold_one(&mut self, event: &FoldedEvent) -> Result<(), RuntimeError> {
        self.followed.push(event.clone());
        fold(&mut self.state, event)
    }

    /// One step, running the effect list in order: appends first, then handoffs, then timers.
    pub fn step(
        &mut self,
        input: mandate_runtime::Input,
        ports: &Ports<'_>,
    ) -> Result<Ran, RuntimeError> {
        let effects = handle(&mut self.state, input, ports)?;
        let mut ran = Ran {
            effects: effects.clone(),
            ..Ran::default()
        };
        for effect in &effects {
            match effect {
                Effect::Journal(draft) => match self.next_append {
                    AppendOutcome::Committed => {
                        let seq = self.head().0.saturating_add(1);
                        let stored = FoldedEvent {
                            stream: AGENT_STREAM.to_owned(),
                            seq: Seq(seq),
                            event_id: draft.event_id.clone(),
                            event_type: draft.event_type.clone(),
                            causation_id: draft.causation_id.clone(),
                            actor: ActorKind::Agent,
                            payload: draft.payload.clone(),
                        };
                        self.agent_journal.push(stored.clone());
                        fold(&mut self.state, &stored).unwrap_or_else(|e| {
                            panic!(
                                "the writer cannot fold back its own {}: {e}",
                                stored.event_type
                            )
                        });
                        ran.drafts.push(draft.clone());
                    }
                    AppendOutcome::Unresolved => {
                        ran.drafts.push(draft.clone());
                        return Ok(ran);
                    }
                    AppendOutcome::Fenced => {
                        self.stopped = true;
                        return Ok(ran);
                    }
                },
                Effect::Intent(handoff) => ran.handed.push(handoff.clone()),
                Effect::NotifyApproval(notification) => {
                    ran.approval_notifications.push(notification.clone());
                }
                Effect::Timer(request) => {
                    match request {
                        TimerRequest::Arm { id, at } => {
                            self.armed.insert(id.clone(), *at);
                        }
                        TimerRequest::Cancel { id } => {
                            self.armed.remove(id);
                        }
                    }
                    ran.timers.push(request.clone());
                }
                Effect::Notify(reference) => ran.notifications.push(reference.message_key),
            }
        }
        Ok(ran)
    }

    /// The same, panicking on a refusal, for the many tests whose subject is the effect list.
    pub fn run(&mut self, input: mandate_runtime::Input, ports: &Ports<'_>) -> Ran {
        self.step(input, ports)
            .unwrap_or_else(|e| panic!("step refused with {}: {e}", e.code()))
    }

    /// A crash and restart: a new process, a new epoch, the same journal, folded from seq 1.
    pub fn restart(&self, ports: &Ports<'_>) -> (Self, Ran) {
        let mut next = Self::new(self.epoch.0.saturating_add(1));
        next.followed = self.followed.clone();
        next.agent_journal = self.agent_journal.clone();
        for event in self.agent_journal.iter().chain(self.followed.iter()) {
            fold(&mut next.state, event)
                .unwrap_or_else(|e| panic!("replay refused with {}: {e}", e.code()));
        }
        let epoch = next.epoch;
        let ran = next.run(mandate_runtime::Input::Started(epoch), ports);
        (next, ran)
    }
}

/// The ports a test drives, with the pieces it wants to vary.
pub fn ports<'a>(
    ids: &'a TestIds,
    gate: &'a dyn GateDryRun,
    plan: &'a dyn OrderPlan,
    view: &'a MandateView,
) -> Ports<'a> {
    Ports {
        ids,
        gate,
        plan,
        flatten: &DEFAULT_FLATTEN,
        view,
    }
}

static DEFAULT_FLATTEN: FixedFlatten = FixedFlatten { sells: Vec::new() };

/// The same, with a planner a kill-switch case wants to vary.
pub fn ports_with_flatten<'a>(
    ids: &'a TestIds,
    gate: &'a dyn GateDryRun,
    plan: &'a dyn OrderPlan,
    flatten: &'a dyn FlattenPlanner,
    view: &'a MandateView,
) -> Ports<'a> {
    Ports {
        ids,
        gate,
        plan,
        flatten,
        view,
    }
}
