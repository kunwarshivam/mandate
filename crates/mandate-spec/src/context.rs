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
use mandate_domain::{AssetClass, AssetId, AutonomyDecision, Environment, ThesisRefusal};
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
    /// latest one per agent replaces the earlier ones. Journal spec v0.7 §9.2 closes `AgentDeployed`
    /// and v0.8 §9.3 an applied `MandateVersionApplied`, each read from the stored document its
    /// version names (DEC-403, DEC-404).
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

impl JournaledFact {
    /// The fact one journal record maps to (journal spec v0.7 §9.2's mapping table, E7-10), or
    /// `None` for a record that maps to none: a `ConfigSnapshotRegistered` of any kind but
    /// `model_version`, and every event type the table does not name.
    ///
    /// `payload` is the record's payload as `append` stored it. `documents` returns the stored
    /// canonical mandate document a version names, which an `AgentDeployed` is read from: the
    /// connection, environment, allocation, and pinned instruments are the document's, never restated
    /// in the record. `account_connection` is the connection the record's account stream belongs to,
    /// which no record names yet (DEC-261 item 10), so an `AccountSnapshotRecorded` needs it.
    ///
    /// `Err` when the record cannot be mapped: a deployment or a version change whose document is not
    /// stored, or does not hash to the version it is stored under; a version change whose
    /// `classification` is not `change::classify`'s verdict of its two documents, applied or rejected
    /// alike (DEC-404 item 5); a snapshot with no connection given; or a member the fact needs that
    /// the payload does not hold in §9.2's or §9.3's form. A record that cannot be mapped is
    /// refused, never skipped, because a fact the fold never sees takes the value that refuses only
    /// for facts that add (module doc), and a dropped `ConnectionRevoked` would not.
    pub fn from_record(
        event_type: &str,
        payload: &Value,
        documents: &dyn Fn(&Digest) -> Option<Value>,
        account_connection: Option<&ConnectionId>,
    ) -> Result<Option<Self>, SpecError> {
        let record = Record(payload);
        let fact = match event_type {
            "ConnectionEstablished" => Self::ConnectionEstablished {
                connection_id: record.connection("connection_id")?,
                environment: match record.text("environment")? {
                    "paper" => Environment::Paper,
                    "live" => Environment::Live,
                    _ => return Err(malformed("environment")),
                },
            },
            "ConnectionRevoked" => Self::ConnectionRevoked {
                connection_id: record.connection("connection_id")?,
            },
            "DisclosureAccepted" => Self::DisclosureAccepted {
                version: record.digest("version")?,
            },
            "ConfigSnapshotRegistered" if record.text("kind")? != MODEL_KIND => return Ok(None),
            "ConfigSnapshotRegistered" => Self::ModelRegistered {
                id: ModelId::parse(record.text("model_id")?).map_err(|_| malformed("model_id"))?,
                model: RegisteredModel {
                    version: record.text("model_version")?.to_owned(),
                    content_hash: record.digest("content_hash")?,
                    params: record
                        .list("params")?
                        .iter()
                        .map(|p| p.as_str().map(str::to_owned).ok_or(malformed("params")))
                        .collect::<Result<_, _>>()?,
                    admits_instruments: match payload.get("admits_instruments") {
                        Some(Value::Bool(admits)) => *admits,
                        _ => return Err(malformed("admits_instruments")),
                    },
                },
            },
            "MandateVersionCreated" => Self::MandateVersionCreated {
                version: MandateVersion::named(record.digest("mandate_version")?),
                sources: record
                    .list("provenance")?
                    .iter()
                    .map(|entry| {
                        let entry = Record(entry);
                        Ok((
                            Pointer::new(entry.text("path")?),
                            source(entry.text("source")?)?,
                        ))
                    })
                    .collect::<Result<_, SpecError>>()?,
            },
            "MandateConfirmed" => Self::MandateConfirmed {
                version: MandateVersion::named(record.digest("mandate_version")?),
                confirmed_paths: record
                    .list("confirmed_paths")?
                    .iter()
                    .map(|p| {
                        p.as_str()
                            .filter(|t| !t.is_empty())
                            .map(Pointer::new)
                            .ok_or(malformed("confirmed_paths"))
                    })
                    .collect::<Result<_, _>>()?,
            },
            "AgentDeployed" => {
                let version = record.digest("mandate_version")?;
                let stored = documents(&version).ok_or(malformed("mandate_version"))?;
                let mandate = Mandate::parse(&stored).map_err(|_| malformed("mandate_version"))?;
                if mandate.version()?.digest() != version {
                    return Err(malformed("mandate_version"));
                }
                Self::AgentVersionActive {
                    agent: AgentId::new(record.id("agent_id")?),
                    connection_id: mandate.connection_id.clone(),
                    environment: mandate.environment,
                    allocation_usd: mandate
                        .capital
                        .allocation_usd
                        .to_usd()
                        .map_err(|_| malformed("mandate_version"))?,
                    pinned: mandate
                        .universe
                        .pinned_instruments
                        .iter()
                        .map(|i| i.asset_id.clone())
                        .collect(),
                }
            }
            "AgentStopped" => Self::AgentStopped {
                agent: AgentId::new(record.id("agent_id")?),
                connection_id: record.connection("connection_id")?,
                retired_on: Date::parse(record.text("retired_on")?)
                    .map_err(|_| malformed("retired_on"))?,
                loss_added_usd: record.usd("loss_added")?,
            },
            "UniverseChanged" => Self::UniverseChanged {
                agent: AgentId::new(record.id("agent_id")?),
                instrument: AssetId::parse(record.text("instrument")?)
                    .map_err(|_| malformed("instrument"))?,
                admitted: match record.text("change")? {
                    "admitted" => true,
                    "removed" => false,
                    _ => return Err(malformed("change")),
                },
            },
            "MandateVersionApplied" => {
                let stored = |name: &'static str| -> Result<Mandate, SpecError> {
                    let version = record.digest(name)?;
                    let document = documents(&version).ok_or(malformed(name))?;
                    let mandate = Mandate::parse(&document).map_err(|_| malformed(name))?;
                    if mandate.version()?.digest() != version {
                        return Err(malformed(name));
                    }
                    Ok(mandate)
                };
                let (old, new) = (stored("old_version")?, stored("new_version")?);
                let verdict = crate::change::classify(&old, &new)?.class;
                if record.text("classification")? != verdict.as_str() {
                    return Err(malformed("classification"));
                }
                match record.text("result")? {
                    "applied" => {}
                    "rejected" => return Ok(None),
                    _ => return Err(malformed("result")),
                }
                Self::AgentVersionActive {
                    agent: AgentId::new(record.id("agent_id")?),
                    connection_id: new.connection_id.clone(),
                    environment: new.environment,
                    allocation_usd: new
                        .capital
                        .allocation_usd
                        .to_usd()
                        .map_err(|_| malformed("new_version"))?,
                    pinned: new
                        .universe
                        .pinned_instruments
                        .iter()
                        .map(|i| i.asset_id.clone())
                        .collect(),
                }
            }
            "AccountSnapshotRecorded" => Self::AccountSnapshot {
                connection_id: account_connection
                    .cloned()
                    .ok_or(malformed("account_connection"))?,
                equity_usd: record.usd("equity")?,
            },
            _ => return Ok(None),
        };
        Ok(Some(fact))
    }
}

/// The registration's re-derivation of a research-agent thesis record's verdict (journal spec
/// v0.9 §9.4; DEC-413 item 5, DEC-414; #490 round 1, M2, as #470 round 2 minor 5 ruled for §9.3):
/// mandate spec §8.5 checks 4, 5, 6, 10, and 16's revision cap, re-derived from the stored mandate
/// document `mandate_version` names, which is the record's `config_refs.mandate_version`, the
/// mandate in force when the thesis was judged.
///
/// `payload` is a `ThesisProposed` or `ThesisRevised` payload in §9.4's form. `Ok(())` when the
/// record's verdict passes over no check that mandate fails, and a refusal at check 5, 6, or 10 is
/// one the mandate fails too. The policy overlay only tightens, so a check the mandate fails always
/// fails; checks 4 and 16 the overlay or the folded lineage can also fail, so a refusal there need
/// not fail by the mandate.
///
/// `Err(InvalidInput)` naming `mandate_version` when the document is not stored or does not hash to
/// the version it is stored under; naming `admitted` when the record admits past a check the
/// mandate fails; and naming `reason` when the record is refused at a check later than the first
/// one the mandate fails, or at check 5, 6, or 10 when the mandate passes it.
///
/// Check 4 asks for a research envelope on a mandate whose model admits instruments as one
/// expression, as `mandate-research` does: V-036 makes "no admitting model" and "no research
/// envelope" the same condition for a validated mandate, so two disjuncts would differ only on
/// documents validation rejects.
pub fn check_thesis_record(
    payload: &Value,
    mandate_version: &Digest,
    documents: &dyn Fn(&Digest) -> Option<Value>,
) -> Result<(), SpecError> {
    let stored = documents(mandate_version).ok_or(malformed("mandate_version"))?;
    let mandate = Mandate::parse(&stored).map_err(|_| malformed("mandate_version"))?;
    if mandate.version()?.digest() != *mandate_version {
        return Err(malformed("mandate_version"));
    }
    let record = Record(payload);
    let asset_class =
        AssetClass::parse(record.text("asset_class")?).map_err(|_| malformed("asset_class"))?;
    let revision = payload
        .get("revision")
        .and_then(Value::as_int)
        .ok_or(malformed("revision"))?;
    let admits_instruments = mandate
        .behavior
        .signal_models
        .iter()
        .any(|model| model.admits_instruments);
    let fails = |check: u8| match check {
        4 => mandate
            .behavior
            .research
            .as_ref()
            .filter(|_| admits_instruments)
            .is_none(),
        5 => mandate.universe.pinned,
        6 => mandate.autonomy.admission == AutonomyDecision::Deny,
        10 => !mandate.universe.asset_classes.contains(&asset_class),
        16 => {
            revision
                > mandate
                    .behavior
                    .research
                    .as_ref()
                    .map_or(0, |research| u64::from(research.max_revisions_per_lineage))
        }
        _ => false,
    };
    let first = THESIS_MANDATE_CHECKS
        .into_iter()
        .find(|check| fails(*check));
    match payload.get("admitted") {
        Some(Value::Bool(true)) => match first {
            Some(_) => Err(malformed("admitted")),
            None => Ok(()),
        },
        Some(Value::Bool(false)) => {
            let reason = record.text("reason")?;
            let check = ThesisRefusal::parse(reason)
                .map_err(|_| malformed("reason"))?
                .check_number();
            let later_than_first = first.is_some_and(|first| check > first);
            let unfailed_by_the_mandate = MANDATE_ALONE_CHECKS.contains(&check) && !fails(check);
            if later_than_first || unfailed_by_the_mandate {
                Err(malformed("reason"))
            } else {
                Ok(())
            }
        }
        _ => Err(malformed("admitted")),
    }
}

/// The §8.5 checks the stored mandate decides, in check order (DEC-413 item 5): a research agent
/// that admits instruments (4), an unpinned universe (5), admission not `deny` (6), the instrument's
/// asset class (10), and the revision cap (16). The policy overlay only tightens, so a check the
/// mandate fails always fails.
const THESIS_MANDATE_CHECKS: [u8; 5] = [4, 5, 6, 10, 16];

/// The checks that read the mandate alone, so a refusal at one of them must also fail by the
/// document. Checks 4 and 16 are not among them: the overlay can fail 4, and 16 also reads the folded
/// `retired` flag. Check 6 stays only while the overlay raises `auto` to `ask` and never to `deny`
/// (DEC-413 item 4, DEC-414 item 4).
const MANDATE_ALONE_CHECKS: [u8; 3] = [5, 6, 10];

/// `ConfigSnapshotRegistered`'s kind for a signal model (journal spec §9.2).
const MODEL_KIND: &str = "model_version";

/// A member the fact needs that the payload does not hold in §9.2's form.
fn malformed(what: &'static str) -> SpecError {
    SpecError::InvalidInput { what }
}

fn source(text: &str) -> Result<Source, SpecError> {
    Ok(match text {
        "user_stated" => Source::UserStated,
        "user_entered" => Source::UserEntered,
        "template_structure" => Source::TemplateStructure,
        "platform_proposed" => Source::PlatformProposed,
        "platform_default" => Source::PlatformDefault,
        _ => return Err(malformed("provenance")),
    })
}

/// A record's payload, read member by member, each refused under its own name.
struct Record<'a>(&'a Value);

impl<'a> Record<'a> {
    fn text(&self, name: &'static str) -> Result<&'a str, SpecError> {
        self.0
            .get(name)
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .ok_or(malformed(name))
    }

    fn id(&self, name: &'static str) -> Result<&'a str, SpecError> {
        let text = self.text(name)?;
        ConnectionId::parse(text)
            .map(|_| text)
            .map_err(|_| malformed(name))
    }

    fn connection(&self, name: &'static str) -> Result<ConnectionId, SpecError> {
        ConnectionId::parse(self.text(name)?).map_err(|_| malformed(name))
    }

    fn digest(&self, name: &'static str) -> Result<Digest, SpecError> {
        self.text(name)?
            .strip_prefix("sha256:")
            .and_then(Digest::from_hex)
            .ok_or(malformed(name))
    }

    fn usd(&self, name: &'static str) -> Result<Usd, SpecError> {
        Usd::parse(self.text(name)?).map_err(|_| malformed(name))
    }

    fn list(&self, name: &'static str) -> Result<&'a [Value], SpecError> {
        self.0
            .get(name)
            .and_then(Value::as_array)
            .ok_or(malformed(name))
    }
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
                mandate: None,
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

    /// The reviewer's probe on #262 round 2: an empty pinned list recorded only through an index
    /// below it names nothing, so `/universe/pinned_instruments` itself is added unconfirmed, and with
    /// `admission: auto` unrecorded, V-020 and V-022 both fire (DEC-169 item 5's empty-container case).
    #[test]
    fn a_record_below_an_empty_array_leaves_the_array_unconfirmed() -> Result<(), String> {
        let draft = mandate(&[
            ("/universe/pinned", "false"),
            ("/universe/pinned_instruments", "[]"),
            ("/autonomy/admission", r#""auto""#),
        ])?;
        let context = ValidationContext::from_journal(
            &draft,
            args(conn("conn_alpaca_paper_01")?)?,
            &records(&draft, &["/universe/pinned_instruments/0"])?,
        )
        .map_err(|e| e.to_string())?;
        let pinned = context
            .provenance
            .entries()
            .get(&Pointer::new("/universe/pinned_instruments"));
        if pinned
            != Some(&Provenance {
                source: Source::UserEntered,
                confirmed: false,
            })
        {
            return Err(format!(
                "`/universe/pinned_instruments` must be added unconfirmed, got {pinned:?}"
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

/// `JournaledFact::from_record`'s label readings, each value of each closed set, judged inside this
/// crate so the mutation gate sees them (DEC-303 item 10).
#[cfg(test)]
mod record_tests {
    use std::collections::{BTreeMap, BTreeSet};

    use mandate_canon::{Digest, Key, Value};
    use mandate_domain::{AssetId, Environment};
    use mandate_time::Date;

    use super::{AgentId, ContextArgs, JournaledFact};
    use crate::SpecError;
    use crate::document::{ConnectionId, Pointer, Provenance, Source};
    use crate::validate::ValidationContext;
    use crate::validate::tests::mandate;

    fn object(members: &[(&str, Value)]) -> Result<Value, SpecError> {
        let mut out = mandate_canon::Object::new();
        for (name, value) in members {
            let key = Key::new(name).map_err(|_| SpecError::InvalidInput { what: "key" })?;
            out.insert(key, value.clone());
        }
        Ok(Value::Object(out))
    }

    fn text(value: &str) -> Value {
        Value::Str(value.to_owned())
    }

    fn nothing(_: &Digest) -> Option<Value> {
        None
    }

    #[test]
    fn each_environment_maps_to_its_own() -> Result<(), SpecError> {
        for (label, environment) in [("paper", Environment::Paper), ("live", Environment::Live)] {
            let established = object(&[
                ("connection_id", text("conn_1")),
                ("broker", text("alpaca")),
                ("environment", text(label)),
                ("scopes", Value::Array(vec![text("trading")])),
            ])?;
            assert_eq!(
                JournaledFact::from_record("ConnectionEstablished", &established, &nothing, None)?,
                Some(JournaledFact::ConnectionEstablished {
                    connection_id: ConnectionId::parse("conn_1")?,
                    environment,
                }),
                "{label}"
            );
        }
        Ok(())
    }

    #[test]
    fn each_provenance_source_maps_to_its_own() -> Result<(), SpecError> {
        let version = format!("sha256:{}", "a".repeat(64));
        for (label, source) in [
            ("user_stated", Source::UserStated),
            ("user_entered", Source::UserEntered),
            ("template_structure", Source::TemplateStructure),
            ("platform_proposed", Source::PlatformProposed),
            ("platform_default", Source::PlatformDefault),
        ] {
            let entry = object(&[("path", text("/risk")), ("source", text(label))])?;
            let created = object(&[
                ("mandate_version", text(&version)),
                ("provenance", Value::Array(vec![entry])),
                ("record_ref", text(&version)),
            ])?;
            let fact =
                JournaledFact::from_record("MandateVersionCreated", &created, &nothing, None)?;
            let Some(JournaledFact::MandateVersionCreated { sources, .. }) = fact else {
                return Err(SpecError::InvalidInput { what: "the fact" });
            };
            assert_eq!(
                sources,
                BTreeMap::from([(Pointer::new("/risk"), source)]),
                "{label}"
            );
        }
        Ok(())
    }
    fn digest_text(byte: char) -> String {
        format!("sha256:{}", byte.to_string().repeat(64))
    }

    fn refused(event_type: &str, payload: &Value) -> Option<&'static str> {
        match JournaledFact::from_record(event_type, payload, &nothing, None) {
            Err(SpecError::InvalidInput { what }) => Some(what),
            _ => None,
        }
    }

    /// `confirmed_paths` is a list of §9.2 pointers, never the empty one (DEC-303 item 13): `""`
    /// would cover every path, so a `platform_default` path would read as confirmed, which V-020
    /// and V-022 exist to stop. The record is refused, so the context built from the records that
    /// map leaves the path unconfirmed (#461 round 1, M1).
    #[test]
    fn an_empty_confirmed_path_is_refused_and_confirms_nothing() -> Result<(), SpecError> {
        let draft = mandate(&[]).map_err(|_| SpecError::InvalidInput { what: "the draft" })?;
        let version = format!("sha256:{}", draft.version()?.digest().to_hex());
        let entry = object(&[
            ("path", text("/autonomy")),
            ("source", text("platform_default")),
        ])?;
        let created = object(&[
            ("mandate_version", text(&version)),
            ("provenance", Value::Array(vec![entry])),
            ("record_ref", text(&digest_text('b'))),
        ])?;
        let confirmed = object(&[
            ("mandate_version", text(&version)),
            ("confirmed_paths", Value::Array(vec![text("")])),
            ("record_ref", text(&digest_text('b'))),
        ])?;
        assert_eq!(
            refused("MandateConfirmed", &confirmed),
            Some("confirmed_paths")
        );
        let facts: Vec<JournaledFact> = [
            ("MandateVersionCreated", &created),
            ("MandateConfirmed", &confirmed),
        ]
        .into_iter()
        .filter_map(|(event_type, payload)| {
            JournaledFact::from_record(event_type, payload, &nothing, None)
                .ok()
                .flatten()
        })
        .collect();
        let args = ContextArgs {
            agent: AgentId::new("a"),
            connection_id: ConnectionId::parse("conn_alpaca_paper_01")?,
            validation_date: Date::parse("2026-09-24")
                .map_err(|_| SpecError::InvalidInput { what: "date" })?,
            membership: None,
            instrument_groups: BTreeMap::new(),
            eligibility_failures: BTreeSet::new(),
        };
        let context = ValidationContext::from_journal(&draft, args, &facts)?;
        assert_eq!(
            context.provenance.entries().get(&Pointer::new("/autonomy")),
            Some(&Provenance {
                source: Source::PlatformDefault,
                confirmed: false,
            })
        );
        Ok(())
    }

    /// A risk-state record is never skipped: one that cannot be mapped, here because it lacks every
    /// member §9.3 gives it, is refused, so the fold fails closed (DEC-303 item 6, DEC-404).
    #[test]
    fn a_risk_state_record_is_never_skipped() -> Result<(), SpecError> {
        let record = object(&[
            ("agent_id", text("agent_a")),
            ("connection_id", text("conn_1")),
            ("mandate_version", text(&digest_text('a'))),
        ])?;
        for event_type in ["MandateVersionApplied", "UniverseChanged"] {
            assert!(
                JournaledFact::from_record(event_type, &record, &nothing, None).is_err(),
                "{event_type}"
            );
        }
        Ok(())
    }

    fn universe_change(change: &str) -> Result<Value, SpecError> {
        object(&[
            ("agent_id", text("agent_b")),
            ("instrument", text("7b4a1c2e-2222-4a2b-9c3d-000000000002")),
            ("change", text(change)),
            ("reason", text("version_applied")),
            ("thesis_id", Value::Null),
            ("lineage_id", Value::Null),
            ("universe_size_after", Value::Null),
            ("risk_clock", text("2026-09-22T16:00:00.000000000Z")),
        ])
    }

    /// `UniverseChanged` maps to its agent, its instrument, and whether it was admitted (journal spec
    /// §9.3).
    #[test]
    fn a_universe_change_maps_to_its_instrument_admitted_or_removed() -> Result<(), SpecError> {
        let instrument = AssetId::parse("7b4a1c2e-2222-4a2b-9c3d-000000000002")?;
        for (change, admitted) in [("admitted", true), ("removed", false)] {
            assert_eq!(
                JournaledFact::from_record(
                    "UniverseChanged",
                    &universe_change(change)?,
                    &nothing,
                    None
                )?,
                Some(JournaledFact::UniverseChanged {
                    agent: AgentId::new("agent_b"),
                    instrument: instrument.clone(),
                    admitted,
                }),
                "{change}"
            );
        }
        let refused = JournaledFact::from_record(
            "UniverseChanged",
            &universe_change("paused")?,
            &nothing,
            None,
        );
        assert_eq!(refused, Err(SpecError::InvalidInput { what: "change" }));
        Ok(())
    }

    fn version_record(
        old: &Digest,
        new: &Digest,
        classification: &str,
        result: &str,
    ) -> Result<Value, SpecError> {
        let sha = |d: &Digest| text(&format!("sha256:{}", d.to_hex()));
        object(&[
            ("agent_id", text("agent_a")),
            ("old_version", sha(old)),
            ("new_version", sha(new)),
            ("classification", text(classification)),
            ("step_up", Value::Null),
            ("result", text(result)),
            ("reason", Value::Null),
            ("allocation_change", Value::Null),
            ("max_loss_from_allocation", Value::Null),
            ("risk_clock", text("2026-09-22T14:30:00.000000000Z")),
        ])
    }

    /// An applied `MandateVersionApplied` maps to the agent's version in force, read from the stored
    /// document `new_version` names, and a rejected one to none, but only under the classification
    /// mandate spec §9.2 gives its two stored documents: under any other it is refused at
    /// `classification`, applied or rejected alike. The pairs cover the §9.2 rows rule 33 cannot see
    /// (#482 round 1, M1): a risk maximum, unpinning, `max_instruments`, protection off, a later
    /// `end_date`, a signal-model change, an asset class added, `behavior.research` set from null,
    /// pinning from a version with an admitting model (reducing, DEC-121) and from one without, and
    /// an autonomy `then` in each direction. Each verdict is written from §9.2's table by hand, not
    /// computed by the classifier under test (DEC-403 item 5; #470 round 2, minor 5). The pairs are
    /// parsed, not validated: six break a V-rule (V-008, V-013, V-034, V-036), so this also pins that
    /// the mapping does not validate the stored documents, which classification never consults
    /// (#482 round 2, m1).
    #[test]
    fn a_version_maps_only_under_the_classification_its_documents_give() -> Result<(), String> {
        const RESEARCH: &str =
            r#"{"interval_s": 3600, "cost_cap_usd_per_day": "5", "max_revisions_per_lineage": 3}"#;
        let base = mandate(&[])?;
        let unpinned = mandate(&[("/universe/pinned", "false")])?;
        let admitting = mandate(&[
            ("/universe/pinned", "false"),
            ("/behavior/research", RESEARCH),
            ("/behavior/signal_models/0/admits_instruments", "true"),
        ])?;
        let changed = |patch: (&str, &str)| mandate(&[patch]);
        let pairs = [
            (
                &base,
                changed(("/risk/max_order_usd", r#""2000""#))?,
                "risk_increasing",
            ),
            (
                &base,
                changed(("/risk/max_order_usd", r#""500""#))?,
                "risk_reducing",
            ),
            (&base, changed(("/name", r#""renamed""#))?, "neutral"),
            (
                &base,
                changed(("/universe/pinned", "false"))?,
                "risk_increasing",
            ),
            (
                &base,
                changed(("/universe/max_instruments", "5"))?,
                "risk_increasing",
            ),
            (
                &base,
                changed(("/protection/enabled", "false"))?,
                "risk_increasing",
            ),
            (
                &base,
                changed(("/goal/end_date", r#""2027-06-30""#))?,
                "risk_increasing",
            ),
            (
                &base,
                changed(("/behavior/signal_models/0/weight", r#""0.5""#))?,
                "risk_increasing",
            ),
            (
                &base,
                changed(("/universe/asset_classes", r#"["crypto", "us_equity"]"#))?,
                "risk_increasing",
            ),
            (
                &base,
                changed(("/behavior/research", RESEARCH))?,
                "risk_increasing",
            ),
            (&admitting, base.clone(), "risk_reducing"),
            (&unpinned, base.clone(), "risk_increasing"),
            (
                &base,
                changed(("/autonomy/rules/0/then", r#""deny""#))?,
                "risk_reducing",
            ),
            (
                &base,
                changed(("/autonomy/rules/0/then", r#""auto""#))?,
                "risk_increasing",
            ),
        ];
        let canonical = |m: &crate::Mandate| m.canonical().map_err(|e| e.to_string());
        let digest =
            |m: &crate::Mandate| m.version().map(|v| v.digest()).map_err(|e| e.to_string());
        let mut stored = BTreeMap::new();
        for (old, new, _) in &pairs {
            stored.insert(digest(old)?, canonical(old)?);
            stored.insert(digest(new)?, canonical(new)?);
        }
        let documents = |d: &Digest| stored.get(d).cloned();
        for (n, (old, new, verdict)) in pairs.iter().enumerate() {
            let applied = Some(JournaledFact::AgentVersionActive {
                agent: AgentId::new("agent_a"),
                connection_id: new.connection_id.clone(),
                environment: new.environment,
                allocation_usd: new
                    .capital
                    .allocation_usd
                    .to_usd()
                    .map_err(|e| e.to_string())?,
                pinned: new
                    .universe
                    .pinned_instruments
                    .iter()
                    .map(|i| i.asset_id.clone())
                    .collect(),
            });
            for (result, fact) in [("applied", applied), ("rejected", None)] {
                for label in ["risk_increasing", "risk_reducing", "neutral"] {
                    let record = version_record(&digest(old)?, &digest(new)?, label, result)
                        .map_err(|e| e.to_string())?;
                    let got = JournaledFact::from_record(
                        "MandateVersionApplied",
                        &record,
                        &documents,
                        None,
                    );
                    let want = if label == *verdict {
                        Ok(fact.clone())
                    } else {
                        Err(SpecError::InvalidInput {
                            what: "classification",
                        })
                    };
                    assert_eq!(
                        got, want,
                        "pair {n} ({verdict}), {result}, labelled {label}"
                    );
                }
            }
        }
        Ok(())
    }

    /// A version's document must be stored and hash to the digest that names it: an absent one, or
    /// an impostor stored under another document's digest, is refused at the member that names it,
    /// `old_version` before `new_version`, never mapped from whatever is stored (#482 round 1, m4;
    /// as `AgentDeployed`).
    #[test]
    fn a_version_document_that_is_absent_or_an_impostor_is_refused() -> Result<(), String> {
        let old = mandate(&[])?;
        let new = mandate(&[("/name", r#""renamed""#)])?;
        let other = mandate(&[("/name", r#""impostor""#)])?;
        let canonical = |m: &crate::Mandate| m.canonical().map_err(|e| e.to_string());
        let digest =
            |m: &crate::Mandate| m.version().map(|v| v.digest()).map_err(|e| e.to_string());
        let record = version_record(&digest(&old)?, &digest(&new)?, "neutral", "applied")
            .map_err(|e| e.to_string())?;
        let honest = [
            (digest(&old)?, canonical(&old)?),
            (digest(&new)?, canonical(&new)?),
        ];
        let cases = [
            (vec![], "old_version"),
            (vec![honest[1].clone()], "old_version"),
            (vec![honest[0].clone()], "new_version"),
            (
                vec![(digest(&old)?, canonical(&other)?), honest[1].clone()],
                "old_version",
            ),
            (
                vec![honest[0].clone(), (digest(&new)?, canonical(&other)?)],
                "new_version",
            ),
        ];
        for (documents, what) in cases {
            let stored: BTreeMap<Digest, Value> = documents.into_iter().collect();
            let got = JournaledFact::from_record(
                "MandateVersionApplied",
                &record,
                &|d: &Digest| stored.get(d).cloned(),
                None,
            );
            assert_eq!(got, Err(SpecError::InvalidInput { what }), "{what}");
        }
        Ok(())
    }

    /// `result` is read exhaustively: `applied` maps, `rejected` maps to none, and any other value is
    /// refused at `result`, never read as a rejection (#497 round 1, m2).
    #[test]
    fn a_version_record_with_an_unknown_result_is_refused() -> Result<(), String> {
        let old = mandate(&[])?;
        let new = mandate(&[("/name", r#""renamed""#)])?;
        let canonical = |m: &crate::Mandate| m.canonical().map_err(|e| e.to_string());
        let digest =
            |m: &crate::Mandate| m.version().map(|v| v.digest()).map_err(|e| e.to_string());
        let stored: BTreeMap<Digest, Value> = [
            (digest(&old)?, canonical(&old)?),
            (digest(&new)?, canonical(&new)?),
        ]
        .into_iter()
        .collect();
        let record = version_record(&digest(&old)?, &digest(&new)?, "neutral", "pending")
            .map_err(|e| e.to_string())?;
        assert_eq!(
            JournaledFact::from_record(
                "MandateVersionApplied",
                &record,
                &|d: &Digest| stored.get(d).cloned(),
                None
            ),
            Err(SpecError::InvalidInput { what: "result" })
        );
        Ok(())
    }

    /// A member the mapping cannot read is refused under its own name, never skipped and never
    /// defaulted (DEC-303 item 10): an unparsable date, a non-text `params` element, an absent
    /// `admits_instruments`, an empty `kind`, and a bare hex digest (#461 round 1, M3, m1, m2).
    #[test]
    fn each_unreadable_member_is_refused_by_name() -> Result<(), SpecError> {
        let stopped = object(&[
            ("agent_id", text("agent_a")),
            ("connection_id", text("conn_1")),
            ("reason", text("owner_stop")),
            ("retired_on", text("2026-02-30")),
            ("loss_added", text("125.5")),
        ])?;
        let registration = |kind: &str, params: Vec<Value>, admits: Option<bool>| {
            let mut members = vec![
                ("kind", text(kind)),
                ("content_hash", text(&digest_text('c'))),
                ("model_id", text("quant.momentum")),
                ("model_version", text("1.2.0")),
                ("params", Value::Array(params)),
            ];
            if let Some(admits) = admits {
                members.push(("admits_instruments", Value::Bool(admits)));
            }
            object(&members)
        };
        let accepted = registration("model_version", vec![text("lookback_bars")], Some(true))?;
        assert!(
            JournaledFact::from_record("ConfigSnapshotRegistered", &accepted, &nothing, None)?
                .is_some(),
            "the well-formed registration maps"
        );
        let disclosure = object(&[
            ("document", text("terms")),
            ("version", text(&"d".repeat(64))),
            ("user", text("user_owner_01")),
        ])?;
        let cases = [
            ("AgentStopped", stopped, "retired_on"),
            (
                "ConfigSnapshotRegistered",
                registration(
                    "model_version",
                    vec![text("lookback_bars"), Value::Bool(true)],
                    Some(true),
                )?,
                "params",
            ),
            (
                "ConfigSnapshotRegistered",
                registration("model_version", vec![text("lookback_bars")], None)?,
                "admits_instruments",
            ),
            (
                "ConfigSnapshotRegistered",
                registration("", vec![], None)?,
                "kind",
            ),
            ("DisclosureAccepted", disclosure, "version"),
        ];
        for (event_type, payload, member) in cases {
            assert_eq!(refused(event_type, &payload), Some(member), "{event_type}");
        }
        Ok(())
    }
}

/// The thesis registration's re-derivation (DEC-414): pending until its implementation PR.
#[cfg(test)]
mod thesis_tests {
    use std::collections::BTreeMap;

    use mandate_canon::{Digest, Int, Key, Value};

    use super::check_thesis_record;
    use crate::validate::tests::mandate;
    use crate::{Mandate, SpecError};

    const RESEARCH: &str =
        r#"{"interval_s": 3600, "cost_cap_usd_per_day": "5", "max_revisions_per_lineage": 3}"#;

    /// The test base made a research agent's mandate: unpinned, `behavior.research` set, and its one
    /// signal model admitting instruments (§8.5 checks 4 and 5 pass), `us_equity` only, admission
    /// `ask`, a revision cap of 3.
    ///
    /// These documents are parsed, not validated, and are not valid mandates: the admitting model
    /// keeps the base's `quant.momentum` id, where §8.4 and V-036 want an `llm.` one. That is
    /// acceptable here because `check_thesis_record` reads only typed fields and no V-rule; the
    /// backlog records making them valid (#510 round 1, minor 1, as #482 round 2's m1 for
    /// `record_tests`). The base itself, as `unresearched`, fails check 4 on both disjuncts (no
    /// `behavior.research` and no admitting model), so no case tells them apart; V-036 makes them one
    /// condition for a validated mandate, as `mandate-research` notes.
    const RESEARCH_PATCHES: [(&str, &str); 3] = [
        ("/universe/pinned", "false"),
        ("/behavior/research", RESEARCH),
        ("/behavior/signal_models/0/admits_instruments", "true"),
    ];

    fn research(extra: &[(&str, &str)]) -> Result<Mandate, String> {
        let mut patches = RESEARCH_PATCHES.to_vec();
        patches.extend_from_slice(extra);
        mandate(&patches)
    }

    /// A §9.4 payload with every member, admitted unless `reason` is given.
    fn thesis(reason: Option<&str>, revision: u64, asset_class: &str) -> Result<Value, String> {
        let text = |v: &str| Value::Str(v.to_owned());
        let int = |n: u64| Int::new(n).map(Value::Int).ok_or("an integer");
        let digest = |c: char| text(&format!("sha256:{}", c.to_string().repeat(64)));
        let predecessor = if revision > 0 {
            text("th_0")
        } else {
            Value::Null
        };
        let members = [
            ("model_id", text("quant.momentum")),
            ("model_version", text("1.0.0")),
            ("content_hash", digest('2')),
            ("thesis_id", text("th_1")),
            ("lineage_id", text("th_0")),
            ("revision", int(revision)?),
            ("predecessor_thesis_id", predecessor),
            ("autopsy_ref", Value::Null),
            (
                "instrument_id",
                text("7b4a1c2e-5555-4a2b-9c3d-000000000005"),
            ),
            ("asset_class", text(asset_class)),
            ("direction", text("long")),
            ("as_of", text("2026-09-22T14:00:00.000000000Z")),
            ("expires_at", text("2026-09-23T14:00:00.000000000Z")),
            ("horizon_s", int(86_400)?),
            ("conviction", text("0.7")),
            ("confidence", text("0.8")),
            ("evidence_ref", Value::Null),
            ("evidence_sources", Value::Array(vec![text("src.filings")])),
            ("corroboration", text("market_data")),
            ("invalidation", text("Guidance is cut.")),
            ("allowlist_version", int(1)?),
            ("prompt_ref", digest('a')),
            ("response_ref", digest('b')),
            ("admitted", Value::Bool(reason.is_none())),
            ("reason", reason.map_or(Value::Null, text)),
        ];
        let mut out = mandate_canon::Object::new();
        for (name, value) in members {
            out.insert(Key::new(name).map_err(|_| "key")?, value);
        }
        Ok(Value::Object(out))
    }

    /// The stored documents, each under its own version's digest.
    struct Store(BTreeMap<Digest, Value>);

    impl Store {
        fn of<const N: usize>(mandates: [&Mandate; N]) -> Result<(Self, [Digest; N]), String> {
            let mut stored = BTreeMap::new();
            let mut digests = Vec::new();
            for m in mandates {
                let digest = m.version().map_err(|e| e.to_string())?.digest();
                stored.insert(digest, m.canonical().map_err(|e| e.to_string())?);
                digests.push(digest);
            }
            let digests = digests
                .try_into()
                .map_err(|_| "one digest per mandate".to_owned())?;
            Ok((Self(stored), digests))
        }

        fn judge(&self, payload: &Value, version: &Digest) -> Result<(), SpecError> {
            check_thesis_record(payload, version, &|d: &Digest| self.0.get(d).cloned())
        }
    }

    fn refused(what: &'static str) -> Result<(), SpecError> {
        Err(SpecError::InvalidInput { what })
    }

    /// Every verdict the mandate allows is accepted: an admission under the research mandate, a
    /// refusal at a check the mandate fails, a refusal earlier than it, and a refusal at a check only
    /// the overlay or the folded lineage decides (4, and 16 under the cap). The mandates are the
    /// mandate reference cases' shapes: pinned (MI-20, MC-N08), admission `deny` (MC-N15), and a
    /// revision past the cap (MC-N24).
    #[test]
    fn a_thesis_whose_verdict_its_mandate_allows_is_accepted() -> Result<(), String> {
        let open = research(&[])?;
        let pinned = research(&[("/universe/pinned", "true")])?;
        let denied = research(&[("/autonomy/admission", r#""deny""#)])?;
        let (store, v) = Store::of([&open, &pinned, &denied])?;
        let cases = [
            ("admitted", thesis(None, 0, "us_equity")?, &v[0]),
            (
                "refused later, mandate passes",
                thesis(Some("eligibility_floor"), 0, "us_equity")?,
                &v[0],
            ),
            (
                "refused at check 4 the overlay decides",
                thesis(Some("research_disabled"), 0, "us_equity")?,
                &v[0],
            ),
            ("admitted at the cap", thesis(None, 3, "us_equity")?, &v[0]),
            (
                "retired under the cap",
                thesis(Some("lineage_retired"), 1, "us_equity")?,
                &v[0],
            ),
            (
                "retired past the cap",
                thesis(Some("lineage_retired"), 4, "us_equity")?,
                &v[0],
            ),
            (
                "refused at check 10",
                thesis(Some("not_allowed_asset_class"), 0, "crypto")?,
                &v[0],
            ),
            (
                "pinned, refused at check 5",
                thesis(Some("universe_pinned"), 0, "us_equity")?,
                &v[1],
            ),
            (
                "pinned, ignored at check 1",
                thesis(Some("direction_not_allowed"), 0, "us_equity")?,
                &v[1],
            ),
            (
                "denied, refused at check 6",
                thesis(Some("admission_denied"), 0, "us_equity")?,
                &v[2],
            ),
            (
                "denied, refused at check 4 before it",
                thesis(Some("research_disabled"), 0, "us_equity")?,
                &v[2],
            ),
        ];
        assert_eq!(
            store.judge(&thesis(None, 0, "us_equity")?, &v[1]),
            Err(SpecError::InvalidInput { what: "admitted" }),
            "the paired control: an admission under a pinned universe is refused"
        );
        for (name, payload, version) in cases {
            assert_eq!(store.judge(&payload, version), Ok(()), "{name}");
        }
        Ok(())
    }

    /// An admission past any check the mandate fails is refused at `admitted`: a pinned universe
    /// (check 5, MI-20), admission `deny` (6), an asset class outside `universe.asset_classes` (10),
    /// a revision past the cap (16), and no research agent at all (4).
    #[test]
    fn a_thesis_admitted_past_a_check_its_mandate_fails_is_refused() -> Result<(), String> {
        let open = research(&[])?;
        let pinned = research(&[("/universe/pinned", "true")])?;
        let denied = research(&[("/autonomy/admission", r#""deny""#)])?;
        let unresearched = mandate(&[("/universe/pinned", "false")])?;
        let (store, v) = Store::of([&open, &pinned, &denied, &unresearched])?;
        let cases = [
            ("check 5", thesis(None, 0, "us_equity")?, &v[1]),
            ("check 6", thesis(None, 0, "us_equity")?, &v[2]),
            ("check 10", thesis(None, 0, "crypto")?, &v[0]),
            ("check 16", thesis(None, 4, "us_equity")?, &v[0]),
            ("check 4", thesis(None, 0, "us_equity")?, &v[3]),
        ];
        assert_eq!(
            store.judge(&thesis(None, 0, "us_equity")?, &v[0]),
            Ok(()),
            "the paired control: an admission the mandate allows is accepted"
        );
        for (name, payload, version) in cases {
            assert_eq!(
                store.judge(&payload, version),
                refused("admitted"),
                "{name}"
            );
        }
        Ok(())
    }

    /// A refusal at a check later than the first one the mandate fails is refused at `reason`: the
    /// first failure decides (§8.5), and the mandate's checks come before eligibility, sources, the
    /// cap and a full universe.
    #[test]
    fn a_thesis_refused_after_its_mandate_fails_an_earlier_check_is_refused() -> Result<(), String>
    {
        let open = research(&[])?;
        let pinned = research(&[("/universe/pinned", "true")])?;
        let denied = research(&[("/autonomy/admission", r#""deny""#)])?;
        let both = research(&[
            ("/universe/pinned", "true"),
            ("/autonomy/admission", r#""deny""#),
        ])?;
        let unresearched = mandate(&[("/universe/pinned", "false")])?;
        let (store, v) = Store::of([&open, &pinned, &denied, &both, &unresearched])?;
        let cases = [
            (
                "pinned and denied, refused at 6",
                thesis(Some("admission_denied"), 0, "us_equity")?,
                &v[3],
            ),
            (
                "pinned, refused at 12",
                thesis(Some("eligibility_floor"), 0, "us_equity")?,
                &v[1],
            ),
            (
                "pinned, refused at 6",
                thesis(Some("admission_denied"), 0, "us_equity")?,
                &v[1],
            ),
            (
                "denied, refused at 17",
                thesis(Some("universe_full"), 0, "us_equity")?,
                &v[2],
            ),
            (
                "crypto, refused at 14",
                thesis(Some("source_not_allowlisted"), 0, "crypto")?,
                &v[0],
            ),
            (
                "past the cap, refused at 17",
                thesis(Some("universe_full"), 4, "us_equity")?,
                &v[0],
            ),
            (
                "no research agent, refused at 17",
                thesis(Some("universe_full"), 0, "us_equity")?,
                &v[4],
            ),
        ];
        assert_eq!(
            store.judge(&thesis(Some("universe_pinned"), 0, "us_equity")?, &v[1]),
            Ok(()),
            "the paired control: a refusal at the first check the mandate fails is accepted"
        );
        for (name, payload, version) in cases {
            assert_eq!(store.judge(&payload, version), refused("reason"), "{name}");
        }
        Ok(())
    }

    /// A refusal at check 5, 6, or 10 the mandate passes is refused at `reason`: those three read
    /// the mandate alone (check 6 while the overlay raises `auto` only to `ask`), so the record
    /// names a failure its own mandate does not show.
    #[test]
    fn a_thesis_refused_at_a_mandate_check_its_mandate_passes_is_refused() -> Result<(), String> {
        let open = research(&[])?;
        let (store, v) = Store::of([&open])?;
        assert_eq!(
            store.judge(&thesis(Some("eligibility_floor"), 0, "us_equity")?, &v[0]),
            Ok(()),
            "the paired control: a refusal at a check the mandate does not decide is accepted"
        );
        for reason in [
            "universe_pinned",
            "admission_denied",
            "not_allowed_asset_class",
        ] {
            assert_eq!(
                store.judge(&thesis(Some(reason), 0, "us_equity")?, &v[0]),
                refused("reason"),
                "{reason}"
            );
        }
        Ok(())
    }

    /// The mandate is the stored document `config_refs.mandate_version` names, the one in force when
    /// the thesis was judged: one admission is accepted under the research mandate and refused under
    /// a pinned one, both stored, so the check reads the named document and no other.
    #[test]
    fn a_thesis_is_judged_under_the_mandate_it_names() -> Result<(), String> {
        let open = research(&[])?;
        let pinned = research(&[("/universe/pinned", "true")])?;
        let (store, v) = Store::of([&open, &pinned])?;
        let admitted = thesis(None, 0, "us_equity")?;
        assert_eq!(store.judge(&admitted, &v[0]), Ok(()));
        assert_eq!(store.judge(&admitted, &v[1]), refused("admitted"));
        Ok(())
    }

    /// A mandate that is not stored, or is stored under another document's digest, is refused at
    /// `mandate_version`, never judged from whatever is stored.
    #[test]
    fn a_thesis_mandate_that_is_absent_or_an_impostor_is_refused() -> Result<(), String> {
        let open = research(&[])?;
        let pinned = research(&[("/universe/pinned", "true")])?;
        let (_, v) = Store::of([&open, &pinned])?;
        let pinned_document = pinned.canonical().map_err(|e| e.to_string())?;
        let impostor = Store(BTreeMap::from([(v[0], pinned_document)]));
        let empty = Store(BTreeMap::new());
        let refused_record = thesis(Some("eligibility_floor"), 0, "us_equity")?;
        let real = Store::of([&open])?.0;
        assert_eq!(
            real.judge(&refused_record, &v[0]),
            Ok(()),
            "the paired control: the named mandate, stored under its own digest, is judged"
        );
        for (name, store) in [("absent", &empty), ("impostor", &impostor)] {
            assert_eq!(
                store.judge(&refused_record, &v[0]),
                refused("mandate_version"),
                "{name}"
            );
        }
        Ok(())
    }
}
