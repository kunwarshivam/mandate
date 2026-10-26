//! The account's risk folds, one per deployed agent, in the executor process (DEC-2804 items 2, 4,
//! 5 and 8, DEC-2399).
//!
//! The book opens each agent's `AccountRiskFold` flat at its first deployment on the account, from
//! inputs the journal fixes as of that deployment, so a replay at any later head opens the same
//! fold. The process folds every committed batch into it, appends what it owes as the next batch,
//! and asks it whether a tick is worth a `ClockAdvanced` copy. An agent without a fold has an
//! unreadable view, `not_opened`: its openings are held, never its exits (`AGENTS.md` rules 3 and
//! 13).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use mandate_canon::{Value, parse};
use mandate_domain::{AgentMode, AssetClass};
use mandate_executor::risk_state::{
    AccountRiskFold, ExchangeRiskCalendar, RiskAppend, RiskView, UnreadableCause,
};
use mandate_executor::tripwire::TripwireState;
use mandate_executor::{AgentId, MandateVersion};
use mandate_journal::StoredEvent;
use mandate_num::{Price, Qty, Usd};
use mandate_spec::ValidatedMandate;
use mandate_spec::risk::{Opening, risk_day};
use mandate_time::{Date, UtcNanos};

use crate::follow::{FirstDeployment, Following};

/// What an agent's fold opens on, read from its deployment (DEC-2804 item 2): its confirmed version
/// validated on the opening's equity, its reviewed instrument (a canonical asset id) and asset
/// class, the connection's loss carry, and the reviewed quote staleness in whole seconds.
pub struct RiskOpening {
    pub mandate: ValidatedMandate,
    pub instrument: String,
    pub asset_class: AssetClass,
    pub inherited_loss_usd: Usd,
    pub mark_max_age_s: u32,
}

/// Reads an agent's [`RiskOpening`] from `control`, the control rows up to and including its first
/// deployment, with its version validated on `equity` as of `date`, the opening's New York date.
/// `None` when any of it does not read, which opens no fold.
pub trait RiskOpenings {
    fn opening(
        &self,
        agent: &AgentId,
        control: &[StoredEvent],
        equity: Usd,
        date: Date,
    ) -> Option<RiskOpening>;
}

/// The openings of a process that reads no deployment: no fold opens, so every opening is held.
pub struct Unopened;

impl RiskOpenings for Unopened {
    fn opening(
        &self,
        _agent: &AgentId,
        _control: &[StoredEvent],
        _equity: Usd,
        _date: Date,
    ) -> Option<RiskOpening> {
        None
    }
}

/// The calendar a fold of `class` steps on (DEC-2804 item 6), loaded once per process; `None`
/// when it cannot load, which opens no fold.
fn calendar(class: AssetClass) -> Option<&'static ExchangeRiskCalendar> {
    static EQUITY: OnceLock<Option<ExchangeRiskCalendar>> = OnceLock::new();
    static CRYPTO: OnceLock<Option<ExchangeRiskCalendar>> = OnceLock::new();
    let loaded = if class == AssetClass::Crypto {
        &CRYPTO
    } else {
        &EQUITY
    };
    loaded
        .get_or_init(|| ExchangeRiskCalendar::new(class).ok())
        .as_ref()
}

/// The view of an agent with no fold (DEC-2804 item 2): unreadable, `not_opened`, `exits_only`.
pub const NOT_OPENED: RiskView = RiskView::Unreadable {
    cause: UnreadableCause::NotOpened,
    head: 0,
    mode: AgentMode::ExitsOnly,
};

/// The account's risk folds.
pub struct RiskBook {
    account: String,
    /// Each open fold, with the version its records name in `config_refs` (journal spec §9).
    folds: BTreeMap<AgentId, (AccountRiskFold<'static>, MandateVersion)>,
    /// The New York risk day of the last `ClockAdvanced` copy on the account stream, from which
    /// the next tick crosses midnight (DEC-2804 item 5).
    copied: Option<Date>,
    /// The control stream's length and the account stream's head at the last pass that left an
    /// agent without a fold, so the account stream is read again only once either has grown.
    tried: Option<(usize, u64)>,
}

impl RiskBook {
    /// The book of the account stream `account`, with no fold open.
    pub fn new(account: &str) -> Self {
        Self {
            account: account.to_owned(),
            folds: BTreeMap::new(),
            copied: None,
            tried: None,
        }
    }

    /// Reads, from the control stream as `following` and `control` show it, which agents have a
    /// fold (DEC-2804 items 2 and 8), and opens each one missing over the account stream's rows,
    /// which `account` reads only when one is and either stream grew since the last pass that
    /// left one unopened (`head` is the account stream's). No fold is open while more than one agent that has
    /// not stopped is followed. A stopped agent's fold, and one whose agent is redeployed on
    /// another version, is dropped, as a replay that saw that row would never have opened it.
    pub fn follow(
        &mut self,
        following: &Following,
        control: &[StoredEvent],
        (head, account): (u64, &mut dyn FnMut() -> Option<Vec<StoredEvent>>),
        openings: &dyn RiskOpenings,
    ) {
        let running: Vec<&AgentId> = following
            .agents
            .keys()
            .filter(|agent| !following.stopped(agent))
            .collect();
        let alone = running.len() == 1;
        self.folds.retain(|agent, _| {
            alone
                && running.contains(&agent)
                && following
                    .first
                    .get(agent)
                    .is_some_and(|first| !first.redeployed)
        });
        let read_at = (control.len(), head);
        if !alone || self.tried == Some(read_at) {
            return;
        }
        let mut rows = None;
        let mut unopened = false;
        for agent in running {
            if self.folds.contains_key(agent) {
                continue;
            }
            let Some(first) = following.first.get(agent).filter(|first| !first.redeployed) else {
                unopened = true;
                continue;
            };
            if rows.is_none() {
                rows = account();
            }
            let Some(rows) = rows.as_deref() else {
                return;
            };
            match self.open(agent, first, control, rows, openings) {
                Some(fold) => {
                    self.folds
                        .insert(agent.clone(), (fold, first.version.clone()));
                }
                None => unopened = true,
            }
        }
        self.tried = unopened.then_some(read_at);
    }

    /// `agent`'s fold opened flat at `first` and folded over the account stream's `rows` from seq
    /// 2, or `None` when its equity, its deployment, its calendar or the opening does not read, or
    /// its universe may hold a second instrument (DEC-2804 items 2 and 8).
    fn open(
        &self,
        agent: &AgentId,
        first: &FirstDeployment,
        control: &[StoredEvent],
        rows: &[StoredEvent],
        openings: &dyn RiskOpenings,
    ) -> Option<AccountRiskFold<'static>> {
        let at = UtcNanos::from_parts(first.at.secs(), 0).ok()?;
        let equity = equity_at(rows, at)?;
        let upto: Vec<StoredEvent> = control
            .iter()
            .filter(|row| row.seq <= first.seq)
            .cloned()
            .collect();
        let read = openings.opening(agent, &upto, equity, risk_day(at).ok()?.day)?;
        if read.mandate.mandate().universe.max_instruments > 1 {
            return None;
        }
        let opening = Opening {
            position_qty: Qty::ZERO,
            avg_cost: Price::parse("1").ok()?,
            asset_class: read.asset_class,
            at,
            inherited_loss_usd: read.inherited_loss_usd,
            mark_max_age_s: read.mark_max_age_s,
        };
        let mut fold = AccountRiskFold::open(
            &self.account,
            1,
            &agent.0,
            &read.mandate,
            &opening,
            Some(&read.instrument),
            calendar(read.asset_class)?,
            TripwireState::default(),
        )
        .ok()?;
        for row in rows.iter().filter(|row| row.seq > 1) {
            fold.fold(row).ok()?;
        }
        Some(fold)
    }

    /// Folds committed account-stream `rows` into every open fold, and notes each `ClockAdvanced`
    /// copy's risk day. A fold that refuses a row as another stream's is dropped.
    pub fn fold(&mut self, rows: &[StoredEvent]) {
        for row in rows {
            if row.event_type == "ClockAdvanced"
                && let Some(day) = clock_of(row).and_then(|clock| risk_day(clock).ok())
            {
                self.copied = Some(day.day);
            }
            self.folds.retain(|_, (fold, _)| fold.fold(row).is_ok());
        }
    }

    /// The records the folds owe, in the order they are owed (DEC-2399 item 3), each with its
    /// agent's version: the next batch the process appends.
    pub fn owed(&self) -> Vec<(MandateVersion, RiskAppend)> {
        self.folds
            .values()
            .flat_map(|(fold, version)| fold.owed().map(|owed| (version.clone(), owed.clone())))
            .collect()
    }

    /// Whether the tick `clock`, as the `ClockAdvanced` copy `copy` would journal it, is worth
    /// journaling (DEC-2804 item 5): it crosses New York midnight from the last copy, or a copy of
    /// some fold stepped with it returns a record. The folds themselves do not step it.
    pub fn wants(&self, clock: UtcNanos, copy: &StoredEvent) -> bool {
        let day = risk_day(clock).ok().map(|day| day.day);
        day.is_none()
            || day != self.copied
            || self.folds.values().any(|(fold, _)| {
                let mut probe = fold.clone();
                probe.fold(copy).is_ok_and(|records| !records.is_empty())
            })
    }

    /// `agent`'s risk view: its fold's, or [`NOT_OPENED`].
    pub fn view(&self, agent: &AgentId) -> RiskView {
        self.folds
            .get(agent)
            .map_or(NOT_OPENED, |(fold, _)| fold.view().clone())
    }
}

/// The equity an opening at `at` is validated on, chosen by journal order (DEC-2804 item 2): the
/// boundary is the first row whose `event_time` is at or after `at`; the equity is the last
/// `AccountSnapshotRecorded` before it, else the first one after it. `None` until both are
/// journaled, or when the snapshot's `equity` does not read.
fn equity_at(rows: &[StoredEvent], at: UtcNanos) -> Option<Usd> {
    let boundary = rows
        .iter()
        .position(|row| event_time(row).is_some_and(|time| time >= at))?;
    let (before, after) = rows.split_at(boundary);
    let snapshot = |row: &&StoredEvent| row.event_type == "AccountSnapshotRecorded";
    let chosen = before
        .iter()
        .rev()
        .find(snapshot)
        .or_else(|| after.iter().find(snapshot))?;
    let body = parse(&chosen.body).ok()?;
    let equity = body.get("payload")?.get("equity")?.as_str()?;
    Usd::parse(equity).ok()
}

fn event_time(row: &StoredEvent) -> Option<UtcNanos> {
    let body = parse(&row.body).ok()?;
    UtcNanos::parse(body.get("event_time")?.as_str()?).ok()
}

/// The tick a `ClockAdvanced` row carries.
fn clock_of(row: &StoredEvent) -> Option<UtcNanos> {
    let body = parse(&row.body).ok()?;
    let clock = body.get("payload")?.get("clock").and_then(Value::as_str)?;
    UtcNanos::parse(clock).ok()
}
