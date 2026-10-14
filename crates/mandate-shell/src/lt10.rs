//! The shell watch's own end question (E7-26's shell-side row; the [first live trade
//! brief](../../../docs/project/tasks/first-live-trade.md)'s LT-10, its G1b row's
//! **bounded reads until LT-10**, the founder's run's step 5: "the run stays until LT-10
//! holds"; [DEC-912](../../../docs/project/decisions/DEC-912.md) item 3;
//! [DEC-858](../../../docs/project/decisions/DEC-858.md) items 4 and 5;
//! [DEC-853](../../../docs/project/decisions/DEC-853.md) item 5;
//! [DEC-877](../../../docs/project/decisions/DEC-877.md) item 1).
//!
//! The shell's watch ended, before this row, on the bare terminal read of its `TERMINAL_STATUSES`
//! check, and a terminal entry with its protection not yet resting over the filled quantity is
//! exactly the unprotected interval LT-10 exists for (DEC-912's rejected alternative: "End the
//! watch once the entry is terminal, as the paper watch does"). This seam is the per-wake
//! question that replaces that read **inside the shell's own watch loop**: the
//! cycle's `Session` is private to this crate (DEC-475 — its assembled stages and setup stay
//! private, "callers can supply model opinions but cannot replace the builder, gate, journal or
//! executor"), so the install is this crate's row and no caller's, and the question is asked
//! by the watch (`crate::tracer`'s `Session::watch`), never answered from outside it. Like
//! [`close_window_bound`](crate::close_window_bound) and
//! [`poll_interval`](crate::poll_interval) it is a pure function of the facts the run already
//! holds: a broker read, the executor's answer, the report's own alert keys and two clock
//! instants. Nothing here reaches a host, an order or a credential (`AGENTS.md` rules 7 and 8).

use mandate_time::UtcNanos;

use crate::error::{ShellError, refused};
use crate::stages::Stage;
use crate::watch::session_end;

/// The watch's per-wake LT-10 question (DEC-912 item 3): may the run end at `now`?
///
/// The watch hands this the facts one wake reads — `read`, the entry's last broker read; `uncovered`,
/// the executor's answer for whether a filled quantity is left with no protection resting over
/// the whole of it; `alerts`, the alert keys the run's own report holds — plus the wake's clock
/// second and `bound`, the close-window bound the watch already holds. The cap is read here off
/// the same calendar the bound read (`session_end` over the `ExchangeCalendar`, DEC-877 item 1's
/// instant: the end of the regular session `bound` is in — 16:00 New York on a full day, the
/// early close on a short one), and a bound no regular session holds fails the watch closed, as
/// the bound's own read already does (FT-8, `AGENTS.md` rule 3): the run refuses rather than
/// read on with no cap it can place.
///
/// The answers, each a whole state of the run:
///
/// - `Ok(true)`: the run may end (LT-10). The entry is terminal in DEC-858 item 4's exact four —
///   `filled`, `canceled`, `expired`, `rejected` — and any filled quantity is covered: nothing
///   filled (flat), or the profile's strongest protection resting over the whole of it; or a
///   filled quantity is uncovered and the run's report holds the unprotected interval's own
///   alert key, `unprotected_interval_limit` (notifications spec §4.2's `protection` row), the
///   interval's journaling itself staying the executor's (`ProtectionChanged`, bounded by
///   `max_unprotected_s`, trading-domain spec §5.4).
/// - `Ok(false)`: the run keeps reading, inside its bounds. Until either half holds it keeps
///   reading — the entry still working, a read absent or one the executor's codec names no
///   terminal for, or a filled quantity uncovered without the reported interval. Only the
///   interval's own key releases an uncovered run: `stop_watchdog`, the `protection` row's
///   other key, is another alarm's and shows nothing for this interval.
/// - [`ShellError::CancelUnconfirmed`]: the cap, inclusive — `now` at or past the session's end
///   with LT-10 not holding (DEC-858 item 5, DEC-853 item 5, DEC-877 item 1). The watch never
///   reads again and sends nothing at or after the session's end; the run ends failing closed
///   with the shell's existing `cancel_unconfirmed` refusal, the entry's last state staying
///   the executor's journaled `pending_cancel` for the next start's reconciliation. The
///   `Ok(true)` above is answered first, so a watch whose LT-10 holds ends clean at or past
///   the cap, needing no further read — a settled run ends before the cap ever refuses it.
///
/// # The install
///
/// `crate::tracer`'s `Session::watch` asks this at every wake, in place of the bare terminal
/// read it kept before this row (DEC-914 item 7: the call site is this implementation row's to
/// write and its review's to judge): once over the facts the run already holds, before the wake
/// sends anything, which stops a capped watch before it reads there (DEC-877 item 1: "The watch
/// stops at the first wake at or past that instant, before any read there"), and once over the
/// wake's own fresh read and the executor's protection answer after the wake has handed both to
/// the executor, so a read that settles LT-10 ends the run at its own wake, never the next one.
///
/// # Errors
///
/// [`ShellError::Refused`] with [`crate::error::Cause::Absent`] for a bound no regular session holds (FT-8),
/// and [`ShellError::CancelUnconfirmed`] at or past the cap with LT-10 not holding.
pub fn watch_may_end(
    read: Option<&str>,
    uncovered: bool,
    alerts: &[&'static str],
    now: UtcNanos,
    bound: UtcNanos,
) -> Result<bool, ShellError> {
    let cap = session_end(bound).map_err(refused(Stage::Executor))?;
    let terminal = matches!(read, Some("filled" | "canceled" | "expired" | "rejected"));
    let settled = terminal && (!uncovered || alerts.contains(&"unprotected_interval_limit"));
    if settled {
        return Ok(true);
    }
    if now >= cap {
        return Err(ShellError::CancelUnconfirmed);
    }
    Ok(false)
}
