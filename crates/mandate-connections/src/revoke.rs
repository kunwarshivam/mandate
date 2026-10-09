//! The platform-side revoke of a connection (workspace API §4.5 and §5.6, connections spec §5.4
//! and §9.1; DEC-693, DEC-694). Each function takes the facts it decides on and returns a typed
//! [`RevokePlan`]; `mandate-api-server` maps each [`RevokeEffect`] to its journal event in order
//! and appends them in one batch, or reports the refusal and appends nothing.
//!
//! - The **ordinary revoke** ([`plan_ordinary`]) decides from the journal folds, never from a read
//!   model (DEC-693 item 6). A fold that cannot answer is [`AgentsFold::CannotAnswer`] or
//!   [`PositionsFold::CannotAnswer`], a value of its own, so it can never be passed as "stopped"
//!   or "no positions" (DEC-693 item 7, `AGENTS.md` rule 3).
//! - The **compromised revoke** ([`plan_compromised`]) takes no fold at all: nothing it decides
//!   waits on positions, agent modes, or a fold (API-7, API-8, `AGENTS.md` rule 13). Its kill
//!   switch comes first and commits even without valid step-up (DEC-158 option (c)).
//! - What a compromised revoke leaves at the broker is assembled for display only, after the batch
//!   commits or for the pre-confirmation render, by [`remaining_positions`] (DEC-693 item 8).
//! - [`revoke_digest`] is the step-up action digest of both paths (DEC-693 items 1, 3 to 5).

use std::collections::BTreeSet;

use mandate_canon::Digest;
use mandate_domain::AssetId;

use crate::ConnectError;
use crate::record::ConnectionId;

/// Whether the step-up evidence for this request's digest counts (identity spec §7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepUp {
    Verified,
    NotVerified,
}

/// The modes the folds give the connection's agents (DEC-693 item 6); none at all is `AllStopped`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentsFold {
    AllStopped,
    SomeNotStopped,
    /// A fold the revoke needs is unavailable, stale, or behind its watermark (DEC-693 item 7).
    CannotAnswer,
}

/// The positions the folds give the connection's agents and account (an empty set: none held).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PositionsFold {
    Answered(BTreeSet<AssetId>),
    /// As [`AgentsFold::CannotAnswer`].
    CannotAnswer,
}

/// The facts an ordinary revoke is refused on (workspace API §4.5, connections spec §9.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrdinaryFacts {
    pub agents: AgentsFold,
    pub positions: PositionsFold,
}

/// `ConnectionRevoked` version 2's `reason` (journal spec §9.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevokeReason {
    Owner,
    Compromised,
}

/// One record the server appends, in plan order, in one batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevokeEffect {
    /// `OwnerCommandIssued` with `kill_switch` at this connection's scope (workspace API §5.6).
    KillSwitch { connection_id: ConnectionId },
    /// `ConnectionRevoked` v2; a `Compromised` one cites the kill switch before it (rule 84).
    Revoke {
        connection_id: ConnectionId,
        reason: RevokeReason,
    },
}

/// Why a revoke, or a compromised revoke's revocation half, is refused. The wire codes for the
/// fold refusals are owed to workspace API §3.5 (DEC-693 item 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevokeRefusal {
    /// The step-up evidence does not count; the caller reports its §3.5 step-up code.
    StepUpNotVerified,
    /// An agent on the connection holds positions (workspace API §4.5).
    PositionsHeld,
    /// An agent on the connection is not stopped (workspace API §4.5).
    AgentsNotStopped,
    /// A fold cannot answer: retry, or use the compromised path (DEC-693 item 7).
    FoldCannotAnswer,
}

/// What the server does with one revoke request: never both effects and a whole refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevokePlan {
    /// Append these effects, in this order, in one batch.
    Commit(Vec<RevokeEffect>),
    /// A compromised revoke without valid step-up: append the kill switch alone (`effect:
    /// recorded`) and refuse only the revocation (workspace API §5.6, API-7).
    KillSwitchOnly {
        kill_switch: RevokeEffect,
        revocation: RevokeRefusal,
    },
    /// Append nothing.
    Refused(RevokeRefusal),
}

/// What a compromised revoke leaves at the broker (workspace API §5.6 step 3, DEC-693 item 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemainingPositions {
    /// Nothing authoritative answers; shown as "unknown", never 0, "none", or an empty list.
    Unknown,
    Known(BTreeSet<AssetId>),
}

/// Plans the ordinary revoke (`compromised` `false` or absent), from the folds.
pub fn plan_ordinary(
    connection_id: &ConnectionId,
    step_up: StepUp,
    facts: &OrdinaryFacts,
) -> Result<RevokePlan, ConnectError> {
    let _ = (connection_id, step_up, facts);
    Err(ConnectError::Unimplemented { story: "E10-13" })
}

/// Plans the compromised revoke (`compromised: true`): the kill switch, then the revocation.
pub fn plan_compromised(
    connection_id: &ConnectionId,
    step_up: StepUp,
) -> Result<RevokePlan, ConnectError> {
    let _ = (connection_id, step_up);
    Err(ConnectError::Unimplemented { story: "E10-13" })
}

/// The positions to show for a compromised revoke, from the account stream's fold.
pub fn remaining_positions(fold: &PositionsFold) -> Result<RemainingPositions, ConnectError> {
    let _ = fold;
    Err(ConnectError::Unimplemented { story: "E10-13" })
}

/// The step-up digest (kind `connection`): SHA-256 of the canonical `{workspace_id,
/// connection_id, compromised}`, an absent `compromised` binding `false`.
pub fn revoke_digest(
    workspace_id: &str,
    connection_id: &ConnectionId,
    compromised: Option<bool>,
) -> Result<Digest, ConnectError> {
    let _ = (workspace_id, connection_id, compromised);
    Err(ConnectError::Unimplemented { story: "E10-13" })
}
