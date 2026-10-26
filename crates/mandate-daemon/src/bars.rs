//! The runtime's reader of the liquidity floor's and participation's inputs (DEC-2274 item 3):
//! `mandate_bars::liquidity` over the data process's stored bars, at the runtime's journaled risk
//! clock, for an opening's dry run alone. The executor's binding gate reads the same function at
//! its own clock (item 4), so the two rest on one reading and neither trusts the other's figures.
//!
//! It fails closed. With no root, a refused read, an empty trailing window, or no dataset for the
//! instrument there are no figures, and both checks stay deferred to the binding gate, never
//! passed (DEC-2378 item 11). An exit, a protective order and a flatten never read it, so a
//! missing bar never holds risk reduction (`AGENTS.md` rule 13).

use std::collections::BTreeSet;
use std::path::PathBuf;

use mandate_bars::LiquidityFacts;
use mandate_risk::{Deferred, Purpose};
use mandate_time::UtcNanos;

/// The environment variable the chart names the bars' root in, only where the data process runs.
pub const BARS_ENV: &str = "MANDATE_BARS";

/// Where the stored bars are, if anywhere.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BarsReader {
    root: Option<PathBuf>,
}

impl BarsReader {
    /// The reader of [`BARS_ENV`]'s value: an unset or empty root reads nothing.
    pub fn new(root: Option<&str>) -> Self {
        Self {
            root: root.filter(|r| !r.is_empty()).map(PathBuf::from),
        }
    }

    /// The figures an opening's dry run reads for `symbol` at `now`, the runtime's risk clock:
    /// `None` for any purpose but an opening or an increase, which it never reads for, and `None`
    /// when there is no root or the read is refused.
    pub fn opening(&self, purpose: Purpose, symbol: &str, now: UtcNanos) -> Option<LiquidityFacts> {
        if !matches!(purpose, Purpose::Open | Purpose::Increase) {
            return None;
        }
        mandate_bars::liquidity(self.root.as_deref()?, symbol, now).ok()
    }
}

/// Which of the liquidity floor and participation stay deferred to the binding gate: both
/// without figures, neither with them.
pub fn liquidity_deferred(facts: Option<&LiquidityFacts>) -> BTreeSet<Deferred> {
    match facts {
        Some(_) => BTreeSet::new(),
        None => BTreeSet::from([Deferred::LiquidityFloor, Deferred::Participation]),
    }
}
