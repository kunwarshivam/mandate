//! A [`ValidationContext`] built from journaled state (DEC-169, the coordinator's ruling on #171 and
//! #124).
//!
//! [`ValidationContext::from_journal`] is a pure fold over a closed list of [`JournaledFact`]s, in
//! journal order. This crate does not read the journal: the payload schemas and the mapping from a
//! journal record to a fact are stream L's (E7-10, DEC-168), so `mandate-spec` keeps no dependency
//! on `mandate-journal` (DEC-128 item 1). What no event carries comes in as an explicit argument
//! from its owner: the instrument groups (the instrument snapshot, trading spec §7.1), the
//! eligibility failures (E6-7's floor), and the workspace membership.
//!
//! **A fact the fold never saw takes the value that refuses.** No equity snapshot is equity 0 (V-002),
//! no registration is an empty registry (V-007, and never `None`, which means "not checked"), no
//! membership is zero users (V-024), and a connection with no environment, or a revoked one, is
//! `live` (V-001 on a paper mandate). Each of those makes a V-rule fire; none skips a check.
//!
//! **An envelope path the draft's provenance does not mention is unconfirmed** (DEC-169 item 5, the
//! coordinator's ruling on #252): the fold adds a `user_entered`, unconfirmed entry for every part of
//! the document no entry covers, so V-020 fires on it and an `auto` under it is V-022. A record that
//! lost its provenance can therefore never read as the owner having confirmed everything.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::Digest;
use mandate_domain::{AssetId, Environment};
use mandate_num::Usd;
use mandate_time::Date;

use crate::Mandate;
use crate::SpecError;
use crate::document::{ConnectionId, ModelId, ProvenanceMap};
use crate::validate::{GroupId, RegisteredModel, ValidationContext};

/// An agent, as the journal names it. Opaque: the context compares agents and never reads the id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AgentId(String);

impl AgentId {
    pub fn new(id: &str) -> Self {
        Self(id.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One journaled fact the context reads, already mapped from its record by the payload owner (E7-10).
///
/// Closed on purpose: a fact the fold does not name cannot reach a context, so a new source of
/// validation input is a change to this enum and its tests, never a silent extra argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournaledFact {
    /// `AccountSnapshotRecorded`: the account's equity behind a connection. The latest one counts.
    AccountSnapshot {
        connection_id: ConnectionId,
        equity_usd: Usd,
    },
    /// `ConnectionEstablished`: the environment the connection actually is (V-001).
    ConnectionEstablished {
        connection_id: ConnectionId,
        environment: Environment,
    },
    /// `ConnectionRevoked`: the connection's environment is no longer known.
    ConnectionRevoked { connection_id: ConnectionId },
    /// `DisclosureAccepted`: a disclosure version the workspace accepted (V-005).
    DisclosureAccepted { version: Digest },
    /// `AgentDeployed`, or `MandateVersionApplied` for a deployed agent: the version in force. The
    /// latest one per agent replaces the earlier ones.
    AgentVersionActive {
        agent: AgentId,
        connection_id: ConnectionId,
        environment: Environment,
        allocation_usd: Usd,
        pinned: BTreeSet<AssetId>,
    },
    /// `UniverseChanged`: an instrument admitted to, or removed from, an agent's working universe.
    UniverseChanged {
        agent: AgentId,
        instrument: AssetId,
        admitted: bool,
    },
    /// `AgentStopped`: the agent retired on `retired_on`, adding `loss_added_usd` to its connection's
    /// loss carry (mandate spec §5.7). Its claims stay until [`JournaledFact::AgentFlat`].
    AgentStopped {
        agent: AgentId,
        connection_id: ConnectionId,
        retired_on: Date,
        loss_added_usd: Usd,
    },
    /// A retired agent is flat in every instrument it held, so its claims end (trading spec §7.1).
    AgentFlat { agent: AgentId },
    /// `ConfigSnapshotRegistered` for a signal model: registered, or registered again with new terms.
    ModelRegistered { id: ModelId, model: RegisteredModel },
    /// `PlatformOperatorAction` `model_withdrawn`.
    ModelWithdrawn { id: ModelId },
}

/// The workspace's members, as the identity service counts them (V-024).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Membership {
    pub workspace_users: u32,
    pub approver_users: u32,
}

/// What the fold is asked about, and what comes from owners other than the journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextArgs {
    /// The agent the draft is for. Its own allocation and claims are not "other".
    pub agent: AgentId,
    /// The connection the draft names. Only facts about it count toward equity, allocations, claims,
    /// the environment, and the loss carry.
    pub connection_id: ConnectionId,
    pub validation_date: Date,
    /// The draft's own provenance, from its `MandateVersionCreated` and `MandateConfirmed` records.
    /// Every part of the document no entry covers is added to the context as unconfirmed.
    pub provenance: ProvenanceMap,
    /// `None` when the identity service supplied nothing, which counts zero users.
    pub membership: Option<Membership>,
    pub instrument_groups: BTreeMap<AssetId, GroupId>,
    pub eligibility_failures: BTreeSet<AssetId>,
}

/// How long a retired agent's loss stays in its connection's carry (mandate spec §5.7).
pub const LOSS_CARRY_DAYS: u32 = 90;

impl ValidationContext {
    /// The context `validate` reads for `draft`, folded from `facts` in journal order.
    ///
    /// `Err` when a sum cannot be held exactly (the allocations or the loss carry overflow `Usd`), or
    /// when `draft` has no canonical form; a missing fact is never an error, it is the refusing value
    /// the module doc names.
    pub fn from_journal<'a>(
        draft: &Mandate,
        args: ContextArgs,
        facts: impl IntoIterator<Item = &'a JournaledFact>,
    ) -> Result<Self, SpecError> {
        let _ = (draft, args, facts.into_iter().count());
        Err(SpecError::Unimplemented)
    }
}

#[cfg(test)]
mod tests {
    use super::AgentId;

    #[test]
    fn an_agent_id_is_the_text_it_was_given() {
        assert_eq!(AgentId::new("agent_7").as_str(), "agent_7");
    }
}
