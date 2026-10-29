//! An owner's request for an order (journal spec §2 and §9.9, workspace API §4.6, DEC-2858): the
//! control stream's `OwnerRequestSubmitted`, copied at most once and decided as a proposal the
//! order builder clipped, never sized.
//!
//! The request reaches the plan only when it is this deployment's, a buy, with a quantity, of an
//! instrument in the working universe; any other is inert here, because the workspace API refuses
//! it before journaling (DEC-2151) and adding no risk is the safe default (`AGENTS.md` rule 3).
//! Its `DecisionMade` names the request as `causation_id` and records `requested_by` and
//! `client_id`, with no conviction, score or model output (journal spec §9.1 rule 8); the mode,
//! the dry run and the autonomy rules then decide as for any opening, a client's never AUTO.

use crate::error::RuntimeError;
use crate::ports::Ports;
use crate::state::RuntimeState;
use crate::step::Batch;
use crate::types::FoldedEvent;

/// The control-stream record of an owner's request (journal spec §9.9).
pub(crate) const REQUEST_SUBMITTED: &str = "OwnerRequestSubmitted";

/// Decides one `OwnerRequestSubmitted` this runtime has not copied yet.
///
/// # Errors
/// [`RuntimeError::Unimplemented`] until E10-30's slice B lands.
pub(crate) fn requested(
    state: &RuntimeState,
    event: &FoldedEvent,
    ports: &Ports<'_>,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    let _ = (state, event, ports, batch);
    Err(RuntimeError::Unimplemented { story: "E10-30" })
}
