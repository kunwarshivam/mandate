//! The service's answer to one request (DEC-2272 items 4 to 6, E21-62): a research call's agent
//! has its daily cap read from the deployed mandate before the gateway reserves anything, so the
//! call is admitted only against the cap of the version deployed now. A cap that does not read
//! marks the agent unread, and the gateway refuses its research call `meter_unavailable`, reserving
//! and sending nothing. Every other request reaches the gateway as it came.

use mandate_gateway::meter::MeterStore;
use mandate_gateway::provider::{Clock, Transport};
use mandate_gateway::wire::Reply;
use mandate_gateway::{CallRequest, Gateway};
use mandate_journal::{ArtifactSource, Environment, StoredEvent};

use crate::cap::{CapError, DailyCaps};

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

#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
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
        let _ = (workspace, environment, gateway, control, artifacts);
        todo!()
    }

    pub fn gateway(&self) -> &Gateway<T, S, C> {
        let _ = (&self.workspace, &self.caps, &self.control, &self.artifacts);
        todo!()
    }

    /// The start's check (DEC-2272 item 4): the control stream reads, and so does the cap of every
    /// agent it names as deployed, each then held on the gateway.
    ///
    /// # Errors
    /// The first [`CapError`] met; the service then does not start.
    pub fn ready(&mut self) -> Result<(), CapError> {
        let _ = &mut self.gateway;
        todo!()
    }

    /// The gateway's reply to `request`, a research call of this workspace's held first to its
    /// agent's cap as the control stream names it now. A cap the meter will not take leaves the agent
    /// marked unread, so that refusal needs nothing more here.
    pub fn answer(&mut self, request: &CallRequest) -> Reply {
        let _ = request;
        todo!()
    }
}
