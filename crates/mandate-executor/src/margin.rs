//! The broker's maintenance figures, as trading-domain spec §9.2's `intraday_margin` regime reads
//! them: "never create an intraday margin deficit; broker-reported maintenance excess is also
//! checked". The executor owns the account type, so the excess is derived here, once, from the two
//! fields the connector parses (§7.2), and the shell takes it from here rather than computing its
//! own (the first paper trade brief's X-9, slices A1 and Q2; DEC-524).

use mandate_num::Usd;

use crate::error::ExecutorError;
use crate::types::BrokerAccount;

impl BrokerAccount {
    /// Equity less the broker's maintenance margin: positive is an excess, negative a deficit
    /// (§9.2). A negative maintenance margin is no requirement a broker states, and reading one
    /// would raise the excess, so it is refused rather than subtracted (DEC-524 item 2,
    /// `AGENTS.md` rule 3).
    ///
    /// # Errors
    /// [`ExecutorError::Num`] with `NumError::Negative` for a negative maintenance margin, and
    /// with the arithmetic's own error when the difference does not fit a [`Usd`].
    pub fn maintenance_excess(&self) -> Result<Usd, ExecutorError> {
        Err(ExecutorError::Unimplemented { story: "E7-19" })
    }
}
