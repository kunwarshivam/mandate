//! One stream as far as the process read it (DEC-2847, DEC-2854): what its records folded to, and
//! the seq and hash of the last one, so the next read asks only for what follows it. A read the
//! store refuses past the head, or one a check makes due, is read from the first record, which the
//! store verifies whole and which must still hold the head; what follows the head is folded, and
//! nothing before it is folded again.

use mandate_canon::Digest;
use mandate_journal::{StoredEvent, StreamId, TrustedStart};
use mandate_time::UtcNanos;

/// How often, in seconds of the process's injected time, its tails read their streams from the
/// first record (DEC-2854 item 3), as the API's cache does (DEC-2145) and the runtime does
/// (DEC-2852 item 6).
pub const REVERIFY_S: i64 = 60;

#[derive(Debug, Default)]
pub struct Tail<S> {
    head: Option<(u64, Digest)>,
    pub state: S,
    /// Whether the next read reads from the first record.
    due: bool,
    /// Whether the last read was a due one that the store refused, so the stream has not been
    /// verified again since a check fell due.
    refused: bool,
    /// Whether a read from the first record no longer held the head this tail was read to: the
    /// history folded is no longer the stream's.
    broken: bool,
}

impl<S> Tail<S> {
    pub fn new(state: S) -> Self {
        Self {
            head: None,
            state,
            due: false,
            refused: false,
            broken: false,
        }
    }

    /// Makes the next read read from the first record.
    pub fn check(&mut self) {
        self.due = true;
    }

    pub fn refused(&self) -> bool {
        self.refused
    }

    pub fn broken(&self) -> bool {
        self.broken
    }

    /// Folds into the state, in order with `fold`, each record of `stream` that `rows` holds past
    /// the head; `false` when nothing reads, or the stream no longer holds the head, and then the
    /// state must not be read (`AGENTS.md` rule 3). A tail whose stream no longer held its head
    /// asks the store nothing more, so a port read at every ask never reads a whole stream each
    /// time.
    pub fn follow(
        &mut self,
        rows: impl Fn(&StreamId, TrustedStart) -> Option<Vec<StoredEvent>>,
        stream: &StreamId,
        mut fold: impl FnMut(&mut S, &StoredEvent),
    ) -> bool {
        if self.broken {
            return false;
        }
        let since = match self.head {
            None => self.whole(&rows, stream, None),
            Some((seq, hash)) => {
                let past = if self.due {
                    None
                } else {
                    rows(
                        stream,
                        TrustedStart {
                            from_seq: seq.saturating_add(1),
                            prev_hash: hash,
                        },
                    )
                };
                past.or_else(|| self.whole(&rows, stream, Some((seq, hash))))
            }
        };
        let Some(since) = since else {
            return false;
        };
        for row in &since {
            fold(&mut self.state, row);
            self.head = Some((row.seq, row.hash));
        }
        true
    }

    /// The records past `head` of `stream` read from its first record, when the read holds `head`.
    fn whole(
        &mut self,
        rows: &impl Fn(&StreamId, TrustedStart) -> Option<Vec<StoredEvent>>,
        stream: &StreamId,
        head: Option<(u64, Digest)>,
    ) -> Option<Vec<StoredEvent>> {
        let Some(all) = rows(stream, TrustedStart::GENESIS) else {
            self.refused = self.due;
            return None;
        };
        let after = match head {
            None => 0,
            Some((seq, hash)) if all.iter().any(|row| row.seq == seq && row.hash == hash) => seq,
            Some(_) => {
                self.broken = true;
                return None;
            }
        };
        self.due = false;
        self.refused = false;
        Some(all.into_iter().filter(|row| row.seq > after).collect())
    }
}

/// When the tails were last made due to read from the first record.
#[derive(Debug, Default)]
pub struct Checks {
    verified_at: Option<i64>,
}

impl Checks {
    /// Whether a check falls due at `now`: once [`REVERIFY_S`] has passed since the last one, or
    /// when time has gone back past it, so no step back defers a check.
    pub fn due(&mut self, now: UtcNanos) -> bool {
        let at = *self.verified_at.get_or_insert(now.secs());
        let due = now.secs() < at || now.secs() >= at.saturating_add(REVERIFY_S);
        if due {
            self.verified_at = Some(now.secs());
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use mandate_time::TimeError;

    use super::*;

    fn at(secs: i64) -> Result<UtcNanos, TimeError> {
        UtcNanos::from_parts(secs, 0)
    }

    /// DEC-2854 item 3: a check falls due [`REVERIFY_S`] after the last, and at once on any time
    /// earlier than the last's, from which the next is counted.
    #[test]
    fn a_check_falls_due_a_minute_on_or_at_once_when_time_goes_back() -> Result<(), TimeError> {
        let mut checks = Checks::default();
        let due: Vec<bool> = [1_000, 1_059, 1_060, 1_061, 1_119, 1_030, 1_031, 1_090]
            .into_iter()
            .map(|secs| Ok(checks.due(at(secs)?)))
            .collect::<Result<_, TimeError>>()?;
        assert_eq!(due, [false, false, true, false, false, true, false, true]);
        Ok(())
    }
}
