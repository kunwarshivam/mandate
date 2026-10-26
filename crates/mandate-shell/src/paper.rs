//! Trusted, paper-only assembly: E7-7's reviewed-artifact assembly, whose files are deleted with
//! its one-shot run (DEC-475 item 6), now the production door's own (DEC-466, DEC-470, DEC-471).
//!
//! Three steps, each a refusal when its answer is missing, stale, or ambiguous (`AGENTS.md` rule 3):
//!
//! 1. [`Artifacts::from_registered`] takes the confirmed version and the effective registrations,
//!    refuses a member it does not use, and binds the very bytes it checked into the executor's
//!    config references.
//! 2. [`preflight`] reads the broker with GETs only, through `mandate-alpaca`'s own clients: the
//!    account, the positions, the open orders, the asset record, the latest IEX quote, and the
//!    complete IEX minute bars of the last five minutes. [`liquidity_facts`] has
//!    `mandate-liquidity` compute the prior close, the 20-session median dollar volume, and the
//!    20-session average daily volume from typed stored daily bars, and the trailing five-minute
//!    volume from typed minute bars.
//! 3. [`load_contexts`] judges that [`PaperFacts`] snapshot at the run clock and assembles the two
//!    trusted contexts from it and from nothing else. An account holding any position or open order
//!    is refused: this one first-order migration maps no existing broker state (DEC-470 item 1).
//!
//! No value here is invented, and no money or quantity is computed here (DEC-138 item 3): the
//! gate configuration is the trading-domain spec's own defaults, the limits are the mandate's, every
//! market and account fact is the snapshot's, and every figure derived from them is computed by
//! the crate that owns its rule.

mod artifacts;
mod context;
mod facts;
mod gate;
mod judge;

pub use artifacts::{Artifacts, DeploymentInput};
pub use context::{
    Contexts, GateQuote, PaperClock, load_contexts, load_contexts_with_clock, reconcile_context,
};
pub(crate) use facts::refusal;
pub use facts::{
    BrokerFacts, LiquidityFacts, MarketFacts, PaperFacts, daily_closes, fresh_quote,
    liquidity_facts, market_facts, preflight, robinhood_facts,
};

use mandate_num::Usd;

use crate::error::Cause;

fn usd(text: &str) -> Result<Usd, Cause> {
    Usd::parse(text).map_err(Cause::Num)
}

fn absent(what: &'static str) -> Cause {
    Cause::Absent { what }
}

#[cfg(test)]
mod tests;
