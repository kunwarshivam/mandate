//! The wall clock, read only at the server's edge (ES-05). Session timers run on it; everything
//! the API reports about the journal comes from the journal's own times.

use mandate_time::UtcNanos;

/// Where the server reads the time; tests hand in their own.
pub trait Clock: Send + Sync {
    /// Now, or `None` when the clock cannot be read; a request then authenticates nothing.
    fn now(&self) -> Option<UtcNanos>;
}

/// The host's clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    #[allow(
        clippy::disallowed_methods,
        reason = "the server's edge reads the wall clock for session timers only; the journal's times are its own (ES-05)"
    )]
    fn now(&self) -> Option<UtcNanos> {
        let since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?;
        let secs = i64::try_from(since.as_secs()).ok()?;
        UtcNanos::from_parts(secs, since.subsec_nanos()).ok()
    }
}
