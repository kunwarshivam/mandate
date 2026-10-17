//! The lapse's account snapshot, kept for the same run's preflight (E7-26, the R0 rehearsal's
//! R11, [DEC-959](../../../docs/project/decisions/DEC-959.md) item 1): the live start reads the
//! agentic account once for the lapse, and the preflight that follows within the bound takes that
//! read rather than spending the MCP read budget on the same three reads again.

use std::time::Duration;

use mandate_robinhood::AccountSnapshot;
use mandate_time::UtcNanos;

use crate::quote::nanos;

/// The agentic account's snapshot the live start's lapse counted, and the run clock read just
/// before its first read was sent. Only the run that read it holds it: `live_start` returns it
/// beside the session it was read through, and the run hands it to that session's preflight
/// alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LapseSnapshot {
    pub account: AccountSnapshot,
    pub sent: UtcNanos,
}

impl LapseSnapshot {
    /// The snapshot, when at `now` it is at most `max_age` old counted from `sent`, the bound
    /// included, and it names no instrument: no position record and no working order, as the
    /// lapse already required, so nothing in it depends on the tickers the preflight registers
    /// after it was read (DEC-956 item 1). [`None`] when it is older, when the clock ran backwards
    /// and its age cannot be judged (rule 3), or when it lists any position or order, so the
    /// preflight reads the account again.
    pub fn fresh(&self, now: UtcNanos, max_age: Duration) -> Option<&AccountSnapshot> {
        let age = nanos(now)
            .zip(nanos(self.sent))
            .and_then(|(now, sent)| now.checked_sub(sent))
            .and_then(|age| u64::try_from(age).ok())?;
        let flat = self.account.positions.is_empty() && self.account.open_orders.is_empty();
        (flat && Duration::from_nanos(age) <= max_age).then_some(&self.account)
    }
}
