//! The live step: the only producer of effects.

use crate::error::ExecutorError;
use crate::ports::Ports;
use crate::state::ExecutorState;
use crate::types::{Effect, Input};

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
    let _ = (state, input, ports);
    Err(ExecutorError::Unimplemented { story: "E7-2" })
}
