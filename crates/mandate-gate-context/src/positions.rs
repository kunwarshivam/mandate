//! What the owner reads of one agent's positions (DEC-2136): the agent sub-ledger's cost basis,
//! each instrument's latest risk mark, and the account's protection in it, folded from the account
//! stream's rows alone.
//!
//! Each figure has one reading. The basis is `mandate-accounting`'s §8.1 fill rule
//! ([`Position::after_fill`]); the mark is the one [`crate::Journaled`] reads for the gate; and the
//! protection follows the executor's own fold of `ProtectionChanged` (`mandate-executor`'s `fold`),
//! which a property test there checks it against. It is a separate fold from [`crate::Journaled`]
//! because that one refuses a corporate action outright, which would leave the owner no position
//! at all; here an instrument a corporate action touched only has its basis not reported.

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::Position;
use mandate_num::{Price, Qty, SignedQty};
use mandate_time::UtcNanos;

use crate::{Payload, Row, TERMINAL, Unreadable, stamp};

/// A session whose marks count toward agent equity (mandate spec §5.2): an equity's regular
/// session, or crypto's, which never closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CountedSession {
    Regular,
    Crypto,
}

impl CountedSession {
    /// The journal's spelling (journal spec §9.30).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Regular => "regular",
            Self::Crypto => "crypto",
        }
    }
}

/// An instrument's latest version-2 mark from a counted session, as journaled: its bid when it is
/// sane, never a price when it is not (DEC-2136 item 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RiskMark {
    pub bid: Option<Price>,
    pub sane: bool,
    pub session: CountedSession,
    pub at: UtcNanos,
}

/// The account's protection in one instrument (trading spec §5.4, DEC-2136 item 4): the
/// protective orders resting, the quantity they cover, their prices, and the start of the earliest
/// unprotected interval still open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Protection {
    pub orders: Vec<String>,
    pub covered_qty: Qty,
    pub stop: Option<Price>,
    pub take_profit: Option<Price>,
    pub unprotected_since: Option<UtcNanos>,
}

impl Protection {
    /// No protective order and no open interval: what an instrument with no record has.
    pub const NONE: Self = Self {
        orders: Vec::new(),
        covered_qty: Qty::ZERO,
        stop: None,
        take_profit: None,
        unprotected_since: None,
    };
}

/// Reads a `MarkUpdated` row at its `risk_clock` `at`: the instrument and its mark, `None` for a
/// version-1 mark (no verdict, DEC-2305 item 3) or one from a session that does not count. This is
/// the one reading of a mark, [`crate::Journaled`]'s and the owner's.
pub(crate) fn counted_mark(
    payload: &Payload<'_>,
    at: UtcNanos,
) -> Result<Option<(String, RiskMark)>, Unreadable> {
    let Some(sane) = payload.value.get("sane") else {
        return Ok(None);
    };
    let session = match payload.text("session")? {
        "regular" => Some(CountedSession::Regular),
        "crypto" => Some(CountedSession::Crypto),
        "overnight" | "pre_market" | "after_hours" => None,
        _ => return Err((payload.refused)("session")),
    };
    let mark = match sane {
        mandate_canon::Value::Bool(true) => {
            let bid = payload
                .text("bid")
                .ok()
                .and_then(|bid| Price::parse(bid).ok())
                .ok_or_else(|| (payload.refused)("bid"))?;
            (Some(bid), true)
        }
        mandate_canon::Value::Bool(false) => (None, false),
        _ => return Err((payload.refused)("sane")),
    };
    let Some(session) = session else {
        return Ok(None);
    };
    let instrument = payload.text("instrument_id")?.to_owned();
    Ok(Some((
        instrument,
        RiskMark {
            bid: mark.0,
            sane: mark.1,
            session,
            at,
        },
    )))
}

/// An order as the protection fold needs it: whether it is protective, its quantity, and, for a
/// protective leg the broker created, whether its id names a known entry (DEC-160).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Order {
    protective: bool,
    qty: Qty,
    filled: Qty,
    attributed: bool,
}

/// One instrument's resting protection, as the executor's fold keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Resting {
    orders: Vec<String>,
    covered: Qty,
    prices: Option<(Price, Option<Price>)>,
}

/// An unprotected interval, as the executor's fold keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Interval {
    instrument: String,
    started_at: UtcNanos,
    ended: bool,
    alerted: bool,
    uncovered: bool,
    bracket: Option<String>,
}

/// The request recorded immediately before an `OrderSubmitted` (journal spec rule 45).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Request {
    agent: String,
    protective: bool,
    bracket: bool,
}

/// The fold over one account stream for one agent's position read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Positions {
    account_stream: String,
    agent: String,
    requested: Option<Request>,
    own: BTreeSet<String>,
    plain: BTreeMap<String, bool>,
    orders: BTreeMap<String, Order>,
    fills: BTreeSet<String>,
    ledgers: BTreeMap<String, Position>,
    filled: BTreeMap<String, SignedQty>,
    unreported: BTreeSet<String>,
    marks: BTreeMap<String, RiskMark>,
    resting: BTreeMap<String, Resting>,
    awaiting: BTreeMap<String, BTreeSet<String>>,
    intervals: Vec<Interval>,
}

impl Positions {
    /// The fold for `agent` on the account stream `account_stream`.
    pub fn new(account_stream: &str, agent: &str) -> Self {
        Self {
            account_stream: account_stream.to_owned(),
            agent: agent.to_owned(),
            requested: None,
            own: BTreeSet::new(),
            plain: BTreeMap::new(),
            orders: BTreeMap::new(),
            fills: BTreeSet::new(),
            ledgers: BTreeMap::new(),
            filled: BTreeMap::new(),
            unreported: BTreeSet::new(),
            marks: BTreeMap::new(),
            resting: BTreeMap::new(),
            awaiting: BTreeMap::new(),
            intervals: Vec::new(),
        }
    }

    /// Folds one row. A row on another stream changes nothing. A row of a type the fold reads that
    /// does not parse is refused, and the caller serves no position from the rows so far
    /// (DEC-2136 item 6).
    ///
    /// # Errors
    /// [`Unreadable`], naming the row's type and the member that did not parse.
    pub fn fold(&mut self, row: Row<'_>) -> Result<(), Unreadable> {
        if row.stream != self.account_stream {
            return Ok(());
        }
        let refused = |member| Unreadable {
            event_type: row.event_type.to_owned(),
            member,
        };
        let payload = Payload {
            value: row.payload,
            refused: &refused,
        };
        match row.event_type {
            "OrderRequestRecorded" => {
                self.requested = Some(Request {
                    agent: payload.text("agent_id")?.to_owned(),
                    protective: payload.text("purpose")? == "protective",
                    bracket: payload.value.get("order_class").and_then(|c| c.as_str())
                        == Some("bracket"),
                });
            }
            "OrderSubmitted" => self.submitted(&payload)?,
            "FillApplied" | "LateFillApplied" => self.fill(&payload)?,
            "OrderStateChanged" => self.changed(&payload)?,
            "OrderAbandoned" => {
                let order = payload.text("client_order_id")?.to_owned();
                self.orders.entry(order).or_insert(Order {
                    protective: false,
                    qty: Qty::ZERO,
                    filled: Qty::ZERO,
                    attributed: true,
                });
            }
            "PositionReleased" if payload.text("agent_id")? == self.agent => {
                if !quantity(&payload, "qty")?.is_zero() {
                    self.unreported.insert(instrument(&payload)?);
                }
            }
            "CorporateActionApplied" => {
                self.unreported.insert(instrument(&payload)?);
            }
            "MarkUpdated" => {
                let at = stamp(&payload, "risk_clock")?;
                if let Some((instrument, mark)) = counted_mark(&payload, at)? {
                    self.marks.insert(instrument, mark);
                }
            }
            "ProtectionChanged" => self.protection_changed(&payload)?,
            _ => {}
        }
        Ok(())
    }

    /// The agent sub-ledger's position in `instrument` (DEC-2136 item 1): `None` when the agent has
    /// no fill in it, or when a release, a corporate action, or a fee in the asset leaves its basis
    /// for the accounting fold to say.
    pub fn cost_basis(&self, instrument: &str) -> Option<Position> {
        if self.unreported.contains(instrument) {
            return None;
        }
        self.ledgers.get(instrument).copied()
    }

    /// The net quantity of the agent's own fills per instrument, in instrument order, leaving out an
    /// instrument that nets to zero (DEC-2111 item 4): buys add, sells subtract, each `fill_id`
    /// once, over the agent's orders as item 1 of DEC-2136 attributes them.
    pub fn filled(&self) -> impl Iterator<Item = (&str, SignedQty)> {
        self.filled
            .iter()
            .filter(|(_, qty)| !qty.is_zero())
            .map(|(instrument, qty)| (instrument.as_str(), *qty))
    }

    /// The instrument's latest counted version-2 mark (DEC-2136 item 2).
    pub fn risk_mark(&self, instrument: &str) -> Option<RiskMark> {
        self.marks.get(instrument).copied()
    }

    /// The account's protection in `instrument` (DEC-2136 item 4).
    pub fn protection(&self, instrument: &str) -> Protection {
        let unprotected_since = self
            .intervals
            .iter()
            .filter(|interval| interval.instrument == instrument && !interval.ended)
            .map(|interval| interval.started_at)
            .min();
        match self.resting.get(instrument) {
            None => Protection {
                unprotected_since,
                ..Protection::NONE
            },
            Some(resting) => Protection {
                orders: resting.orders.iter().fold(Vec::new(), |mut once, id| {
                    if !once.contains(id) {
                        once.push(id.clone());
                    }
                    once
                }),
                covered_qty: resting.covered,
                stop: resting.prices.map(|(stop, _)| stop),
                take_profit: resting.prices.and_then(|(_, take_profit)| take_profit),
                unprotected_since,
            },
        }
    }

    fn submitted(&mut self, payload: &Payload<'_>) -> Result<(), Unreadable> {
        let order = payload.text("client_order_id")?.to_owned();
        let qty = quantity(payload, "qty")?;
        let request = self.requested.take();
        let protective = request.as_ref().is_some_and(|request| request.protective);
        let agent = match &request {
            Some(request) => {
                self.plain.insert(order.clone(), !request.bracket);
                Some(request.agent.as_str())
            }
            None => {
                let bracket =
                    payload.value.get("order_class").and_then(|c| c.as_str()) == Some("bracket");
                self.plain.insert(order.clone(), !bracket);
                payload.text("agent_id").ok()
            }
        };
        if agent == Some(self.agent.as_str()) {
            self.own.insert(order.clone());
        }
        self.orders.entry(order).or_insert(Order {
            protective,
            qty,
            filled: Qty::ZERO,
            attributed: true,
        });
        Ok(())
    }

    /// A fill: the agent's own moves its sub-ledger by §8.1 and its net filled quantity, each
    /// `fill_id` once, and a fill of a protective order adds to that order's filled quantity, which
    /// its replacement inherits the rest of. Any other fill is another agent's, passed over unread
    /// (DEC-2112 item 2). A fill of a protective leg whose id names no known entry, in an
    /// instrument the agent holds, may be the agent's (DEC-160's single-holder rule, which needs
    /// every agent's lots), so it is refused rather than guessed (DEC-2136 item 1).
    fn fill(&mut self, payload: &Payload<'_>) -> Result<(), Unreadable> {
        let order = payload.text("client_order_id")?.to_owned();
        let protective = self
            .orders
            .get(&order)
            .is_some_and(|known| known.protective);
        if !protective && !self.own.contains(&order) {
            return Ok(());
        }
        let id = payload.text("fill_id")?.to_owned();
        let qty = quantity(payload, "qty_gross")?;
        let price = Price::parse(payload.text("price")?).map_err(|_| (payload.refused)("price"))?;
        let signed = match payload.text("side")? {
            "buy" => SignedQty::from(qty),
            "sell" => SignedQty::from(qty).negated(),
            _ => return Err((payload.refused)("side")),
        };
        let instrument = payload.text("instrument_id")?.to_owned();
        let in_asset = fee_in_asset(payload)?;
        if !self.fills.insert(id) {
            return Ok(());
        }
        if let Some(known) = self.orders.get_mut(&order) {
            known.filled = known
                .filled
                .checked_add(qty)
                .map_err(|_| (payload.refused)("qty_gross"))?;
        }
        if !self.own.contains(&order) {
            let unattributed = self
                .orders
                .get(&order)
                .is_some_and(|known| !known.attributed);
            let held = self
                .filled
                .get(&instrument)
                .is_some_and(|qty| !qty.is_zero());
            if unattributed && held {
                return Err((payload.refused)("client_order_id"));
            }
            return Ok(());
        }
        let net = self
            .filled
            .entry(instrument.clone())
            .or_insert(SignedQty::ZERO);
        *net = net
            .checked_add(signed)
            .map_err(|_| (payload.refused)("qty_gross"))?;
        if in_asset {
            self.unreported.insert(instrument.clone());
        }
        let held = self
            .ledgers
            .get(&instrument)
            .copied()
            .unwrap_or(Position::FLAT);
        let (after, _) = held
            .after_fill(signed, price)
            .map_err(|_| (payload.refused)("qty_gross"))?;
        self.ledgers.insert(instrument, after);
        Ok(())
    }

    /// An order's change of state, as the executor's fold reads it: an `ignored` change changes
    /// nothing; a replacement is a new order of the old one's kind for what it left unfilled, and
    /// the agent's when the old one was; and a terminal state of a protective order takes it off
    /// its instrument's protection with the quantity it covered.
    fn changed(&mut self, payload: &Payload<'_>) -> Result<(), Unreadable> {
        let order = payload.text("client_order_id")?.to_owned();
        let state = payload.text("state")?;
        if let Some(old) = optional_text(payload, "replaces")? {
            let original = self
                .orders
                .get(old)
                .copied()
                .ok_or_else(|| (payload.refused)("replaces"))?;
            if self.own.contains(old) {
                self.own.insert(order.clone());
            }
            let left = original
                .qty
                .checked_sub(original.filled)
                .map_err(|_| (payload.refused)("replaces"))?;
            self.orders.insert(
                order.clone(),
                Order {
                    qty: left,
                    filled: Qty::ZERO,
                    ..original
                },
            );
        }
        if !self.orders.contains_key(&order) {
            return Err((payload.refused)("client_order_id"));
        }
        if payload.flag("ignored")? {
            return Ok(());
        }
        if let Some(successor) = optional_text(payload, "replaced_by")?
            && TERMINAL.contains(&state)
            && self.own.contains(&order)
        {
            self.own.insert(successor.to_owned());
        }
        let ended = self
            .orders
            .get(&order)
            .filter(|known| known.protective && TERMINAL.contains(&state))
            .map(|known| known.qty);
        if let Some(qty) = ended {
            self.leaves_protection(&order, qty);
        }
        Ok(())
    }

    fn leaves_protection(&mut self, order: &str, qty: Qty) {
        let instrument = self
            .resting
            .iter()
            .find(|(_, resting)| resting.orders.iter().any(|id| id == order))
            .map(|(instrument, _)| instrument.clone());
        let Some(instrument) = instrument else {
            return;
        };
        if let Some(resting) = self.resting.get_mut(&instrument) {
            resting.orders.retain(|id| id != order);
            resting.covered = resting.covered.checked_sub(qty).unwrap_or(Qty::ZERO);
            if resting.orders.is_empty() {
                self.resting.remove(&instrument);
            }
        }
    }

    fn is_plain_entry(&self, entry: &str) -> bool {
        self.plain.get(entry).copied().unwrap_or(false)
    }

    /// `ProtectionChanged`, by the executor fold's rules for the resting orders and the
    /// unprotected intervals; the actions that move neither change nothing here.
    fn protection_changed(&mut self, payload: &Payload<'_>) -> Result<(), Unreadable> {
        let instrument = payload.text("instrument_id")?.to_owned();
        let at = stamp(payload, "risk_clock")?;
        match payload.text("action")? {
            "placed" => {
                let orders = names(payload, "orders")?;
                let covered = quantity(payload, "qty")?;
                let prices = prices(payload)?;
                for id in &orders {
                    let entry = protected_entry(id);
                    let attributed = entry
                        .as_ref()
                        .is_some_and(|entry| self.orders.contains_key(entry));
                    if entry.is_some_and(|entry| self.own.contains(&entry))
                        && !self.orders.contains_key(id)
                    {
                        self.own.insert(id.clone());
                    }
                    self.orders.entry(id.clone()).or_insert(Order {
                        protective: true,
                        qty: covered,
                        filled: Qty::ZERO,
                        attributed,
                    });
                }
                let resting = self.resting.entry(instrument).or_insert(Resting {
                    orders: Vec::new(),
                    covered: Qty::ZERO,
                    prices: None,
                });
                resting.orders.extend(orders);
                resting.prices = prices.or(resting.prices);
                resting.covered = resting
                    .covered
                    .checked_add(covered)
                    .map_err(|_| (payload.refused)("qty"))?;
            }
            "cancelled" => {
                let orders = names(payload, "orders")?;
                let uncovered = optional_quantity(payload, "qty")?;
                if let Some(resting) = self.resting.get_mut(&instrument) {
                    resting.orders.retain(|id| !orders.contains(id));
                    if let Some(uncovered) = uncovered {
                        resting.covered =
                            resting.covered.checked_sub(uncovered).unwrap_or(Qty::ZERO);
                    }
                    if resting.orders.is_empty() {
                        self.resting.remove(&instrument);
                    }
                }
            }
            "unprotected_start" | "passive_start" => {
                let passive = payload.text("action")? == "passive_start";
                let bracket = optional_text(payload, "bracket")?.map(str::to_owned);
                let plain_entry = bracket
                    .as_deref()
                    .is_some_and(|entry| self.is_plain_entry(entry));
                if !passive && !plain_entry {
                    let awaited: Option<BTreeSet<String>> = self
                        .awaiting
                        .remove(&instrument)
                        .map(|ids| ids.iter().filter_map(|id| protected_entry(id)).collect());
                    if let Some(waiting) = self.intervals.iter_mut().find(|interval| {
                        interval.instrument == instrument
                            && !interval.ended
                            && (interval.uncovered
                                || awaited.as_ref().is_some_and(|entries| {
                                    interval
                                        .bracket
                                        .as_ref()
                                        .is_none_or(|entry| entries.contains(entry))
                                }))
                    }) {
                        waiting.ended = true;
                    }
                }
                if !passive {
                    self.intervals.push(Interval {
                        instrument,
                        started_at: at,
                        ended: false,
                        alerted: false,
                        uncovered: false,
                        bracket,
                    });
                }
            }
            "unprotected_end" if payload.value.get("acknowledged") == Some(&TRUE) => {
                let entries: BTreeSet<String> = self
                    .awaiting
                    .remove(&instrument)
                    .into_iter()
                    .flatten()
                    .filter_map(|id| protected_entry(&id))
                    .collect();
                if let Some(open) = self.intervals.iter_mut().find(|interval| {
                    interval.instrument == instrument
                        && !interval.ended
                        && interval
                            .bracket
                            .as_ref()
                            .is_none_or(|entry| entries.contains(entry))
                }) {
                    open.ended = true;
                }
                let plain: BTreeSet<String> = entries
                    .into_iter()
                    .filter(|entry| self.is_plain_entry(entry))
                    .collect();
                for open in self.intervals.iter_mut().filter(|interval| {
                    interval.instrument == instrument
                        && !interval.ended
                        && interval
                            .bracket
                            .as_ref()
                            .is_some_and(|entry| plain.contains(entry))
                }) {
                    open.ended = true;
                }
            }
            "interval_limit" | "unprotected_end" => {
                let finished = payload.text("action")? == "unprotected_end";
                let awaiting = names(payload, "awaiting")?;
                let uncovered = payload.value.get("uncovered") == Some(&TRUE);
                let ends = finished && awaiting.is_empty() && !uncovered;
                let named = optional_text(payload, "bracket")?.map(str::to_owned);
                let joins = named
                    .as_deref()
                    .is_some_and(|entry| self.is_plain_entry(entry));
                if !awaiting.is_empty() {
                    let awaited = self.awaiting.entry(instrument.clone()).or_default();
                    if !joins {
                        awaited.clear();
                    }
                    awaited.extend(awaiting);
                }
                if let Some(open) = self.intervals.iter_mut().find(|interval| {
                    interval.instrument == instrument
                        && !interval.ended
                        && (named.is_none() || interval.bracket == named)
                }) {
                    if ends {
                        open.ended = true;
                    } else if uncovered {
                        open.uncovered = true;
                    } else if !finished {
                        open.alerted = true;
                    }
                }
            }
            "intended"
            | "watchdog"
            | "exit_unpriced"
            | "ladder_floor"
            | "expiry_unreplaceable"
            | "rung_short" => {}
            _ => return Err((payload.refused)("action")),
        }
        Ok(())
    }
}

const TRUE: mandate_canon::Value = mandate_canon::Value::Bool(true);

/// The entry a protective leg's id names: everything before its last `-p` (trading spec §2.3).
fn protected_entry(leg: &str) -> Option<String> {
    leg.rsplit_once("-p")
        .map(|(entry, _)| entry.to_owned())
        .filter(|entry| !entry.is_empty())
}

fn instrument(payload: &Payload<'_>) -> Result<String, Unreadable> {
    payload
        .text("instrument_id")
        .or_else(|_| payload.text("instrument"))
        .map(str::to_owned)
}

fn quantity(payload: &Payload<'_>, member: &'static str) -> Result<Qty, Unreadable> {
    Qty::parse(payload.text(member)?).map_err(|_| (payload.refused)(member))
}

fn optional_text<'p>(
    payload: &'p Payload<'_>,
    member: &'static str,
) -> Result<Option<&'p str>, Unreadable> {
    match payload.value.get(member) {
        None | Some(mandate_canon::Value::Null) => Ok(None),
        Some(mandate_canon::Value::Str(text)) => Ok(Some(text.as_str())),
        Some(_) => Err((payload.refused)(member)),
    }
}

fn optional_quantity(
    payload: &Payload<'_>,
    member: &'static str,
) -> Result<Option<Qty>, Unreadable> {
    optional_text(payload, member)?
        .map(|qty| Qty::parse(qty).map_err(|_| (payload.refused)(member)))
        .transpose()
}

fn optional_price(
    payload: &Payload<'_>,
    member: &'static str,
) -> Result<Option<Price>, Unreadable> {
    optional_text(payload, member)?
        .map(|price| Price::parse(price).map_err(|_| (payload.refused)(member)))
        .transpose()
}

/// A record's stop and, unless crypto's, its take-profit.
fn prices(payload: &Payload<'_>) -> Result<Option<(Price, Option<Price>)>, Unreadable> {
    let Some(stop) = optional_price(payload, "stop")? else {
        return Ok(None);
    };
    Ok(Some((stop, optional_price(payload, "take_profit")?)))
}

/// The longest client order id the read serves (the read model's bound; Alpaca's is 128 bytes).
const ORDER_ID_MAX: usize = 200;

/// The order ids a list member names, each non-empty and within [`ORDER_ID_MAX`].
fn names(payload: &Payload<'_>, member: &'static str) -> Result<Vec<String>, Unreadable> {
    match payload.value.get(member) {
        Some(mandate_canon::Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .filter(|id| !id.is_empty() && id.chars().count() <= ORDER_ID_MAX)
                    .map(str::to_owned)
                    .ok_or_else(|| (payload.refused)(member))
            })
            .collect(),
        _ => Err((payload.refused)(member)),
    }
}

/// Whether a fill's `fees` charge any fee in an asset other than USD, which moves the quantity
/// received (trading spec §8.3) by a rule only the accounting fold applies.
fn fee_in_asset(payload: &Payload<'_>) -> Result<bool, Unreadable> {
    let Some(mandate_canon::Value::Array(fees)) = payload.value.get("fees") else {
        return Err((payload.refused)("fees"));
    };
    fees.iter().try_fold(false, |any, fee| {
        let asset = fee
            .get("asset")
            .and_then(mandate_canon::Value::as_str)
            .ok_or_else(|| (payload.refused)("fees"))?;
        Ok(any || asset != "USD")
    })
}
