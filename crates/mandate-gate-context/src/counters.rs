//! The counters an opening on a traded, flat account reads beside the agent's risk
//! ([DEC-2833](../../../docs/project/decisions/DEC-2833.md) item 3): mandate spec §5.3's orders per
//! day and re-entry cooldown, and trading spec §9.6's market-conduct counters, folded from the
//! account stream's rows with the instant the store recorded each. Every reading the specs leave
//! open is the one that counts toward a limit (DEC-176), and a member that does not read leaves the
//! counters unread, which holds the opening and nothing else (`AGENTS.md` rule 13).

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::Value;
use mandate_num::Qty;
use mandate_risk::{AssetId, ConductState, RestingSide};
use mandate_time::{Date, UtcNanos, new_york_date_and_hour};

/// What [`crate::Journaled::counted`] answers: the facts beside the agent's risk that the binding
/// gate reads for an opening on a traded, flat account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counted {
    /// §9.6's counters of the agent and the account. A flat account works no order, so
    /// `resting_since` is empty.
    pub conduct: ConductState,
    /// Mandate §5.3: the agent's opening and increasing orders submitted in the risk day.
    pub orders_today: u32,
    /// Mandate §5.3: per instrument, the instant of the latest sell fill on the account.
    pub last_exit_fill_at: BTreeMap<AssetId, UtcNanos>,
}

/// The order purposes the executor journals, and whether each adds risk.
const PURPOSES: [(&str, bool); 7] = [
    ("open", true),
    ("increase", true),
    ("risk_exit", false),
    ("owner_exit", false),
    ("discretionary_exit", false),
    ("protective", false),
    ("flatten", false),
];

/// The order states that end an order (trading-domain spec §5.7).
const TERMINAL: [&str; 6] = [
    "filled",
    "canceled",
    "rejected",
    "expired",
    "replaced",
    "abandoned",
];

/// One instant a row carries: its `risk_clock` and the instant the store recorded it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamps {
    clock: Option<UtcNanos>,
    recorded: Option<UtcNanos>,
}

/// One of the agent's orders, or a replacement of one.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Order {
    instrument: String,
    adds_risk: bool,
    stamps: Stamps,
}

/// One fill on the account, of any order or none.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Fill {
    order: Option<String>,
    instrument: String,
    side: RestingSide,
    qty: Qty,
    trade_date: Date,
    clock: UtcNanos,
    recorded: UtcNanos,
}

/// The account stream's orders and fills as the counters read them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Ledger {
    unread: bool,
    connections: BTreeSet<String>,
    request: Option<(String, Option<bool>)>,
    orders: BTreeMap<String, Order>,
    fills: Vec<Fill>,
    fill_ids: BTreeSet<String>,
    seen: BTreeSet<String>,
    ended: BTreeSet<String>,
}

impl Ledger {
    /// Folds one row of `event_type` with `payload`, on the account stream when `on_account`,
    /// recorded at `recorded`.
    pub(crate) fn fold(
        &mut self,
        on_account: bool,
        event_type: &str,
        payload: &Value,
        recorded: Option<UtcNanos>,
        agent: &str,
    ) {
        if self
            .read(on_account, event_type, payload, recorded, agent)
            .is_none()
        {
            self.unread = true;
        }
    }

    fn read(
        &mut self,
        on_account: bool,
        event_type: &str,
        payload: &Value,
        recorded: Option<UtcNanos>,
        agent: &str,
    ) -> Option<()> {
        let text = |member: &str| payload.get(member).and_then(Value::as_str);
        let stamps = || Stamps {
            clock: text("risk_clock").and_then(|stamp| UtcNanos::parse(stamp).ok()),
            recorded,
        };
        match (event_type, on_account) {
            ("ConnectionEstablished", false) => {
                self.connections.insert(text("connection_id")?.to_owned());
            }
            ("ConnectionRevoked", false) => {
                self.connections.remove(text("connection_id")?);
            }
            ("OrderRequestRecorded", true) => {
                let adds_risk = text("purpose").and_then(|purpose| {
                    PURPOSES
                        .iter()
                        .find(|(name, _)| *name == purpose)
                        .map(|(_, adds)| *adds)
                });
                self.request = Some((text("agent_id")?.to_owned(), adds_risk));
            }
            ("OrderSubmitted", true) => {
                let order = text("client_order_id")?.to_owned();
                let instrument = text("instrument_id")?.to_owned();
                self.seen.insert(order.clone());
                if let Some((requester, adds_risk)) = self.request.take()
                    && requester == agent
                {
                    let order_of = Order {
                        instrument,
                        adds_risk: adds_risk?,
                        stamps: stamps(),
                    };
                    self.orders.insert(order, order_of);
                }
            }
            ("OrderStateChanged", true) => {
                if payload.get("ignored") == Some(&Value::Bool(true)) {
                    return Some(());
                }
                let order = text("client_order_id")?.to_owned();
                let state = text("state")?;
                if TERMINAL.contains(&state) {
                    self.ended.insert(order.clone());
                } else if !self.ended.contains(&order) {
                    self.seen.insert(order.clone());
                }
                match payload.get("replaced_by") {
                    None | Some(Value::Null) => {}
                    Some(Value::Str(successor)) => {
                        self.seen.insert(successor.clone());
                        if let Some(replaced) = self.orders.get(&order).cloned() {
                            let successor_of = Order {
                                stamps: stamps(),
                                ..replaced
                            };
                            self.orders.insert(successor.clone(), successor_of);
                        }
                    }
                    Some(_) => return None,
                }
            }
            ("OrderAbandoned", true) => {
                self.ended.insert(text("client_order_id")?.to_owned());
            }
            ("FillApplied" | "LateFillApplied", true) => {
                if !self.fill_ids.insert(text("fill_id")?.to_owned()) {
                    return Some(());
                }
                let order = match payload.get("client_order_id") {
                    None | Some(Value::Null) => None,
                    Some(Value::Str(order)) => Some(order.clone()),
                    Some(_) => return None,
                };
                let side = match text("side")? {
                    "buy" => RestingSide::Buy,
                    "sell" => RestingSide::Sell,
                    _ => return None,
                };
                self.fills.push(Fill {
                    order,
                    instrument: text("instrument_id")?.to_owned(),
                    side,
                    qty: Qty::parse(text("qty_gross")?).ok()?,
                    trade_date: Date::parse(text("trade_date")?).ok()?,
                    clock: UtcNanos::parse(text("risk_clock")?).ok()?,
                    recorded: recorded?,
                });
            }
            _ => {}
        }
        Some(())
    }

    /// Whether every order seen on the account has ended, and no order request waits for the
    /// submission journaled with it (journal spec rule 45), which may be working at the broker.
    pub(crate) fn none_working(&self) -> bool {
        self.request.is_none() && self.seen.is_subset(&self.ended)
    }

    /// Whether the counters read: every row they read did, and the workspace has exactly one
    /// connection that no revocation ended, so the account is the whole related-accounts group
    /// (DEC-2833 item 4).
    pub(crate) fn readable(&self) -> bool {
        !self.unread && self.connections.len() == 1
    }

    /// The counters at `now`, read in `now`'s New York risk day.
    pub(crate) fn counted(&self, now: UtcNanos) -> Option<Counted> {
        if !self.readable() {
            return None;
        }
        let today = day(now)?;
        let is_today = |at: Option<UtcNanos>| at.and_then(day) == Some(today);
        let mut conduct = ConductState::default();
        let mut orders_today = 0_u32;
        for order in self.orders.values() {
            let Stamps { clock, recorded } = order.stamps;
            let counts =
                (clock.is_none() && recorded.is_none()) || is_today(clock) || is_today(recorded);
            if !counts {
                continue;
            }
            if order.adds_risk {
                orders_today = orders_today.checked_add(1)?;
            }
            bump(
                &mut conduct.orders_today_per_instrument,
                asset(&order.instrument)?,
            )?;
        }
        let mut filled: BTreeSet<(String, &str)> = BTreeSet::new();
        let mut last_exit_fill_at = BTreeMap::new();
        for fill in &self.fills {
            let instrument = asset(&fill.instrument)?;
            let instant = fill.clock.max(fill.recorded);
            latest(
                &mut conduct.last_opposite_fill_at,
                (instrument.clone(), fill.side),
                instant,
            );
            if fill.side == RestingSide::Sell {
                latest(&mut last_exit_fill_at, instrument.clone(), instant);
            }
            if is_today(Some(fill.clock))
                || is_today(Some(fill.recorded))
                || fill.trade_date == today
            {
                let total = conduct
                    .participation_today
                    .entry(instrument.clone())
                    .or_insert(Qty::ZERO);
                *total = total.checked_add(fill.qty).ok()?;
            }
            let ours = fill
                .order
                .as_deref()
                .filter(|order| self.orders.contains_key(*order));
            if let Some(order) = ours
                && fill.trade_date == today
                && is_today(Some(fill.clock))
                && filled.insert((fill.instrument.clone(), order))
            {
                bump(&mut conduct.filled_today, instrument)?;
            }
        }
        Some(Counted {
            conduct,
            orders_today,
            last_exit_fill_at,
        })
    }
}

/// `at`'s New York date, the risk day it falls in (mandate spec §5.3).
fn day(at: UtcNanos) -> Option<Date> {
    new_york_date_and_hour(at).ok().map(|(date, _)| date)
}

fn asset(instrument: &str) -> Option<AssetId> {
    AssetId::new(instrument).ok()
}

fn bump(counts: &mut BTreeMap<AssetId, u32>, key: AssetId) -> Option<()> {
    let count = counts.entry(key).or_insert(0);
    *count = count.checked_add(1)?;
    Some(())
}

fn latest<K: Ord>(instants: &mut BTreeMap<K, UtcNanos>, key: K, at: UtcNanos) {
    let instant = instants.entry(key).or_insert(at);
    *instant = (*instant).max(at);
}
