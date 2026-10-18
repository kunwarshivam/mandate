//! The process's [`Broker`] over `mandate-alpaca`'s client (DEC-2007 item 2): the same client the
//! shell drives, over whatever transport it is given. This crate names no host and builds no URL;
//! the dev and CI cells hand it [`crate::paper::FixturePaper`].
//!
//! The client's call is async and the process is not, so the adapter owns the runtime that drives
//! it: one current-thread tokio runtime, built once, that holds nothing a restart would lose.

use mandate_alpaca::{RetryPolicy, TokioPause, TradingClient, TradingTransport};
use mandate_domain::{CapabilityProfile, ProfileError};
use mandate_executor::{BrokerConnector, BrokerOutcome, BrokerRequest, ConnectorError};
use tokio::runtime::Runtime;

use crate::Broker;

/// Alpaca's paper trading API over `transport`.
pub struct AlpacaBroker<T> {
    transport: T,
    runtime: Runtime,
}

impl<T: TradingTransport + Clone> AlpacaBroker<T> {
    /// The broker over `transport`.
    ///
    /// # Errors
    /// `runtime_unavailable` when the runtime that drives the client cannot be built.
    pub fn new(transport: T) -> Result<Self, &'static str> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .map_err(|_| "runtime_unavailable")?;
        Ok(Self { transport, runtime })
    }

    fn client(&self) -> TradingClient<T, TokioPause> {
        TradingClient::new(self.transport.clone(), TokioPause, RetryPolicy::default())
    }
}

impl<T: TradingTransport + Clone> Broker for AlpacaBroker<T> {
    fn profile(&self) -> Result<CapabilityProfile, ProfileError> {
        self.client().profile()
    }

    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        let mut client = self.client();
        self.runtime.block_on(client.call(request))
    }
}
