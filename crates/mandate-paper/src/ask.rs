//! The broker's own ask an asked opening's cross-check reads (E7-26, the [first live trade
//! brief](../../../docs/project/tasks/first-live-trade.md)'s O2 and G1b;
//! [DEC-529](../../../docs/project/decisions/DEC-529.md) item 12;
//! [DEC-945](../../../docs/project/decisions/DEC-945.md)): a session hands it in before its
//! cycle takes the session, and the cross-check asks it for the ask at the check itself, after
//! the grant, so a Robinhood ask is the broker's latest and never the preflight's.

use std::rc::Rc;
use std::time::Duration;

use mandate_accounting::InstrumentId;
use mandate_alpaca::Pause;
use mandate_num::Price;
use mandate_robinhood::{Quote, Tools};
use mandate_shell::robinhood::RobinhoodBridge;

use crate::quote::QuoteRefusal;

/// One read of the instrument's quote at the broker: the quote, or [`None`] when the read was
/// refused, failed, or answered something that is not one uncrossed record of the instrument.
pub type QuoteRead = Box<dyn Fn(&InstrumentId) -> Option<Quote>>;

/// Where a session's broker ask comes from.
pub enum BrokerAsk {
    /// The ask the preflight already read: Alpaca's own quote is the IEX quote its data client
    /// read, whose age the shell's judge and the binding gate hold to the bound (DEC-940 item 4).
    Quoted(Price),
    /// Robinhood's own quote, read through the connector's `get_equity_quotes` at the check
    /// (DEC-945 item 1).
    Read(QuoteRead),
    /// The session reads no broker quote, so the cross-check refuses `quote_absent` (rule 3).
    Absent,
}

impl BrokerAsk {
    /// The read of `bridge`'s connector (`RobinhoodBridge::equity_quote`), on its own runtime.
    pub fn robinhood<T: Tools + 'static>(bridge: Rc<RobinhoodBridge<T>>) -> Self {
        let _ = bridge;
        Self::Absent
    }

    /// The ask for `instrument` at the check: [`Self::Quoted`]'s as read; [`Self::Read`]'s read
    /// now, between two readings of `pause`'s clock, and judged by
    /// [`fresh_ask`](crate::quote::fresh_ask) against `max_age`; [`Self::Absent`] refuses.
    ///
    /// # Errors
    /// [`QuoteRefusal::Absent`] or [`QuoteRefusal::Stale`], each before anything is sent.
    pub fn ask<P: Pause + ?Sized>(
        &self,
        instrument: &InstrumentId,
        pause: &P,
        max_age: Duration,
    ) -> Result<Price, QuoteRefusal> {
        let _ = (instrument, pause, max_age);
        Err(QuoteRefusal::Unimplemented { story: "E7-26 O2" })
    }
}
