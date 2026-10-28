//! The [`BarsSource`] the binary writes generations from: `mandate-marketdata`'s batched fetch
//! over a transport, paced to the data host's rate limit (DEC-2283 items 2 and 5). The binary's
//! transport is [`mandate_marketdata::http::AlpacaDataHttp::bars_only`] with the platform data key,
//! which sends it to the bars and corporate-actions endpoints alone; tests give a fake.

use std::collections::{BTreeMap, BTreeSet};

use mandate_marketdata::batch::BarsBatch;
use mandate_marketdata::client::{Client, FetchError, Pause, Transport};
use mandate_marketdata::model::{Bar, CorporateActions, DayRange, Symbol};

use crate::iex::generation::BarsSource;

/// The batched fetch over `T`, run to completion on its own single-threaded runtime.
pub struct ClientSource<T, P> {
    client: Client<T, P>,
    runtime: tokio::runtime::Runtime,
}

impl<T: Transport, P: Pause> ClientSource<T, P> {
    /// # Errors
    /// `runtime_unavailable` when the runtime cannot be built.
    pub fn new(client: Client<T, P>) -> Result<Self, &'static str> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "runtime_unavailable")?;
        Ok(Self { client, runtime })
    }
}

impl<T: Transport, P: Pause> BarsSource for ClientSource<T, P> {
    fn bars(&self, batch: &BarsBatch) -> Result<BTreeMap<Symbol, Vec<Bar>>, FetchError> {
        self.runtime.block_on(self.client.fetch_bars_batch(batch))
    }

    fn actions(
        &self,
        symbols: &BTreeSet<Symbol>,
        range: DayRange,
    ) -> Result<BTreeMap<Symbol, CorporateActions>, FetchError> {
        self.runtime
            .block_on(self.client.fetch_corporate_actions_batch(symbols, range))
    }
}
