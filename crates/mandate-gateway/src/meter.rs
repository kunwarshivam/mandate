//! The meter (inference spec §3.6, §7.3, E15-9): reservations before the first attempt,
//! settlements on completion, and caps checked against spend folded from the meter records alone
//! (journal spec §9.19). One meter writer per workspace; a writer that cannot read or append refuses
//! the call `meter_unavailable`, so nothing is sent.

use std::collections::BTreeMap;

use mandate_canon::{Digest, Value};
use mandate_time::Date;

use crate::kinds::{KeyOwner, Outcome, Purpose};
use crate::money::{MoneyError, Usage, Usd6};
use crate::record::{model_value, tokens_value};
use crate::registry::{ModelRef, int, object};

/// DEC-997 item 3: model spend is capped at $50.00 a month per workspace for paper and demo.
pub const DEMO_WORKSPACE_MONTHLY_MICROS: u64 = 50_000_000;

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
        let _ = (earlier, record); Ok(())
    }

/// Whose spend a cap bounds.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    Workspace,
    Agent(String),
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
        let _ = (records, scope, period); Ok(Usd6::ZERO)
    }

/// The caps one workspace's calls are held to (inference spec §7.3, DEC-997 item 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caps {
    workspace_monthly: Usd6,
    agent_monthly: BTreeMap<String, Usd6>,
    agent_monthly_default: Usd6,
    research_daily: BTreeMap<String, Usd6>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CapError {
    #[error("a per-agent cap above the workspace cap (DEC-997 item 3)")]
    AboveWorkspace,
}

impl Caps {
    /// The workspace's monthly cap; every agent's monthly cap is the same until set lower.
    pub fn new(workspace_monthly: Usd6) -> Self {
        Self {
            workspace_monthly,
            agent_monthly: BTreeMap::new(),
            agent_monthly_default: workspace_monthly,
            research_daily: BTreeMap::new(),
        }
    }

    /// DEC-997 item 3's $50.00 a month.
    pub fn demo() -> Self {
        Self::new(Usd6::from_micros(DEMO_WORKSPACE_MONTHLY_MICROS))
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
/// meter stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeterRefusal {
    BudgetExhausted,
    Unavailable,
}

/// One workspace's meter writer over its store.
#[derive(Debug, Clone)]
pub struct Meter<S> {
    store: S,
    caps: Caps,
}

impl<S: MeterStore> Meter<S> {
    pub fn new(store: S, caps: Caps) -> Self {
        Self { store, caps }
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

    /// Appends `reservation` only if, for every cap that applies, spend plus its maximum is at
    /// most the cap (inference spec §7.3). A store that cannot be read or appended to refuses.
    pub fn reserve(&mut self, reservation: Reservation) -> Result<Reservation, MeterRefusal> {
        let _ = reservation; Err(MeterRefusal::Unavailable)
    }

    /// Appends the settlement. If it cannot be appended, the reservation stays open and counts at
    /// its maximum, which never lowers spend (DEC-998 item 5).
    pub fn settle(&mut self, settlement: Settlement) -> Result<(), StoreError> {
        self.store.append(MeterRecord::Settled(settlement))
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
