//! The escalation fixtures (M7, backlog E8-1 to E8-3): the owner's answers and commands as the
//! control-stream events the CLI commits, a flatten planner that records what it was asked, and a
//! started shell holding one delivered approval request.
//!
//! The control-stream payloads are DEC-257 item 5's reading of journal spec §9 (the control
//! stream's schemas are not closed yet): risk-clock instants as whole seconds, the step-up as
//! `{assertion_id, authenticated_at, method}`, and opaque user ids only.

use std::cell::RefCell;

use mandate_canon::{Digest, Value};
use mandate_runtime::{
    ActorKind, EventDraft, EventId, FlattenPlan, FlattenPlanner, FlattenRequest, FoldedEvent,
    Input, Ports, Seq,
};

use super::{
    ACCOUNT_STREAM, AGENT, CONTROL_STREAM, OWNER, Ran, Shell, clock, event, fresh_output, int,
    object, text, with_clock,
};

/// When the fixture asks, and the deadline its 300 s `timeout_s` gives.
pub const ASKED_AT: i64 = 1_000;
pub const DEADLINE: i64 = 1_300;
/// The fixture's bound limit and the mark folded at the request.
pub const BOUND_LIMIT: &str = "155";

/// A clean startup reconciliation on the account stream: what lifts the startup hold.
pub fn reconciliation(seq: u64, at: i64) -> FoldedEvent {
    event(
        ACCOUNT_STREAM,
        seq,
        "ReconciliationRun",
        with_clock(&[("result", text("clean"))], at),
    )
}

/// A `MarkUpdated` of `AAPL` on the account stream (journal spec §9).
pub fn mark(seq: u64, price: &str, at: i64) -> FoldedEvent {
    event(
        ACCOUNT_STREAM,
        seq,
        "MarkUpdated",
        with_clock(
            &[
                ("instrument", text("AAPL")),
                ("price", text(price)),
                ("source", text("fixture")),
                ("feed", text("sip")),
            ],
            at,
        ),
    )
}

/// An `AgentModeApplied` on the account stream.
pub fn mode_applied(seq: u64, mode: &str, at: i64) -> FoldedEvent {
    event(
        ACCOUNT_STREAM,
        seq,
        "AgentModeApplied",
        with_clock(
            &[("to", text(mode)), ("restriction", text("daily_loss"))],
            at,
        ),
    )
}

/// A `MandateVersionApplied` on the account stream.
pub fn version_applied(seq: u64, version: &str, at: i64) -> FoldedEvent {
    event(
        ACCOUNT_STREAM,
        seq,
        "MandateVersionApplied",
        with_clock(
            &[
                ("classification", text("risk_reducing")),
                ("new_version", text(version)),
                ("result", text("applied")),
            ],
            at,
        ),
    )
}

/// A control-stream event id: ULID-shaped like every journal id, and starting `1`, so no fixture
/// id the runtime derives (which start `0`) can equal one.
pub fn control_id(seq: u64) -> EventId {
    EventId(format!("1{seq:025}"))
}

/// One control-stream event from a user, as the CLI commits it.
pub fn from_user(seq: u64, event_type: &str, payload: Value) -> FoldedEvent {
    FoldedEvent {
        event_id: control_id(seq),
        actor: ActorKind::User,
        ..event(CONTROL_STREAM, seq, event_type, payload)
    }
}

/// Step-up evidence: an assertion id and the risk-clock second it was authenticated at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    pub assertion: String,
    pub authenticated_at: i64,
}

pub fn evidence(assertion: &str, authenticated_at: i64) -> Option<Evidence> {
    Some(Evidence {
        assertion: assertion.to_owned(),
        authenticated_at,
    })
}

fn step_up_value(step_up: Option<&Evidence>) -> Value {
    match step_up {
        Some(e) => object(&[
            ("assertion_id", text(&e.assertion)),
            ("authenticated_at", seconds(e.authenticated_at)),
            ("method", text("cli_confirm")),
        ]),
        None => Value::Null,
    }
}

fn seconds(at: i64) -> Value {
    int(u64::try_from(at).unwrap_or_else(|_| panic!("negative second {at}")))
}

/// What `asking_shell` returns: the request's id, the content hash its payload states, and the
/// batch that asked.
#[derive(Debug, Clone)]
pub struct Asked {
    pub approval: EventId,
    pub content_hash: String,
    pub request: EventDraft,
    pub batch: Ran,
}

/// One `ApprovalResponseSubmitted`.
#[derive(Debug, Clone)]
pub struct Answer {
    pub seq: u64,
    pub approval: EventId,
    pub approved: bool,
    pub content_hash: String,
    pub submitted_at: i64,
    pub step_up: Option<Evidence>,
    pub responder: String,
    pub actor: ActorKind,
}

impl Answer {
    /// The owner's grant of `asked`, submitted at `at` with evidence authenticated at `at`.
    pub fn grant(seq: u64, asked: &Asked, at: i64) -> Self {
        Self {
            seq,
            approval: asked.approval.clone(),
            approved: true,
            content_hash: asked.content_hash.clone(),
            submitted_at: at,
            step_up: evidence(&format!("assertion-{seq}"), at),
            responder: OWNER.to_owned(),
            actor: ActorKind::User,
        }
    }

    /// The owner's skip of `asked`, which needs no step-up.
    pub fn skip(seq: u64, asked: &Asked, at: i64) -> Self {
        Self {
            approved: false,
            step_up: None,
            ..Self::grant(seq, asked, at)
        }
    }

    pub fn event(&self) -> FoldedEvent {
        let payload = object(&[
            ("agent", text(AGENT)),
            ("approval", text(&self.approval.0)),
            (
                "verdict",
                text(if self.approved { "approved" } else { "skipped" }),
            ),
            ("content_hash", text(&self.content_hash)),
            ("submitted_at", seconds(self.submitted_at)),
            ("step_up", step_up_value(self.step_up.as_ref())),
            ("responder", text(&self.responder)),
            ("role", text("approver")),
        ]);
        FoldedEvent {
            actor: self.actor,
            ..from_user(self.seq, "ApprovalResponseSubmitted", payload)
        }
    }
}

/// One `OwnerCommandIssued`. `bid` is `(bid, bid_size, floor)` for an owner exit that confirmed a
/// displayed bid.
#[derive(Debug, Clone)]
pub struct Command {
    pub seq: u64,
    pub command: &'static str,
    pub scope: &'static str,
    pub subject: String,
    pub bid: Option<(&'static str, &'static str, &'static str)>,
    pub submitted_at: i64,
    pub step_up: Option<Evidence>,
}

impl Command {
    /// A command addressed to the fixture's agent, with fresh evidence at `at`.
    pub fn to_agent(seq: u64, command: &'static str, at: i64) -> Self {
        Self {
            seq,
            command,
            scope: "agent",
            subject: AGENT.to_owned(),
            bid: None,
            submitted_at: at,
            step_up: evidence(&format!("assertion-{seq}"), at),
        }
    }

    /// The owner's exit of `AAPL` alone.
    pub fn exit_aapl(seq: u64, at: i64) -> Self {
        Self {
            scope: "instrument",
            subject: "AAPL".to_owned(),
            ..Self::to_agent(seq, "owner_exit", at)
        }
    }

    pub fn event(&self) -> FoldedEvent {
        let (bid, bid_size, floor) = match self.bid {
            Some((bid, size, floor)) => (text(bid), text(size), text(floor)),
            None => (Value::Null, Value::Null, Value::Null),
        };
        let payload = object(&[
            ("agent", text(AGENT)),
            ("command", text(self.command)),
            ("scope", text(self.scope)),
            ("subject", text(&self.subject)),
            ("bid", bid),
            ("bid_size", bid_size),
            ("floor", floor),
            ("submitted_at", seconds(self.submitted_at)),
            ("step_up", step_up_value(self.step_up.as_ref())),
            ("user", text(OWNER)),
        ]);
        from_user(self.seq, "OwnerCommandIssued", payload)
    }
}

/// Folds a control-stream event as the tailer would and hands it to the runtime as its own input.
/// A refusal of either is the test's failure, reported with the runtime's own error.
pub fn tail(shell: &mut Shell, event: &FoldedEvent, ports: &Ports<'_>) -> Ran {
    shell
        .fold_one(event)
        .unwrap_or_else(|e| panic!("folding {} refused: {e}", event.event_type));
    shell.run(Input::Journal(event.clone()), ports)
}

/// A started shell that asked once at `ASKED_AT`, with `AAPL` marked at `mark_price` before it, or
/// with no mark at all. The request is found by type; its content hash is read from the payload
/// as stated, or as all zeros when the payload states none, so that what fails first is the
/// runtime's answer to the response and never this fixture.
pub fn asking_shell(ports: &Ports<'_>, mark_price: Option<&str>) -> (Shell, Asked) {
    let mut shell = Shell::new(1);
    shell
        .fold_one(&reconciliation(1, ASKED_AT))
        .unwrap_or_else(|e| panic!("reconciliation: {e}"));
    if let Some(price) = mark_price {
        shell
            .fold_one(&mark(2, price, ASKED_AT))
            .unwrap_or_else(|e| panic!("mark: {e}"));
    }
    let (mut shell, _) = shell.restart(ports);
    shell.run(Input::ModelOutput(fresh_output(ASKED_AT)), ports);
    let batch = shell.run(Input::Tick(clock(ASKED_AT)), ports);
    let request = batch
        .drafts
        .iter()
        .find(|d| d.event_type == "ApprovalRequested")
        .cloned()
        .unwrap_or_else(|| panic!("the fixture asks: {:?}", batch.draft_types()));
    let content_hash = request
        .payload
        .get("content_hash")
        .and_then(Value::as_str)
        .map_or_else(|| format!("sha256:{}", "0".repeat(64)), str::to_owned);
    let asked = Asked {
        approval: request.event_id.clone(),
        content_hash,
        request,
        batch,
    };
    (shell, asked)
}

/// The next free seq of the account stream in a shell that `asking_shell` built.
pub fn next_account_seq(shell: &Shell) -> u64 {
    shell
        .state
        .head(ACCOUNT_STREAM)
        .map_or(1, |Seq(seq)| seq.saturating_add(1))
}

/// The next free seq of the control stream.
pub fn next_control_seq(shell: &Shell) -> u64 {
    shell
        .state
        .head(CONTROL_STREAM)
        .map_or(1, |Seq(seq)| seq.saturating_add(1))
}

/// `sha256:` and the hex digest of the canonical bytes of `content`: the oracle's own content hash
/// (journal spec §4, `ref`), computed from the payload rather than through `mandate-approval`.
pub fn hash_of(content: &Value) -> String {
    format!(
        "sha256:{}",
        Digest::of(&mandate_canon::to_canonical(content)).to_hex()
    )
}

/// A planner that records every request and returns the plan for the instrument a request names,
/// so a test can read what the runtime asked for as well as what it handed.
pub struct RecordingFlatten {
    pub asked: RefCell<Vec<FlattenRequest>>,
}

impl RecordingFlatten {
    pub fn new() -> Self {
        Self {
            asked: RefCell::new(Vec::new()),
        }
    }
}

impl FlattenPlanner for RecordingFlatten {
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
