//! Pacing requests to the data host's rate limit. Each response's `X-Ratelimit-*` headers say how
//! many requests the account has left and when that budget is whole again; the pacer spends it
//! down to a low-water mark, then waits for the reset. Without usable headers a token bucket holds
//! every span of one window to the configured limit. A 429 closes the budget until the last known
//! reset, or for a full window when none is known. Times are integer nanoseconds since the Unix
//! epoch; there is no floating point.

use std::time::Duration;

use mandate_time::UtcNanos;

use crate::client::RateHeaders;

const NANOS_PER_SEC: i128 = 1_000_000_000;

/// How the client paces itself against the host's per-window request limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RatePolicy {
    /// Requests the account may send per window, the floor when no header says how many are left
    /// (Alpaca's basic data plan allows 200 a minute). Values below 2 count as 2.
    pub limit: u32,
    /// The host's rate window.
    pub window: Duration,
    /// Stop spending a header-reported budget at this many remaining requests: the margin for
    /// other clients of the same account and for clock skew against the host.
    pub low_water: u32,
    /// The most added to a wait for a reset, so clients sharing an account do not wake together.
    pub max_jitter: Duration,
}

impl Default for RatePolicy {
    fn default() -> Self {
        Self {
            limit: 200,
            window: Duration::from_secs(60),
            low_water: 5,
            max_jitter: Duration::from_millis(500),
        }
    }
}

/// What the headers last said: requests left, and when the budget is whole again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Budget {
    remaining: u32,
    reset: i128,
}

/// The token bucket, in units where one request costs `window` nanoseconds' worth: `burst`
/// requests at most, refilled at `refill` requests per window. `burst + refill` equals the limit,
/// so no span of one window admits more than the limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Bucket {
    units: u128,
    capacity: u128,
    cost: u128,
    refill: u128,
    last: Option<i128>,
}

impl Bucket {
    fn new(limit: u32, window: Duration) -> Self {
        let limit = limit.max(2);
        let burst = limit.div_ceil(10);
        let cost = window.as_nanos();
        let capacity = u128::from(burst).saturating_mul(cost);
        Self {
            units: capacity,
            capacity,
            cost,
            refill: u128::from(limit - burst),
            last: None,
        }
    }

    fn refill_to(&mut self, now: i128) {
        if let Some(last) = self.last {
            let elapsed = u128::try_from(now - last).unwrap_or(0);
            self.units = self
                .units
                .saturating_add(elapsed.saturating_mul(self.refill))
                .min(self.capacity);
        }
        self.last = Some(self.last.map_or(now, |last| last.max(now)));
    }

    fn take(&mut self) -> Option<Duration> {
        match self.units.checked_sub(self.cost) {
            Some(left) => {
                self.units = left;
                None
            }
            None => Some(nanos_duration(
                (self.cost - self.units).div_ceil(self.refill),
            )),
        }
    }
}

/// The shared pacing state of one client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pacer {
    policy: RatePolicy,
    budget: Option<Budget>,
    bucket: Bucket,
    in_flight: u32,
}

impl Pacer {
    pub(crate) fn new(policy: RatePolicy) -> Self {
        Self {
            policy,
            budget: None,
            bucket: Bucket::new(policy.limit, policy.window),
            in_flight: 0,
        }
    }

    /// Admits one request at `now`, counting it against the budget and as in flight, or says how
    /// long to wait before asking again.
    pub(crate) fn admit(&mut self, now: UtcNanos) -> Option<Duration> {
        let wait = self.wait(nanos(now));
        if wait.is_none() {
            self.in_flight = self.in_flight.saturating_add(1);
        }
        wait
    }

    fn wait(&mut self, now: i128) -> Option<Duration> {
        self.bucket.refill_to(now);
        let low_water = self.policy.low_water;
        let Some(budget) = self.budget.as_mut().filter(|budget| now < budget.reset) else {
            return self.bucket.take();
        };
        if budget.remaining > low_water {
            budget.remaining -= 1;
            return None;
        }
        let until_reset = u128::try_from(budget.reset - now).unwrap_or(0);
        Some(nanos_duration(until_reset).saturating_add(self.jitter(now)))
    }

    /// Records that an admitted request came back at `now`, with the response's headers if it
    /// got one. The headers' count is reduced by every request still in flight, which the host
    /// may not have counted yet, and a reset more than a window away is taken as one window away.
    /// Headers that do not parse, or whose reset has passed, change nothing; an earlier reset
    /// than the one known is a stale response; for the same reset the lower count wins.
    pub(crate) fn complete(&mut self, headers: Option<&RateHeaders>, now: UtcNanos) {
        self.in_flight = self.in_flight.saturating_sub(1);
        let Some(mut seen) = headers.and_then(parse) else {
            return;
        };
        let now = nanos(now);
        if seen.reset <= now {
            return;
        }
        seen.remaining = seen.remaining.saturating_sub(self.in_flight);
        seen.reset = seen.reset.min(now.saturating_add(self.window()));
        self.budget = Some(match self.budget {
            Some(known) if known.reset == seen.reset => Budget {
                remaining: known.remaining.min(seen.remaining),
                ..known
            },
            Some(known) if known.reset > seen.reset => known,
            Some(_) | None => seen,
        });
    }

    /// Records a 429 at `now`: nothing more is sent until the known reset, or for a full window
    /// when no reset is known or it has passed.
    pub(crate) fn rate_limited(&mut self, now: UtcNanos) {
        let now = nanos(now);
        let reset = match self.budget {
            Some(known) if now < known.reset => known.reset,
            Some(_) | None => now.saturating_add(self.window()),
        };
        self.budget = Some(Budget {
            remaining: 0,
            reset,
        });
    }

    fn window(&self) -> i128 {
        i128::try_from(self.policy.window.as_nanos()).unwrap_or(i128::MAX)
    }

    /// The sub-second part of `now`, scaled to at most `max_jitter`.
    fn jitter(&self, now: i128) -> Duration {
        let fraction = u128::try_from(now.rem_euclid(NANOS_PER_SEC)).unwrap_or(0);
        let scaled = self.policy.max_jitter.as_nanos().saturating_mul(fraction) / 1_000_000_000;
        nanos_duration(scaled)
    }
}

fn parse(headers: &RateHeaders) -> Option<Budget> {
    let limit: u32 = headers.limit.as_deref()?.parse().ok()?;
    let remaining: u32 = headers.remaining.as_deref()?.parse().ok()?;
    let reset: i64 = headers.reset.as_deref()?.parse().ok()?;
    (remaining <= limit).then_some(Budget {
        remaining,
        reset: i128::from(reset) * NANOS_PER_SEC,
    })
}

fn nanos(at: UtcNanos) -> i128 {
    i128::from(at.secs()) * NANOS_PER_SEC + i128::from(at.nanos())
}

fn nanos_duration(nanos: u128) -> Duration {
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}
