//! The cell's research source (DEC-2214 items 2 to 4): each pass's context read from the recorded
//! bundle, the deployment's mandate document, and the workspace's control and agent streams. A
//! fact it cannot read gives the pass no context; it never fills one in (DEC-2210 item 4).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::RangeInclusive;
use std::rc::Rc;

use mandate_canon::{Digest, Value, parse};
use mandate_gateway::meter::SharedMeter;
use mandate_journal::{ArtifactSource, Environment, StoredEvent, StreamId};
use mandate_num::Usd;
use mandate_research::{
    AdmissionFacts, AssetClass, AssetId, AutonomyDecision, GroupId, LineageState, MandateEnvelope,
    PolicyOverlay, ReplayError, ResearchEnvelope, SourceAllowlist, ThesisRecord, ValidatedMandate,
    WorkingUniverse, replay_theses,
};
use mandate_research_run::{InstrumentFact, ItemKind, RetrievedItem, Symbol};
use mandate_runtime::{
    ActiveThesis, EventTriggers, JournalCatalog, Preconditions, RiskClock, RuntimeState,
};
use mandate_spec::Mandate;
use mandate_spec::document::EventTrigger;
use mandate_spec::risk::risk_day;
use mandate_store::Store;
use mandate_time::UtcNanos;

use crate::deployment;
use crate::meter::CellMeter;
use crate::recorded::{Bundle, FeedItem};
use crate::runtime::{ContextUnread, ResearchParts, ResearchSource, StoredNews};

/// The `event_time` of `agent`'s latest research `MeterReserved` on the workspace's meter stream
/// `meter` (DEC-2259 item 3), as the risk clock's second; `None` before the first. Other agents' and
/// other purposes' reservations, and the stream's other records, are passed over.
///
/// # Errors
/// [`FoldError::Unreadable`] naming the member when a `MeterReserved` gives no readable
/// `agent_id`, `purpose` or `event_time`.
pub fn last_research_reservation(
    meter: &[StoredEvent],
    agent: &str,
) -> Result<Option<RiskClock>, FoldError> {
    let unreadable = |member| FoldError::Unreadable { member };
    let mut last = None;
    for row in meter.iter().filter(|row| row.event_type == "MeterReserved") {
        let body = parse(&row.body).map_err(|_| unreadable("payload"))?;
        let payload = body.get("payload").ok_or(unreadable("payload"))?;
        let mine = match payload.get("agent_id") {
            Some(Value::Str(id)) => id == agent,
            Some(Value::Null) => false,
            _ => return Err(unreadable("agent_id")),
        };
        let research = payload
            .get("purpose")
            .and_then(Value::as_str)
            .ok_or(unreadable("purpose"))?
            == "research";
        let at = body
            .get("event_time")
            .and_then(Value::as_str)
            .and_then(|text| UtcNanos::parse_rfc3339(text).ok())
            .ok_or(unreadable("event_time"))?;
        if mine && research {
            last = last.max(Some(RiskClock::from_secs(at.secs())));
        }
    }
    Ok(last)
}

/// Where the source reads a stream's records: the process's store, or a read connection of its own.
pub trait Rows {
    /// The stream's records, or `None` when they cannot be read.
    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>>;
}

impl<S: Store> Rows for S {
    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        Store::rows(self, stream)
    }
}

/// Why a mandate document's research envelope cannot be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    /// The member at this path is missing or out of its range.
    Unreadable { member: &'static str },
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { member } => {
                write!(f, "the mandate document's `{member}` cannot be read")
            }
        }
    }
}

impl std::error::Error for SourceError {}

/// Who the source reads for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFor {
    pub workspace: String,
    pub agent: String,
    pub environment: Environment,
}

/// What an agent stream's own research records fold to (DEC-2249 items 1 and 2): the working
/// universe, the lineages, and the active theses the next pass is handed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchFold {
    pub universe: WorkingUniverse,
    pub lineages: LineageState,
    pub active: Vec<ActiveThesis>,
}

/// Why an agent stream's research records do not fold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldError {
    /// The fold is not implemented yet (E21-59, DEC-2249).
    Unimplemented,
    /// A record's member, by name, is missing or not what it must be, or the bundle names no
    /// symbol for an admitted thesis's instrument (`symbol`).
    Unreadable { member: &'static str },
}

/// Folds the agent stream's own research records, in stream order, as the core judged them
/// (DEC-2249 items 1 and 2). Each `ThesisProposed` and `ThesisRevised` replays its recorded
/// decision into its lineage through `LineageState::replay`; an admitted one becomes its
/// instrument's one universe entry, replacing whatever thesis held it; a `ThesisInvalidated` marks
/// the entry of the thesis it names, when that thesis still holds its instrument. The entries then
/// go through `expire_theses` at `at`, the core's own expiry, and the survivors are the universe
/// and the active theses, each with the symbol `instruments` gives its instrument. Nothing is
/// judged again.
///
/// # Errors
/// [`FoldError::Unreadable`] naming the first member a record cannot give, including an admitted
/// record with a `reason`, a refused one without one, or a reason that is not a refusal's code.
pub fn fold_research(
    agent: &[StoredEvent],
    at: UtcNanos,
    instruments: &[InstrumentFact],
) -> Result<ResearchFold, FoldError> {
    let unreadable = |member| FoldError::Unreadable { member };
    let records = agent
        .iter()
        .filter(|row| {
            matches!(
                row.event_type.as_str(),
                "ThesisProposed" | "ThesisRevised" | "ThesisInvalidated"
            )
        })
        .map(|row| {
            let payload = parse(&row.body)
                .ok()
                .and_then(|body| body.get("payload").cloned())
                .ok_or(ReplayError::Unreadable { member: "payload" })?;
            Ok(ThesisRecord {
                event_type: row.event_type.as_str(),
                payload,
            })
        });
    let replayed = replay_theses(records, at)
        .map_err(|ReplayError::Unreadable { member }| unreadable(member))?;
    let active = replayed
        .active
        .iter()
        .map(|entry| {
            let symbol = instruments
                .iter()
                .find(|fact| fact.asset_id.as_str() == entry.instrument.as_str())
                .map(|fact| fact.symbol.clone())
                .ok_or(unreadable("symbol"))?;
            Ok(ActiveThesis {
                thesis_id: entry.thesis_id.as_str().to_owned(),
                lineage_id: entry.lineage_id.as_str().to_owned(),
                revision: entry.revision,
                instrument: entry.instrument.clone(),
                symbol,
                expires_at: RiskClock::from_secs(entry.expires_at.secs()),
            })
        })
        .collect::<Result<Vec<_>, FoldError>>()?;
    Ok(ResearchFold {
        universe: replayed.universe,
        lineages: replayed.lineages,
        active,
    })
}

/// The cell's research source.
pub struct CellSource {
    who: SourceFor,
    allowlist: SourceAllowlist,
    /// The bundle's halted instruments, or `None` when one cannot be read as an asset, which gives
    /// every pass no context.
    halted: Option<BTreeSet<AssetId>>,
    instruments: Vec<InstrumentFact>,
    feed: Vec<FeedItem>,
    rows: Rc<dyn Rows>,
    documents: Box<dyn ArtifactSource>,
    meter: CellMeter,
}

impl CellSource {
    /// The source over `bundle`'s recorded facts, reading the streams through `rows` and the
    /// mandate documents from `documents`, with a meter of its own that holds no reservation, so
    /// every reserved call on the risk day gives the pass no context (DEC-2249 item 3).
    pub fn new(
        bundle: &Bundle,
        who: SourceFor,
        rows: Box<dyn Rows>,
        documents: Box<dyn ArtifactSource>,
    ) -> Self {
        let rows: Rc<dyn Rows> = Rc::from(rows);
        let meter = CellMeter::new(
            SharedMeter::new(),
            Rc::clone(&rows),
            who.clone(),
            Rc::new(|| UtcNanos::EPOCH),
        );
        Self::metered(bundle, who, rows, documents, meter)
    }

    /// The source over `bundle`'s recorded facts, reading the streams through `rows` and the
    /// mandate documents from `documents`, and the agent's research spend from `meter`, the store
    /// the worker's gateway reserves through (DEC-2249 item 3). A call the meter reserved then
    /// counts at its settled cost, or its maximum while open, and gives the pass no context only
    /// while the meter is held.
    pub fn metered(
        bundle: &Bundle,
        who: SourceFor,
        rows: Rc<dyn Rows>,
        documents: Box<dyn ArtifactSource>,
        meter: CellMeter,
    ) -> Self {
        Self {
            who,
            allowlist: bundle.allowlist.clone(),
            halted: bundle
                .halted
                .iter()
                .map(|id| AssetId::new(id.as_str()).ok())
                .collect(),
            instruments: bundle.plane.instruments.clone(),
            feed: bundle.feed.clone(),
            rows,
            documents,
            meter,
        }
    }

    /// Today's research spend: the recorded costs of the unreserved calls the agent stream records
    /// since the risk day of `clock` began, which counts a record the lagging clock has not reached
    /// yet, plus the meter's research spend for that day, which counts every reserved call at what
    /// the meter holds for it (DEC-2249 item 3). `None` when the stream holds an unreserved
    /// cost or a time it cannot read, on any day, or while the meter is held.
    fn spend(&self, agent: &[StoredEvent], clock: RiskClock) -> Option<Usd> {
        let now = UtcNanos::from_parts(clock.secs(), 0).ok()?;
        let day = risk_day(now).ok()?;
        let mut spend = Usd::parse("0").ok()?;
        for row in agent {
            if row.event_type != "ModelInvocationRecorded" {
                continue;
            }
            let body = parse(&row.body).ok()?;
            let payload = body.get("payload")?;
            if !matches!(payload.get("reservation"), Some(Value::Null)) {
                continue;
            }
            let cost = Usd::parse(payload.get("cost_usd").and_then(Value::as_str)?)
                .ok()
                .filter(|cost| !cost.is_negative())?;
            let at = body
                .get("event_time")
                .and_then(Value::as_str)
                .and_then(|text| UtcNanos::parse_rfc3339(text).ok())?;
            if day.starts_at <= at {
                spend = spend.checked_add(cost).ok()?;
            }
        }
        spend.checked_add(self.meter.research_spend(now)?).ok()
    }

    /// Whether an agent other than this one is deployed in this environment, or `None` when a
    /// deployment record cannot be read.
    fn others_deployed(&self, control: &[StoredEvent]) -> Option<bool> {
        let mut latest = BTreeMap::new();
        for row in control.iter().filter(|row| {
            row.environment == self.who.environment.as_str()
                && matches!(row.event_type.as_str(), "AgentDeployed" | "AgentStopped")
        }) {
            let body = parse(&row.body).ok()?;
            let agent = body
                .get("payload")
                .and_then(|payload| payload.get("agent_id"))
                .and_then(Value::as_str)?
                .to_owned();
            latest.insert(agent, row.event_type == "AgentDeployed");
        }
        Some(
            latest
                .iter()
                .any(|(agent, deployed)| *deployed && *agent != self.who.agent),
        )
    }
}

impl CellSource {
    /// The feed's items that have arrived by `clock` for this agent's latest deployment in this
    /// environment, each at its arrival (DEC-2392), or `None` when that deployment's `event_time`
    /// cannot be read.
    fn arrived(&self, control: &[StoredEvent], clock: RiskClock) -> Option<Vec<RetrievedItem>> {
        let mut deployed_at = None;
        for row in control.iter().rev().filter(|row| {
            row.environment == self.who.environment.as_str() && row.event_type == "AgentDeployed"
        }) {
            let body = parse(&row.body).ok()?;
            if body
                .get("payload")
                .and_then(|payload| payload.get("agent_id"))
                .and_then(Value::as_str)?
                == self.who.agent
            {
                deployed_at = Some(
                    body.get("event_time")
                        .and_then(Value::as_str)
                        .and_then(|text| UtcNanos::parse_rfc3339(text).ok())?,
                );
                break;
            }
        }
        let deployed_at = deployed_at?;
        Some(
            self.feed
                .iter()
                .filter_map(|item| item.arrived(deployed_at))
                .filter(|item| item.knowledge_at.secs() <= clock.secs())
                .map(RetrievedItem::News)
                .collect(),
        )
    }
}

impl ResearchSource for CellSource {
    fn parts(&self, state: &RuntimeState) -> Result<ResearchParts, ContextUnread> {
        let named = |text: String| StreamId::parse(&text).ok_or(ContextUnread::StreamName);
        let control = named(format!("ctl:{}", self.who.workspace))?;
        let stream = named(format!("agent:{}:{}", self.who.workspace, self.who.agent))?;
        let meter = named(format!("meter:{}", self.who.workspace))?;
        let control = self.rows.rows(&control).ok_or(ContextUnread::Control)?;
        let agent = self.rows.rows(&stream).ok_or(ContextUnread::AgentStream)?;
        let meter = self.rows.rows(&meter).ok_or(ContextUnread::Meter)?;
        let last_reservation = last_research_reservation(&meter, &self.who.agent)
            .map_err(|_| ContextUnread::Reservation)?;
        let (_, document) = deployment::document(
            &control,
            &self.who.agent,
            self.who.environment,
            self.documents.as_ref(),
        )
        .map_err(|_| ContextUnread::Deployment)?;
        let envelope = envelope(&document).map_err(|_| ContextUnread::Envelope)?;
        let event_triggers = event_triggers(&document).map_err(|_| ContextUnread::EventTriggers)?;
        if self
            .others_deployed(&control)
            .ok_or(ContextUnread::Deployments)?
        {
            return Err(ContextUnread::AnotherAgent);
        }
        let clock = state.risk_clock().ok_or(ContextUnread::Clock)?;
        let spend = self.spend(&agent, clock).ok_or(ContextUnread::Spend)?;
        let asset = |fact: &InstrumentFact| AssetId::new(fact.asset_id.as_str()).ok();
        let pinned = envelope.universe_pinned;
        let fold = fold_research(
            &agent,
            UtcNanos::from_parts(clock.secs(), 0).map_err(|_| ContextUnread::Clock)?,
            &self.instruments,
        )
        .map_err(|_| ContextUnread::Theses)?;
        if pinned && (fold.lineages != LineageState::default() || !fold.active.is_empty()) {
            return Err(ContextUnread::PinnedWithTheses);
        }
        let eligibility_failures = self
            .instruments
            .iter()
            .filter(|fact| !fact.eligible)
            .map(asset)
            .collect::<Option<_>>()
            .ok_or(ContextUnread::InstrumentFacts)?;
        let instrument_groups = self
            .instruments
            .iter()
            .filter_map(|fact| Some((asset(fact), GroupId::new(fact.group.as_ref()?).ok())))
            .map(|(asset, group)| Some((asset?, group?)))
            .collect::<Option<_>>()
            .ok_or(ContextUnread::InstrumentFacts)?;
        let halted_instruments = self.halted.clone().ok_or(ContextUnread::Halted)?;
        let data_universe = self
            .instruments
            .iter()
            .map(asset)
            .collect::<Option<_>>()
            .ok_or(ContextUnread::InstrumentFacts)?;
        let events = if event_triggers.is_some() {
            self.arrived(&control, clock)
                .ok_or(ContextUnread::Arrivals)?
        } else {
            Vec::new()
        };
        Ok(ResearchParts {
            preconditions: Preconditions::read(&JournalCatalog::JOURNAL),
            mandate: ValidatedMandate::from_validated_envelope(envelope),
            overlay: PolicyOverlay::internal_research_profile(),
            universe: if pinned {
                WorkingUniverse::Known {
                    instruments: BTreeSet::new(),
                    pinned,
                }
            } else {
                fold.universe
            },
            facts: AdmissionFacts {
                allowlist: self.allowlist.clone(),
                eligibility_failures,
                instrument_groups,
                claimed_by_other_agents: BTreeSet::new(),
                halted_instruments,
                disclosures_accepted: BTreeSet::new(),
                data_universe: Some(data_universe),
                research_spend_usd_today: spend,
            },
            lineages: fold.lineages,
            active: fold.active,
            last_reservation,
            data_symbols: self
                .instruments
                .iter()
                .map(|fact| fact.symbol.clone())
                .collect::<BTreeSet<Symbol>>(),
            events,
            event_triggers,
            stored: StoredNews::default(),
        })
    }
}

/// The version's `behavior.research.event_triggers` (mandate spec v0.11 §8.4, DEC-2392) as the
/// core reads it, `None` when the member is absent or `null`.
///
/// # Errors
/// [`SourceError::Unreadable`] when the document does not parse as a mandate.
pub fn event_triggers(document: &Value) -> Result<Option<EventTriggers>, SourceError> {
    let mandate = Mandate::parse(document).map_err(|_| SourceError::Unreadable {
        member: "event_triggers",
    })?;
    Ok(mandate
        .behavior
        .research
        .and_then(|research| research.event_triggers)
        .map(|triggers| EventTriggers {
            kinds: triggers
                .sources
                .iter()
                .map(|source| match source {
                    EventTrigger::Filings => ItemKind::Filing,
                    EventTrigger::News => ItemKind::News,
                })
                .collect(),
            debounce_s: triggers.debounce_s,
            max_runs_per_day: triggers.max_runs_per_day,
        }))
}

/// The research envelope of a mandate `document` (mandate spec §8.5 and §8.6): the universe's
/// fields, whether any signal model admits instruments, `behavior.research`, and the admission
/// autonomy decision.
///
/// # Errors
/// [`SourceError::Unreadable`] naming the first member that is missing or out of its range.
pub fn envelope(document: &Value) -> Result<MandateEnvelope, SourceError> {
    let at = |path: &'static str| {
        path.split('.')
            .try_fold(document, |value, key| value.get(key))
            .ok_or(SourceError::Unreadable { member: path })
    };
    let flag = |path: &'static str| match at(path)? {
        Value::Bool(flag) => Ok(*flag),
        _ => Err(SourceError::Unreadable { member: path }),
    };
    let ranged = |value: &Value, path: &'static str, range: RangeInclusive<u32>| {
        value
            .as_int()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| range.contains(n))
            .ok_or(SourceError::Unreadable { member: path })
    };
    let asset_classes = at("universe.asset_classes")?
        .as_array()
        .ok_or(SourceError::Unreadable {
            member: "universe.asset_classes",
        })?
        .iter()
        .map(|class| match class.as_str() {
            Some("us_equity") => Ok(AssetClass::UsEquity),
            Some("crypto") => Ok(AssetClass::Crypto),
            _ => Err(SourceError::Unreadable {
                member: "universe.asset_classes",
            }),
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if asset_classes.is_empty() {
        return Err(SourceError::Unreadable {
            member: "universe.asset_classes",
        });
    }
    let disclosure = match at("universe.leveraged_etp_disclosure_version")? {
        Value::Null => None,
        Value::Str(text) => Some(
            text.strip_prefix("sha256:")
                .and_then(Digest::from_hex)
                .ok_or(SourceError::Unreadable {
                    member: "universe.leveraged_etp_disclosure_version",
                })?,
        ),
        _ => {
            return Err(SourceError::Unreadable {
                member: "universe.leveraged_etp_disclosure_version",
            });
        }
    };
    let models = at("behavior.signal_models")?
        .as_array()
        .ok_or(SourceError::Unreadable {
            member: "behavior.signal_models",
        })?;
    let mut admits_instruments = false;
    for model in models {
        match model.get("admits_instruments") {
            Some(Value::Bool(admits)) => admits_instruments |= *admits,
            _ => {
                return Err(SourceError::Unreadable {
                    member: "behavior.signal_models",
                });
            }
        }
    }
    let research = match at("behavior.research")? {
        Value::Null => None,
        research => {
            let member = |key: &'static str, path: &'static str| {
                research
                    .get(key)
                    .ok_or(SourceError::Unreadable { member: path })
            };
            Some(ResearchEnvelope {
                interval_s: ranged(
                    member("interval_s", "behavior.research.interval_s")?,
                    "behavior.research.interval_s",
                    300..=604_800,
                )?,
                cost_cap_usd_per_day: member(
                    "cost_cap_usd_per_day",
                    "behavior.research.cost_cap_usd_per_day",
                )?
                .as_str()
                .and_then(|text| Usd::parse(text).ok())
                .ok_or(SourceError::Unreadable {
                    member: "behavior.research.cost_cap_usd_per_day",
                })?,
                max_revisions_per_lineage: ranged(
                    member(
                        "max_revisions_per_lineage",
                        "behavior.research.max_revisions_per_lineage",
                    )?,
                    "behavior.research.max_revisions_per_lineage",
                    0..=10,
                )?,
            })
        }
    };
    let admission = match at("autonomy.admission")?.as_str() {
        Some("auto") => AutonomyDecision::Auto,
        Some("ask") => AutonomyDecision::Ask,
        Some("deny") => AutonomyDecision::Deny,
        _ => {
            return Err(SourceError::Unreadable {
                member: "autonomy.admission",
            });
        }
    };
    Ok(MandateEnvelope {
        universe_pinned: flag("universe.pinned")?,
        max_instruments: ranged(
            at("universe.max_instruments")?,
            "universe.max_instruments",
            1..=20,
        )?,
        asset_classes,
        leveraged_etps_enabled: flag("universe.leveraged_etps_enabled")?,
        leveraged_etp_disclosure_version: disclosure,
        admits_instruments,
        research,
        admission,
    })
}
