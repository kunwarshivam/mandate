//! The live step: the only producer of effects.

use crate::error::RuntimeError;
use crate::ports::Ports;
use crate::state::RuntimeState;
use crate::types::{Effect, Input};

/// One step of the runtime (ADR-0001 ES-06).
///
/// The **only** producer of effects, which is what makes a replay safe: [`crate::fold`] emits
/// nothing, so recovery cannot re-send, and nothing but this function can reach the sink.
///
/// The returned list is ordered and the shell runs it in order, stopping at the first append that is
/// neither `Committed` nor `AlreadyCommitted` and discarding the rest; the next start re-derives
/// from the fold, which is why discarding is safe. Within one list every [`Effect::Intent`] follows
/// the [`Effect::Journal`] that records it — **except for [`Input::Started`]**, whose handoffs
/// re-send intents whose `IntentProposed` committed in an earlier run, so there the rule reads
/// "preceded by the draft that records it, or by a draft the fold saw committed before the call"
/// (DEC-131 item 7).
///
/// `Input::Started` is recovery: its first effect is the startup hold's `AgentModeChanged` when the
/// mode changed, and it never re-journals an intent, an observation, a decision, or an approval.
pub fn handle(
    state: &mut RuntimeState,
    input: Input,
    ports: &Ports<'_>,
) -> Result<Vec<Effect>, RuntimeError> {
    let _ = (state, input, ports);
    Err(RuntimeError::Unimplemented { story: "E6-1" })
}
