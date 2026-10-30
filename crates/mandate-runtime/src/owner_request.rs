//! An owner's request for an order (journal spec §2 and §9.9, workspace API §4.6, DEC-2858): the
//! control stream's `OwnerRequestSubmitted`, copied at most once and decided as a proposal the
//! order builder clipped, never sized.
//!
//! The request reaches the plan only when it is this deployment's, a buy, with a quantity, of an
//! instrument in the working universe; any other is inert here, because the workspace API refuses
//! it before journaling (DEC-2151) and adding no risk is the safe default (`AGENTS.md` rule 3).
//! Its `DecisionMade` names the request as `causation_id` and records `requested_by` and
//! `client_id`, with no conviction, score or model output (journal spec §9.1 rule 8); the mode,
//! the dry run and the autonomy rules then decide as for any opening, a client's never AUTO and an
//! owner's never asked (DEC-2858 item 10): the owner's Send is the confirmation. A
//! client's request arrives with an actor of kind `user` and `requested_by` `client` (journal spec
//! §9.9 rule 79), so who asked is read from `requested_by` and `client_id`.

use crate::error::RuntimeError;
use crate::payload;
use crate::ports::Ports;
use crate::state::RuntimeState;
use crate::step::{self, Batch, Next};
use crate::types::{
    Autonomy, Classified, DryRunVerdict, FoldedEvent, Mode, NotificationRef, OwnerRequest,
    Requester, RuntimeAlert,
};

/// The control-stream record of an owner's request (journal spec §9.9).
pub(crate) const REQUEST_SUBMITTED: &str = "OwnerRequestSubmitted";

/// The label of mandate spec §6.2 step 5a's client ceiling, which a client's AUTO opening becomes.
const CLIENT_CEILING: &str = "client_ceiling";

/// The label of mandate spec §6.2 step 5d, the owner's own request, which an owner's `ask` becomes.
const OWNER_REQUEST: &str = "owner_request";

/// Decides one `OwnerRequestSubmitted` this runtime has not copied yet: the plan's clipped
/// proposal, denied by name when the mode admits no opening, otherwise through the dry run and the
/// autonomy rules, with the client ceiling over a client's; then the `DecisionMade` caused by the
/// request, and the intent, the ask, or the skipped decision's alert after it. A request this
/// deployment cannot act on, or one the plan cannot price, journals nothing.
///
/// # Errors
/// [`RuntimeError`] when a draft cannot be written.
pub(crate) fn requested(
    state: &RuntimeState,
    event: &FoldedEvent,
    ports: &Ports<'_>,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    let Some(request) = request_of(state, event, ports) else {
        return Ok(());
    };
    let (Some(now), Some(proposal)) = (
        state.risk_clock(),
        ports.plan.requested(ports.view, &request),
    ) else {
        return Ok(());
    };
    let verdict = match mode_denial(state.effective_mode()) {
        Some(reason_code) => DryRunVerdict::Deny {
            reason_code: reason_code.to_owned(),
        },
        None => step::dry_run(state, ports, &proposal, now),
    };
    let classified = ceiling(
        step::overlaid(ports.plan.classify(ports.view, &proposal), &verdict),
        &request.requested_by,
    );
    let (verdict, then) = step::next(
        state,
        &proposal,
        verdict,
        &classified,
        now,
        ports.view,
        Some(&request.requested_by),
    );
    let inputs = state.signal_inputs(now);
    let decision = batch.journal(
        "DecisionMade",
        Some(request.request.clone()),
        step::decided(
            &proposal,
            &inputs,
            &verdict,
            &classified,
            Some(&request.requested_by),
        )?,
    )?;
    match then {
        Next::Nothing if !allowed(&verdict) => {
            batch.notify(NotificationRef {
                subject_event: decision,
                kind: RuntimeAlert::DecisionSkipped,
            });
            Ok(())
        }
        then => step::allowed(&proposal, then, ports, decision, batch),
    }
}

fn allowed(verdict: &DryRunVerdict) -> bool {
    matches!(
        verdict,
        DryRunVerdict::Allow | DryRunVerdict::AllowDeferring { .. }
    )
}

/// The request as the plan reads it, or `None` when it is inert here: another deployment's, a
/// side other than `buy`, no quantity, or an instrument outside the working universe, which the
/// workspace API refuses before journaling (DEC-2151, DEC-2858 items 3, 4 and 6); or a member that
/// does not parse.
fn request_of(
    state: &RuntimeState,
    event: &FoldedEvent,
    ports: &Ports<'_>,
) -> Option<OwnerRequest> {
    let text = |field| payload::str_of(&event.payload, field);
    if text("agent_id") != Some(state.deployment().agent.0.as_str()) || text("side") != Some("buy")
    {
        return None;
    }
    let instrument = payload::instrument_of(text("instrument_id")?).ok()?;
    if !ports.view.working_universe.contains(&instrument) {
        return None;
    }
    let qty = payload::qty_of(text("quantity")?).ok()?;
    let requested_by = match (text("requested_by")?, text("client_id")) {
        ("owner", None) => Requester::Owner,
        ("client", Some(client_id)) => Requester::Client {
            client_id: client_id.to_owned(),
        },
        _ => return None,
    };
    Some(OwnerRequest {
        request: event.event_id.clone(),
        instrument,
        qty,
        requested_by,
    })
}

/// The gate's code for a mode that admits no opening (trading spec §9.1 check 1), which the owner
/// is shown by name rather than having the request vanish.
fn mode_denial(mode: Mode) -> Option<&'static str> {
    match mode {
        Mode::Normal => None,
        Mode::ExitsOnly => Some("agent_exits_only"),
        Mode::Paused => Some("agent_paused"),
        Mode::Stopped => Some("agent_stopped"),
    }
}

/// Mandate spec §6.2 steps 5a and 5d: a client's opening is never AUTO (MI-30), so an `auto`
/// becomes an `ask` labelled `client_ceiling`; an owner's own request is its own approval
/// (DEC-2858 item 10, founder-ruled), so an `ask` from the rules, the default or a deferred check
/// becomes an `auto` labelled `owner_request`. Neither touches a `deny`, and the binding gate judges
/// every check at submission as for any opening.
fn ceiling(classified: Classified, requested_by: &Requester) -> Classified {
    match (requested_by, classified.autonomy) {
        (Requester::Client { .. }, Autonomy::Auto) => Classified {
            autonomy: Autonomy::Ask,
            decided_by: Some(CLIENT_CEILING.to_owned()),
        },
        (Requester::Owner, Autonomy::Ask) => Classified {
            autonomy: Autonomy::Auto,
            decided_by: Some(OWNER_REQUEST.to_owned()),
        },
        _ => classified,
    }
}
