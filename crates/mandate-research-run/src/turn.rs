//! One turn's gateway call ([agent harness spec](../../../docs/specs/agent-harness.md) v0.2 §6.9
//! step 3, §10.1; DEC-2256): the pure step from the core's turn to the gateway's request. The seed's
//! typed inputs are rendered exactly as the single call renders them, the prefix carries only what
//! is fixed for the run (its kind), and the turn adds the earlier turns, the menu, the spend ceiling,
//! the deadline, and every observation. The shell loads the transcript's artifacts; nothing here
//! reads a clock or a store.

use mandate_gateway::CallRequest;
use mandate_gateway::Gateway;
use mandate_gateway::meter::MeterStore;
use mandate_gateway::provider::{Clock, Transport};
use mandate_gateway::tools::Turn;
use mandate_time::UtcNanos;

use crate::agentic::{RunKind, TurnPlan};
use crate::call::{Answer, CallContext, CallPlan, ResearchCall, after_ms};
use crate::error::ResearchRunError;
use crate::retrieve::Retrieval;

/// One earlier turn as the worker loaded it: the reply's response bytes, and each tool result's
/// bytes in call order, as the model was given them (§6.9 step 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EarlierTurn {
    pub response: Vec<u8>,
    pub results: Vec<Vec<u8>>,
}

/// The worker's call for one turn of an agentic run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnCall {
    /// The seed's call, its items paired with the call plan's leading observations.
    pub seed: ResearchCall,
    pub kind: RunKind,
    pub plan: TurnPlan,
    pub transcript: Vec<EarlierTurn>,
    /// Every observation the turn's prompt holds: the seed's, then each read's.
    pub observations: Vec<String>,
    /// The dispatch instant plus the lesser of `deadline_ms` and the plan's time left.
    pub deadline: UtcNanos,
}

impl TurnCall {
    /// The call for `plan`'s turn, dispatched at `dispatched_at` (DEC-2256 items 2 to 4).
    ///
    /// # Errors
    /// [`ResearchRunError::ObservationsMismatch`] when the call plan names fewer observations than
    /// the seed holds items, [`ResearchRunError::LoopOrder`] for a transcript that is not this
    /// turn's, [`ResearchRunError::InvalidId`] for a result that is not UTF-8, and
    /// [`ResearchRunError::TimeOutOfRange`] for a deadline past the representable range.
    pub fn build(
        call: &CallPlan,
        plan: &TurnPlan,
        kind: RunKind,
        seed: &Retrieval,
        context: CallContext,
        transcript: Vec<EarlierTurn>,
        dispatched_at: UtcNanos,
    ) -> Result<Self, ResearchRunError> {
        let earlier = usize::try_from(plan.turn.saturating_sub(1)).unwrap_or(usize::MAX);
        if plan.turn == 0 || transcript.len() != earlier {
            return Err(ResearchRunError::LoopOrder {
                what: "a transcript that is not this turn's",
            });
        }
        if transcript
            .iter()
            .flat_map(|turn| &turn.results)
            .any(|result| std::str::from_utf8(result).is_err())
        {
            return Err(ResearchRunError::InvalidId {
                what: "tool result",
            });
        }
        let items = seed.items.len();
        if call.observations.len() < items {
            return Err(ResearchRunError::ObservationsMismatch {
                items,
                observations: call.observations.len(),
            });
        }
        let mut seeded = call.clone();
        seeded.observations.truncate(items);
        let seed_call = ResearchCall::build(&seeded, seed, context, dispatched_at)?;
        let within = u32::try_from(u64::from(call.deadline_ms).min(plan.time_left_ms))
            .unwrap_or(call.deadline_ms);
        Ok(Self {
            seed: seed_call,
            kind,
            plan: plan.clone(),
            transcript,
            observations: call.observations.clone(),
            deadline: after_ms(dispatched_at, within)?,
        })
    }

    /// The gateway's request for this turn (inference spec §3.1, DEC-2256 item 2).
    ///
    /// # Errors
    /// [`ResearchRunError::InvalidId`] when an input cannot be written as canonical text.
    pub fn request(&self) -> Result<CallRequest, ResearchRunError> {
        let mut request = self.seed.request()?;
        request.run_kind = Some(self.kind.as_str().to_owned());
        request.tools = self
            .plan
            .tools
            .iter()
            .map(|tool| tool.name().to_owned())
            .collect();
        request.turns = self
            .transcript
            .iter()
            .map(|turn| {
                let results = turn
                    .results
                    .iter()
                    .map(|result| {
                        String::from_utf8(result.clone()).map_err(|_| ResearchRunError::InvalidId {
                            what: "tool result",
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Turn {
                    response: turn.response.clone(),
                    results,
                })
            })
            .collect::<Result<Vec<_>, ResearchRunError>>()?;
        request.spend_ceiling = Some(self.plan.spend_ceiling);
        request.deadline = self.deadline;
        request.observations = self.observations.clone();
        Ok(request)
    }
}

/// Everything a turn's call is built from but the dispatch instant, which the gateway's clock gives.
#[derive(Debug, Clone)]
pub struct TurnParts<'a> {
    pub call: &'a CallPlan,
    pub plan: &'a TurnPlan,
    pub kind: RunKind,
    pub seed: &'a Retrieval,
    pub context: CallContext,
    pub transcript: Vec<EarlierTurn>,
}

/// The worker's turn (§6.9 step 3): builds the turn's call at the gateway's own clock, as the single
/// [`crate::call`] does, so the deadline counts from the dispatch instant, and sends it through the
/// gateway, which renders, guards, reserves, calls, and validates the reply against the menu.
///
/// # Errors
/// [`ResearchRunError`] when the turn's call cannot be built; nothing is then sent.
pub fn call_turn<T: Transport, S: MeterStore, C: Clock>(
    gateway: &mut Gateway<T, S, C>,
    parts: TurnParts<'_>,
) -> Result<(TurnCall, Answer), ResearchRunError> {
    let dispatched_at = gateway.clock_mut().now();
    let built = TurnCall::build(
        parts.call,
        parts.plan,
        parts.kind,
        parts.seed,
        parts.context,
        parts.transcript,
        dispatched_at,
    )?;
    let request = built.request()?;
    let answer = match gateway.call(&request) {
        Ok(record) => Answer::Recorded(record),
        Err(error) => Answer::Refused(error),
    };
    Ok((built, answer))
}
