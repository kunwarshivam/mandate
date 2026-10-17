//! The Robinhood ticker of each instrument the run registered (DEC-956 item 1). The executor names
//! an equity by its asset id (mandate spec §3), and Robinhood's tools name it by its ticker, the
//! registered instrument snapshot's `symbol` (DEC-523). The connector sends the ticker on every
//! call and reads every answer's ticker back as the asset id; an instrument with no registered
//! ticker is never sent, and an answer naming another ticker than the one asked for is never
//! read as the instrument asked about.

use std::collections::BTreeMap;

use mandate_accounting::InstrumentId;

use crate::RobinhoodError;

/// Each registered asset id beside its Robinhood ticker, one to one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tickers {
    by_asset: BTreeMap<InstrumentId, InstrumentId>,
    by_ticker: BTreeMap<InstrumentId, InstrumentId>,
}

impl Tickers {
    /// The run's registered instruments, each as `(asset id, symbol)` from its instrument
    /// snapshot. An empty symbol is [`RobinhoodError::NoSymbol`]; an asset under two symbols, or
    /// a symbol under two assets, is [`RobinhoodError::AmbiguousSymbol`], since either reading
    /// could send one instrument's order as another's. A pair given twice is one pair.
    pub fn registered(
        pairs: impl IntoIterator<Item = (InstrumentId, String)>,
    ) -> Result<Self, RobinhoodError> {
        let mut tickers = Self::default();
        for (asset, symbol) in pairs {
            let ticker = InstrumentId::new(&symbol).map_err(|_| RobinhoodError::NoSymbol)?;
            let known = tickers.by_asset.get(&asset);
            let taken = tickers.by_ticker.get(&ticker);
            if known.is_some_and(|known| *known != ticker) || taken.is_some_and(|t| *t != asset) {
                return Err(RobinhoodError::AmbiguousSymbol);
            }
            tickers.by_asset.insert(asset.clone(), ticker.clone());
            tickers.by_ticker.insert(ticker, asset);
        }
        Ok(tickers)
    }

    /// The ticker Robinhood names `asset` by, or [`RobinhoodError::NoSymbol`] when the run
    /// registered none for it.
    pub(crate) fn ticker(&self, asset: &InstrumentId) -> Result<&InstrumentId, RobinhoodError> {
        self.by_asset.get(asset).ok_or(RobinhoodError::NoSymbol)
    }

    /// `ticker` itself when the run registered it for an instrument, or
    /// [`RobinhoodError::NoSymbol`]: the preflight and the quote name an instrument by the
    /// ticker the broker and the market data name it by, and read only a registered one.
    pub(crate) fn named(&self, ticker: &InstrumentId) -> Result<&InstrumentId, RobinhoodError> {
        self.by_ticker
            .get_key_value(ticker)
            .map(|(registered, _)| registered)
            .ok_or(RobinhoodError::NoSymbol)
    }

    /// The asset id of a registered `ticker`; a ticker the run did not register stays as the
    /// broker spelled it, so an instrument the run does not trade is never read as one it does.
    pub(crate) fn platform(&self, ticker: InstrumentId) -> InstrumentId {
        match self.by_ticker.get(&ticker) {
            Some(asset) => asset.clone(),
            None => ticker,
        }
    }
}
