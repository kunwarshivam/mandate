//! The fill model itself (trading-domain spec §6.4, rules 1 to 9).

use crate::{FirstBarVolumes, Instrument, SimBar, SimConfig, SimError, SimOrder, SimOutcome};
use mandate_time::UtcNanos;

/// Walks `bars` once, filling `orders` under spec §6.4, and returns every fill, every canceled OCO
/// leg, and each order's end state.
///
/// The rules, in the order the walk applies them to a bar:
///
/// 1. **Timing.** An order is eligible from the first bar starting at or after its decision time
///    plus the configured latencies (rule 1). The bar that produced the decision starts strictly
///    earlier, so it never fills.
/// 2. **Sessions.** A bar fills an order only in a session that order may trade: the regular
///    session, plus the extended sessions when `extended_hours` is set, and any session for a
///    crypto instrument, whose bars are continuous (rule 2, spec §4.3). A day order's remainder is
///    canceled after the last bar of its last eligible session.
/// 3. **Volume cap.** Each bar fills at most `truncate(fraction × reference volume, increment)`
///    across every order in the instrument, taken in submission order. The reference volume is the
///    most recent earlier bar of the same session and trading day; for a session's first covered
///    bar it is the 20-session median from `first_bar_volumes`; when neither exists the cap is 0
///    (rule 3, DEC-106 item 4). `coverage_start` is the first instant the data covers: a bar whose
///    session began before it has no known previous bar and no median, so its cap is 0.
/// 4. **Market orders** fill at the open moved against the order by `s` (rule 4), taker.
/// 5. **Limit orders** marketable on arrival fill at `min(limit, open ± s)` as taker and keep doing
///    so while later bars open within the limit; a bar that opens beyond the limit turns the
///    remainder into a resting limit. A resting limit fills at its limit as maker when the bar's
///    extreme passes it strictly; a touch is not a fill. On an auction bar a resting limit the open
///    gaps through fills at the open, with no slippage and no liquidity flag (rule 5, DEC-106
///    items 5 and 6).
/// 6. **Stop orders** fill at the open moved by `s` when the open has already passed the stop, and
///    at the stop moved by `s` when the bar's extreme reaches it. An equity stop triggers only on a
///    regular-session bar (rule 6).
/// 7. **Stop-limit orders** whose trigger bar opens beyond the limit rest as a limit at `L` from
///    that same bar; otherwise the triggered fill is at the limit or the triggered price, whichever
///    is worse for the order (rule 7, DEC-106 item 7).
/// 8. **OCO pairs.** If the open reaches a leg, that leg fills first under its own rule; otherwise,
///    with both legs reachable inside the bar, the stop fills first. The first fill of either leg,
///    partial or whole, cancels the other (rule 8, DEC-106 item 8).
/// 9. Fill prices are **not tick-rounded** (rule 9). A price needing more than the 9 places spec
///    §2.1 allows is rounded against the order (DEC-106 item 2).
///
/// Errors: a bar sequence that is not strictly increasing, inconsistent, or labelled with a session
/// that starts after it; an order with no quantity, crossed stop-limit or OCO prices, or a resting
/// bar the sequence does not hold; and any arithmetic that is not exact.
pub fn simulate(
    config: &SimConfig,
    instrument: &Instrument,
    bars: &[SimBar],
    coverage_start: UtcNanos,
    first_bar_volumes: &dyn FirstBarVolumes,
    orders: &[SimOrder],
) -> Result<SimOutcome, SimError> {
    let _ = (
        config,
        instrument,
        bars,
        coverage_start,
        first_bar_volumes,
        orders,
    );
    Err(SimError::Unsimulated)
}
