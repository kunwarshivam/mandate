//! The connection's health as the broker's answers show it (connections spec §8.2, DEC-687,
//! DEC-2832): failures counted per class, each count reset only by a good result of its own class,
//! and the troubles the core is told of at every pass while a count stays at its threshold.
//!
//! The classes read from an answer:
//! - **network:** a lost answer (`Unknown`) or one the connector could not read (`Unreadable`) is
//!   a failure, since an ambiguous answer is never read as healthy; any answer the broker gave,
//!   a rejection included, is a good result;
//! - **authorization:** a rejection with HTTP `401` is a failure; any other answer the broker gave
//!   is a good result, except a `403`, which is the broker's refusal and counts neither way.
//!
//! A request that never left the process (`NotSent`) says nothing about the connection and counts
//! neither way. The counts live only in this process: a restart starts them again, and what they
//! led to is on the stream (journal spec §9.8).

use mandate_executor::{
    BrokerOutcome, ConnectionTrouble, ConnectorError, DegradeReason, SuspendReason,
};

/// The status a broker refuses a credential it no longer accepts with: an authorization failure.
pub const UNAUTHORIZED: u16 = 401;
/// The status of the broker's own refusal of a request, which trading spec §7.3's reject rows read:
/// it says nothing of the credential.
pub const FORBIDDEN: u16 = 403;

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
        if self.authorization >= AUTHORIZATION_FAILURES {
            troubles.push(ConnectionTrouble::Suspended(
                SuspendReason::AuthorizationFailed,
            ));
        }
        troubles
    }
}

fn readings(answer: &Result<BrokerOutcome, ConnectorError>) -> (Reading, Reading) {
    match answer {
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
