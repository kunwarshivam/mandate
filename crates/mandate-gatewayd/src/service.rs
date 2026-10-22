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

    /// The caps the service holds, one per deployed agent.
    pub fn daily_caps(&self) -> &DailyCaps {
        todo!()
    }

    /// The start's check (DEC-2272 item 4): the control stream reads, and so does the cap of every
    /// agent it names as deployed, each then held on the gateway.
    ///
    /// # Errors
    /// The first [`CapError`] met; the service then does not start.
    pub fn ready(&mut self) -> Result<(), CapError> {
        let rows = self.control.rows().ok_or(CapError::ControlUnreadable)?;
        for agent in cap::agents(&rows, self.caps.environment())? {
            let daily = self.caps.current(&rows, &agent, self.artifacts.as_ref())?;
            self.gateway
                .meter_mut()
                .set_research_daily(&agent, Some(daily))
                .map_err(|_| CapError::DocumentUnavailable)?;
        }
        Ok(())
    }

    /// The gateway's reply to `request`, a research call of this workspace's held first to its
    /// agent's cap as the control stream names it now. A cap the meter will not take leaves the agent
    /// marked unread, so that refusal needs nothing more here.
    pub fn answer(&mut self, request: &CallRequest) -> Reply {
        if request.purpose == Purpose::Research
            && request.workspace_id == self.workspace
            && let Some(agent) = request.agent_id.as_deref()
        {
            let daily = self
                .control
                .rows()
                .ok_or(CapError::ControlUnreadable)
                .and_then(|rows| self.caps.current(&rows, agent, self.artifacts.as_ref()))
                .ok();
            let _unread_when_above_the_budget =
                self.gateway.meter_mut().set_research_daily(agent, daily);
        }
        self.gateway.call(request)
    }
}
