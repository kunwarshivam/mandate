#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The journaled facts an opening's builder and gate read (DEC-2378 S1): the mandate version in
//! force, the account's latest snapshot, and each instrument's latest risk mark, folded from
//! journal rows alone. The cell's runtime and the executor both fold through it, so the two cannot
//! disagree about a mark or a snapshot.
//!
//! **The risk clock is the only time.** [`Journaled::now`] is the later of two journaled clocks:
//! the latest `risk_clock` (or a copied `ClockAdvanced`'s `clock`) on the account stream, and the
//! latest `ClockAdvanced` on the scheduler's clock stream, which ticks even when the account
//! stream is quiet, so a fact ages while nothing is written about it. Nothing here reads a wall
//! clock, so a replay of the same rows gives the same facts (mandate spec §5.2).
//!
//! **It fails closed** (DEC-2378 item 3). [`Journaled::facts`] answers [`Held`] unless every fact
//! is journaled, current by the risk clock, and readable, and a row of a type it reads that does
//! not parse holds every later answer too. It is for openings only: an exit, a protective order
//! and a flatten never consult it, so a missing fact never holds risk reduction (`AGENTS.md`
//! rule 13).

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::Value;
use mandate_num::{Price, Qty, SignedQty, Usd};
use mandate_time::UtcNanos;

mod rule_set;

pub use rule_set::{RuleSetError, gate_rule_set};

/// One journal row, as its store hands it over, in journal order.
#[derive(Debug, Clone, Copy)]
pub struct Row<'a> {
    pub stream: &'a str,
    pub event_type: &'a str,
    pub payload: &'a Value,
}

/// How old, in risk-clock seconds, a fact may grow before it no longer admits an opening.
///
/// Both count every second of the risk clock. For an equity's mark that is tighter than mandate
/// spec §5.2's regular-session seconds, so it can only hold more (DEC-2378 item 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Freshness {
    pub mark_max_age_s: u32,
    pub snapshot_max_age_s: u32,
}

/// The account as its latest `AccountSnapshotRecorded` states it (journal spec §9.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountFacts {
    pub status: String,
    pub crypto_status: String,
    pub trading_blocked: bool,
    pub account_blocked: bool,
    pub trade_suspended_by_user: bool,
    pub multiplier: u64,
    pub equity: Usd,
    pub cash: Usd,
    pub buying_power: Usd,
    pub non_marginable_buying_power: Usd,
    pub at: UtcNanos,
}

/// An instrument's latest sane risk mark from a counted session (journal spec §9.30).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark {
    pub bid: Price,
    pub at: UtcNanos,
}

/// Every fact an opening needs, each current at `now`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub now: UtcNanos,
    pub mandate_version: String,
    pub account: AccountFacts,
    pub mark: Mark,
}

/// Why an opening is held: the first fact, in this order, that is not journaled, current, or
/// readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Held {
    #[error("a journal row the fold reads did not parse")]
    Unreadable,
    #[error("no risk clock on the account stream")]
    NoClock,
    #[error("no mandate version in force for the agent")]
    NoMandate,
    #[error("no account snapshot")]
    NoSnapshot,
    #[error("the account snapshot is stale")]
    StaleSnapshot,
    #[error("no sane mark for the instrument")]
    NoMark,
    #[error("the instrument's latest mark is not sane")]
    InsaneMark,
    #[error("the instrument's mark is stale")]
    StaleMark,
}

/// A row of a type the fold reads that does not parse, naming the event type and the member.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{event_type} is unreadable at {member}")]
pub struct Unreadable {
    pub event_type: String,
    pub member: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Latest {
    Sane(Mark),
    NotSane,
}

/// The fold over one account's journal rows for one agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Journaled {
    account_stream: String,
    agent: String,
    clock_stream: String,
    account_now: Option<UtcNanos>,
    scheduler_now: Option<UtcNanos>,
    deployed: Option<String>,
    applied: Option<String>,
    stopped: bool,
    account: Option<AccountFacts>,
    marks: BTreeMap<String, Latest>,
    requested_by: Option<String>,
    orders: BTreeMap<String, String>,
    ended: BTreeSet<String>,
    fills: BTreeSet<String>,
    positions: BTreeMap<String, SignedQty>,
    released: BTreeMap<String, Qty>,
    models: BTreeMap<(String, String), String>,
    account_positions: BTreeMap<String, SignedQty>,
    unreadable: bool,
}

impl Journaled {
    /// The fold for `agent` on the account stream `account_stream`, ticked by the scheduler's
    /// clock stream `clock_stream` (`clock:{workspace}`).
    pub fn new(account_stream: &str, clock_stream: &str, agent: &str) -> Self {
        Self {
            account_stream: account_stream.to_owned(),
            agent: agent.to_owned(),
            clock_stream: clock_stream.to_owned(),
            account_now: None,
            scheduler_now: None,
            deployed: None,
            applied: None,
            stopped: false,
            account: None,
            marks: BTreeMap::new(),
            requested_by: None,
            orders: BTreeMap::new(),
            ended: BTreeSet::new(),
            fills: BTreeSet::new(),
            positions: BTreeMap::new(),
            released: BTreeMap::new(),
            models: BTreeMap::new(),
            account_positions: BTreeMap::new(),
            unreadable: false,
        }
    }

    /// Folds one row. A row of a type the fold reads that does not parse is refused and holds
    /// every later [`Journaled::facts`], since it may have changed a fact.
    pub fn fold(&mut self, row: Row<'_>) -> Result<(), Unreadable> {
        let folded = self.read(row);
        if folded.is_err() {
            self.unreadable = true;
        }
        folded
    }

    /// The agent's sub-ledger, as the executor's `sub_ledger` computes it (DEC-2378 item 7): per
    /// instrument, what the agent's own fills hold less what the agent released to the owner
    /// (DEC-2354), capped at what the account holds, and only where that is above zero. A flatten
    /// sells it with or without the facts an opening needs (`AGENTS.md` rule 13).
    pub fn held(&self) -> BTreeMap<String, Qty> {
        self.positions
            .iter()
            .filter_map(|(instrument, lots)| {
                let released = self.released.get(instrument).copied().unwrap_or(Qty::ZERO);
                let own = lots.checked_add(SignedQty::from(released).negated()).ok()?;
                let open = self
                    .account_positions
                    .get(instrument)
                    .copied()
                    .unwrap_or(SignedQty::ZERO);
                let held = own.min(open);
                (!held.is_negative() && held != SignedQty::ZERO)
                    .then(|| (instrument.clone(), held.abs()))
            })
            .collect()
    }

    /// The agent's own orders that no terminal state has ended: each `client_order_id` with its
    /// instrument, which a flatten cancels (DEC-2378 item 7).
    pub fn working(&self) -> BTreeMap<String, String> {
        self.orders
            .iter()
            .filter(|(order, _)| !self.ended.contains(*order))
            .map(|(order, instrument)| (order.clone(), instrument.clone()))
            .collect()
    }

    /// Each model the workspace registered (`ConfigSnapshotRegistered` of kind `model_version`),
    /// by its id and version, with the content hash it was registered under, the latest one
    /// where it was registered again. The builder sizes only outputs of a model listed here
    /// (DEC-2378 item 10).
    pub fn models(&self) -> BTreeMap<(String, String), String> {
        self.models.clone()
    }

    /// The mandate version in force: the latest one applied on the account stream, else the one
    /// deployed, and none once the agent is stopped. The streams are folded in any interleaving,
    /// so the answer is the same whichever stream reaches the fold first: a deployment comes once
    /// and before every version applied over it, and a stop is final (DEC-1173).
    fn mandate(&self) -> Option<String> {
        if self.stopped {
            return None;
        }
        self.applied.clone().or_else(|| self.deployed.clone())
    }

    /// The later of the account stream's latest risk clock and the scheduler's latest tick.
    pub fn now(&self) -> Option<UtcNanos> {
        self.account_now.max(self.scheduler_now)
    }

    /// The facts an opening in `instrument` reads, or why it is held.
    pub fn facts(&self, instrument: &str, fresh: Freshness) -> Result<Facts, Held> {
        if self.unreadable {
            return Err(Held::Unreadable);
        }
        if self.scheduler_now.is_none() {
            return Err(Held::NoClock);
        }
        let now = self.now().ok_or(Held::NoClock)?;
        let mandate_version = self.mandate().ok_or(Held::NoMandate)?;
        let account = self.account.clone().ok_or(Held::NoSnapshot)?;
        if reached(now, account.at, fresh.snapshot_max_age_s) {
            return Err(Held::StaleSnapshot);
        }
        let mark = match self.marks.get(instrument) {
            None => return Err(Held::NoMark),
            Some(Latest::NotSane) => return Err(Held::InsaneMark),
            Some(Latest::Sane(mark)) => *mark,
        };
        if reached(now, mark.at, fresh.mark_max_age_s) {
            return Err(Held::StaleMark);
        }
        Ok(Facts {
            now,
            mandate_version,
            account,
            mark,
        })
    }

    fn read(&mut self, row: Row<'_>) -> Result<(), Unreadable> {
        let on_account = row.stream == self.account_stream;
        let refused = |member| Unreadable {
            event_type: row.event_type.to_owned(),
            member,
        };
        let payload = Payload {
            value: row.payload,
            refused: &refused,
        };
        if row.stream == self.clock_stream {
            if row.event_type == "ClockAdvanced" {
                let tick = stamp(&payload, "clock")?;
                self.scheduler_now = Some(later(self.scheduler_now, tick, &payload, "clock")?);
            }
            return Ok(());
        }
        let at = if on_account {
            self.advance(&payload, row.event_type)?
        } else {
            None
        };
        let ours = || payload.text("agent_id").map(|agent| agent == self.agent);
        match (row.event_type, at) {
            ("AgentDeployed", _) if ours()? => {
                self.deployed = Some(payload.text("mandate_version")?.to_owned());
            }
            ("AgentStopped", _) if ours()? => self.stopped = true,
            ("MandateVersionApplied", Some(_)) if ours()? => match payload.text("result")? {
                "applied" => self.applied = Some(payload.text("new_version")?.to_owned()),
                "rejected" => {}
                _ => return Err(refused("result")),
            },
            ("AccountSnapshotRecorded", Some(at)) => {
                self.account = Some(AccountFacts {
                    status: payload.text("status")?.to_owned(),
                    crypto_status: payload.text("crypto_status")?.to_owned(),
                    trading_blocked: payload.flag("trading_blocked")?,
                    account_blocked: payload.flag("account_blocked")?,
                    trade_suspended_by_user: payload.flag("trade_suspended_by_user")?,
                    multiplier: payload
                        .value
                        .get("multiplier")
                        .and_then(Value::as_int)
                        .ok_or_else(|| refused("multiplier"))?,
                    equity: payload.usd("equity")?,
                    cash: payload.usd("cash")?,
                    buying_power: payload.usd("buying_power")?,
                    non_marginable_buying_power: payload.usd("non_marginable_buying_power")?,
                    at,
                });
            }
            ("MarkUpdated", Some(at)) => self.mark(&payload, at)?,
            ("OrderRequestRecorded", _) if on_account => {
                self.requested_by = Some(payload.text("agent_id")?.to_owned());
            }
            ("OrderSubmitted", _) if on_account => {
                let order = payload.text("client_order_id")?.to_owned();
                let instrument = payload.text("instrument_id")?.to_owned();
                if self.requested_by.take().as_deref() == Some(self.agent.as_str()) {
                    self.orders.insert(order, instrument);
                }
            }
            ("FillApplied" | "LateFillApplied", _) if on_account => self.fill(&payload)?,
            ("OrderStateChanged", _) if on_account => self.changed(&payload)?,
            ("OrderAbandoned", _) if on_account => {
                self.ended
                    .insert(payload.text("client_order_id")?.to_owned());
            }
            ("PositionReleased", _) if on_account => self.released(&payload)?,
            ("ConfigSnapshotRegistered", _) if payload.text("kind")? == "model_version" => {
                let model = (
                    payload.text("model_id")?.to_owned(),
                    payload.text("model_version")?.to_owned(),
                );
                let hash = payload.text("content_hash")?.to_owned();
                self.models.insert(model, hash);
            }
            ("FeesCharged", _) if on_account => self.fees(&payload)?,
            ("CorporateActionApplied", _) if on_account => {
                return Err(refused("corporate_action"));
            }
            _ => {}
        }
        Ok(())
    }

    /// Moves the clock to the row's `risk_clock`, or a `ClockAdvanced`'s `clock`, and answers it
    /// for the row to be read at. A row with neither moves nothing and answers the clock so far,
    /// unless it is of a type read at its own clock, which then has none.
    fn advance(
        &mut self,
        payload: &Payload<'_>,
        event_type: &str,
    ) -> Result<Option<UtcNanos>, Unreadable> {
        let member = if event_type == "ClockAdvanced" {
            "clock"
        } else {
            "risk_clock"
        };
        if payload.value.get(member).is_none() {
            return if CLOCKED.contains(&event_type) {
                Err((payload.refused)(member))
            } else {
                Ok(self.account_now)
            };
        }
        let at = later(self.account_now, stamp(payload, member)?, payload, member)?;
        self.account_now = Some(at);
        Ok(Some(at))
    }

    /// A fill of one of the agent's own orders, each `fill_id` once. An order is the agent's when
    /// the `OrderRequestRecorded` journaled immediately before its `OrderSubmitted` names the agent
    /// (journal spec rule 45).
    fn fill(&mut self, payload: &Payload<'_>) -> Result<(), Unreadable> {
        let ours = self.orders.contains_key(payload.text("client_order_id")?);
        let id = payload.text("fill_id")?.to_owned();
        let qty =
            Qty::parse(payload.text("qty_gross")?).map_err(|_| (payload.refused)("qty_gross"))?;
        let signed = match payload.text("side")? {
            "buy" => SignedQty::from(qty),
            "sell" => SignedQty::from(qty).negated(),
            _ => return Err((payload.refused)("side")),
        };
        let instrument = payload.text("instrument_id")?.to_owned();
        if !self.fills.insert(id) {
            return Ok(());
        }
        add(&mut self.account_positions, &instrument, signed, payload)?;
        if ours {
            add(&mut self.positions, &instrument, signed, payload)?;
        }
        Ok(())
    }

    /// An order's change of state. A terminal state ends the order, and a replacement of one of
    /// the agent's orders is the agent's too. A change the executor journaled as `ignored`
    /// changes nothing.
    fn changed(&mut self, payload: &Payload<'_>) -> Result<(), Unreadable> {
        let order = payload.text("client_order_id")?.to_owned();
        let state = payload.text("state")?;
        let ignored = payload.flag("ignored")?;
        let successor = match payload.value.get("replaced_by") {
            None | Some(Value::Null) => None,
            Some(Value::Str(successor)) => Some(successor.clone()),
            Some(_) => return Err((payload.refused)("replaced_by")),
        };
        let Some(instrument) = self.orders.get(&order).cloned().filter(|_| !ignored) else {
            return Ok(());
        };
        if TERMINAL.contains(&state) {
            self.ended.insert(order);
        }
        if let Some(successor) = successor {
            self.orders.insert(successor, instrument);
        }
        Ok(())
    }

    /// The agent's release of `qty` to the owner (DEC-2354), which leaves its sub-ledger.
    fn released(&mut self, payload: &Payload<'_>) -> Result<(), Unreadable> {
        let agent = payload.text("agent_id")?;
        let qty = Qty::parse(payload.text("qty")?).map_err(|_| (payload.refused)("qty"))?;
        let instrument = payload
            .text("instrument_id")
            .or_else(|_| payload.text("instrument"))?
            .to_owned();
        if agent != self.agent {
            return Ok(());
        }
        let total = self.released.entry(instrument).or_insert(Qty::ZERO);
        *total = total
            .checked_add(qty)
            .map_err(|_| (payload.refused)("qty"))?;
        Ok(())
    }

    /// A fee charged in the crypto asset itself, which leaves the account's position.
    fn fees(&mut self, payload: &Payload<'_>) -> Result<(), Unreadable> {
        if payload.text("family")? != "crypto_asset" {
            return Ok(());
        }
        let accrued =
            Qty::parse(payload.text("accrued")?).map_err(|_| (payload.refused)("accrued"))?;
        let instrument = payload
            .text("instrument_id")
            .or_else(|_| payload.text("instrument"))?
            .to_owned();
        let charged = SignedQty::from(accrued).negated();
        add(&mut self.account_positions, &instrument, charged, payload)
    }

    fn mark(&mut self, payload: &Payload<'_>, at: UtcNanos) -> Result<(), Unreadable> {
        let Some(sane) = payload.value.get("sane") else {
            return Ok(());
        };
        let counted = match payload.text("session")? {
            "regular" | "crypto" => true,
            "overnight" | "pre_market" | "after_hours" => false,
            _ => return Err((payload.refused)("session")),
        };
        let latest = match sane {
            Value::Bool(true) => {
                let bid = payload
                    .text("bid")
                    .ok()
                    .and_then(|bid| Price::parse(bid).ok())
                    .ok_or_else(|| (payload.refused)("bid"))?;
                Latest::Sane(Mark { bid, at })
            }
            Value::Bool(false) => Latest::NotSane,
            _ => return Err((payload.refused)("sane")),
        };
        if counted {
            let instrument = payload.text("instrument_id")?.to_owned();
            self.marks.insert(instrument, latest);
        }
        Ok(())
    }
}

/// The order states that end an order (trading-domain spec §5.7), `abandoned` among them.
const TERMINAL: [&str; 6] = [
    "filled",
    "canceled",
    "rejected",
    "expired",
    "replaced",
    "abandoned",
];

/// The account-stream types the fold reads at their own `risk_clock`, which each must carry.
const CLOCKED: [&str; 4] = [
    "AccountSnapshotRecorded",
    "ClockAdvanced",
    "MandateVersionApplied",
    "MarkUpdated",
];

/// Adds `qty` to `instrument`'s position in `positions`.
fn add(
    positions: &mut BTreeMap<String, SignedQty>,
    instrument: &str,
    qty: SignedQty,
    payload: &Payload<'_>,
) -> Result<(), Unreadable> {
    let position = positions
        .entry(instrument.to_owned())
        .or_insert(SignedQty::ZERO);
    *position = position
        .checked_add(qty)
        .map_err(|_| (payload.refused)("qty"))?;
    Ok(())
}

/// A journaled instant in `member`, or the member refused.
fn stamp(payload: &Payload<'_>, member: &'static str) -> Result<UtcNanos, Unreadable> {
    payload
        .value
        .get(member)
        .and_then(Value::as_str)
        .and_then(|stamp| UtcNanos::parse(stamp).ok())
        .ok_or_else(|| (payload.refused)(member))
}

/// `at`, refused when it goes back behind `so_far`, the same stream's clock: each stream's clock
/// only moves forward (journal spec §2).
fn later(
    so_far: Option<UtcNanos>,
    at: UtcNanos,
    payload: &Payload<'_>,
    member: &'static str,
) -> Result<UtcNanos, Unreadable> {
    if so_far.is_some_and(|so_far| at < so_far) {
        return Err((payload.refused)(member));
    }
    Ok(at)
}

/// Whether a fact observed at `at` has reached `max_age_s` by `now`.
fn reached(now: UtcNanos, at: UtcNanos, max_age_s: u32) -> bool {
    now.secs().saturating_sub(at.secs()) >= i64::from(max_age_s)
}

struct Payload<'a> {
    value: &'a Value,
    refused: &'a dyn Fn(&'static str) -> Unreadable,
}

impl Payload<'_> {
    fn text(&self, member: &'static str) -> Result<&str, Unreadable> {
        self.value
            .get(member)
            .and_then(Value::as_str)
            .ok_or_else(|| (self.refused)(member))
    }

    fn flag(&self, member: &'static str) -> Result<bool, Unreadable> {
        match self.value.get(member) {
            Some(Value::Bool(flag)) => Ok(*flag),
            _ => Err((self.refused)(member)),
        }
    }

    fn usd(&self, member: &'static str) -> Result<Usd, Unreadable> {
        Usd::parse(self.text(member)?).map_err(|_| (self.refused)(member))
    }
}
