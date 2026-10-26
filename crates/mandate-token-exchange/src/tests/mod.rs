//! Tests of the token-exchange process, grouped by the module they test, with no network. A test
//! marked `#[ignore = "pending <story>"]` fails at that story's stub until the story's code lands.

use std::time::Duration;

use mandate_connections::exchange::CodeWait;
use mandate_time::UtcNanos;

mod form;
mod process;
mod response;
mod run;
mod vault;

#[cfg(debug_assertions)]
mod cell;
#[cfg(debug_assertions)]
mod http;
#[cfg(debug_assertions)]
mod startup;

/// A wait whose code is already there: its clock stands at the start, and a pause would mean the
/// run did not find the code at once, which these tests never expect.
pub struct Waitless;

/// The start every [`Waitless`] run reads.
pub fn started() -> UtcNanos {
    UtcNanos::from_parts(1_790_000_000, 0).unwrap()
}

/// A deadline one lifetime after [`started`], as a fresh request's.
pub fn deadline() -> UtcNanos {
    UtcNanos::from_parts(1_790_000_600, 0).unwrap()
}

impl CodeWait for Waitless {
    fn now(&mut self) -> UtcNanos {
        started()
    }

    fn pause(&mut self, _pause: Duration) {
        panic!("a run whose code is waiting never pauses");
    }
}

/// A wait whose clock moves only by its pauses, from [`started`], in whole seconds.
pub struct Ticking(pub i64);

impl CodeWait for Ticking {
    fn now(&mut self) -> UtcNanos {
        UtcNanos::from_parts(self.0, 0).unwrap()
    }

    fn pause(&mut self, pause: Duration) {
        self.0 += i64::try_from(pause.as_secs()).unwrap();
    }
}
