//! The meter (inference spec §3.6, §7.3, E15-9): reservations before the first attempt,
//! settlements on completion, and caps checked against spend folded from the meter records alone
//! (journal spec §9.19). One meter writer per workspace; a writer that cannot read or append refuses
//! the call `meter_unavailable`, so nothing is sent.
//!
//! **Budgets are settings** ([DEC-2207](../../../docs/project/decisions/DEC-2207.md), DEC-2203 item
//! 9): no figure is in this crate. The deployment's monthly cap folds from the deployment meter
//! stream, a copy of every workspace's records, which each reservation reaches before the
//! workspace's own stream, so any failure between the two only over-counts.

use core::fmt;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use mandate_canon::{Digest, Value};
use mandate_time::Date;

use crate::kinds::{KeyOwner, Outcome, Purpose};
use crate::money::{MoneyError, Usage, Usd6};
use crate::provider::IdSource;
use crate::record::{model_value, tokens_value};
use crate::registry::{ModelRef, int, object};

/// A call's reservation of its maximum cost (`MeterReserved`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reservation {
    pub event_id: String,
    pub call_id: String,
    pub agent_id: Option<String>,
    pub purpose: Purpose,
    pub model: ModelRef,
    pub endpoint: String,
    pub key_owner: KeyOwner,
    pub price_table_ref: Digest,
    pub max_cost: Usd6,
    pub risk_day: Date,
}

/// A reserved call's actual cost (`MeterSettled`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settlement {
    pub event_id: String,
    pub call_id: String,
    pub reservation: String,
    pub outcome: Outcome,
    pub attempts: u32,
    pub tokens: Usage,
    pub cost: Usd6,
}

/// One record of the meter stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeterRecord {
    Reserved(Reservation),
    Settled(Settlement),
}

impl MeterRecord {
    pub fn event_id(&self) -> &str {
        match self {
            Self::Reserved(r) => &r.event_id,
            Self::Settled(s) => &s.event_id,
        }
    }

    pub fn event_type(&self) -> &'static str {
        match self {
            Self::Reserved(_) => "MeterReserved",
            Self::Settled(_) => "MeterSettled",
        }
    }

    /// The record a meter stream's `event_type` draft at `event_id` holds, read back from its §9.19
    /// `payload` (DEC-2258 item 3): the exact inverse of [`MeterRecord::payload`]. `None` for any
    /// other event type, or a payload with a member missing, a member §9.19 does not list, or a
    /// value of the wrong kind or out of its range.
    ///
    /// Read leniently, then kept only when its own payload is `payload` exactly, so whatever the
    /// payload holds that [`MeterRecord::payload`] would not write reads as nothing.
    pub fn from_journal(event_type: &str, event_id: &str, payload: &Value) -> Option<Self> {
        let text = |member: &str| payload.get(member).and_then(Value::as_str);
        let owned = |member: &str| text(member).map(str::to_owned);
        let digest =
            |reference: Option<&str>| Digest::from_hex(reference?.strip_prefix("sha256:")?);
        let record = match event_type {
            "MeterReserved" => Self::Reserved(Reservation {
                event_id: event_id.to_owned(),
                call_id: owned("call_id")?,
                agent_id: match payload.get("agent_id")? {
                    Value::Null => None,
                    Value::Str(agent) => Some(agent.clone()),
                    _ => return None,
                },
                purpose: [
                    Purpose::Research,
                    Purpose::LlmSignal,
                    Purpose::FastSignal,
                    Purpose::Compiler,
                ]
                .into_iter()
                .find(|purpose| Some(purpose.code()) == text("purpose"))?,
                model: {
                    let model = payload.get("model")?;
                    let member = |name: &str| model.get(name).and_then(Value::as_str);
                    ModelRef {
                        model_id: member("model_id")?.to_owned(),
                        model_version: member("model_version")?.to_owned(),
                        content_hash: digest(member("content_hash"))?,
                    }
                },
                endpoint: owned("endpoint")?,
                key_owner: [KeyOwner::Platform, KeyOwner::Customer]
                    .into_iter()
                    .find(|owner| Some(owner.code()) == text("key_owner"))?,
                price_table_ref: digest(text("price_table_ref"))?,
                max_cost: Usd6::parse(text("max_cost_usd")?).ok()?,
                risk_day: Date::parse(text("risk_day")?).ok()?,
            }),
            "MeterSettled" => Self::Settled(Settlement {
                event_id: event_id.to_owned(),
                call_id: owned("call_id")?,
                reservation: owned("reservation")?,
                outcome: Outcome::ALL
                    .into_iter()
                    .filter(|outcome| !outcome.is_refusal())
                    .find(|outcome| Some(outcome.code()) == text("outcome"))?,
                attempts: u32::try_from(payload.get("attempts")?.as_int()?).ok()?,
                tokens: {
                    let tokens = payload.get("tokens")?;
                    let count = |name: &str| tokens.get(name).and_then(Value::as_int);
                    Usage {
                        input: count("input")?,
                        output: count("output")?,
                        cached: count("cached")?,
                    }
                },
                cost: Usd6::parse(text("cost_usd")?).ok()?,
            }),
            _ => return None,
        };
        (record.payload() == *payload).then_some(record)
    }

    /// The journal payload of journal spec §9.19.
    pub fn payload(&self) -> Value {
        match self {
            Self::Reserved(r) => object([
                (
                    "agent_id",
                    r.agent_id.clone().map_or(Value::Null, Value::Str),
                ),
                ("call_id", Value::Str(r.call_id.clone())),
                ("endpoint", Value::Str(r.endpoint.clone())),
                ("key_owner", Value::Str(r.key_owner.code().to_owned())),
                ("max_cost_usd", Value::Str(r.max_cost.to_string())),
                ("model", model_value(&r.model)),
                (
                    "price_table_ref",
                    Value::Str(format!("sha256:{}", r.price_table_ref)),
                ),
                ("purpose", Value::Str(r.purpose.code().to_owned())),
                ("risk_day", Value::Str(r.risk_day.to_string())),
            ]),
            Self::Settled(s) => object([
                ("attempts", int(u64::from(s.attempts))),
                ("call_id", Value::Str(s.call_id.clone())),
                ("cost_usd", Value::Str(s.cost.to_string())),
                ("outcome", Value::Str(s.outcome.code().to_owned())),
                ("reservation", Value::Str(s.reservation.clone())),
                ("tokens", tokens_value(s.tokens)),
            ]),
        }
    }
}

/// Why a meter append was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    #[error("the meter stream cannot be read or appended to")]
    Unavailable,
    #[error("another writer holds the meter stream (journal spec §5.1)")]
    Fenced,
    #[error("journal spec rule 158: {0}")]
    Rule158(&'static str),
}

/// The meter stream's store: the journal in a real cell, memory in tests. An implementation keeps
/// journal spec rule 158: one reservation per call, one settlement per reservation.
pub trait MeterStore {
    fn read(&self) -> Result<Vec<MeterRecord>, StoreError>;
    fn append(&mut self, record: MeterRecord) -> Result<(), StoreError>;
}

/// An in-memory meter stream that keeps rule 158, and can be made unavailable or fenced to
/// exercise `meter_unavailable`.
#[derive(Debug, Clone, Default)]
pub struct MemoryMeter {
    records: Vec<MeterRecord>,
    failure: Option<StoreError>,
    append_limit: Option<usize>,
}

impl MemoryMeter {
    pub fn new() -> Self {
        Self::default()
    }

    /// From now on, every read and append fails with `failure`, or none does.
    pub fn fail_with(&mut self, failure: Option<StoreError>) {
        self.failure = failure;
    }

    /// From now on, an append that would make the stream longer than `limit` records fails
    /// `Unavailable`, as a writer that loses the journal mid-call does.
    pub fn limit_appends(&mut self, limit: Option<usize>) {
        self.append_limit = limit;
    }

    pub fn records(&self) -> &[MeterRecord] {
        &self.records
    }
}

impl MeterStore for MemoryMeter {
    fn read(&self) -> Result<Vec<MeterRecord>, StoreError> {
        match self.failure {
            Some(failure) => Err(failure),
            None => Ok(self.records.clone()),
        }
    }

    fn append(&mut self, record: MeterRecord) -> Result<(), StoreError> {
        if let Some(failure) = self.failure {
            return Err(failure);
        }
        if self
            .append_limit
            .is_some_and(|limit| self.records.len() >= limit)
        {
            return Err(StoreError::Unavailable);
        }
        rule_158(&self.records, &record)?;
        self.records.push(record);
        Ok(())
    }
}

/// Journal spec rule 158 for `record` appended after `earlier`.
pub fn rule_158(earlier: &[MeterRecord], record: &MeterRecord) -> Result<(), StoreError> {
    match record {
        MeterRecord::Reserved(r) => {
            let again = earlier
                .iter()
                .any(|e| matches!(e, MeterRecord::Reserved(x) if x.call_id == r.call_id));
            if again {
                return Err(StoreError::Rule158("the call is already reserved"));
            }
        }
        MeterRecord::Settled(s) => {
            let reserved = earlier.iter().find_map(|e| match e {
                MeterRecord::Reserved(x) if x.event_id == s.reservation => Some(x),
                _ => None,
            });
            let Some(reserved) = reserved else {
                return Err(StoreError::Rule158(
                    "no earlier reservation has this event id",
                ));
            };
            if reserved.call_id != s.call_id {
                return Err(StoreError::Rule158("the reservation is another call's"));
            }
            let settled = earlier
                .iter()
                .any(|e| matches!(e, MeterRecord::Settled(x) if x.reservation == s.reservation));
            if settled {
                return Err(StoreError::Rule158("the reservation is already settled"));
            }
        }
    }
    Ok(())
}

/// A meter store several writers' handles share, as every workspace's gateway in one process shares
/// the deployment's meter writer. A real cell reaches that writer through a port instead.
#[derive(Debug, Clone, Default)]
pub struct SharedMeter(Rc<RefCell<MemoryMeter>>);

impl SharedMeter {
    pub fn new() -> Self {
        Self::default()
    }

    /// The shared store's records, in append order.
    pub fn records(&self) -> Vec<MeterRecord> {
        self.0.borrow().records().to_vec()
    }

    /// As [`MemoryMeter::fail_with`], for every handle.
    pub fn fail_with(&self, failure: Option<StoreError>) {
        self.0.borrow_mut().fail_with(failure);
    }

    /// As [`MemoryMeter::limit_appends`], for every handle.
    pub fn limit_appends(&self, limit: Option<usize>) {
        self.0.borrow_mut().limit_appends(limit);
    }
}

impl MeterStore for SharedMeter {
    fn read(&self) -> Result<Vec<MeterRecord>, StoreError> {
        self.0.borrow().read()
    }

    fn append(&mut self, record: MeterRecord) -> Result<(), StoreError> {
        self.0.borrow_mut().append(record)
    }
}

/// Whose spend a cap bounds. `Deployment` folds the deployment meter stream; the others fold the
/// workspace's.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    Workspace,
    Agent(String),
    Deployment,
}

/// The period a cap sums over: one risk day, or every risk day of one calendar month.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Period {
    Day(Date),
    Month { year: u16, month: u8 },
}

impl Period {
    pub fn month_of(day: Date) -> Self {
        Self::Month {
            year: day.year(),
            month: day.month(),
        }
    }

    fn holds(self, day: Date) -> bool {
        match self {
            Self::Day(d) => d == day,
            Self::Month { year, month } => day.year() == year && day.month() == month,
        }
    }
}

/// Journal spec §9.19's fold: for each reservation in the scope and period, its settlement's cost
/// when settled and its maximum when open.
pub fn spend(records: &[MeterRecord], scope: &Scope, period: Period) -> Result<Usd6, MoneyError> {
    let mut settled: BTreeMap<&str, Usd6> = BTreeMap::new();
    for record in records {
        if let MeterRecord::Settled(s) = record {
            let cost = settled
                .get(s.reservation.as_str())
                .map_or(s.cost, |c| (*c).max(s.cost));
            settled.insert(s.reservation.as_str(), cost);
        }
    }
    records
        .iter()
        .filter_map(|r| match r {
            MeterRecord::Reserved(x) => Some(x),
            MeterRecord::Settled(_) => None,
        })
        .filter(|x| match scope {
            Scope::Workspace | Scope::Deployment => true,
            Scope::Agent(agent) => x.agent_id.as_deref() == Some(agent.as_str()),
        })
        .filter(|x| period.holds(x.risk_day))
        .try_fold(Usd6::ZERO, |total, x| {
            total.checked_add(
                settled
                    .get(x.event_id.as_str())
                    .copied()
                    .unwrap_or(x.max_cost),
            )
        })
}

/// The caps one workspace's calls are held to on its own stream (inference spec §7.3): its monthly
/// budget, a workspace setting, and the per-agent caps under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caps {
    workspace_monthly: Usd6,
    agent_monthly: BTreeMap<String, Usd6>,
    agent_monthly_default: Usd6,
    research_daily: BTreeMap<String, Usd6>,
    research_unreadable: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CapError {
    #[error("a per-agent cap above the workspace's monthly budget")]
    AboveWorkspace,
}

impl Caps {
    /// The workspace's monthly budget, from its setting; every agent's monthly cap is the same until
    /// set lower.
    pub fn new(workspace_monthly: Usd6) -> Self {
        Self {
            workspace_monthly,
            agent_monthly: BTreeMap::new(),
            agent_monthly_default: workspace_monthly,
            research_daily: BTreeMap::new(),
            research_unreadable: BTreeSet::new(),
        }
    }

    pub fn workspace_monthly(&self) -> Usd6 {
        self.workspace_monthly
    }

    /// One agent's monthly cap, at most the workspace's.
    pub fn with_agent_monthly(mut self, agent: &str, cap: Usd6) -> Result<Self, CapError> {
        if cap > self.workspace_monthly {
            return Err(CapError::AboveWorkspace);
        }
        self.agent_monthly.insert(agent.to_owned(), cap);
        Ok(self)
    }

    /// The monthly cap of every agent without one of its own, at most the workspace's.
    pub fn with_agent_monthly_default(mut self, cap: Usd6) -> Result<Self, CapError> {
        if cap > self.workspace_monthly {
            return Err(CapError::AboveWorkspace);
        }
        self.agent_monthly_default = cap;
        Ok(self)
    }

    /// The research agent's daily cap, `behavior.research.cost_cap_usd_per_day` tightened by
    /// policy (inference spec §7.3); it bounds the agent's research calls per risk day.
    pub fn with_research_daily(mut self, agent: &str, cap: Usd6) -> Result<Self, CapError> {
        if cap > self.workspace_monthly {
            return Err(CapError::AboveWorkspace);
        }
        self.research_daily.insert(agent.to_owned(), cap);
        Ok(self)
    }

    /// Whether `agent`'s research daily cap is marked as one that did not read, so its research
    /// calls are refused `Unavailable` (DEC-2272 item 5).
    pub fn research_unreadable(&self, agent: &str) -> bool {
        self.research_unreadable.contains(agent)
    }

    /// Every cap a call of `agent` for `purpose` on `day` must fit under, with its scope and
    /// period, workspace first.
    pub fn applying(
        &self,
        agent: Option<&str>,
        purpose: Purpose,
        day: Date,
    ) -> Vec<(Scope, Period, Usd6)> {
        let mut out = vec![(
            Scope::Workspace,
            Period::month_of(day),
            self.workspace_monthly,
        )];
        if let Some(agent) = agent {
            let monthly = self
                .agent_monthly
                .get(agent)
                .copied()
                .unwrap_or(self.agent_monthly_default);
            out.push((
                Scope::Agent(agent.to_owned()),
                Period::month_of(day),
                monthly,
            ));
            if purpose == Purpose::Research
                && let Some(daily) = self.research_daily.get(agent)
            {
                out.push((Scope::Agent(agent.to_owned()), Period::Day(day), *daily));
            }
        }
        out
    }
}

/// Why the meter refused a reservation; either way nothing is sent and nothing is recorded on the
/// workspace's meter stream. `Orphaned` is `meter_unavailable` that left a copy open on the
/// deployment's stream (DEC-2207 item 4), so its caller alerts the operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeterRefusal {
    BudgetExhausted,
    Unavailable,
    Orphaned,
}

/// The deployment's meter writer, as a workspace's writer reaches it: the deployment meter stream
/// and the deployment's monthly cap, a deployment setting (DEC-2207 items 1 to 3).
pub struct DeploymentMeter {
    store: Box<dyn MeterStore>,
    monthly: Usd6,
    ids: Box<dyn IdSource>,
}

impl fmt::Debug for DeploymentMeter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeploymentMeter")
            .field("monthly", &self.monthly)
            .finish_non_exhaustive()
    }
}

impl DeploymentMeter {
    /// The deployment's writer over its stream, with its monthly cap and the source of its copies'
    /// event ids: each copy is its own record (DEC-2207 item 2).
    pub fn new(store: Box<dyn MeterStore>, monthly: Usd6, ids: Box<dyn IdSource>) -> Self {
        Self {
            store,
            monthly,
            ids,
        }
    }

    pub fn monthly(&self) -> Usd6 {
        self.monthly
    }

    /// Checks the deployment cap and appends the copy, in one step (DEC-2207 item 4).
    fn reserve(&mut self, reservation: &Reservation) -> Result<(), MeterRefusal> {
        let copy = Reservation {
            event_id: self.ids.next_id(),
            ..reservation.clone()
        };
        let records = self.store.read().map_err(|_| MeterRefusal::Unavailable)?;
        let spent = spend(
            &records,
            &Scope::Deployment,
            Period::month_of(copy.risk_day),
        )
        .map_err(|_| MeterRefusal::BudgetExhausted)?;
        let after = spent
            .checked_add(copy.max_cost)
            .map_err(|_| MeterRefusal::BudgetExhausted)?;
        if after > self.monthly {
            return Err(MeterRefusal::BudgetExhausted);
        }
        self.store
            .append(MeterRecord::Reserved(copy))
            .map_err(|_| MeterRefusal::Unavailable)
    }

    /// Copies a settlement, naming the deployment stream's copy of its reservation (DEC-2207 item
    /// 2). With no copy of the call's reservation, nothing is appended.
    fn settle(&mut self, settlement: &Settlement) -> Result<(), StoreError> {
        let records = self.store.read()?;
        let reservation = records
            .iter()
            .find_map(|r| match r {
                MeterRecord::Reserved(x) if x.call_id == settlement.call_id => {
                    Some(x.event_id.clone())
                }
                _ => None,
            })
            .ok_or(StoreError::Rule158("no copy of the call's reservation"))?;
        self.store.append(MeterRecord::Settled(Settlement {
            event_id: self.ids.next_id(),
            reservation,
            ..settlement.clone()
        }))
    }

    /// The deployment's spend for one calendar month, from the deployment stream.
    pub fn spend(&self, period: Period) -> Result<Usd6, MeterRefusal> {
        let records = self.store.read().map_err(|_| MeterRefusal::Unavailable)?;
        spend(&records, &Scope::Deployment, period).map_err(|_| MeterRefusal::Unavailable)
    }
}

/// One workspace's meter writer over its store, reaching the deployment's writer for the
/// deployment cap.
#[derive(Debug)]
pub struct Meter<S> {
    store: S,
    caps: Caps,
    deployment: DeploymentMeter,
}

impl<S: MeterStore> Meter<S> {
    pub fn new(store: S, caps: Caps, deployment: DeploymentMeter) -> Self {
        Self {
            store,
            caps,
            deployment,
        }
    }

    pub fn deployment(&self) -> &DeploymentMeter {
        &self.deployment
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    pub fn caps(&self) -> &Caps {
        &self.caps
    }

    /// Replaces `agent`'s research daily cap on a running meter (DEC-2272 item 5): `Some` sets it,
    /// at most the workspace's monthly budget; `None` marks it unread, so the agent's research calls
    /// are refused `Unavailable`, reserving nothing, until a cap is set again.
    ///
    /// # Errors
    /// [`CapError::AboveWorkspace`] for a cap above the workspace's monthly budget, which leaves the
    /// agent marked unread rather than holding its earlier cap.
    pub fn set_research_daily(&mut self, agent: &str, cap: Option<Usd6>) -> Result<(), CapError> {
        self.caps.research_daily.remove(agent);
        self.caps.research_unreadable.insert(agent.to_owned());
        let Some(cap) = cap else {
            return Ok(());
        };
        if cap > self.caps.workspace_monthly {
            return Err(CapError::AboveWorkspace);
        }
        self.caps.research_unreadable.remove(agent);
        self.caps.research_daily.insert(agent.to_owned(), cap);
        Ok(())
    }

    /// Appends `reservation` only if, for every cap that applies, spend plus its maximum is at
    /// most the cap (inference spec §7.3), the deployment's included. The workspace's caps are
    /// checked first with nothing appended; then the deployment's writer checks its cap and appends
    /// the copy; then the workspace's own reservation is appended (DEC-2207 item 4). A store that
    /// cannot be read or appended to refuses; a workspace append that fails after the copy leaves
    /// the copy open, an orphan that only over-counts. A research call of an agent whose daily cap
    /// is marked unread is refused `Unavailable` before anything is read or appended.
    pub fn reserve(&mut self, reservation: Reservation) -> Result<Reservation, MeterRefusal> {
        if reservation.purpose == Purpose::Research
            && reservation
                .agent_id
                .as_deref()
                .is_some_and(|agent| self.caps.research_unreadable(agent))
        {
            return Err(MeterRefusal::Unavailable);
        }
        let records = self.store.read().map_err(|_| MeterRefusal::Unavailable)?;
        let caps = self.caps.applying(
            reservation.agent_id.as_deref(),
            reservation.purpose,
            reservation.risk_day,
        );
        for (scope, period, cap) in caps {
            let spent =
                spend(&records, &scope, period).map_err(|_| MeterRefusal::BudgetExhausted)?;
            let after = spent
                .checked_add(reservation.max_cost)
                .map_err(|_| MeterRefusal::BudgetExhausted)?;
            if after > cap {
                return Err(MeterRefusal::BudgetExhausted);
            }
        }
        self.deployment.reserve(&reservation)?;
        self.store
            .append(MeterRecord::Reserved(reservation.clone()))
            .map_err(|_| MeterRefusal::Orphaned)?;
        Ok(reservation)
    }

    /// Appends the settlement to the workspace's stream, then, only once that is appended, its copy
    /// to the deployment's (DEC-2207 item 5), so the deployment never counts a call below the
    /// workspace's count of it. Either append that fails leaves that stream's reservation open at
    /// its maximum, which never lowers spend (DEC-998 item 5).
    pub fn settle(&mut self, settlement: Settlement) -> Result<(), StoreError> {
        self.store
            .append(MeterRecord::Settled(settlement.clone()))?;
        self.deployment.settle(&settlement)
    }

    /// Spend for a scope and period, from the store's records.
    pub fn spend(&self, scope: &Scope, period: Period) -> Result<Usd6, MeterRefusal> {
        let records = self.store.read().map_err(|_| MeterRefusal::Unavailable)?;
        spend(&records, scope, period).map_err(|_| MeterRefusal::Unavailable)
    }

    /// The research agent's spend for `day`, for `mandate-research`'s check 7 (inference spec
    /// §7.3, E15-9).
    pub fn research_spend(&self, agent: &str, day: Date) -> Result<Usd6, MeterRefusal> {
        self.spend(&Scope::Agent(agent.to_owned()), Period::Day(day))
    }

    /// The reservations of one agent, read only (agent harness spec §10.1 ask 2).
    pub fn reservations_of(&self, agent: &str) -> Result<Vec<Reservation>, MeterRefusal> {
        let records = self.store.read().map_err(|_| MeterRefusal::Unavailable)?;
        Ok(records
            .into_iter()
            .filter_map(|r| match r {
                MeterRecord::Reserved(x) if x.agent_id.as_deref() == Some(agent) => Some(x),
                _ => None,
            })
            .collect())
    }
}
