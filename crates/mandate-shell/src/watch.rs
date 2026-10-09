//! E1b's bounded watch after the one submission (the first paper trade brief's E1b, FT-11,
//! [DEC-853](../../../docs/project/decisions/DEC-853.md) items 5 and 6): the shell keeps the
//! cycle's session, wakes every poll interval to read the entry back by its client order id and
//! tick the executor, and at the close-window bound hands the executor `CancelOpenings` for the
//! deployment's agent and instrument. The watch never sends a request of its own but that read:
//! every cancel is the executor's, journaled before it leaves (rule 5, rule 12, FT-1).

use std::time::Duration;

use mandate_executor::ExecutorConfig;
use mandate_risk::GateConfig;
use mandate_time::UtcNanos;

use crate::error::Cause;

/// What the watch needs from outside the cycle: the process's clock and timer, the bound and the
/// poll interval (DEC-853 items 5 and 6). The shell reads no wall clock of its own (ES-21).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watch<P> {
    pub pause: P,
    pub bound: UtcNanos,
    pub interval: Duration,
}

/// DEC-853 item 5: the start of the close window of the regular session `now` is in, that
/// session's end on `mandate_time::ExchangeCalendar` less `close_window_minutes` minutes.
///
/// # Errors
/// [`Cause::Absent`] when `now` is not inside a regular session before its close window (FT-8).
pub fn close_window_bound(now: UtcNanos, gate: &GateConfig) -> Result<UtcNanos, Cause> {
    let _ = (now, gate);
    Err(Cause::Unimplemented { story: "E7-19" })
}

/// DEC-853 item 6: the least of `exit_step_s`, `unknown_absent_window_s` divided by
/// `unknown_absent_lookups` (whole seconds, rounded down, at least 1),
/// `bracket_partial_fill_timeout_s` and `max_unprotected_s`.
///
/// # Errors
/// [`Cause::Absent`] when a member is not a positive number of seconds.
pub fn poll_interval(executor: &ExecutorConfig) -> Result<Duration, Cause> {
    let _ = executor;
    Err(Cause::Unimplemented { story: "E7-19" })
}
