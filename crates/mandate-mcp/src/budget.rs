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
#[expect(
    dead_code,
    reason = "read by E7-16's implementation, which replaces the stubs below"
)]
struct TokenBucket {
    config: BucketConfig,
    tokens: u32,
    anchor: Duration,
}

/// The two buckets of one connection, on a monotonic clock the caller supplies as the time since
/// any fixed origin.
#[derive(Debug, Clone)]
#[expect(
    dead_code,
    reason = "read by E7-16's implementation, which replaces the stubs below"
)]
pub struct RateBudget {
    ordinary: TokenBucket,
    reserved: TokenBucket,
}

impl RateBudget {
    /// Both buckets start full. A zero capacity or refill period is [`McpError::BadConfig`].
    pub fn new(config: BudgetConfig, now: Duration) -> Result<Self, McpError> {
        let _ = (config, now);
        Err(McpError::Unimplemented { story: "E7-16" })
    }

    /// Takes one token for a call of `class`, or [`McpError::Throttled`] and nothing is sent.
    pub fn try_acquire(&mut self, class: CallClass, now: Duration) -> Result<(), McpError> {
        let _ = (class, now);
        Err(McpError::Unimplemented { story: "E7-16" })
    }
}
