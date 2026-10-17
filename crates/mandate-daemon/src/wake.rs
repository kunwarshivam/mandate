//! The wake-up hint (DEC-17, ADR-0001 ES-20): Postgres `LISTEN`/`NOTIFY`, or anything else that
//! says "the journal may have moved". A hint only ends a wait early. The loop reads the journal on
//! every pass whether or not one came, so a lost, late, or repeated hint costs latency and never
//! correctness (`mandate_runtime`'s "no transport payload is ever an input").

use std::thread;
use std::time::Duration;

/// Waits for at most `bound`, returning early when a hint arrives.
pub trait Wake {
    /// Whether a hint ended the wait before `bound`.
    fn wait(&mut self, bound: Duration) -> bool;
}

/// No hint ever comes: every wait runs its whole bound. What the loop does with no notification
/// channel at all, and what a test uses to show the hints decide nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct Sleep;

impl Wake for Sleep {
    fn wait(&mut self, bound: Duration) -> bool {
        thread::sleep(bound);
        false
    }
}
