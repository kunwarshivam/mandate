//! The live start's gate on the meters (DEC-2257 item 2, DEC-2258, E21-62): the service starts live
//! only when both meter streams it writes, the workspace's and the deployment's, read back now. A
//! meter that cannot be read would refuse every call `meter_unavailable`; refusing the start says
//! so once, before the listener opens, and never serves a call against a meter held in memory.

use mandate_gateway::meter::MeterStore;

/// Why the service did not start live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StartError {
    #[error("the workspace's meter stream cannot be read")]
    WorkspaceMeter,
    #[error("the deployment's meter stream cannot be read")]
    DeploymentMeter,
}

impl StartError {
    /// The code the process exits with.
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn code(self) -> &'static str {
        let _ = self;
        todo!()
    }
}

/// Answers whether both meters read back, the workspace's checked first.
///
/// # Errors
/// [`StartError`] naming the first meter that does not read.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn meters_ready(
    workspace: &dyn MeterStore,
    deployment: &dyn MeterStore,
) -> Result<(), StartError> {
    let _ = (workspace, deployment);
    todo!()
}
