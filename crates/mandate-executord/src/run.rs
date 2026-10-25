//! A broker and the wiring that belongs with it (E21-30, DEC-2359 item 9). The fixture broker
//! serves with the fixture's wiring, which reads no quote; a broker whose transport reads market
//! data serves with the paper wiring, whose quote feed reads over that same transport. Only
//! [`fixture`] and [`paper`] make a [`Run`], and each takes only its own kind of broker, so the
//! serve path cannot pair the paper broker with a wiring that reads no quote.

use mandate_alpaca::{DataTransport, Pause, TradingTransport};

use crate::Wiring;
use crate::alpaca::AlpacaBroker;
use crate::paper::FixturePaper;

#[allow(
    non_upper_case_globals,
    reason = "the pending gate recognises a stub by the name `Unimplemented` (DEC-137)"
)]
pub const Unimplemented: &str = "unimplemented";

/// A broker paired with its wiring, which only [`fixture`] and [`paper`] make.
pub struct Run<B> {
    broker: B,
    wiring: Wiring,
}

impl<B> Run<B> {
    /// The wiring the run serves with.
    pub fn wiring(&self) -> &Wiring {
        &self.wiring
    }

    /// The broker and its wiring, for the serve path.
    pub fn into_parts(self) -> (B, Wiring) {
        (self.broker, self.wiring)
    }
}

/// The fixture broker with the fixture's wiring, which reads no quote.
///
/// # Errors
/// The fixture wiring's refusal.
pub fn fixture(
    broker: AlpacaBroker<FixturePaper>,
) -> Result<Run<AlpacaBroker<FixturePaper>>, &'static str> {
    let _ = broker;
    Err(Unimplemented)
}

/// A broker whose transport reads market data, with the paper wiring: its quote feed reads over
/// that transport, judged on `clock` ([`crate::credential::paper_wiring`]).
///
/// # Errors
/// The paper wiring's refusal.
pub fn paper<T, P>(broker: AlpacaBroker<T>, clock: P) -> Result<Run<AlpacaBroker<T>>, &'static str>
where
    T: TradingTransport + DataTransport + Clone + 'static,
    P: Pause + 'static,
{
    let _ = (broker, clock);
    Err(Unimplemented)
}
