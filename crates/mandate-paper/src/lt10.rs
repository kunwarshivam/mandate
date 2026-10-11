//! The live watch's end (E7-26, the [first live trade
//! brief](../../../docs/project/tasks/first-live-trade.md)'s G1b; LT-10): the run stays until
//! the entry is terminal and any filled quantity is covered by the profile's strongest
//! protection, or the unprotected interval is journaled and its alert key is in the run's
//! report — and the watch's reads never pass the session's end, so the run fails closed rather
//! than reading unboundedly ([DEC-912](../../../docs/project/decisions/DEC-912.md) item 3;
//! DEC-853 item 5's cap). The reads themselves are the shell's watch (DEC-853 items 5 and 6);
//! this seam answers the one question each wake asks: may the run end?

use mandate_time::UtcNanos;

/// What the run's reads last showed of the entry and its protection (LT-10): the terminal state
/// of the entry the run submitted, whether a filled quantity is left with no protection resting
/// over it, and the alert key of the unprotected interval the run's report holds, when it holds
/// one (`unprotected_interval_limit`, notifications spec §4.2's `protection` row; the interval's
/// journaling is the executor's, `max_unprotected_s`-bounded, trading-domain spec §5.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watched {
    /// Whether the broker reported the entry terminal.
    pub entry_terminal: bool,
    /// Whether a filled quantity is left with no protection resting over it — [`false`] when
    /// nothing filled or the profile's stop covers the whole of it.
    pub uncovered: bool,
    /// The unprotected interval's alert key, when the run's report holds it.
    pub alerted: Option<&'static str>,
}

/// Why the watch keeps reading or fails closed ([DEC-912](../../../docs/project/decisions/DEC-912.md)
/// item 3): each refusal is a stop after which no further read happens, each with its own reason
/// code (ADR-0001 ES-09).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Lt10Refusal {
    /// The body of every stub in the tests PR (DEC-77).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    /// LT-10 does not hold: the entry is still working, or a filled quantity is uncovered with
    /// no alerted interval to show for it. The run keeps reading, inside its bounds.
    #[error("the run cannot end: the entry or its protection is unsettled")]
    Unsettled,
    /// The watch reached the cap — the session's end — with LT-10 not holding: fail closed,
    /// never one more read.
    #[error("the watch reached the session's cap with the entry unsettled")]
    Capped,
}

impl Lt10Refusal {
    /// The stable reason code of each refusal (ADR-0001 ES-09): `lt10_unsettled`,
    /// `watch_cap_reached`, and the stub's `unimplemented`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unimplemented { .. } => "unimplemented",
            Self::Unsettled => "lt10_unsettled",
            Self::Capped => "watch_cap_reached",
        }
    }
}

/// LT-10's first and only exit question ([DEC-912](../../../docs/project/decisions/DEC-912.md)
/// item 3): may the run end at `now`? It may when [`Watched`] shows the entry terminal and any
/// filled quantity covered — nothing filled, or the profile's strongest protection resting over
/// the whole of it — and also when a filled quantity is uncovered but the run's report already
/// holds the unprotected interval's alert key (LT-10's halves; the key is
/// `unprotected_interval_limit` alone, notifications spec §4.2's `protection` row — the row's
/// other key, `stop_watchdog`, is another alarm's and still shows nothing for this interval,
/// so it holds the run open). It may not while the entry is still working or the filled
/// quantity is uncovered and unalerted, and past `cap` — the
/// session's end, the bound the paper watch already fails at — an unsettled watch fails closed
/// ([`Lt10Refusal::Capped`]) and never reads again, so no implementation of this seam can loop
/// unboundedly. A settled watch ends clean at or past the cap, needing no further read.
///
/// # Errors
/// [`Lt10Refusal::Unsettled`] while the run keeps reading inside its bounds, or
/// [`Lt10Refusal::Capped`] past them.
pub fn lt10(watched: &Watched, now: UtcNanos, cap: UtcNanos) -> Result<(), Lt10Refusal> {
    let interval_reported = matches!(watched.alerted, Some("unprotected_interval_limit"));
    let settled = watched.entry_terminal && (!watched.uncovered || interval_reported);
    if settled {
        return Ok(());
    }
    if now >= cap {
        return Err(Lt10Refusal::Capped);
    }
    Err(Lt10Refusal::Unsettled)
}
