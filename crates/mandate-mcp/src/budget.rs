//! The per-connection call budget (connections spec §6.5): an ordinary token bucket for every
//! call, and a reserved one that only risk-reducing calls draw on, so no run of reads can leave
//! a cancel, an exit, or a kill-switch order without a token (`AGENTS.md` rule 13).

use std::time::Duration;

use crate::error::McpError;

/// Which budget a call may draw on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallClass {
    /// Reads and openings: the ordinary bucket only.
    Ordinary,
    /// Cancels, exits, protective orders, and kill-switch orders: the reserved bucket first, then
    /// the ordinary one.
    RiskReducing,
}

/// One bucket: up to `capacity` tokens, one more every `refill_every` while below capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BucketConfig {
    pub capacity: u32,
    pub refill_every: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetConfig {
    pub ordinary: BucketConfig,
    pub reserved: BucketConfig,
}

#[derive(Debug, Clone)]
struct TokenBucket {
    config: BucketConfig,
    tokens: u32,
    anchor: Duration,
}

impl TokenBucket {
    fn new(config: BucketConfig, now: Duration) -> Result<Self, McpError> {
        if config.capacity == 0 || config.refill_every.is_zero() {
            return Err(McpError::BadConfig);
        }
        Ok(Self {
            config,
            tokens: config.capacity,
            anchor: now,
        })
    }

    fn try_take(&mut self, now: Duration) -> bool {
        self.refill(now);
        match self.tokens.checked_sub(1) {
            Some(left) => {
                self.tokens = left;
                true
            }
            None => false,
        }
    }

    /// Adds the whole refill periods since the anchor, and restarts the period once full. A
    /// clock that went backwards adds nothing.
    fn refill(&mut self, now: Duration) {
        let periods = now
            .saturating_sub(self.anchor)
            .as_nanos()
            .checked_div(self.config.refill_every.as_nanos())
            .unwrap_or(0);
        let missing = self.config.capacity.saturating_sub(self.tokens);
        let added = u32::try_from(periods).ok().filter(|added| *added < missing);
        let advanced = added.and_then(|added| {
            self.config
                .refill_every
                .checked_mul(added)
                .and_then(|span| self.anchor.checked_add(span))
                .map(|anchor| (added, anchor))
        });
        match advanced {
            Some((added, anchor)) => {
                self.tokens = self.tokens.saturating_add(added);
                self.anchor = anchor;
            }
            None => {
                self.tokens = self.config.capacity;
                self.anchor = now;
            }
        }
    }
}

/// The two buckets of one connection, on a monotonic clock the caller supplies as the time since
/// any fixed origin.
#[derive(Debug, Clone)]
pub struct RateBudget {
    ordinary: TokenBucket,
    reserved: TokenBucket,
}

impl RateBudget {
    /// Both buckets start full. A zero capacity or refill period is [`McpError::BadConfig`].
    pub fn new(config: BudgetConfig, now: Duration) -> Result<Self, McpError> {
        Ok(Self {
            ordinary: TokenBucket::new(config.ordinary, now)?,
            reserved: TokenBucket::new(config.reserved, now)?,
        })
    }

    /// Takes one token for a call of `class`, or [`McpError::Throttled`] and nothing is sent.
    pub fn try_acquire(&mut self, class: CallClass, now: Duration) -> Result<(), McpError> {
        let admitted = match class {
            CallClass::Ordinary => self.ordinary.try_take(now),
            CallClass::RiskReducing => self.reserved.try_take(now) || self.ordinary.try_take(now),
        };
        if admitted {
            Ok(())
        } else {
            Err(McpError::Throttled)
        }
    }
}
