//! The rehearsal's owner clock (DEC-962 item 2): in the CLI built with `rh-sim`, which only the
//! R0 rehearsal's build turns on, the moment the owner ran a command is [`SIM_CLOCK_VAR`]'s
//! instant when it is set, so the owner's gestures, codes and grants fall on the same run clock as
//! the rehearsal runner's and every age and deadline rule applies to them unchanged. The founder's
//! CLI is built with no feature and holds neither the variable nor this module
//! (`tests/sim_clock.rs`).

use mandate_time::UtcNanos;

use crate::control::Now;

/// The variable the rehearsal sets for every process it starts: an RFC 3339 instant.
pub const SIM_CLOCK_VAR: &str = "MANDATE_RH_SIM_CLOCK";

/// The variable is set and is not an RFC 3339 instant; the command refuses, and never falls back
/// to the system clock for a clock it was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimClockError;

impl std::fmt::Display for SimClockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the rehearsal's owner clock is not an RFC 3339 instant")
    }
}

impl std::error::Error for SimClockError {}

/// The moment the owner ran the command: `value`'s instant and its second, or `system` when the
/// variable is unset.
///
/// # Errors
/// [`SimClockError`] for a value that is not an RFC 3339 instant.
pub fn owner_now(value: Option<&str>, system: Now) -> Result<Now, SimClockError> {
    let Some(value) = value else {
        return Ok(system);
    };
    let at = UtcNanos::parse_rfc3339(value).map_err(|_| SimClockError)?;
    Ok(Now {
        at,
        secs: at.secs(),
    })
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "test fixtures: an instant that does not parse fails the test"
)]
mod tests;
