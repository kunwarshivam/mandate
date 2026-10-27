//! One pass of the data process's IEX source (data-plane spec §3.8, DEC-2283), at the clock the
//! binary hands in: the daily generation due, its closes objects, the minute generation due, and
//! pruning once a minute. What it reports is counts and codes only, never a symbol or a price
//! (DP-17), so the binary logs it as it is; a refused platform key asks the binary to stop.

use mandate_time::{ExchangeCalendar, UtcNanos};

use crate::iex::generation::{BarsSource, Writer};
use crate::iex::instruments::Registered;
use crate::iex::schedule::{Jitter, daily_target, minute_target};

const MINUTE_S: i64 = 60;

/// What one pass did, for the binary to log and act on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tick {
    /// Lines to log: each a code and, where it has one, a count or an artifact reference.
    pub log: Vec<String>,
    /// Whether the source refused the platform key, so the process must stop.
    pub stop: bool,
}

/// The data process's IEX source across passes.
pub struct Pass {
    writer: Writer,
    jitter: Jitter,
    pruned: Option<i64>,
    said: Option<&'static str>,
}

impl Pass {
    pub fn new(writer: Writer, jitter: Jitter) -> Self {
        Self {
            writer,
            jitter,
            pruned: None,
            said: None,
        }
    }

    /// One pass over `registered`'s instruments at `now`.
    pub fn step(
        &mut self,
        source: &dyn BarsSource,
        calendar: &ExchangeCalendar,
        registered: &Registered,
        now: UtcNanos,
    ) -> Tick {
        let mut tick = Tick::default();
        let instruments = match registered {
            Registered::Instruments(instruments) => instruments,
            Registered::NotPaper => {
                self.once(&mut tick, "not_paper");
                return tick;
            }
            Registered::Unreadable => {
                self.once(&mut tick, "snapshots_unreadable");
                return tick;
            }
        };
        self.said = None;
        match daily_target(calendar, now, self.jitter) {
            Ok(Some(session)) => {
                match self
                    .writer
                    .daily(source, calendar, session, instruments, now)
                {
                    Ok(daily) => {
                        count(&mut tick, "daily_written", daily.written.len());
                        count(&mut tick, "daily_waiting", daily.waiting.len());
                        count(&mut tick, "daily_refused", daily.refused.len());
                    }
                    Err(error) => {
                        tick.log
                            .push(format!("daily_not_written: {}", error.code()));
                        tick.stop |= error.key_refused();
                    }
                }
                for (_, reference) in self.writer.closes(session, instruments, now) {
                    tick.log.push(format!("closes_stored {reference}"));
                }
            }
            Ok(None) => {}
            Err(_) => tick.log.push("daily_not_named: time".to_owned()),
        }
        match minute_target(calendar, now, self.jitter) {
            Ok(Some(minute)) => match self.writer.minute(source, minute, instruments, now) {
                Ok(written) => {
                    count(&mut tick, "minute_written", written.written.len());
                    count(&mut tick, "minute_refused", written.refused.len());
                }
                Err(error) => {
                    tick.log
                        .push(format!("minute_not_written: {}", error.code()));
                    tick.stop |= error.key_refused();
                }
            },
            Ok(None) => {}
            Err(_) => tick.log.push("minute_not_named: time".to_owned()),
        }
        let minute = now.secs().div_euclid(MINUTE_S);
        if self.pruned != Some(minute) {
            self.pruned = Some(minute);
            if let Err(error) = self.writer.prune(calendar, now) {
                tick.log.push(format!("not_pruned: {}", error.code()));
            }
        }
        tick
    }

    /// Logs `code` once while it holds.
    fn once(&mut self, tick: &mut Tick, code: &'static str) {
        if self.said != Some(code) {
            self.said = Some(code);
            tick.log.push(code.to_owned());
        }
    }
}

fn count(tick: &mut Tick, code: &str, n: usize) {
    if n > 0 {
        tick.log.push(format!("{code} {n}"));
    }
}
