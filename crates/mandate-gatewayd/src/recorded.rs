//! The service's `--recorded` mode (DEC-2326 item 3, DEC-2210, DEC-2266): the gateway over the
//! recorded transport, replaying a recorded `research.json`'s model half exactly as the cell's
//! worker does, so every test and the cell run the real service with no provider reached. Each
//! call is primed with the reply its turn takes: the scripted turn a recording names for an
//! agentic call, else the recorded final reply.

use std::collections::BTreeSet;

use mandate_gateway::fake::{Recorded, RecordedTransport, openrouter_reply};
use mandate_gateway::guard::GuardContext;
use mandate_gateway::policy::EndpointPolicy;
use mandate_gateway::provider::Delivery;
use mandate_gateway::registry::{Locality, Reported};
use mandate_gateway::{CallRequest, RateCaps, Settings};
use mandate_research_run::recorded::{RecordedModel, RecordedReply, ScriptedTurn};
use mandate_research_run::recorded_reply;

/// The workspace's calls a minute, fixed whatever the entry pins, as the cell's gateway holds them
/// (DEC-2316).
pub const WORKSPACE_RATE: u32 = 12;

/// The calls a minute an agent may make beyond the turns one run of its entry may take (DEC-2316).
pub const RATE_MARGIN: u32 = 2;

/// What the service replays: the recorded reply, the scripted turns, and what the endpoint
/// reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replies {
    reply: RecordedReply,
    turns: Vec<ScriptedTurn>,
    reported: Reported,
}

impl Replies {
    pub fn new(model: &RecordedModel) -> Self {
        Self {
            reply: model.reply.clone(),
            turns: model.turns.clone(),
            reported: model.probe.clone(),
        }
    }

    /// The reply `request` takes: an agentic call's scripted turn, the one after the turns its
    /// transcript already holds, when the recording scripts one; else the recorded final reply.
    pub fn reply_for(&self, request: &CallRequest) -> Recorded {
        let scripted = request
            .run_kind
            .as_ref()
            .and_then(|_| self.turns.get(request.turns.len()));
        let (latency_ms, body) = match scripted {
            Some(turn) => {
                let calls: Vec<(&str, &str)> = turn
                    .calls
                    .iter()
                    .map(|(name, arguments)| (name.as_str(), arguments.as_str()))
                    .collect();
                (
                    turn.latency_ms,
                    openrouter_reply(None, &calls, &self.reported, turn.usage),
                )
            }
            None => (
                self.reply.latency_ms,
                recorded_reply(
                    &self.reply.output,
                    self.reported.model.as_deref().unwrap_or_default(),
                    self.reported.provider.as_deref().unwrap_or_default(),
                    self.reply.usage,
                ),
            ),
        };
        Recorded {
            latency_ms,
            delivery: Delivery::Response {
                status: 200,
                body,
                retry_after_ms: None,
            },
        }
    }

    /// Scripts `request`'s reply on `transport`, replacing any reply a call never took.
    pub fn prime(&self, transport: &mut RecordedTransport, request: &CallRequest) {
        transport.clear_script();
        transport.script(vec![self.reply_for(request)]);
    }
}

/// The gateway's settings for `workspace` over the recording's endpoint: hosted, through its one
/// upstream, under the cell's rate caps (DEC-2316).
pub fn settings(workspace: &str, model: &RecordedModel) -> Settings {
    let turns = model
        .entry
        .agentic
        .as_ref()
        .map_or(1, |bounds| bounds.max_turns);
    Settings {
        workspace_id: workspace.to_owned(),
        policy: EndpointPolicy::new(
            BTreeSet::from([Locality::Hosted]),
            Some(BTreeSet::from([model.endpoint.route.upstream.clone()])),
            None,
        ),
        guard: GuardContext::default(),
        rate: RateCaps {
            per_workspace: WORKSPACE_RATE,
            per_agent: turns.saturating_add(RATE_MARGIN),
        },
    }
}
