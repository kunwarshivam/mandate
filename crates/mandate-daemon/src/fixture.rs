//! The fixture wiring the binary runs until the post-R0 hook wires a deployment's production
//! ports (DEC-968 item 6, DEC-1012 item 6): a gate that denies every proposal, a plan that proposes
//! nothing, a flatten planner with nothing to cancel or sell, and one fixed mandate version. A
//! process on it opens its stream, acts on the owner's kill switch, and follows the clock, and can
//! never propose an order.

use mandate_canon::{Digest, Key, Object, Value, stream_event_id};
use mandate_journal::Environment;
use mandate_runtime::{
    ApprovalSettings, Autonomy, Classified, DryRunVerdict, EventDraft, EventId, FlattenPlan,
    FlattenPlanner, FlattenRequest, GateDryRun, IdGen, MandateView, OrderPlan, Proposal, Seq,
    SignalInputs, WriterEpoch,
};

use crate::runtime::{Refs, Wiring};

/// The agent stream's deterministic event ids, derived from the stream as well as the epoch, head
/// and ordinal ([`stream_event_id`]), so two agents' streams in one journal never share an id
/// (journal spec §3) and a retry re-derives the same one (§5.1).
pub struct Ids {
    stream: String,
}

impl Ids {
    /// The ids of `stream`, the agent stream the process writes.
    pub fn on(stream: &str) -> Self {
        Self {
            stream: stream.to_owned(),
        }
    }
}

impl IdGen for Ids {
    fn event_id(&self, epoch: WriterEpoch, head: Seq, ordinal: u32) -> EventId {
        EventId(stream_event_id(&self.stream, epoch.0, head.0, ordinal))
    }
}

/// Denies every proposal (`AGENTS.md` rule 3).
struct DenyAll;

impl GateDryRun for DenyAll {
    fn check(&self, _: &Proposal) -> DryRunVerdict {
        DryRunVerdict::Deny {
            reason_code: "fixture_wiring".into(),
        }
    }
}

/// Proposes nothing, and would deny anything.
struct NoPlan;

impl OrderPlan for NoPlan {
    fn plan(&self, _: &MandateView, _: &SignalInputs) -> Option<Proposal> {
        None
    }

    fn classify(&self, _: &MandateView, _: &Proposal) -> Classified {
        Classified {
            autonomy: Autonomy::Deny,
            decided_by: None,
        }
    }
}

/// A fixture deployment holds nothing, so its flatten cancels and sells nothing.
struct NothingHeld;

impl FlattenPlanner for NothingHeld {
    fn plan(&self, request: &FlattenRequest) -> FlattenPlan {
        FlattenPlan {
            cancel_client_order_ids: Vec::new(),
            sells: Vec::new(),
            purpose: request.initiator.sell_purpose(),
            confirmation: request.confirmation.clone(),
        }
    }
}

/// Every draft at the draft's own [`EventDraft::schema_version`] with the fixture's one mandate version.
struct FixtureMandate;

impl Refs for FixtureMandate {
    fn refs(&self, draft: &EventDraft) -> Option<(u64, Object)> {
        let version = format!("sha256:{}", Digest::of(b"mandate-daemon fixture").to_hex());
        let mut refs = Object::new();
        refs.insert(Key::new("mandate_version").ok()?, Value::Str(version));
        Some((draft.schema_version(), refs))
    }
}

/// The fixture wiring for the agent stream `stream`, with `owner` as the deployment's one
/// approver and author.
pub fn wiring(owner: &str, stream: &str) -> Wiring {
    wiring_for(&[owner.to_owned()], owner, stream)
}

/// The fixture wiring for the agent stream `stream`, with the deployment's `approvers` and
/// `author` (DEC-1016). An empty approver set grants no approval, which the deny-all wiring never
/// asks for anyway.
pub fn wiring_for(approvers: &[String], author: &str, stream: &str) -> Wiring {
    Wiring {
        ids: Box::new(Ids::on(stream)),
        gate: Box::new(DenyAll),
        plan: Box::new(NoPlan),
        flatten: Box::new(NothingHeld),
        refs: Box::new(FixtureMandate),
        view: MandateView {
            version: "fixture".into(),
            working_universe: Default::default(),
            restricted_instruments: Default::default(),
            approval: ApprovalSettings {
                approvers: approvers.iter().cloned().collect(),
                author: author.to_owned(),
                timeout_s: 300,
                environment: Environment::Paper,
                live_cli_confirm: None,
            },
        },
        research: None,
        models: None,
        facts: None,
        approvers: None,
    }
}
