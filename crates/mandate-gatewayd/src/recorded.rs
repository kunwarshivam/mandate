//! The service's `--recorded` mode (DEC-2326 item 3, DEC-2210, DEC-2266): the gateway over the
//! recorded transport, replaying a recorded `research.json`'s model half exactly as the cell's
//! worker does, so every test and the cell run the real service with no provider reached. Each
//! call is primed with the reply its turn takes: the scripted turn a recording names for an
//! agentic call, else the recorded final reply.

use mandate_gateway::fake::{Recorded, RecordedTransport};
use mandate_gateway::registry::Reported;
use mandate_gateway::{CallRequest, Settings};
use mandate_research_run::recorded::{RecordedModel, RecordedReply, ScriptedTurn};

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

#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
impl Replies {
    pub fn new(model: &RecordedModel) -> Self {
        let _ = model;
        todo!()
    }

    /// The reply `request` takes: an agentic call's scripted turn, the one after the turns its
    /// transcript already holds, when the recording scripts one; else the recorded final reply.
    pub fn reply_for(&self, request: &CallRequest) -> Recorded {
        let _ = (&self.reply, &self.turns, &self.reported, request);
        todo!()
    }

    /// Scripts `request`'s reply on `transport`, replacing any reply a call never took.
    pub fn prime(&self, transport: &mut RecordedTransport, request: &CallRequest) {
        let _ = (transport, request);
        todo!()
    }
}

/// The gateway's settings for `workspace` over the recording's endpoint: hosted, through its one
/// upstream, under the cell's rate caps (DEC-2316).
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn settings(workspace: &str, model: &RecordedModel) -> Settings {
    let _ = (workspace, model);
    todo!()
}
