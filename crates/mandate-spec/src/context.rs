//! A [`ValidationContext`] built from journaled state (DEC-169, the coordinator's ruling on #171 and
//! #124).
//!
//! [`ValidationContext::from_journal`] is a pure fold over a closed list of [`JournaledFact`]s, in
//! journal order. This crate does not read the journal: the payload schemas and the mapping from a
//! journal record to a fact are stream L's (E7-10, DEC-168), so `mandate-spec` keeps no dependency
//! on `mandate-journal` (DEC-128 item 1). What no event carries comes in as an explicit argument
//! from its owner: the instrument groups (the instrument snapshot, trading spec §7.1), the
//! eligibility failures (E6-7's floor), and the workspace membership. The draft's provenance is folded
//! too, from its `MandateVersionCreated` and `MandateConfirmed` records (the coordinator's ruling on
//! #255).
//!
//! **A fact the fold never saw takes the value that refuses.** No equity snapshot is equity 0 (V-002),
//! no registration is an empty registry (V-007, and never `None`, which means "not checked"), no
//! membership is zero users (V-024), and a connection with no environment, or a revoked one, is
//! `live` (V-001 on a paper mandate). Each of those makes a V-rule fire; none skips a check.
//!
//! **An envelope path the draft's provenance does not mention is unconfirmed** (DEC-169 item 5, the
//! coordinator's rulings on #252 and #255): the fold adds a `user_entered`, unconfirmed entry for
//! every part of the document no entry covers, so V-020 fires on it and an `auto` under it is V-022.
//! With no `MandateVersionCreated` for the draft, every envelope path is unconfirmed; and a path is
//! confirmed only by a `MandateConfirmed` for the draft's own version naming it or an ancestor.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Digest, Value};
use mandate_domain::{AssetId, Environment};
use mandate_num::Usd;
use mandate_time::Date;

use crate::Mandate;
use crate::MandateVersion;
use crate::SpecError;
use crate::document::{ConnectionId, ModelId, Pointer, Provenance, ProvenanceMap, Source};
use crate::validate::{
    GroupId, PreviousVersion, RegisteredModel, SYSTEM_FIELDS, ValidationContext, covers,
};

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
    /// `ConnectionRevoked`: the connection's environment is no longer known, and neither is its equity,
    /// so V-001 and V-002 both refuse until the connection is established and snapshotted again.
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
    /// `UniverseChanged`: an instrument admitted to, or removed from, an agent's working universe. An
    /// agent with no `AgentVersionActive` has no known connection, so its admitted instruments count as
    /// claimed on every connection (V-006 fails closed, #262 round 1).
    UniverseChanged {
        agent: AgentId,
        instrument: AssetId,
        admitted: bool,
    },
    /// `AgentStopped`: the agent retired on `retired_on`, adding `loss_added_usd` to its connection's
    /// loss carry (mandate spec §5.7). It retires the agent only when `connection_id` is the one the
    /// agent's latest version named; otherwise its allocation and claims still count (rule 3). Its
    /// claims stay until [`JournaledFact::AgentFlat`].
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
    /// `MandateVersionCreated`: the source of each path of a version, as the compiler recorded it
    /// (§2.1, §10). Only the draft's own version counts, and the latest record for it wins.
    MandateVersionCreated {
        version: MandateVersion,
        sources: BTreeMap<Pointer, Source>,
    },
    /// `MandateConfirmed`: the paths the owner confirmed on a version. Only the draft's own version
    /// counts, and the latest record for it wins; a path confirms itself and everything under it.
    MandateConfirmed {
        version: MandateVersion,
        confirmed_paths: BTreeSet<Pointer>,
    },
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
    /// when `draft` has no canonical form or version; a missing fact is never an error, it is the refusing value
    /// the module doc names.
    pub fn from_journal<'a>(
        draft: &Mandate,
        args: ContextArgs,
        facts: impl IntoIterator<Item = &'a JournaledFact>,
    ) -> Result<Self, SpecError> {
        let mut fold = Fold::default();
        for fact in facts {
            fold.apply(fact);
        }
        let provenance = fold.provenance(draft.version()?);
        let mut context = fold.context(args)?;
        context.provenance = completed(&draft.canonical()?, &provenance);
        Ok(context)
    }
}

/// An agent as the fold last saw it.
#[derive(Debug, Default)]
struct AgentState {
    /// The version in force: its connection, environment, allocation, and pinned instruments.
    version: Option<(ConnectionId, Environment, Usd, BTreeSet<AssetId>)>,
    retired: bool,
    flat_since_retired: bool,
    working: BTreeSet<AssetId>,
}

#[derive(Debug, Default)]
struct Fold {
    equity: BTreeMap<ConnectionId, Usd>,
    environment: BTreeMap<ConnectionId, Environment>,
    disclosures: BTreeSet<Digest>,
    agents: BTreeMap<AgentId, AgentState>,
    retirements: Vec<(ConnectionId, Date, Usd)>,
    registry: BTreeMap<ModelId, RegisteredModel>,
    sources: BTreeMap<MandateVersion, BTreeMap<Pointer, Source>>,
    confirmations: BTreeMap<MandateVersion, BTreeSet<Pointer>>,
}

impl Fold {
    fn apply(&mut self, fact: &JournaledFact) {
        match fact {
            JournaledFact::AccountSnapshot {
                connection_id,
                equity_usd,
            } => {
                self.equity.insert(connection_id.clone(), *equity_usd);
            }
            JournaledFact::ConnectionEstablished {
                connection_id,
                environment,
            } => {
                self.environment.insert(connection_id.clone(), *environment);
            }
            JournaledFact::ConnectionRevoked { connection_id } => {
                self.environment.remove(connection_id);
                self.equity.remove(connection_id);
            }
            JournaledFact::DisclosureAccepted { version } => {
                self.disclosures.insert(*version);
            }
            JournaledFact::AgentVersionActive {
                agent,
                connection_id,
                environment,
                allocation_usd,
                pinned,
            } => {
                let state = self.agents.entry(agent.clone()).or_default();
                state.version = Some((
                    connection_id.clone(),
                    *environment,
                    *allocation_usd,
                    pinned.clone(),
                ));
                state.retired = false;
                state.flat_since_retired = false;
            }
            JournaledFact::UniverseChanged {
                agent,
                instrument,
                admitted,
            } => {
                let working = &mut self.agents.entry(agent.clone()).or_default().working;
                if *admitted {
                    working.insert(instrument.clone());
                } else {
                    working.remove(instrument);
                }
            }
            JournaledFact::AgentStopped {
                agent,
                connection_id,
                retired_on,
                loss_added_usd,
            } => {
                let state = self.agents.entry(agent.clone()).or_default();
                let named = state
                    .version
                    .as_ref()
                    .is_some_and(|(version_connection, ..)| version_connection == connection_id);
                if named {
                    state.retired = true;
                    state.flat_since_retired = false;
                }
                self.retirements
                    .push((connection_id.clone(), *retired_on, *loss_added_usd));
            }
            JournaledFact::AgentFlat { agent } => {
                let state = self.agents.entry(agent.clone()).or_default();
                state.flat_since_retired = state.retired;
            }
            JournaledFact::ModelRegistered { id, model } => {
                self.registry.insert(id.clone(), model.clone());
            }
            JournaledFact::ModelWithdrawn { id } => {
                self.registry.remove(id);
            }
            JournaledFact::MandateVersionCreated { version, sources } => {
                self.sources.insert(*version, sources.clone());
            }
            JournaledFact::MandateConfirmed {
                version,
                confirmed_paths,
            } => {
                self.confirmations.insert(*version, confirmed_paths.clone());
            }
        }
    }

    /// The draft's recorded sources, each confirmed only when its version's latest confirmation names
    /// the path or an ancestor of it.
    fn provenance(&self, draft: MandateVersion) -> ProvenanceMap {
        let confirmed = self.confirmations.get(&draft);
        ProvenanceMap::new(
            self.sources
                .get(&draft)
                .into_iter()
                .flatten()
                .map(|(path, source)| {
                    let confirmed = confirmed.is_some_and(|paths| {
                        paths
                            .iter()
                            .any(|named| covers(named.as_str(), path.as_str()))
                    });
                    (
                        path.clone(),
                        Provenance {
                            source: *source,
                            confirmed,
                        },
                    )
                })
                .collect(),
        )
    }

    fn context(self, args: ContextArgs) -> Result<ValidationContext, SpecError> {
        let ours = &args.connection_id;
        let mut other_allocations_usd = Usd::ZERO;
        let mut claimed_by_other_agents = BTreeSet::new();
        for (id, state) in &self.agents {
            if *id == args.agent {
                continue;
            }
            let Some((connection_id, _, allocation, pinned)) = &state.version else {
                claimed_by_other_agents.extend(state.working.iter().cloned());
                continue;
            };
            if connection_id != ours {
                continue;
            }
            if !state.retired {
                other_allocations_usd = other_allocations_usd.checked_add(*allocation)?;
            }
            if !state.flat_since_retired {
                claimed_by_other_agents.extend(pinned.iter().cloned());
                claimed_by_other_agents.extend(state.working.iter().cloned());
            }
        }
        let mut connection_loss_carry_usd = Usd::ZERO;
        for (connection_id, retired_on, loss) in &self.retirements {
            let counted = connection_id == ours
                && within(*retired_on, args.validation_date, LOSS_CARRY_DAYS)
                && !loss.is_negative();
            if counted {
                connection_loss_carry_usd = connection_loss_carry_usd.checked_add(*loss)?;
            }
        }
        let previous_version = self
            .agents
            .get(&args.agent)
            .and_then(|state| state.version.as_ref())
            .map(|(connection_id, environment, _, _)| PreviousVersion {
                environment: *environment,
                connection_id: connection_id.clone(),
            });
        let membership = args.membership.unwrap_or(Membership {
            workspace_users: 0,
            approver_users: 0,
        });
        Ok(ValidationContext {
            account_equity_usd: self.equity.get(ours).copied().unwrap_or(Usd::ZERO),
            other_allocations_usd,
            validation_date: args.validation_date,
            registry: Some(self.registry),
            provenance: ProvenanceMap::default(),
            workspace_users: membership.workspace_users,
            approver_users: membership.approver_users,
            disclosures_accepted: self.disclosures,
            instrument_groups: args.instrument_groups,
            claimed_by_other_agents,
            connection_environment: Some(
                self.environment
                    .get(ours)
                    .copied()
                    .unwrap_or(Environment::Live),
            ),
            connection_loss_carry_usd,
            eligibility_failures: args.eligibility_failures,
            previous_version,
        })
    }
}

/// Whether `retired_on` is at most `days` days before `today`, or after it. A date `Date` cannot step
/// past counts as inside, so the carry never drops a loss it cannot place.
fn within(retired_on: Date, today: Date, days: u32) -> bool {
    if retired_on > today {
        return false;
    }
    let mut reached = retired_on;
    for _ in 0..days {
        if reached >= today {
            return true;
        }
        match reached.next() {
            Ok(next) => reached = next,
            Err(_) => return true,
        }
    }
    reached >= today
}

/// `given`, with a `user_entered`, unconfirmed entry added for every part of `document` no entry
/// covers (DEC-169 item 5). The walk stops at a path an entry covers, descends only into a member or
/// item with an entry somewhere below it, and never adds a system field.
fn completed(document: &Value, given: &ProvenanceMap) -> ProvenanceMap {
    let mut entries = given.entries().clone();
    let mut added = Vec::new();
    walk(document, "", given, &mut added);
    for path in added {
        entries.insert(
            Pointer::new(&path),
            Provenance {
                source: Source::UserEntered,
                confirmed: false,
            },
        );
    }
    ProvenanceMap::new(entries)
}

fn walk(node: &Value, path: &str, given: &ProvenanceMap, added: &mut Vec<String>) {
    let children: Vec<(String, &Value)> = match node {
        Value::Object(members) => members
            .iter()
            .map(|(key, child)| (format!("{path}/{}", escaped(key.as_str())), child))
            .collect(),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(index, child)| (format!("{path}/{index}"), child))
            .collect(),
        _ => Vec::new(),
    };
    for (child_path, child) in children {
        if SYSTEM_FIELDS
            .iter()
            .any(|system| covers(system, &child_path))
        {
            continue;
        }
        let entries = given.entries().keys().map(Pointer::as_str);
        let mut covered = false;
        let mut below = false;
        for entry in entries {
            covered = covered || covers(entry, &child_path);
            below = below || covers(&child_path, entry);
        }
        if covered {
            continue;
        }
        if below && has_children(child) {
            walk(child, &child_path, given, added);
        } else {
            added.push(child_path);
        }
    }
}

/// Whether the walk can descend into `node`. A recorded path below a scalar, or below an empty array
/// or object, names nothing in the document, so the scalar itself is added rather than skipped
/// (#262 round 1, the blocker).
fn has_children(node: &Value) -> bool {
    match node {
        Value::Object(members) => !members.is_empty(),
        Value::Array(items) => !items.is_empty(),
        _ => false,
    }
}

/// A member name as an RFC 6901 reference token.
fn escaped(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use mandate_domain::{AssetId, Environment};
    use mandate_num::Usd;
    use mandate_time::Date;

    use mandate_canon::Value;
    use proptest::prelude::*;

    use super::{AgentId, ContextArgs, JournaledFact};
    use crate::Mandate;
    use crate::document::{ConnectionId, Pointer, Provenance, Source};
    use crate::validate::tests::mandate;
    use crate::validate::{ValidationContext, Violation, validate};

    #[test]
    fn an_agent_id_is_the_text_it_was_given() {
        assert_eq!(AgentId::new("agent_7").as_str(), "agent_7");
    }

    fn conn(id: &str) -> Result<ConnectionId, String> {
        ConnectionId::parse(id).map_err(|e| e.to_string())
    }

    fn args(connection_id: ConnectionId) -> Result<ContextArgs, String> {
        Ok(ContextArgs {
            agent: AgentId::new("a"),
            connection_id,
            validation_date: Date::parse("2026-09-24").map_err(|e| e.to_string())?,
            membership: None,
            instrument_groups: BTreeMap::new(),
            eligibility_failures: BTreeSet::new(),
        })
    }

    fn records(draft: &Mandate, paths: &[&str]) -> Result<[JournaledFact; 2], String> {
        let version = draft.version().map_err(|e| e.to_string())?;
        Ok([
            JournaledFact::MandateVersionCreated {
                version,
                sources: paths
                    .iter()
                    .map(|path| (Pointer::new(path), Source::UserEntered))
                    .collect(),
            },
            JournaledFact::MandateConfirmed {
                version,
                confirmed_paths: paths.iter().map(|path| Pointer::new(path)).collect(),
            },
        ])
    }

    /// The reviewer's repro on #262: every field recorded and confirmed except `admission`, recorded
    /// only through a path below it. The scalar is added unconfirmed, so V-020 and V-022 both fire.
    #[test]
    fn a_record_below_a_scalar_leaves_the_scalar_unconfirmed() -> Result<(), String> {
        let draft = mandate(&[("/autonomy/admission", r#""auto""#)])?;
        let recorded = [
            "/name",
            "/environment",
            "/connection_id",
            "/capital",
            "/goal",
            "/universe",
            "/behavior",
            "/protection",
            "/risk",
            "/notifications",
            "/autonomy/rules",
            "/autonomy/default",
            "/autonomy/approval",
            "/autonomy/admission/phantom",
        ];
        let context = ValidationContext::from_journal(
            &draft,
            args(conn("conn_alpaca_paper_01")?)?,
            &records(&draft, &recorded)?,
        )
        .map_err(|e| e.to_string())?;
        let admission = context
            .provenance
            .entries()
            .get(&Pointer::new("/autonomy/admission"));
        if admission
            != Some(&Provenance {
                source: Source::UserEntered,
                confirmed: false,
            })
        {
            return Err(format!(
                "`/autonomy/admission` must be added unconfirmed, got {admission:?}"
            ));
        }
        let report = validate(&draft, &context).map_err(|e| e.to_string())?;
        for code in [Violation::V020, Violation::V022] {
            if !report.violations.contains(&code) {
                return Err(format!("{code} must fire, got {:?}", report.violations));
            }
        }
        Ok(())
    }

    /// Every leaf of `value` below `path`, as the oracle enumerates it: its own walk, sharing nothing
    /// with the fill's.
    fn leaves(value: &Value, path: &str, out: &mut Vec<String>) {
        match value {
            Value::Object(members) if !members.is_empty() => {
                for (key, child) in members {
                    leaves(child, &format!("{path}/{}", key.as_str()), out);
                }
            }
            Value::Array(items) if !items.is_empty() => {
                for (index, child) in items.iter().enumerate() {
                    leaves(child, &format!("{path}/{index}"), out);
                }
            }
            _ => out.push(path.to_owned()),
        }
    }

    const RECORDABLE: [&str; 18] = [
        "",
        "/name",
        "/name/below",
        "/autonomy",
        "/autonomy/admission",
        "/autonomy/admission/phantom",
        "/autonomy/rules/0/when",
        "/autonomy/rules/7",
        "/risk/max_drawdown",
        "/risk/max_drawdown/0",
        "/risk/drawdown_ladder/1",
        "/risk/drawdown_ladder/1/at/deeper",
        "/behavior/signal_models/0/weight",
        "/behavior/signal_models/0/params/1/value",
        "/universe/pinned_instruments/5",
        "/notifications/quiet_hours",
        "/notifications/quiet_hours/start/x",
        "/mandate_schema_version",
    ];

    proptest! {
        /// Whatever paths the draft's record names, including paths below scalars and past the end
        /// of arrays, every leaf of the document outside the system fields ends up covered by an entry
        /// (DEC-169 item 5, the blocker of #262 round 1).
        #[test]
        fn every_document_path_is_covered_after_the_fill(
            chosen in prop::collection::btree_set(prop::sample::select(RECORDABLE.to_vec()), 0..8),
        ) {
            let draft = mandate(&[]).map_err(TestCaseError::fail)?;
            let paths: Vec<&str> = chosen.into_iter().collect();
            let facts = records(&draft, &paths).map_err(TestCaseError::fail)?;
            let context = ValidationContext::from_journal(
                &draft,
                args(conn("conn_alpaca_paper_01").map_err(TestCaseError::fail)?)
                    .map_err(TestCaseError::fail)?,
                &facts,
            )
            .map_err(|e| TestCaseError::fail(e.to_string()))?;
            let document = draft.canonical().map_err(|e| TestCaseError::fail(e.to_string()))?;
            let mut all = Vec::new();
            leaves(&document, "", &mut all);
            for leaf in all {
                if leaf == "/mandate_schema_version" || leaf == "/source_text_ref" {
                    continue;
                }
                let covered = context.provenance.entries().keys().any(|entry| {
                    let entry = entry.as_str();
                    leaf == entry
                        || leaf.strip_prefix(entry).is_some_and(|rest| rest.starts_with('/'))
                        || entry.is_empty()
                });
                prop_assert!(covered, "`{}` is covered by no entry, recorded {:?}", leaf, paths);
            }
        }
    }

    /// An agent known only by its working universe has no known connection; its claims count (V-006
    /// fails closed, #262 round 1).
    #[test]
    fn an_agent_with_no_version_claims_what_it_admitted() -> Result<(), String> {
        let admitted =
            AssetId::parse("7b4a1c2e-aaaa-4a2b-9c3d-00000000000a").map_err(|e| e.to_string())?;
        let facts = [JournaledFact::UniverseChanged {
            agent: AgentId::new("b"),
            instrument: admitted.clone(),
            admitted: true,
        }];
        let read = ValidationContext::from_journal(
            &mandate(&[])?,
            args(conn("conn_alpaca_paper_01")?)?,
            &facts,
        )
        .map_err(|e| e.to_string())?;
        if read.claimed_by_other_agents != BTreeSet::from([admitted]) {
            return Err(format!(
                "b's admission must count, got {:?}",
                read.claimed_by_other_agents
            ));
        }
        Ok(())
    }

    /// A revoked connection's equity is no longer known, so V-002 sees 0 until a new snapshot
    /// (#262 round 1).
    #[test]
    fn a_revoked_connection_has_no_equity() -> Result<(), String> {
        let ours = conn("conn_alpaca_paper_01")?;
        let snapshot = JournaledFact::AccountSnapshot {
            connection_id: ours.clone(),
            equity_usd: Usd::parse("25000").map_err(|e| e.to_string())?,
        };
        let revoked = JournaledFact::ConnectionRevoked {
            connection_id: ours.clone(),
        };
        for (facts, wanted) in [
            (vec![snapshot.clone(), revoked.clone()], Usd::ZERO),
            (
                vec![revoked, snapshot],
                Usd::parse("25000").map_err(|e| e.to_string())?,
            ),
        ] {
            let read = ValidationContext::from_journal(&mandate(&[])?, args(ours.clone())?, &facts)
                .map_err(|e| e.to_string())?;
            if read.account_equity_usd != wanted {
                return Err(format!(
                    "expected {wanted:?}, got {:?}",
                    read.account_equity_usd
                ));
            }
        }
        Ok(())
    }

    /// The claims half of the conservative retirement (#255 round 2, n1, required by the coordinator):
    /// a stop naming another connection than the agent's version does not retire it, so a later
    /// `AgentFlat` releases nothing, and V-006 still sees the instrument it pins as claimed.
    #[test]
    fn a_stop_on_another_connection_then_flat_keeps_the_claims() -> Result<(), String> {
        let ours = ConnectionId::parse("conn_alpaca_paper_01").map_err(|e| e.to_string())?;
        let theirs = ConnectionId::parse("conn_alpaca_paper_02").map_err(|e| e.to_string())?;
        let pinned =
            AssetId::parse("7b4a1c2e-aaaa-4a2b-9c3d-00000000000a").map_err(|e| e.to_string())?;
        let date = Date::parse("2026-09-24").map_err(|e| e.to_string())?;
        let b = AgentId::new("b");
        let facts = [
            JournaledFact::AgentVersionActive {
                agent: b.clone(),
                connection_id: ours.clone(),
                environment: Environment::Paper,
                allocation_usd: Usd::parse("3000").map_err(|e| e.to_string())?,
                pinned: BTreeSet::from([pinned.clone()]),
            },
            JournaledFact::AgentStopped {
                agent: b.clone(),
                connection_id: theirs,
                retired_on: date,
                loss_added_usd: Usd::ZERO,
            },
            JournaledFact::AgentFlat { agent: b },
        ];
        let args = ContextArgs {
            agent: AgentId::new("a"),
            connection_id: ours,
            validation_date: date,
            membership: None,
            instrument_groups: BTreeMap::new(),
            eligibility_failures: BTreeSet::new(),
        };
        let read = ValidationContext::from_journal(&mandate(&[])?, args, &facts)
            .map_err(|e| e.to_string())?;
        if read.claimed_by_other_agents != BTreeSet::from([pinned]) {
            return Err(format!(
                "b's claim must stand, got {:?}",
                read.claimed_by_other_agents
            ));
        }
        if read.other_allocations_usd != Usd::parse("3000").map_err(|e| e.to_string())? {
            return Err(format!(
                "b's allocation must stand, got {:?}",
                read.other_allocations_usd
            ));
        }
        Ok(())
    }
}
