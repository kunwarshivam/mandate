//! One effect list under construction.
//!
//! Every draft is folded into a working copy of the state the moment it is written, so a later
//! decision in the same list sees what the earlier drafts did — exactly what a replay of the
//! journal will see — and the list is one sequence rather than a set of guesses (task brief
//! interpretation 20).

use mandate_canon::Value;

use crate::error::ExecutorError;
use crate::fold::fold;
use crate::payload::{clock, object};
use crate::ports::Ports;
use crate::state::ExecutorState;
use crate::types::{
    ActivityCursor, BrokerRequest, Effect, EventDraft, EventId, FoldedEvent, NotificationRef,
    RiskClock, Seq, WriterEpoch,
};

pub(crate) struct Batch<'p, 'a> {
    pub(crate) ports: &'p Ports<'a>,
    pub(crate) view: ExecutorState,
    pub(crate) effects: Vec<Effect>,
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
            drafted: 0,
        })
    }

    /// The risk-clock second this batch acts at.
    pub(crate) fn at(&self) -> RiskClock {
        self.view.clock()
    }

    /// The id the next draft will carry: `(epoch, head, ordinal)`, so a retry of the same batch
    /// at the same head derives the same ids and the append answers `AlreadyCommitted`.
    pub(crate) fn next_id(&self) -> EventId {
        self.ports.ids.event_id(self.epoch, self.head, self.drafted)
    }

    /// Drafts one account-stream event stamped with the batch's risk clock, folds it into the
    /// working copy, and answers its id.
    pub(crate) fn journal(
        &mut self,
        event_type: &str,
        causation_id: Option<EventId>,
        mut pairs: Vec<(&str, Value)>,
    ) -> Result<EventId, ExecutorError> {
        pairs.push(("risk_clock", clock(self.at())?));
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
