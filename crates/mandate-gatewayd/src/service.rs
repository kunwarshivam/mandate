//! The service's answer to one request (DEC-2272 items 4 to 6, E21-62): a research call's agent
//! has its daily cap read from the deployed mandate before the gateway reserves anything, so the
//! call is admitted only against the cap of the version deployed now. A cap that does not read
//! marks the agent unread, and the gateway refuses its research call `meter_unavailable`, reserving
//! and sending nothing. Every other request reaches the gateway as it came.

use mandate_gateway::kinds::Purpose;
use mandate_gateway::meter::MeterStore;
use mandate_gateway::provider::{Clock, Transport};
use mandate_gateway::wire::Reply;
use mandate_gateway::{CallRequest, Gateway};
use mandate_journal::{ArtifactSource, Environment, StoredEvent};

use crate::cap::{self, CapError, DailyCaps};

/// The workspace's control stream as the service reads it, `None` when it cannot be read.
pub trait Control {
    fn rows(&self) -> Option<Vec<StoredEvent>>;
}

/// The gateway with what it reads each research call's cap from.
pub struct Service<T, S, C> {
    workspace: String,
    gateway: Gateway<T, S, C>,
    caps: DailyCaps,
    control: Box<dyn Control>,
    artifacts: Box<dyn ArtifactSource>,
}

impl<T: Transport, S: MeterStore, C: Clock> Service<T, S, C> {
    /// The service for `workspace` in `environment`, reading caps under the gateway's own monthly
    /// budget.
    pub fn new(
        workspace: &str,
        environment: Environment,
        gateway: Gateway<T, S, C>,
        control: Box<dyn Control>,
        artifacts: Box<dyn ArtifactSource>,
    ) -> Self {
        let mut gateway = gateway;
        gateway.meter_mut().require_research_daily();
        let caps = DailyCaps::new(environment, gateway.meter().caps().workspace_monthly());
        Self {
            workspace: workspace.to_owned(),
            gateway,
            caps,
            control,
            artifacts,
        }
    }

    pub fn gateway(&self) -> &Gateway<T, S, C> {
        &self.gateway
    }

    /// The gateway itself, for the evaluation gate and the start's identity probes, which call it
    /// with purpose `evaluation` and no agent, so no research cap applies (DEC-2216 item 1).
    pub fn gateway_mut(&mut self) -> &mut Gateway<T, S, C> {
        &mut self.gateway
    }

    /// The gateway's transport, for a recorded service to prime with each call's reply.
    pub fn transport_mut(&mut self) -> &mut T {
        self.gateway.transport_mut()
    }

    /// The caps the service holds, one per deployed agent.
    pub fn daily_caps(&self) -> &DailyCaps {
        &self.caps
    }

    /// The start's check (DEC-2272 item 4): the control stream reads, and so does the cap of every
    /// agent it names as deployed, each then held on the gateway.
    ///
    /// # Errors
    /// The first [`CapError`] met; the service then does not start.
    pub fn ready(&mut self) -> Result<(), CapError> {
        let rows = self.control.rows().ok_or(CapError::ControlUnreadable)?;
        let deployed = cap::deployed_agents(&rows, self.caps.environment())?;
        self.caps.retain(&deployed);
        self.gateway.meter_mut().retain_research_daily(&deployed);
        for agent in &deployed {
            let daily = self.caps.current(&rows, agent, self.artifacts.as_ref())?;
            self.gateway
                .meter_mut()
                .set_research_daily(agent, Some(daily))
                .map_err(|_| CapError::DocumentUnavailable)?;
        }
        Ok(())
    }

    /// The gateway's reply to `request`, a research call of this workspace's held first to its
    /// agent's cap as the control stream names it now. Every control read that succeeds prunes what
    /// is held to the agents deployed now; an agent outside them has nothing read or set, and the
    /// meter refuses it `budget_exhausted`. A deployed agent's cap that does not read, or a control
    /// stream that does not, marks a known agent unread, refused `meter_unavailable`. So what the
    /// service holds is bounded by the agents deployed, whatever names its callers send.
    pub fn answer(&mut self, request: &CallRequest) -> Reply {
        if request.purpose == Purpose::Research
            && request.workspace_id == self.workspace
            && let Some(agent) = request.agent_id.as_deref()
        {
            let rows = self.control.rows();
            let deployed = rows
                .as_deref()
                .ok_or(CapError::ControlUnreadable)
                .and_then(|rows| cap::deployed_agents(rows, self.caps.environment()));
            let known = self
                .gateway
                .meter()
                .caps()
                .research_agents()
                .contains(agent);
            match (rows, deployed) {
                (Some(rows), Ok(deployed)) => {
                    self.caps.retain(&deployed);
                    self.gateway.meter_mut().retain_research_daily(&deployed);
                    if deployed.contains(agent) {
                        let daily = self
                            .caps
                            .current(&rows, agent, self.artifacts.as_ref())
                            .ok();
                        let _unread_when_unreadable_or_above_the_budget =
                            self.gateway.meter_mut().set_research_daily(agent, daily);
                    }
                }
                _ if known => {
                    let _marked_unread = self.gateway.meter_mut().set_research_daily(agent, None);
                }
                _ => {}
            }
        }
        self.gateway.call(request)
    }
}
