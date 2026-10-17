//! The run's clock (DEC-962 item 1): the system clock in every build but the rehearsal's, and in
//! the `rh-sim` build the clock [`SIM_CLOCK_VAR`] sets, so the rehearsal against `mandate-rh-sim`
//! can be made on a day the market is shut (R0's R12) with every session, age, skew and
//! cross-check rule applied unchanged to the instant it names.
//!
//! [`RunClock`] is a type alias chosen by `cfg(feature = "rh-sim")`, as DEC-961's market source
//! is: [`SystemClock`] in the default and `live` builds, which never read the variable, and
//! [`SimClock`] in the rehearsal's. [`SimClock`], [`SIM_CLOCK_VAR`] and [`sim_clock`] are compiled
//! only with `rh-sim` and in the crate's own unit tests, so a build without `rh-sim` holds
//! neither the variable's name nor the override (`tests/sim_clock.rs`).

#[cfg(any(test, feature = "rh-sim"))]
use std::time::Duration;

#[cfg(any(test, feature = "rh-sim"))]
use mandate_alpaca::Pause;
#[cfg(any(test, feature = "rh-sim"))]
use mandate_time::UtcNanos;

use crate::{PaperError, SystemClock};

/// The variable the rehearsal build reads its run clock from: an RFC 3339 instant, the run
/// clock's reading when the process starts (DEC-962 item 1).
#[cfg(any(test, feature = "rh-sim"))]
pub const SIM_CLOCK_VAR: &str = "MANDATE_RH_SIM_CLOCK";

/// This build's run clock (DEC-962 item 1): the clock the rehearsal sets, in the `rh-sim` build.
#[cfg(feature = "rh-sim")]
pub type RunClock = SimClock;

/// This build's run clock (DEC-962 item 1): the system clock in every build without `rh-sim`,
/// the `live` one included.
#[cfg(not(feature = "rh-sim"))]
pub type RunClock = SystemClock;

/// A run clock as the process reads it from its environment (DEC-962 item 1).
pub trait ReadClock: Sized {
    /// The clock `vars` give this build.
    ///
    /// # Errors
    /// [`PaperError::SimClock`] in the rehearsal build, for a clock variable that is set and is
    /// not an RFC 3339 instant.
    fn read(vars: &[(String, String)]) -> Result<Self, PaperError>;
}

impl ReadClock for SystemClock {
    /// The system clock, whatever `vars` holds.
    fn read(_vars: &[(String, String)]) -> Result<Self, PaperError> {
        Ok(SystemClock)
    }
}

#[cfg(any(test, feature = "rh-sim"))]
impl ReadClock for SimClock {
    /// [`sim_clock`] over the system clock.
    fn read(vars: &[(String, String)]) -> Result<Self, PaperError> {
        sim_clock(vars, SystemClock)
    }
}

/// The run clock the process reads from its environment `vars`: this build's [`RunClock`]'s
/// [`ReadClock::read`].
///
/// # Errors
/// [`PaperError::SimClock`] in the rehearsal build, for a clock variable that is set and is not an
/// RFC 3339 instant.
pub fn run_clock(vars: &[(String, String)]) -> Result<RunClock, PaperError> {
    RunClock::read(vars)
}

/// The rehearsal's run clock: [`SIM_CLOCK_VAR`]'s instant when it was made, moved from then on
/// exactly as `base` moves, so the run's own elapsed time, its pauses and its timeouts are real.
/// Its pause is `base`'s.
#[cfg(any(test, feature = "rh-sim"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimClock<B = SystemClock> {
    base: B,
    start: UtcNanos,
    base_at_start: UtcNanos,
}

#[cfg(any(test, feature = "rh-sim"))]
impl<B: Pause> SimClock<B> {
    /// The clock that reads `start` now and moves with `base` from here.
    pub fn starting_at(start: UtcNanos, base: B) -> Self {
        let base_at_start = base.now();
        Self {
            base,
            start,
            base_at_start,
        }
    }
}

#[cfg(any(test, feature = "rh-sim"))]
impl<B: Pause> Pause for SimClock<B> {
    /// `start` moved by the base's time since the clock was made. An instant past the range
    /// [`UtcNanos`] holds, which no rehearsal reaches, reads as `start`.
    fn now(&self) -> UtcNanos {
        nanos(self.base.now())
            .zip(nanos(self.base_at_start))
            .and_then(|(now, then)| now.checked_sub(then))
            .zip(nanos(self.start))
            .and_then(|(elapsed, start)| start.checked_add(elapsed))
            .and_then(instant)
            .unwrap_or(self.start)
    }

    fn pause(&self, duration: Duration) -> impl Future<Output = ()> {
        self.base.pause(duration)
    }
}

/// The run clock `vars` set over `base` (DEC-962 item 1): [`SIM_CLOCK_VAR`]'s instant, moving with
/// `base`, or `base` itself when the variable is unset.
///
/// # Errors
/// [`PaperError::SimClock`] for a value that is not an RFC 3339 instant; the run never falls back
/// to `base` for a clock it was given and cannot read.
#[cfg(any(test, feature = "rh-sim"))]
pub fn sim_clock<B: Pause>(vars: &[(String, String)], base: B) -> Result<SimClock<B>, PaperError> {
    let given = vars
        .iter()
        .find(|(name, _)| name == SIM_CLOCK_VAR)
        .map(|(_, value)| UtcNanos::parse_rfc3339(value).map_err(|_| PaperError::SimClock))
        .transpose()?;
    let start = given.unwrap_or_else(|| base.now());
    Ok(SimClock::starting_at(start, base))
}

/// `at` as nanoseconds since the epoch.
#[cfg(any(test, feature = "rh-sim"))]
fn nanos(at: UtcNanos) -> Option<i128> {
    i128::from(at.secs())
        .checked_mul(NANOS_PER_SEC)?
        .checked_add(i128::from(at.nanos()))
}

/// Nanoseconds since the epoch as an instant, if [`UtcNanos`] holds it.
#[cfg(any(test, feature = "rh-sim"))]
fn instant(total: i128) -> Option<UtcNanos> {
    let secs = i64::try_from(total.checked_div_euclid(NANOS_PER_SEC)?).ok()?;
    let nanos = u32::try_from(total.checked_rem_euclid(NANOS_PER_SEC)?).ok()?;
    UtcNanos::from_parts(secs, nanos).ok()
}

#[cfg(any(test, feature = "rh-sim"))]
const NANOS_PER_SEC: i128 = 1_000_000_000;

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    reason = "test fixtures: a fixture that does not build fails the test, and the oracle's own nanosecond sums are small"
)]
mod tests;
