//! The connection's health as the broker's answers show it (connections spec §8.2, DEC-687,
//! DEC-2832): failures counted per class, each count reset only by a good result of its own class,
//! and the troubles the core is told of at every pass while a count stays at its threshold.
//!
//! The classes read from an answer:
//! - **network:** a lost answer (`Unknown`: a timeout, a dropped connection, a `429` or `5xx`) or
//!   one the connector could not read (`Unreadable`) is a failure, since an ambiguous answer is
//!   never read as healthy; any answer the broker gave, a rejection or a refused credential
//!   included, is a good result;
//! - **authorization:** a refused credential (`Unknown(Unauthorized)`, or a rejection with HTTP
//!   `401` from a connector that answers it so) is a failure; any other answer the broker gave is
//!   a good result, except a `403`, which is the broker's refusal and counts neither way.
//!
//! - **headroom:** a response whose rate limit leaves under a fifth of the budget is a failure,
//!   one that leaves more is a good result, and one that reports no rate limit counts neither way
//!   (connections spec §8.2, DEC-2842).
//!
//! A request that never left the process (`NotSent`) says nothing about the connection and counts
//! neither way. The counts live only in this process: a restart starts them again, and what they
//! led to is on the stream (journal spec §9.8).

use mandate_alpaca::RateLimit;
use mandate_executor::{
    BrokerOutcome, BrokerUnknown, ConnectionTrouble, ConnectorError, DegradeReason, SuspendReason,
};

/// The status a broker refuses a credential it no longer accepts with: an authorization failure.
pub const UNAUTHORIZED: u16 = 401;
/// The status of the broker's own refusal of a request, which trading spec §7.3's reject rows read:
/// it says nothing of the credential.
pub const FORBIDDEN: u16 = 403;

/// How often, in seconds, the process probes the connection's health with a read of the broker's
/// account (connections spec §8.2's cadence while any agent is deployed, Proposed; DEC-2837). It is
/// fixed for every process: the faster of §8.2's two cadences, so no executor probes less often.
pub const PROBE_EVERY_S: i64 = 60;

/// Responses in a row whose rate limit leaves under a fifth of the budget that make the
/// connection `degraded` (§8.2: under 20% of the published or default budget; DEC-2842). One is
/// enough: §8.2 counts no run for headroom.
pub const HEADROOM_FAILURES: u32 = 1;

/// Consecutive network failures that make the connection `degraded` (§8.2, Proposed).
pub const NETWORK_FAILURES: u32 = 3;
/// Consecutive authorization failures that make the connection `suspended` (§8.2, Proposed).
pub const AUTHORIZATION_FAILURES: u32 = 2;

/// What one answer says of each class: a failure, a good result, or nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reading {
    Failed,
    Good,
    Silent,
}

/// The two counted classes' consecutive failures.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Health {
    network: u32,
    authorization: u32,
    headroom: u32,
    /// Whether a later permission check failed in this process (DEC-2840). Only a replaced
    /// credential ends `suspended` (connections spec §9.1), so nothing here clears it.
    check_failed: bool,
}

impl Health {
    /// Counts one broker answer.
    pub fn observe(&mut self, answer: &Result<BrokerOutcome, ConnectorError>) {
        let (network, authorization) = readings(answer);
        count(&mut self.network, network);
        count(&mut self.authorization, authorization);
    }

    /// Counts a failed read of a one-read broker's account, which answers as one call.
    pub fn observe_error(&mut self, error: &ConnectorError) {
        self.observe(&Err(*error));
    }

    /// Counts the rate limit a broker's response reported (DEC-2842): under a fifth of the budget
    /// left is a failure of the headroom class, more is a good result, none counts neither way.
    pub fn observe_rate_limit(&mut self, rate_limit: Option<RateLimit>) {
        let reading = match rate_limit {
            None => Reading::Silent,
            Some(RateLimit { limit, remaining }) => {
                if u64::from(remaining).saturating_mul(5) < u64::from(limit) {
                    Reading::Failed
                } else {
                    Reading::Good
                }
            }
        };
        count(&mut self.headroom, reading);
    }

    /// Notes a later permission check that failed (DEC-2840).
    pub fn check_failed(&mut self) {
        self.check_failed = true;
    }

    /// Counts a read of a one-read broker's account that the broker answered.
    pub fn observe_answered(&mut self) {
        count(&mut self.network, Reading::Good);
        count(&mut self.authorization, Reading::Good);
    }

    /// The troubles to report this pass, network first: the core restricts the account on the
    /// first and records the state change of each on later passes (DEC-2826), holding a degrading
    /// one while `suspended`.
    pub fn troubles(&self) -> Vec<ConnectionTrouble> {
        let mut troubles = Vec::new();
        if self.network >= NETWORK_FAILURES {
            troubles.push(ConnectionTrouble::Degraded(DegradeReason::NetworkErrors));
        }
        if self.headroom >= HEADROOM_FAILURES {
            troubles.push(ConnectionTrouble::Degraded(DegradeReason::RateHeadroom));
        }
        if self.authorization >= AUTHORIZATION_FAILURES {
            troubles.push(ConnectionTrouble::Suspended(
                SuspendReason::AuthorizationFailed,
            ));
        }
        if self.check_failed {
            troubles.push(ConnectionTrouble::Suspended(SuspendReason::CheckFailed));
        }
        troubles
    }
}

fn readings(answer: &Result<BrokerOutcome, ConnectorError>) -> (Reading, Reading) {
    match answer {
        Err(ConnectorError::Unknown(BrokerUnknown::Unauthorized)) => {
            (Reading::Good, Reading::Failed)
        }
        Err(ConnectorError::Unknown(_) | ConnectorError::Unreadable { .. }) => {
            (Reading::Failed, Reading::Silent)
        }
        Err(ConnectorError::NotSent { .. }) => (Reading::Silent, Reading::Silent),
        Ok(BrokerOutcome::Rejected(reject)) if reject.http_status == UNAUTHORIZED => {
            (Reading::Good, Reading::Failed)
        }
        Ok(BrokerOutcome::Rejected(reject)) if reject.http_status == FORBIDDEN => {
            (Reading::Good, Reading::Silent)
        }
        Ok(_) => (Reading::Good, Reading::Good),
    }
}

fn count(failures: &mut u32, reading: Reading) {
    match reading {
        Reading::Failed => *failures = failures.saturating_add(1),
        Reading::Good => *failures = 0,
        Reading::Silent => {}
    }
}

#[cfg(test)]
mod tests {
    use mandate_alpaca::RateLimit;
    use mandate_executor::{
        BrokerUnknown, ConnectionTrouble, ConnectorError, DegradeReason, SuspendReason,
    };

    use super::Health;

    /// DEC-2842: a rate limit under a fifth of the budget degrades at once, a fifth or more is a
    /// good result that clears the class, and no rate limit leaves it as it was.
    #[test]
    fn under_a_fifth_of_the_budget_degrades_and_no_reading_counts_neither_way() {
        let left = |remaining| {
            Some(RateLimit {
                limit: 200,
                remaining,
            })
        };
        let degraded = vec![ConnectionTrouble::Degraded(DegradeReason::RateHeadroom)];
        let mut health = Health::default();
        health.observe_rate_limit(left(40));
        assert!(
            health.troubles().is_empty(),
            "exactly a fifth left is enough"
        );
        health.observe_rate_limit(left(39));
        assert_eq!(health.troubles(), degraded);
        health.observe_rate_limit(None);
        assert_eq!(health.troubles(), degraded, "no reading changes nothing");
        health.observe_rate_limit(left(40));
        assert!(health.troubles().is_empty());
    }

    /// A one-read broker's account reads count as one answer each (DEC-2832): two refused
    /// credentials in a row suspend, and an answered read between them resets the count.
    #[test]
    fn a_one_read_brokers_reads_count_and_an_answered_one_resets() {
        let refused = ConnectorError::Unknown(BrokerUnknown::Unauthorized);
        let suspended = vec![ConnectionTrouble::Suspended(
            SuspendReason::AuthorizationFailed,
        )];
        let mut health = Health::default();
        health.observe_error(&refused);
        health.observe_answered();
        health.observe_error(&refused);
        assert!(
            health.troubles().is_empty(),
            "the answered read reset the count"
        );
        health.observe_error(&refused);
        assert_eq!(health.troubles(), suspended);
        health.observe_answered();
        assert!(health.troubles().is_empty());
    }
}
