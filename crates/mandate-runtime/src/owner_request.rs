//! An owner's request for an order (journal spec §2 and §9.9, workspace API §4.6, DEC-2858): the
//! control stream's `OwnerRequestSubmitted`, copied at most once and decided as a proposal the
//! order builder clipped, never sized.
//!
//! The request reaches the plan only when it is this deployment's, a buy, with a quantity, of an
//! instrument in the working universe; any other is inert here, because the workspace API refuses
//! it before journaling (DEC-2151) and adding no risk is the safe default (`AGENTS.md` rule 3).
//! Its `DecisionMade` names the request as `causation_id` and records `requested_by` and
//! `client_id`, with no conviction, score or model output (journal spec §9.1 rule 8); the mode,
//! the dry run and the autonomy rules then decide as for any opening, a client's never AUTO. A
//! client's request arrives with an actor of kind `user` and `requested_by` `client` (journal spec
//! §9.9 rule 79), so who asked is read from `requested_by` and `client_id`.

use crate::error::RuntimeError;
use crate::ports::Ports;
use crate::state::RuntimeState;
use crate::step::Batch;
use crate::types::FoldedEvent;

/// The control-stream record of an owner's request (journal spec §9.9).
pub(crate) const REQUEST_SUBMITTED: &str = "OwnerRequestSubmitted";

/// Decides one `OwnerRequestSubmitted` this runtime has not copied yet.
///
/// Inert until E10-30's slice B lands: it journals and hands nothing, so the request is never
/// acted on, and never fails the step either, because a failed step stops the shell's control pass
/// and would leave a later kill switch in the same read unhanded (`AGENTS.md` rule 13). Its pending
/// tests are behaviour-only rows for that reason.
///
/// # Errors
/// None yet; the implementation returns [`RuntimeError`] when a draft cannot be written.
pub(crate) fn requested(
    state: &RuntimeState,
    event: &FoldedEvent,
    ports: &Ports<'_>,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    let _ = (state, event, ports, batch);
    Ok(())
}
