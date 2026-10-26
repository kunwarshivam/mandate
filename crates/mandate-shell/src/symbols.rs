//! The shell's connector that reads its broker's answers through [`BrokerSymbols`], so each
//! instrument the run registered is named by the asset id the executor sent (DEC-938 item 1,
//! DEC-2812).

use mandate_domain::{CapabilityProfile, ProfileError};
pub use mandate_executor::BrokerSymbols;
use mandate_executor::{BrokerOutcome, BrokerRequest, ConnectorError};

use crate::stages::Connector;

/// A connector whose answers name each registered instrument by its asset id.
pub struct Platform<C> {
    pub inner: C,
    pub symbols: BrokerSymbols,
}

impl<C: Connector + ?Sized> Connector for Platform<Box<C>> {
    fn profile(&self) -> Result<CapabilityProfile, ProfileError> {
        self.inner.profile()
    }

    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        self.inner
            .call(request)
            .map(|outcome| self.symbols.outcome(outcome))
    }
}

#[cfg(test)]
mod tests {
    use mandate_accounting::InstrumentId;
    use mandate_executor::{BrokerOutcome, BrokerPosition, BrokerRequest, ConnectorError};
    use mandate_num::{Price, SignedQty};

    use super::{BrokerSymbols, Platform};
    use crate::stages::Connector;

    const ASSET: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";

    fn id(name: &str) -> Result<InstrumentId, String> {
        InstrumentId::new(name).map_err(|e| format!("{name}: {e}"))
    }

    fn spy() -> Result<BrokerSymbols, String> {
        Ok(BrokerSymbols::registered([(id("SPY")?, id(ASSET)?)]))
    }

    fn position(named: &str) -> Result<BrokerPosition, String> {
        Ok(BrokerPosition {
            instrument: id(named)?,
            qty: SignedQty::parse("1").map_err(|e| e.to_string())?,
            avg_entry_price: Price::parse("763.75").map_err(|e| e.to_string())?,
        })
    }

    struct Answers(Result<BrokerOutcome, ConnectorError>);

    impl Connector for Answers {
        fn call(&mut self, _request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
            self.0.clone()
        }
    }

    /// The wrapper maps what its connector answered, and passes a refusal and the profile as they
    /// came.
    #[test]
    fn the_platform_connector_maps_its_inner_answers() -> Result<(), String> {
        let answered = BrokerOutcome::Positions(vec![position("SPY")?]);
        let mut platform = Platform {
            inner: Box::new(Answers(Ok(answered))),
            symbols: spy()?,
        };
        let read = platform.call(&BrokerRequest::ListPositions);
        assert_eq!(read, Ok(BrokerOutcome::Positions(vec![position(ASSET)?])));
        assert_eq!(platform.profile(), mandate_alpaca::alpaca_profile());
        let refused = ConnectorError::NotSent { code: "offline" };
        let mut platform = Platform {
            inner: Box::new(Answers(Err(refused))),
            symbols: spy()?,
        };
        assert_eq!(platform.call(&BrokerRequest::ListPositions), Err(refused));
        Ok(())
    }
}
