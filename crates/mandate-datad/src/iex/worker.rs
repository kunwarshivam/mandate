//! The IEX source's worker loop (DEC-2283): it reads the control stream from where it stopped every
//! [`REFRESH_S`], makes one [`Pass`] at each turn, and tells the probe loop its [`Progress`], until
//! told to stop or the platform key is refused. Every way out of the loop, a panic among them,
//! marks the worker stopped, so the probe loop never waits on a worker that is gone. The binary
//! hands it the journal read, the clock and the pause, so it runs here over fakes as it runs there
//! over Postgres, the host clock and a one-second sleep.

use std::sync::atomic::{AtomicBool, Ordering};

use mandate_journal::{ArtifactSource, StoredEvent, TrustedStart};
use mandate_time::{ExchangeCalendar, UtcNanos};

use crate::iex::generation::BarsSource;
use crate::iex::pass::Pass;
use crate::iex::progress::Progress;
use crate::iex::registrations::Registrations;

/// How often the control stream is read again, in seconds.
pub const REFRESH_S: i64 = 60;

/// The control stream's rows from `start` on, chained to it, or a code.
pub trait ControlRows {
    /// # Errors
    /// A code naming why the rows could not be read.
    fn rows_from(&self, start: TrustedStart) -> Result<Vec<StoredEvent>, &'static str>;
}

/// What the worker works with.
pub struct Parts<'a> {
    pub source: &'a dyn BarsSource,
    pub control: &'a dyn ControlRows,
    pub artifacts: &'a dyn ArtifactSource,
    pub calendar: &'a ExchangeCalendar,
    pub pass: Pass,
}

/// Marks the worker stopped however the loop is left.
struct Stopper<'a>(&'a Progress);

impl Drop for Stopper<'_> {
    fn drop(&mut self) {
        self.0.stopped();
    }
}

/// Runs the worker until `terminate` or a refused key, `now` its clock, `pause` between turns,
/// `log` each line it says.
pub fn run(
    mut parts: Parts<'_>,
    terminate: &AtomicBool,
    progress: &Progress,
    now: &dyn Fn() -> UtcNanos,
    pause: &dyn Fn(),
    log: &mut dyn FnMut(&str),
) {
    let _stopper = Stopper(progress);
    let mut kept = Registrations::default();
    let mut read_at: Option<i64> = None;
    let mut held = None;
    while !terminate.load(Ordering::SeqCst) {
        let at = now();
        let due = read_at.is_none_or(|read| at.secs().saturating_sub(read) >= REFRESH_S);
        if due {
            match parts.control.rows_from(kept.next_start()) {
                Ok(rows) => {
                    kept.extend(rows);
                    held = Some(kept.registered(parts.artifacts));
                }
                Err(code) => log(&format!("registrations_not_read: {code}")),
            }
            read_at = Some(at.secs());
        }
        if let Some(registered) = &held {
            let tick = parts
                .pass
                .step(parts.source, parts.calendar, registered, at);
            for line in &tick.log {
                log(line);
            }
            if tick.stop {
                return;
            }
            progress.passed(at);
        }
        pause();
    }
}
