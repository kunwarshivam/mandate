//! One effect list under construction.
//!
//! Every draft is folded into a working copy of the state the moment it is written, so a later
//! decision in the same list sees what the earlier drafts did — exactly what a replay of the
//! journal will see — and the list is one sequence rather than a set of guesses (task brief
//! interpretation 20).

use mandate_canon::{Digest, Key, Object, Value};

use crate::error::ExecutorError;
use crate::fold::fold;
use crate::payload::{object, risk_clock_stamp};
use crate::ports::{BindingGateSource, Ports};
use crate::state::ExecutorState;
use crate::types::{
    ActivityCursor, BrokerRequest, Effect, EventDraft, EventId, FoldedEvent, NotificationRef,
    RiskClock, Seq, WriterEpoch,
};

pub(crate) struct Batch<'p, 'a> {
    pub(crate) ports: &'p Ports<'a>,
    pub(crate) view: ExecutorState,
    pub(crate) effects: Vec<Effect>,
    binding_gate: Option<&'p dyn BindingGateSource>,
    epoch: WriterEpoch,
    head: Seq,
    drafted: u32,
}

impl<'p, 'a> Batch<'p, 'a> {
    /// A batch at the state's current head. Nothing is drafted before `Input::Started` has taken
    /// an epoch, because an event id is a function of the epoch (DEC-131 item 6).
    pub(crate) fn new(state: &ExecutorState, ports: &'p Ports<'a>) -> Result<Self, ExecutorError> {
        Ok(Self {
            ports,
            epoch: state.epoch.ok_or(ExecutorError::NotStarted)?,
            head: state.account_head(),
            view: state.clone(),
            effects: Vec::new(),
            binding_gate: default_binding_gate(),
            drafted: 0,
        })
    }

    pub(crate) fn bind(&mut self, source: &'p dyn BindingGateSource) {
        self.binding_gate = Some(source);
    }

    pub(crate) fn binding_gate(&self) -> Option<&dyn BindingGateSource> {
        self.binding_gate
    }

    /// The risk-clock second this batch acts at.
    pub(crate) fn at(&self) -> RiskClock {
        self.view.clock()
    }

    /// The id the next draft will carry: `(epoch, head, ordinal)`, so a retry of the same batch
    /// at the same head derives the same ids and the append answers `AlreadyCommitted`.
    pub(crate) fn next_id(&self) -> EventId {
        self.id_after(0)
    }

    /// The id of the draft `later` places after the next, for a record naming a later event.
    pub(crate) fn id_after(&self, later: u32) -> EventId {
        self.ports
            .ids
            .event_id(self.epoch, self.head, self.drafted.saturating_add(later))
    }

    /// Drafts one account-stream event stamped with the batch's risk clock as §9.2's whole-second
    /// timestamp (DEC-306), folds it into the
    /// working copy, and answers its id.
    pub(crate) fn journal(
        &mut self,
        event_type: &str,
        causation_id: Option<EventId>,
        pairs: Vec<(&str, Value)>,
    ) -> Result<EventId, ExecutorError> {
        self.journal_with_refs(event_type, causation_id, Object::new(), pairs)
    }

    pub(crate) fn journal_with_refs(
        &mut self,
        event_type: &str,
        causation_id: Option<EventId>,
        config_refs: Object,
        mut pairs: Vec<(&str, Value)>,
    ) -> Result<EventId, ExecutorError> {
        pairs.push(("risk_clock", risk_clock_stamp(self.at())?));
        let payload = object(pairs)?;
        let event_id = self.next_id();
        let stream = self.view.account_stream();
        let seq = Seq(self
            .head
            .0
            .saturating_add(u64::from(self.drafted))
            .saturating_add(1));
        fold(
            &mut self.view,
            &FoldedEvent {
                stream,
                seq,
                event_id: event_id.clone(),
                event_type: event_type.to_owned(),
                causation_id: causation_id.clone(),
                payload: payload.clone(),
            },
        )?;
        self.drafted = self.drafted.saturating_add(1);
        self.effects.push(Effect::Journal(EventDraft {
            event_id: event_id.clone(),
            event_type: event_type.to_owned(),
            schema_version: schema_version(event_type),
            config_refs,
            causation_id,
            payload,
        }));
        Ok(event_id)
    }

    pub(crate) fn broker(&mut self, request: BrokerRequest) {
        self.effects.push(Effect::Broker(request));
    }

    /// An owner alert: the id of the event it is about and a message key, nothing else
    /// (`AGENTS.md` rule 6, DEC-11).
    pub(crate) fn notify(&mut self, subject_event: EventId, message_key: &'static str) {
        self.effects.push(Effect::Notify(NotificationRef {
            subject_event,
            message_key,
        }));
    }

    /// The four reads of a reconciliation pass (trading-domain spec §11), for the shell to gather
    /// into one [`crate::BrokerSnapshot`].
    pub(crate) fn request_reconciliation(&mut self) {
        let since = self
            .view
            .checkpoint
            .clone()
            .unwrap_or_else(|| ActivityCursor(String::new()));
        self.broker(BrokerRequest::ListOpenOrders);
        self.broker(BrokerRequest::ListPositions);
        self.broker(BrokerRequest::GetAccount);
        self.broker(BrokerRequest::ListActivities { since });
    }
}

pub(crate) fn config_refs(refs: &[(&'static str, Option<&str>)]) -> Result<Object, ExecutorError> {
    let mut object = Object::new();
    for (name, raw) in refs {
        let raw = raw.ok_or(ExecutorError::BindingGateInputMissing)?;
        let digest = raw.strip_prefix("sha256:").and_then(Digest::from_hex);
        if digest.is_none() {
            return Err(ExecutorError::BindingGateInputMissing);
        }
        let key = Key::new(name).map_err(|_| ExecutorError::NonCanonicalPayload {
            field: (*name).to_owned(),
        })?;
        object.insert(key, Value::Str(raw.to_owned()));
    }
    Ok(object)
}

pub(crate) fn schema_version(event_type: &str) -> u64 {
    match event_type {
        "IntentReceived" | "GateDecided" | "OrderSubmitted" => 2,
        _ => 1,
    }
}

#[cfg(test)]
fn default_binding_gate() -> Option<&'static dyn BindingGateSource> {
    Some(&crate::ports::ALLOWING_BINDING_GATE)
}

#[cfg(not(test))]
fn default_binding_gate() -> Option<&'static dyn BindingGateSource> {
    None
}

#[cfg(test)]
mod tests {
    use super::{config_refs, schema_version};

    #[test]
    fn the_executor_owns_each_account_draft_schema_version() {
        for event_type in ["IntentReceived", "GateDecided", "OrderSubmitted"] {
            assert_eq!(schema_version(event_type), 2, "{event_type}");
        }
        for event_type in [
            "OrderRequestRecorded",
            "ProtectionChanged",
            "ReconciliationRun",
            "OwnerCommandRefused",
        ] {
            assert_eq!(schema_version(event_type), 1, "{event_type}");
        }
    }

    #[test]
    fn config_references_are_digest_refs_or_the_draft_is_refused() -> Result<(), String> {
        let digest = format!("sha256:{}", "1".repeat(64));
        let bare = "1".repeat(64);
        assert!(config_refs(&[("mandate_version", Some(&digest))]).is_ok());
        for raw in [None, Some(""), Some(bare.as_str()), Some("sha256:not-hex")] {
            let error = config_refs(&[("mandate_version", raw)])
                .err()
                .ok_or("an invalid config reference was accepted")?;
            assert_eq!(error, crate::error::ExecutorError::BindingGateInputMissing);
        }
        Ok(())
    }
}
