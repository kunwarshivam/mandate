//! Each agent's daily research cap, read by the service from the deployed mandate (DEC-2272,
//! DEC-2254, E21-62): the agent's deployed version from its workspace's control stream, the
//! version's artifact checked against its hash, and `behavior.research.cost_cap_usd_per_day`
//! tightened by the internal research profile. Nothing the caller sends sets it, and anything that
//! does not read refuses with its code rather than leaving a cap the confirmed mandate did not give.

use std::collections::BTreeMap;

use mandate_gateway::money::Usd6;
use mandate_journal::{ArtifactSource, Environment, StoredEvent};

/// Why an agent's cap could not be read. Each names only its code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CapError {
    #[error("the control stream does not read")]
    ControlUnreadable,
    #[error("the deployed mandate's document gives no cap the gateway can hold")]
    DocumentUnavailable,
    #[error("the deployed mandate is for another environment")]
    EnvironmentMismatch,
}

impl CapError {
    /// The code the start exits with, or a call's refusal is logged under.
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn code(self) -> &'static str {
        let _ = self;
        todo!()
    }
}

/// What an agent's latest deployment record on the control stream says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Deployment {
    /// Deployed with this mandate version, an artifact reference.
    Deployed(String),
    /// Never deployed in this environment, or stopped since.
    NotDeployed,
}

/// The agent's latest `AgentDeployed` or `AgentStopped` in `environment` (DEC-2272 item 1).
///
/// # Errors
/// [`CapError::ControlUnreadable`] for a deployment record whose body does not read.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn deployment(
    control: &[StoredEvent],
    agent: &str,
    environment: Environment,
) -> Result<Deployment, CapError> {
    let _ = (control, agent, environment);
    todo!()
}

/// The cap `deployment` gives (DEC-2272 items 1 to 4): zero when not deployed or when the
/// document's `behavior.research` is `null`.
///
/// # Errors
/// [`CapError::DocumentUnavailable`] for an artifact that is missing, does not hash to its
/// reference or does not parse, a cap missing, malformed, beyond `Usd6` or above
/// `workspace_monthly`; [`CapError::EnvironmentMismatch`] for another environment's document.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn research_cap(
    deployment: &Deployment,
    environment: Environment,
    store: &dyn ArtifactSource,
    workspace_monthly: Usd6,
) -> Result<Usd6, CapError> {
    let _ = (deployment, environment, store, workspace_monthly);
    todo!()
}

/// Each agent's cap with the deployment it was read from, read again whenever the control stream
/// names another (DEC-2272 item 5): a call is only ever admitted against the cap of the version
/// deployed now.
pub struct DailyCaps {
    environment: Environment,
    workspace_monthly: Usd6,
    held: BTreeMap<String, (Deployment, Usd6)>,
}

impl DailyCaps {
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn new(environment: Environment, workspace_monthly: Usd6) -> Self {
        let _ = (environment, workspace_monthly);
        todo!()
    }

    /// `agent`'s cap for the deployment `control` names now: the one held when it is unchanged,
    /// else read again. A read that fails drops what was held, so the agent has no cap to use
    /// until a read succeeds.
    ///
    /// # Errors
    /// As [`deployment`] and [`research_cap`].
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn current(
        &mut self,
        control: &[StoredEvent],
        agent: &str,
        store: &dyn ArtifactSource,
    ) -> Result<Usd6, CapError> {
        let _ = (
            control,
            agent,
            store,
            &self.environment,
            self.workspace_monthly,
            &self.held,
        );
        todo!()
    }
}
