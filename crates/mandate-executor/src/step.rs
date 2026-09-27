//! The live step: the only producer of effects.

use crate::batch::Batch;
use crate::error::ExecutorError;
use crate::intent::{received, release_held, resume};
use crate::orders::{absent, cancelled, described, duplicate, fill, lookups_due, silence};
use crate::ports::Ports;
use crate::state::{ExecutorState, UnresolvedAppend};
use crate::types::{
    BrokerOutcome, BrokerRequest, BrokerUpdate, Effect, Input, OrderState, WriterEpoch,
};

/// One step of the executor (ADR-0001 ES-06).
///
/// The **only** producer of effects, which is what makes a replay safe: [`crate::fold`] emits
/// nothing, so recovery cannot re-send, and nothing but this function can reach the connector.
///
/// The returned list is ordered and the shell runs it in order: it appends first, stopping at the
/// first append that is neither `Committed` nor `AlreadyCommitted` and discarding the rest, and
/// makes a broker request only after the append that records it has committed. Within one list
/// **every [`Effect::Broker`] that submits follows the `OrderSubmitted` draft that names it**, so
/// write-before-acting (journal spec §5.2, `AGENTS.md` rule 5, DEC-07) is structural rather than a
/// convention: there is no code path that can submit without the draft.
///
/// A whole sequence — cancel, confirmation, gate re-run, submit, re-placement — is one ordered
/// list from one call, so a crash inside a sequence leaves the journal saying exactly where it
/// stopped and [`Input::Started`] resumes from that point rather than restarting the sequence
/// (task brief interpretation 20).
///
/// `Input::Started` is recovery. It never resubmits: for every `OrderSubmitted` the fold carries
/// with no acknowledgment it emits a `GetOrderByClientId`, and an order the broker does not have
/// returns to `Intent` only after `unknown_absent_lookups` absences spanning
/// `unknown_absent_window_s` (trading-domain spec §5.7, interpretation 9).
pub fn handle(
    state: &mut ExecutorState,
    input: Input,
    ports: &Ports<'_>,
) -> Result<Vec<Effect>, ExecutorError> {
    if let Input::Started(epoch) = input {
        return started(state, epoch, ports);
    }
    if !state.started {
        return Err(ExecutorError::NotStarted);
    }
    if let Some(batch) = &state.unresolved
        && batch.input != input
    {
        return Err(ExecutorError::AppendUnresolved { head: batch.head.0 });
    }
    if let Input::Tick(at) = &input {
        state.now = state.now.max(Some(*at));
    }
    let head = state.account_head();
    let mut batch = Batch::new(state, ports)?;
    step(&mut batch, input.clone())?;
    let effects = batch.effects;
    let drafts: Vec<_> = effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Journal(draft) => Some(draft.clone()),
            _ => None,
        })
        .collect();
    if !drafts.is_empty() {
        state.unresolved = Some(UnresolvedAppend {
            head,
            input,
            drafts,
        });
    }
    Ok(effects)
}

/// `Input::Started`: the process folded the stream and took an epoch. Every order whose outcome
/// the journal does not know is queried, a received intent too old to submit is abandoned, and
/// the startup reconciliation is requested; nothing is submitted until it has run.
fn started(
    state: &mut ExecutorState,
    epoch: WriterEpoch,
    ports: &Ports<'_>,
) -> Result<Vec<Effect>, ExecutorError> {
    if state.started {
        return Err(ExecutorError::AlreadyStarted);
    }
    state.epoch = Some(epoch);
    state.started = true;
    let mut batch = Batch::new(state, ports)?;
    let unresolved: Vec<_> =
        batch
            .view
            .unacknowledged()
            .into_iter()
            .chain(batch.view.orders.values().filter(|order| {
                matches!(order.state, OrderState::Unknown | OrderState::PendingCancel)
            }))
            .map(|order| order.client_order_id.clone())
            .collect();
    for id in unresolved {
        batch.broker(BrokerRequest::GetOrderByClientId(id));
    }
    resume(&mut batch)?;
    batch.request_reconciliation();
    Ok(batch.effects)
}

fn step(batch: &mut Batch<'_, '_>, input: Input) -> Result<(), ExecutorError> {
    match input {
        Input::Started(_) => Err(ExecutorError::AlreadyStarted),
        Input::Journal(_) | Input::Market(_) => Ok(()),
        Input::Tick(_) => {
            lookups_due(batch);
            release_held(batch)
        }
        Input::Intent(handoff) => received(batch, handoff),
        Input::Broker(Err(_)) => silence(batch),
        Input::Broker(Ok(outcome)) => outcome_of(batch, outcome),
        Input::BrokerUpdate(BrokerUpdate::Order(order)) => described(batch, &order),
        Input::BrokerUpdate(BrokerUpdate::Fill(one)) => fill(batch, &one),
        Input::BrokerUpdate(_) | Input::BrokerSnapshot(_) | Input::Command(_) => later_slice(),
    }
}

fn outcome_of(batch: &mut Batch<'_, '_>, outcome: BrokerOutcome) -> Result<(), ExecutorError> {
    match outcome {
        BrokerOutcome::Submitted(order) | BrokerOutcome::Order(order) => described(batch, &order),
        BrokerOutcome::DuplicateClientOrderId { client_order_id } => {
            duplicate(batch, &client_order_id)
        }
        BrokerOutcome::Absent { client_order_id } => absent(batch, &client_order_id),
        BrokerOutcome::CancelAccepted { .. } => cancelled(),
        BrokerOutcome::Rejected(_)
        | BrokerOutcome::Account(_)
        | BrokerOutcome::OpenOrders(_)
        | BrokerOutcome::Positions(_)
        | BrokerOutcome::Activities { .. }
        | BrokerOutcome::AccountWideAccepted => later_slice(),
    }
}

/// Fills and fees, the account and its restrictions, reconciliation, and the agent kill switch
/// (trading-domain spec §5.5, §5.7, §7.3, §10, §11): the later slices of this stack.
fn later_slice() -> Result<(), ExecutorError> {
    Err(ExecutorError::Unimplemented { story: "E7-3" })
}
