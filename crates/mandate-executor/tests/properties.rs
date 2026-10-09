//! Property tests for the idempotent executor, against four oracles that share no code with the
//! crates under test
//! ([task brief](../../../docs/project/tasks/M6-K-executor-and-connector.md)).
//!
//! 1. **The broker-side counter** lives in `common::FakeConnector`: a `BTreeMap` of how many
//!    distinct submissions it accepted per client order id, accumulated on the broker side rather
//!    than derived from the journal, so an executor that journals one `OrderSubmitted` and sends
//!    twice still fails.
//! 2. **The shadow order book** rebuilds each order's state, filled quantity, and reservation
//!    from the emitted drafts' payloads alone, through a transition table transcribed here from
//!    trading-domain spec §5.7, so an executor that keeps state the journal does not carry fails.
//! 3. **The shadow position ledger** re-accumulates positions, cash, and fees from `FillApplied`,
//!    `LateFillApplied`, `FeesCharged`, and the corporate-action events with `i128` arithmetic,
//!    independently of `mandate-accounting`, so a reconciliation that compares the ledger against
//!    itself cannot pass.
//! 4. **The protection accountant** sums protective sell quantity per instrument and records
//!    every interval in which that sum is below the position, from the drafts alone, so "every
//!    unprotected interval is journaled and bounded" is checked against an interval set the
//!    executor did not build.
//!
//! Every property first compares the **number** of emitted effects, drafts, or accepted
//! submissions with the oracle's, so none can pass on an empty list (the E4-1 lesson, and
//! [#134]'s round-1 finding 5).
//!
//! The generators keep every stream's risk clock **monotone**, as journal spec §2 requires of the
//! `risk_clock` field: a script that moved it backwards would produce a run the journal itself
//! would reject ([#134]'s round-1 finding 3).
//!
//! [#134]: https://github.com/kunwarshivam/mandate/pull/134

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{
    ACCOUNT_STREAM, CrashPoint, FixedInstruments, FixedMandate, Ran, Shell, TestIds, clock, config,
    handoff, named_orders, opening, ports, protected_instrument, quote, risk_exit, scope, snapshot,
    stream_opened,
};
use mandate_canon::Value;
use mandate_executor::{
    BrokerOutcome, BrokerRequest, BrokerUnknown, BrokerUpdate, ClientOrderId, Command, Effect,
    EventDraft, EventId, ExecutorState, Initiator, Input, IntentId, KillScope, OrderState, Purpose,
    ReconcileReason, ReconciliationVerdict, RiskClock, Seq,
};
use proptest::prelude::*;

const AAPL: &str = FixedInstruments::LIQUID_EQUITY;
const CPHC: &str = FixedInstruments::THIN_EQUITY;

/// The one quote every script shows, in both instruments: a sell limit at or below the bid, or a
/// buy limit at or above the ask, is marketable against it.
const BID: &str = "150";
const ASK: &str = "150.2";

/// Trading-domain spec §5.7's state names, transcribed here rather than read from the crate, so
/// that the two agreeing means something.
fn shadow_state(name: &str) -> Option<&'static str> {
    Some(match name {
        "intent" => "intent",
        "submitting" => "submitting",
        "accepted" => "accepted",
        "partially_filled" => "partially_filled",
        "pending_cancel" => "pending_cancel",
        "pending_replace" => "pending_replace",
        "unknown" => "unknown",
        "filled" => "filled",
        "canceled" => "canceled",
        "rejected" => "rejected",
        "expired" => "expired",
        "replaced" => "replaced",
        "abandoned" => "abandoned",
        _ => return None,
    })
}

/// The six terminal states §5.7's diagram names, each of which releases a reservation.
const TERMINAL: [&str; 6] = [
    "filled",
    "canceled",
    "rejected",
    "expired",
    "replaced",
    "abandoned",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ShadowOrder {
    state: String,
    fills: BTreeSet<String>,
    filled_units: i128,
    qty_units: i128,
    reserved: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ShadowBook {
    orders: BTreeMap<String, ShadowOrder>,
    /// Every `OrderSubmitted` seen, as `(client_order_id, attempt)`, which is what "at most one
    /// `OrderSubmitted` per attempt" is read off.
    submissions: BTreeSet<(String, u64)>,
    event_ids: Vec<String>,
    /// Every protective order a `ProtectionChanged placed` names. One no `OrderSubmitted` recorded
    /// (a bracket's or OCO's leg, which the broker creates) also joins `orders` as a live sell of
    /// the covered quantity, `accepted`, holding a zero reservation until a terminal state:
    /// trading-domain spec §5.4 ("Ownership of broker-created legs") and DEC-160 (3).
    protective: BTreeSet<String>,
}

/// Reads one text field of a draft payload.
fn field<'a>(draft: &'a EventDraft, name: &str) -> Option<&'a str> {
    draft.payload.get(name).and_then(Value::as_str)
}

fn number(draft: &EventDraft, name: &str) -> Option<u64> {
    draft.payload.get(name).and_then(Value::as_int)
}

/// Canonical decimal text to integer units at nine places, computed here rather than through
/// `mandate-num`, so the two agreeing means something.
fn units(text: &str) -> Option<i128> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (whole, fraction) = match digits.split_once('.') {
        Some((w, f)) => (w, f),
        None => (digits, ""),
    };
    if fraction.len() > 9 || whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut value: i128 = whole.parse().ok()?;
    value = value.checked_mul(1_000_000_000)?;
    let mut scaled: i128 = if fraction.is_empty() {
        0
    } else {
        fraction.parse().ok()?
    };
    for _ in fraction.len()..9 {
        scaled = scaled.checked_mul(10)?;
    }
    value = value.checked_add(scaled)?;
    Some(if negative { -value } else { value })
}

/// A draft's `risk_clock` in whole seconds since the epoch. Journal spec §4.7 and its `risk_clock`
/// row make it a timestamp, `YYYY-MM-DDTHH:MM:SS[.fraction]Z` on a whole second, and "never
/// integer seconds", so it is parsed here, with the civil-date arithmetic written out rather than
/// borrowed from `mandate-time`, so that the two agreeing means something.
fn risk_seconds(draft: &EventDraft) -> Option<i64> {
    let text = field(draft, "risk_clock")?;
    let (date, time) = text.strip_suffix('Z')?.split_once('T')?;
    let mut ymd = date.splitn(3, '-').map(str::parse::<i64>);
    let (year, month, day) = (ymd.next()?.ok()?, ymd.next()?.ok()?, ymd.next()?.ok()?);
    let whole = time.split_once('.').map_or(time, |(whole, _)| whole);
    let mut hms = whole.splitn(3, ':').map(str::parse::<i64>);
    let (hour, minute, second) = (hms.next()?.ok()?, hms.next()?.ok()?, hms.next()?.ok()?);
    let shifted = if month <= 2 { year - 1 } else { year };
    let era = shifted.div_euclid(400);
    let year_of_era = shifted - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Each `OrderSubmitted` with the `OrderRequestRecorded` its `causation_id` names merged in: from
/// schema version 2 the intent id, purpose and agent ride that companion, not the submission
/// (journal spec §9.5, rule 45; DEC-446). A submission whose companion is missing keeps its own
/// members only, so an oracle that needs the intent sees none rather than a neighbour's.
fn submissions_with_requests(drafts: &[EventDraft]) -> Vec<Value> {
    let requests: BTreeMap<&str, &Value> = drafts
        .iter()
        .filter(|d| d.event_type == "OrderRequestRecorded")
        .map(|d| (d.event_id.0.as_str(), &d.payload))
        .collect();
    drafts
        .iter()
        .filter(|d| d.event_type == "OrderSubmitted")
        .map(|d| {
            let mut merged = d.payload.clone();
            if let (Value::Object(merged), Some(Value::Object(companion))) = (
                &mut merged,
                d.causation_id
                    .as_ref()
                    .and_then(|cause| requests.get(cause.0.as_str()).copied()),
            ) {
                for (key, value) in companion {
                    merged.entry(key.clone()).or_insert_with(|| value.clone());
                }
            }
            merged
        })
        .collect()
}

impl ShadowBook {
    fn of(drafts: &[EventDraft]) -> Self {
        let mut book = Self::default();
        for draft in drafts {
            book.event_ids.push(draft.event_id.0.clone());
            match draft.event_type.as_str() {
                "OrderSubmitted" => {
                    let Some(id) = field(draft, "client_order_id") else {
                        continue;
                    };
                    let attempt = number(draft, "attempt").unwrap_or(0);
                    book.submissions.insert((id.to_owned(), attempt));
                    let entry = book.orders.entry(id.to_owned()).or_default();
                    entry.state = "submitting".to_owned();
                    entry.reserved = true;
                    entry.qty_units = field(draft, "qty").and_then(units).unwrap_or(0);
                }
                "OrderStateChanged" => {
                    let Some(id) = field(draft, "client_order_id") else {
                        continue;
                    };
                    let Some(state) = field(draft, "state").and_then(shadow_state) else {
                        continue;
                    };
                    let entry = book.orders.entry(id.to_owned()).or_default();
                    entry.state = state.to_owned();
                    if TERMINAL.contains(&state) {
                        entry.reserved = false;
                    }
                }
                "OrderAbandoned" => {
                    if let Some(id) = field(draft, "client_order_id") {
                        let entry = book.orders.entry(id.to_owned()).or_default();
                        entry.state = "abandoned".to_owned();
                        entry.reserved = false;
                    }
                }
                "ProtectionChanged" if field(draft, "action") == Some("placed") => {
                    let covered = field(draft, "qty").and_then(units).unwrap_or(0);
                    for id in named_orders(&draft.payload, "orders") {
                        book.protective.insert(id.clone());
                        book.orders
                            .entry(id.to_owned())
                            .or_insert_with(|| ShadowOrder {
                                state: "accepted".to_owned(),
                                fills: BTreeSet::new(),
                                filled_units: 0,
                                qty_units: covered,
                                reserved: true,
                            });
                    }
                }
                "FillApplied" | "LateFillApplied" => {
                    let (Some(id), Some(fill)) =
                        (field(draft, "client_order_id"), field(draft, "fill_id"))
                    else {
                        continue;
                    };
                    let entry = book.orders.entry(id.to_owned()).or_default();
                    if entry.fills.insert(fill.to_owned()) {
                        entry.filled_units = entry
                            .filled_units
                            .saturating_add(field(draft, "qty_gross").and_then(units).unwrap_or(0));
                    }
                }
                _ => {}
            }
        }
        book
    }

    /// The same view taken from the crate's own folded state, for the comparison.
    fn of_state(state: &ExecutorState) -> Self {
        let mut book = Self::default();
        for (id, order) in state.orders() {
            let name = match order.state {
                OrderState::Intent => "intent",
                OrderState::Submitting => "submitting",
                OrderState::Accepted => "accepted",
                OrderState::PartiallyFilled => "partially_filled",
                OrderState::PendingCancel => "pending_cancel",
                OrderState::PendingReplace => "pending_replace",
                OrderState::Unknown => "unknown",
                OrderState::Filled => "filled",
                OrderState::Canceled => "canceled",
                OrderState::Rejected => "rejected",
                OrderState::Expired => "expired",
                OrderState::Replaced => "replaced",
                OrderState::Abandoned => "abandoned",
            };
            book.orders.insert(
                id.as_str().to_owned(),
                ShadowOrder {
                    state: name.to_owned(),
                    fills: BTreeSet::new(),
                    filled_units: units(&order.filled_qty.to_string()).unwrap_or(0),
                    qty_units: units(&order.qty.to_string()).unwrap_or(0),
                    reserved: state.reservations().contains_key(id),
                },
            );
        }
        book
    }

    /// Compares only what both sides can know: the state name, the filled units, and whether a
    /// reservation is held. The fill-id set is the shadow's own bookkeeping.
    fn agrees_with(&self, other: &Self) -> Result<(), String> {
        if self.orders.is_empty() {
            return Err(
                "the journal names no order at all, so there is nothing to agree on".into(),
            );
        }
        for extra in other.orders.keys() {
            if !self.orders.contains_key(extra) && !self.protective.contains(extra) {
                return Err(format!(
                    "the state carries {extra}, which no draft names: state the journal does not \
                     carry"
                ));
            }
        }
        for (id, mine) in &self.orders {
            let Some(theirs) = other.orders.get(id) else {
                return Err(format!("{id} is in the journal and not in the state"));
            };
            if mine.state != theirs.state {
                return Err(format!(
                    "{id}: the journal says {} and the state says {}",
                    mine.state, theirs.state
                ));
            }
            if mine.filled_units != theirs.filled_units {
                return Err(format!(
                    "{id}: the journal's fills sum to {} and the state says {}",
                    mine.filled_units, theirs.filled_units
                ));
            }
            if mine.reserved != theirs.reserved {
                return Err(format!(
                    "{id}: the journal implies reserved={} and the state says {}",
                    mine.reserved, theirs.reserved
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ShadowLedger {
    positions: BTreeMap<String, i128>,
    cash_units: i128,
    fee_units: i128,
    simulated_fee_units: i128,
    applied: BTreeSet<String>,
}

impl ShadowLedger {
    fn of(drafts: &[EventDraft]) -> Self {
        let mut ledger = Self::default();
        for draft in drafts {
            match draft.event_type.as_str() {
                "FillApplied" | "LateFillApplied" => {
                    let Some(fill) = field(draft, "fill_id") else {
                        continue;
                    };
                    if !ledger.applied.insert(fill.to_owned()) {
                        continue;
                    }
                    let Some(name) = field(draft, "instrument") else {
                        continue;
                    };
                    let quantity = field(draft, "qty_gross").and_then(units).unwrap_or(0);
                    let price = field(draft, "price").and_then(units).unwrap_or(0);
                    let signed = if field(draft, "side") == Some("sell") {
                        -quantity
                    } else {
                        quantity
                    };
                    let position = ledger.positions.entry(name.to_owned()).or_insert(0);
                    *position = position.saturating_add(signed);
                    let notional = quantity.saturating_mul(price).saturating_div(1_000_000_000);
                    ledger.cash_units = if signed.is_negative() {
                        ledger.cash_units.saturating_add(notional)
                    } else {
                        ledger.cash_units.saturating_sub(notional)
                    };
                }
                "FeesCharged" => {
                    let accrued = field(draft, "accrued").and_then(units).unwrap_or(0);
                    if draft.payload.get("simulated") == Some(&Value::Bool(true)) {
                        ledger.simulated_fee_units =
                            ledger.simulated_fee_units.saturating_add(accrued);
                    } else {
                        ledger.fee_units = ledger.fee_units.saturating_add(accrued);
                    }
                }
                "CorporateActionApplied" => {
                    let (Some(name), Some(ratio)) = (
                        field(draft, "instrument"),
                        field(draft, "ratio").and_then(units),
                    ) else {
                        continue;
                    };
                    if ratio > 0 {
                        let position = ledger.positions.entry(name.to_owned()).or_insert(0);
                        *position = position.saturating_mul(ratio).saturating_div(1_000_000_000);
                    }
                }
                _ => {}
            }
        }
        ledger
    }

    fn of_state(state: &ExecutorState) -> Self {
        Self {
            positions: state
                .positions()
                .iter()
                .map(|(name, qty)| {
                    (
                        name.as_str().to_owned(),
                        units(&qty.to_string()).unwrap_or(0),
                    )
                })
                .filter(|(_, qty)| *qty != 0)
                .collect(),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Interval {
    instrument: String,
    started_at: i64,
    ended_at: Option<i64>,
    /// Whether a bracket's partial fill opened it (its `unprotected_start` names the `bracket`):
    /// its legs are held, not cancelled (§5.4). Every other interval opens where protection is
    /// cancelled for an exit or a re-placement.
    bracket: bool,
    /// The bracket entry its `unprotected_start` names, if any: an `unprotected_end` naming an
    /// entry ends that entry's interval only, so two partly filled brackets in one instrument keep
    /// two intervals (DEC-521 item 3).
    entry: Option<String>,
}

/// One `ProtectionChanged placed`: the orders it names, in one instrument, and the quantity they
/// still cover, which their fills reduce.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Placement {
    instrument: String,
    orders: BTreeSet<String>,
    remaining: i128,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ProtectionAccountant {
    intervals: Vec<Interval>,
    /// Per instrument ever placed, the protective sell quantity still resting: every placement
    /// not yet cancelled, abandoned or terminal, less its fills (§5.4's tranche model), and zero
    /// where none rests any more.
    covered: BTreeMap<String, i128>,
    /// The latest `risk_clock` any draft carried, the end an interval still open is measured to.
    last_clock: i64,
}

impl ProtectionAccountant {
    /// Builds the interval set from the drafts alone: a `ProtectionChanged` whose action opens an
    /// interval starts one, and one whose action closes it ends it. An interval that never ends
    /// is exactly what "journaled from start to end" forbids. The resting protective quantity is
    /// kept the same way: a placement covers its quantity until its orders are recorded
    /// cancelled, abandoned or terminal (§5.7: a terminal order rests no more), and each fill of
    /// one of its orders takes the fill's quantity off it.
    fn of(drafts: &[EventDraft]) -> Self {
        let mut accountant = Self::default();
        let mut placements: Vec<Placement> = Vec::new();
        for draft in drafts {
            let at = risk_seconds(draft);
            if let Some(at) = at {
                accountant.last_clock = accountant.last_clock.max(at);
            }
            let at = at.unwrap_or(accountant.last_clock);
            match draft.event_type.as_str() {
                "FillApplied" | "LateFillApplied" => {
                    let (Some(id), Some(quantity)) = (
                        field(draft, "client_order_id"),
                        field(draft, "qty_gross").and_then(units),
                    ) else {
                        continue;
                    };
                    for placement in placements.iter_mut().filter(|p| p.orders.contains(id)) {
                        placement.remaining = placement.remaining.saturating_sub(quantity);
                    }
                }
                "OrderStateChanged" => {
                    if let Some(id) = field(draft, "client_order_id")
                        && field(draft, "state")
                            .and_then(shadow_state)
                            .is_some_and(|state| TERMINAL.contains(&state))
                    {
                        placements.retain(|p| !p.orders.contains(id));
                    }
                }
                "OrderAbandoned" => {
                    if let Some(id) = field(draft, "client_order_id") {
                        placements.retain(|p| !p.orders.contains(id));
                    }
                }
                "ProtectionChanged" => {
                    let (Some(name), Some(action)) =
                        (protected_instrument(&draft.payload), field(draft, "action"))
                    else {
                        continue;
                    };
                    let named: BTreeSet<String> =
                        named_orders(&draft.payload, "orders").into_iter().collect();
                    match action {
                        "unprotected_start" => accountant.intervals.push(Interval {
                            instrument: name.to_owned(),
                            started_at: at,
                            ended_at: None,
                            bracket: field(draft, "bracket").is_some(),
                            entry: field(draft, "bracket").map(str::to_owned),
                        }),
                        "unprotected_end" => {
                            let entry = field(draft, "bracket");
                            if let Some(open) = accountant.intervals.iter_mut().rev().find(|i| {
                                i.instrument == name
                                    && i.ended_at.is_none()
                                    && (entry.is_none() || i.entry.as_deref() == entry)
                            }) {
                                open.ended_at = Some(at);
                            }
                        }
                        "placed" => {
                            accountant.covered.entry(name.to_owned()).or_insert(0);
                            placements.push(Placement {
                                instrument: name.to_owned(),
                                orders: named,
                                remaining: field(draft, "qty").and_then(units).unwrap_or(0),
                            });
                        }
                        "cancelled" => {
                            placements.retain(|p| p.orders.is_disjoint(&named));
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        for placement in &placements {
            let covered = accountant
                .covered
                .entry(placement.instrument.clone())
                .or_insert(0);
            *covered = covered.saturating_add(placement.remaining.max(0));
        }
        accountant
    }

    fn open(&self) -> usize {
        self.intervals
            .iter()
            .filter(|i| i.ended_at.is_none())
            .count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    /// A fresh or repeated intent. An opening is protected (a GTC bracket at 170 and 140 for two
    /// shares, so a fill of one is a partial fill) or plain (one share); an exit sells one share.
    Intent {
        which: u8,
        exiting: bool,
        other: bool,
        protected: bool,
    },
    Acknowledge,
    Timeout,
    Absent,
    /// One share of the most recent order the broker holds with anything left to fill.
    Fill,
    Cancelled,
    Snapshot,
    KillSwitch,
    Restart,
    /// Thirty seconds with nothing but the clock, which is what lets a bracket's partial-fill
    /// timeout and `max_unprotected_s` pass inside one script.
    Wait,
    /// A reconciliation in which the broker also shows an order nobody on this platform placed:
    /// external activity, which alerts the owner (§7.1).
    External,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        3 => (0u8..4, any::<bool>(), any::<bool>(), any::<bool>()).prop_map(
            |(which, exiting, other, protected)| Step::Intent {
                which,
                exiting,
                other,
                protected,
            }
        ),
        2 => Just(Step::Acknowledge),
        1 => Just(Step::Timeout),
        1 => Just(Step::Absent),
        2 => Just(Step::Fill),
        1 => Just(Step::Cancelled),
        1 => Just(Step::Snapshot),
        1 => Just(Step::KillSwitch),
        1 => Just(Step::Restart),
        1 => Just(Step::Wait),
        1 => Just(Step::External),
    ]
}

/// The steps every script starts with: a plain opening on `AAPL`, acknowledged by the broker. A
/// correct executor must submit it — the account is clean, the intent fresh, and the gate has
/// nothing to deny — so every property below runs against at least one real order, and an
/// executor that does nothing fails before any property is checked (review round 1, finding 1).
const PREFIX: [Step; 2] = [
    Step::Intent {
        which: 0,
        exiting: false,
        other: false,
        protected: false,
    },
    Step::Acknowledge,
];

/// The intent id the prefix submits, and every other intent id a script uses.
fn intent_named(which: u8) -> String {
    format!("01JABCDEFGHJKMNPQRSTVWXY{which:02}")
}

/// The steps half the scripts continue with: the prefix's order fills, then a protected entry of
/// two `CPHC` shares is acknowledged and one share fills, so a partly filled bracket — and with it
/// an unprotected interval — is on the table before the random steps begin. Without it a random
/// script reaches a protected position in about one run in forty.
const PROTECTED_LEAD: [Step; 4] = [
    Step::Fill,
    Step::Intent {
        which: 1,
        exiting: false,
        other: true,
        protected: true,
    },
    Step::Acknowledge,
    Step::Fill,
];

fn scripted() -> impl Strategy<Value = Vec<Step>> {
    (any::<bool>(), prop::collection::vec(step(), 1..14)).prop_map(|(lead, random)| {
        let mut script = PREFIX.to_vec();
        if lead {
            script.extend(PROTECTED_LEAD);
        }
        script.extend(random);
        script
    })
}

/// Every script with the protected lead, for the properties about the protection it produces: a
/// protected position is then an input, so its protection is asserted rather than assumed, and an
/// executor that places none fails them instead of skipping every case.
fn scripted_protected() -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec(step(), 1..14).prop_map(|random| {
        let mut script = PREFIX.to_vec();
        script.extend(PROTECTED_LEAD);
        script.extend(random);
        script
    })
}

/// Every script with the protected lead followed by the `Fill` that completes its entry: the second
/// of the bracket's two `CPHC` shares. §5.4 holds the bracket legs until the entry is completely
/// filled, so after this fill they are active and the position is protected before the random
/// steps begin, which is what lets the protected quantity be asserted rather than assumed.
fn scripted_protected_complete() -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec(step(), 1..14).prop_map(|random| {
        let mut script = PREFIX.to_vec();
        script.extend(PROTECTED_LEAD);
        script.push(Step::Fill);
        script.extend(random);
        script
    })
}

/// The steps that make the executor doubt a submission and then resubmit it: a fresh plain
/// opening on `AAPL` whose submission times out, so it is `Unknown`, then three absences the broker
/// reports over more than the fifteen-second window, each after a wait, so §5.7's
/// `Unknown → Intent` edge fires and the gate re-runs. Without them a random script reaches a
/// resubmission too rarely for the properties about one to judge any.
const DOUBTED_LEAD: [Step; 8] = [
    Step::Intent {
        which: 3,
        exiting: false,
        other: false,
        protected: false,
    },
    Step::Timeout,
    Step::Absent,
    Step::Wait,
    Step::Absent,
    Step::Wait,
    Step::Absent,
    Step::Wait,
];

/// Every script with the doubted lead after the prefix, for the properties about a resubmission.
fn scripted_doubted() -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec(step(), 1..14).prop_map(|random| {
        let mut script = PREFIX.to_vec();
        script.extend(DOUBTED_LEAD);
        script.extend(random);
        script
    })
}

/// Defect E5b's shape (#668 round 2; backlog E5b), after the prefix: a completed CPHC bracket, a
/// risk exit whose protection's cancel is confirmed so it is submitted, the protection's cancel
/// delivered again, and a second risk exit. The ladder steps the first exit before the broker has
/// acknowledged it, and the second goes beside that cancel. The pinned seed's random steps never
/// reach it, so it leads every script of the property it pins, which fails on it until E5b's fix.
const STEP_BESIDE_LEAD: [Step; 9] = [
    Step::Fill,
    Step::Intent {
        which: 1,
        exiting: false,
        other: true,
        protected: true,
    },
    Step::Acknowledge,
    Step::Fill,
    Step::Fill,
    Step::Intent {
        which: 3,
        exiting: true,
        other: true,
        protected: false,
    },
    Step::Cancelled,
    Step::Cancelled,
    Step::Intent {
        which: 2,
        exiting: true,
        other: true,
        protected: false,
    },
];

fn scripted_beside_a_step() -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec(step(), 1..14).prop_map(|random| {
        let mut script = PREFIX.to_vec();
        script.extend(STEP_BESIDE_LEAD);
        script.extend(random);
        script
    })
}

/// Defect E4b's shape (#668 round 2; backlog E4b), after the protected lead: a second CPHC
/// bracket, a risk exit, a fill, a cancel confirmed and the waits that leave an OCO's
/// acknowledgment awaited while a later interval starts, which ends the first open interval rather
/// than the awaited one. The property it pins draws half its scripts from it and half from the
/// wide protected search (#668 round 3, minor a), so it fails at every seed until E4b's fix and
/// still searches widely after it.
const AWAITED_LEAD: [Step; 8] = [
    Step::Intent {
        which: 3,
        exiting: false,
        other: true,
        protected: true,
    },
    Step::Intent {
        which: 2,
        exiting: true,
        other: true,
        protected: false,
    },
    Step::Fill,
    Step::Intent {
        which: 0,
        exiting: false,
        other: false,
        protected: false,
    },
    Step::Cancelled,
    Step::Wait,
    Step::Timeout,
    Step::Acknowledge,
];

fn scripted_awaited() -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec(step(), 1..14).prop_map(|random| {
        let mut script = PREFIX.to_vec();
        script.extend(PROTECTED_LEAD);
        script.extend(AWAITED_LEAD);
        script.extend(random);
        script
    })
}

/// After the protected lead (bracket 01 partly filled at 34): two waits take the clock to 102,
/// where 01's partial-fill timeout asks its cancel and its interval is alerted; bracket 02 partly
/// fills at 110; 01's cancel is confirmed at 114, so the OCO for its share is awaited; bracket 03
/// partly fills at 122, and its start must end 01's awaited interval, not 02's, the latest open one
/// (#771 review). The script runs to 172, past 02's bound (170) but short of 03's (182), so an
/// executor that ended 02's interval instead raises no alert inside it and is caught.
const AWAITED_FIRST_LEAD: [Step; 12] = [
    Step::Wait,
    Step::Wait,
    Step::Intent {
        which: 2,
        exiting: false,
        other: true,
        protected: true,
    },
    Step::Fill,
    Step::Cancelled,
    Step::Intent {
        which: 3,
        exiting: false,
        other: true,
        protected: true,
    },
    Step::Fill,
    Step::Wait,
    Step::Intent {
        which: 0,
        exiting: false,
        other: false,
        protected: false,
    },
    Step::Intent {
        which: 0,
        exiting: false,
        other: false,
        protected: false,
    },
    Step::Intent {
        which: 0,
        exiting: false,
        other: false,
        protected: false,
    },
    Step::Intent {
        which: 0,
        exiting: false,
        other: false,
        protected: false,
    },
];

fn scripted_awaited_first() -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec(step(), 1..14).prop_map(|random| {
        let mut script = PREFIX.to_vec();
        script.extend(PROTECTED_LEAD);
        script.extend(AWAITED_FIRST_LEAD);
        script.extend(random);
        script
    })
}

/// The steps that age an opening past `max_intent_age_s` (120 seconds) before anything lets it go:
/// a restart, so the startup reconciliation holds every opening (`startup_reconciliation_pending`),
/// a fresh plain opening on `AAPL`, four thirty-four-second waits, and the snapshot that ends the
/// hold. The opening must then be abandoned, never submitted.
const STALE_LEAD: [Step; 7] = [
    Step::Restart,
    Step::Intent {
        which: 3,
        exiting: false,
        other: false,
        protected: false,
    },
    Step::Wait,
    Step::Wait,
    Step::Wait,
    Step::Wait,
    Step::Snapshot,
];

/// Every script with the stale lead after the prefix, for the property about an intent's age.
fn scripted_stale() -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec(step(), 1..14).prop_map(|random| {
        let mut script = PREFIX.to_vec();
        script.extend(STALE_LEAD);
        script.extend(random);
        script
    })
}

/// Every script with the protected lead completed and then a risk exit of one `CPHC` share whose
/// protection's cancel the broker confirms, so the exit is submitted while protection is
/// cancelled (§5.4's marketable exit sequence) before the random steps begin.
fn scripted_exiting() -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec(step(), 1..14).prop_map(|random| {
        let mut script = PREFIX.to_vec();
        script.extend(PROTECTED_LEAD);
        script.push(Step::Fill);
        script.push(Step::Intent {
            which: 2,
            exiting: true,
            other: true,
            protected: false,
        });
        script.push(Step::Cancelled);
        script.extend(random);
        script
    })
}

/// What one scripted run produced: every effect, in order, the shell it ended in, and the
/// broker's own picture of what it holds.
struct Run {
    shell: Shell,
    drafts: Vec<EventDraft>,
    effects: Vec<Effect>,
    /// The client order id the prefix's intent was submitted under.
    first_id: String,
    /// The account stream's head once the prefix had run, which is the head of a snapshot taken
    /// then and published later.
    prefix_head: Seq,
    broker: BrokerModel,
    /// The effects of each kill-switch command's own batch, in order, so a property can see what
    /// the switch did before it journaled anything.
    kill_batches: Vec<Vec<Effect>>,
    /// For each kill-switch command, in order, whether the broker was then holding a working order
    /// it had acknowledged: one the switch must cancel (§5.5), read from the broker model rather
    /// than the executor.
    kill_working: Vec<bool>,
}

/// The broker's side of a run, kept by the script rather than read from the executor: every order
/// it was sent, whether it acknowledged, filled, or cancelled it, every fill it reported, and the
/// positions those fills make. A `Snapshot` step shows the executor exactly this.
#[derive(Debug, Default, Clone)]
struct BrokerModel {
    orders: BTreeMap<String, BrokerSide>,
    order_of_arrival: Vec<String>,
    fills: Vec<mandate_executor::BrokerFill>,
    positions: BTreeMap<String, i128>,
    cancels_asked: Vec<String>,
    /// The cash its fills moved, in nine-place units, from the 20000 it reported at the start.
    cash_moved: i128,
}

#[derive(Debug, Clone)]
struct BrokerSide {
    instrument: String,
    side: mandate_accounting::Side,
    qty_units: i128,
    filled_units: i128,
    acknowledged: bool,
    cancelled: bool,
    /// The purpose the submission carried, so a protective order is told from the agent's own.
    purpose: Purpose,
}

impl BrokerModel {
    fn sent(&mut self, ran: &Ran) {
        for order in ran.submissions() {
            let id = order.client_order_id.as_str().to_owned();
            if self.orders.contains_key(&id) {
                continue;
            }
            let qty_units = order
                .oco
                .as_ref()
                .map_or(order.qty, |legs| legs.qty)
                .to_string();
            self.orders.insert(
                id.clone(),
                BrokerSide {
                    instrument: order.instrument.as_str().to_owned(),
                    side: order.side,
                    qty_units: units(&qty_units).unwrap_or(0),
                    filled_units: 0,
                    acknowledged: false,
                    cancelled: false,
                    purpose: order.purpose,
                },
            );
            self.order_of_arrival.push(id);
        }
        for request in &ran.requests {
            if let BrokerRequest::Cancel { client_order_id } = request {
                self.cancels_asked.push(client_order_id.as_str().to_owned());
            }
        }
    }

    /// Whether an agent kill switch must now cancel something at this broker: a working,
    /// acknowledged order of the agent's own whose cancel has not already been asked for. A
    /// protective order is not one: an automated switch leaves protection in place until the
    /// session (§5.5), so it may cancel none (#244 round 3, major 1).
    fn must_cancel_at_a_switch(&self) -> bool {
        self.order_of_arrival.iter().any(|id| {
            self.working(id)
                && self
                    .orders
                    .get(id)
                    .is_some_and(|o| o.acknowledged && o.purpose != Purpose::Protective)
                && !self.cancels_asked.contains(id)
        })
    }

    fn working(&self, id: &str) -> bool {
        self.orders
            .get(id)
            .is_some_and(|o| !o.cancelled && o.filled_units < o.qty_units)
    }

    fn broker_order(&self, id: &str, status: &str) -> Option<mandate_executor::BrokerOrder> {
        let order = self.orders.get(id)?;
        Some(common::broker_order(
            &format!("b-{id}"),
            Some(id),
            &order.instrument,
            order.side,
            &decimal(order.qty_units),
            &decimal(order.filled_units),
            status,
        ))
    }

    fn snapshot(&self, head: Seq) -> mandate_executor::BrokerSnapshot {
        let mut taken = snapshot(head.0, ReconcileReason::Scheduled);
        taken.open_orders = self
            .order_of_arrival
            .iter()
            .filter(|id| self.working(id))
            .filter(|id| self.orders.get(*id).is_some_and(|o| o.acknowledged))
            .filter_map(|id| {
                let partly = self.orders.get(id).is_some_and(|o| o.filled_units > 0);
                self.broker_order(
                    id,
                    if partly {
                        "partially_filled"
                    } else {
                        "accepted"
                    },
                )
            })
            .collect();
        taken.fills = self.fills.clone();
        taken.account.cash = common::usd(&decimal(
            20_000_000_000_000_i128.saturating_add(self.cash_moved),
        ));
        taken.positions = self
            .positions
            .iter()
            .filter(|(_, held)| **held != 0)
            .map(|(name, held)| common::broker_position(name, &decimal(*held)))
            .collect();
        taken
    }
}

/// Integer units at nine places back to canonical decimal text.
fn decimal(value: i128) -> String {
    let negative = value < 0;
    let magnitude = value.unsigned_abs();
    let whole = magnitude / 1_000_000_000;
    let fraction = magnitude % 1_000_000_000;
    let mut text = if fraction == 0 {
        whole.to_string()
    } else {
        let digits = format!("{fraction:09}");
        format!("{whole}.{}", digits.trim_end_matches('0'))
    };
    if negative {
        text.insert(0, '-');
    }
    text
}

/// Plays a script. The risk clock only ever advances — by four seconds a step, and thirty more
/// on a `Wait` — so no run this generator produces is one the journal would reject.
fn play(script: &[Step]) -> Run {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, CPHC]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);

    let mut shell = Shell::new(1);
    let mut drafts = Vec::new();
    let mut effects = Vec::new();
    let mut at: i64 = 10;
    let mut broker = BrokerModel::default();
    let mut fills: u32 = 0;
    let mut prefix_head = Seq(0);
    let mut kill_batches: Vec<Vec<Effect>> = Vec::new();
    let mut kill_working: Vec<bool> = Vec::new();

    let record = |ran: Ran,
                  drafts: &mut Vec<EventDraft>,
                  effects: &mut Vec<Effect>,
                  broker: &mut BrokerModel| {
        drafts.extend(ran.drafts.iter().cloned());
        effects.extend(ran.effects.iter().cloned());
        broker.sent(&ran);
        ran
    };

    shell.fold_one(&stream_opened()).unwrap_or_else(|e| {
        panic!("the stream must open: {e}");
    });
    let (restarted, started_effects) = shell.restart(&ports);
    record(started_effects, &mut drafts, &mut effects, &mut broker);
    shell = restarted;
    let mut startup = broker.snapshot(shell.head());
    startup.reason = ReconcileReason::Startup;
    record(
        shell.run(Input::BrokerSnapshot(startup), &ports),
        &mut drafts,
        &mut effects,
        &mut broker,
    );
    record(
        shell.run(
            Input::BrokerUpdate(BrokerUpdate::Account(common::broker_account())),
            &ports,
        ),
        &mut drafts,
        &mut effects,
        &mut broker,
    );

    for (index, one) in script.iter().enumerate() {
        at = at.saturating_add(if *one == Step::Wait { 34 } else { 4 });
        record(
            shell.run(Input::Tick(clock(at)), &ports),
            &mut drafts,
            &mut effects,
            &mut broker,
        );
        match one {
            Step::Intent {
                which,
                exiting,
                other,
                protected,
            } => {
                let name = if *other { CPHC } else { AAPL };
                record(
                    shell.run(Input::Market(quote(name, BID, ASK, at)), &ports),
                    &mut drafts,
                    &mut effects,
                    &mut broker,
                );
                let body = match (*exiting, *protected) {
                    (true, _) => risk_exit(name, "1", "150"),
                    (false, true) => {
                        common::protected_opening(name, "2", "150", "140", Some("170"))
                    }
                    (false, false) => opening(name, "1", "150"),
                };
                record(
                    shell.run(handoff(&intent_named(*which), common::AGENT, body), &ports),
                    &mut drafts,
                    &mut effects,
                    &mut broker,
                );
            }
            Step::Acknowledge => {
                let waiting = broker
                    .order_of_arrival
                    .iter()
                    .rev()
                    .find(|id| {
                        broker.working(id)
                            && broker.orders.get(*id).is_some_and(|o| !o.acknowledged)
                    })
                    .cloned();
                if let Some(id) = waiting {
                    if let Some(order) = broker.orders.get_mut(&id) {
                        order.acknowledged = true;
                    }
                    if let Some(described) = broker.broker_order(&id, "accepted") {
                        record(
                            shell.run(
                                Input::Broker(Ok(BrokerOutcome::Submitted(described))),
                                &ports,
                            ),
                            &mut drafts,
                            &mut effects,
                            &mut broker,
                        );
                    }
                }
            }
            Step::Timeout => {
                record(
                    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports),
                    &mut drafts,
                    &mut effects,
                    &mut broker,
                );
            }
            Step::Absent => {
                if let Some(id) = broker.order_of_arrival.last().cloned() {
                    record(
                        shell.run(
                            Input::Broker(Ok(BrokerOutcome::Absent {
                                client_order_id: id,
                            })),
                            &ports,
                        ),
                        &mut drafts,
                        &mut effects,
                        &mut broker,
                    );
                }
            }
            Step::Fill => {
                let target = broker
                    .order_of_arrival
                    .iter()
                    .rev()
                    .find(|id| broker.working(id))
                    .cloned();
                if let Some(id) = target {
                    fills = fills.saturating_add(1);
                    let Some(order) = broker.orders.get_mut(&id) else {
                        continue;
                    };
                    order.acknowledged = true;
                    order.filled_units = order.filled_units.saturating_add(1_000_000_000);
                    let signed = if order.side == mandate_accounting::Side::Buy {
                        1_000_000_000
                    } else {
                        -1_000_000_000
                    };
                    let position = broker
                        .positions
                        .entry(order.instrument.clone())
                        .or_insert(0);
                    *position = position.saturating_add(signed);
                    broker.cash_moved =
                        broker.cash_moved.saturating_sub(signed.saturating_mul(150));
                    let fill = mandate_executor::BrokerFill {
                        instrument: common::instrument(&order.instrument),
                        side: order.side,
                        ..common::broker_fill(&format!("f-{fills}"), Some(&id), "1", "150")
                    };
                    broker.fills.push(fill.clone());
                    record(
                        shell.run(Input::BrokerUpdate(BrokerUpdate::Fill(fill)), &ports),
                        &mut drafts,
                        &mut effects,
                        &mut broker,
                    );
                }
            }
            Step::Cancelled => {
                let target = broker
                    .cancels_asked
                    .iter()
                    .rev()
                    .find(|id| broker.working(id) || !broker.orders.contains_key(*id))
                    .cloned();
                if let Some(id) = target {
                    if let Some(order) = broker.orders.get_mut(&id) {
                        order.cancelled = true;
                    }
                    record(
                        shell.run(
                            Input::Broker(Ok(BrokerOutcome::CancelAccepted {
                                client_order_id: id,
                            })),
                            &ports,
                        ),
                        &mut drafts,
                        &mut effects,
                        &mut broker,
                    );
                }
            }
            Step::Snapshot => {
                let taken = broker.snapshot(shell.head());
                record(
                    shell.run(Input::BrokerSnapshot(taken), &ports),
                    &mut drafts,
                    &mut effects,
                    &mut broker,
                );
            }
            Step::KillSwitch => {
                kill_working.push(broker.must_cancel_at_a_switch());
                let ran = record(
                    shell.run(
                        Input::Command(Command::KillSwitch {
                            scope: KillScope::Agent(common::agent(common::AGENT)),
                            initiator: Initiator::RiskLimit,
                            confirmation: None,
                        }),
                        &ports,
                    ),
                    &mut drafts,
                    &mut effects,
                    &mut broker,
                );
                kill_batches.push(ran.effects);
            }
            Step::Restart => {
                let (next, ran) = shell.restart_keeping_broker(&ports);
                shell = next;
                record(ran, &mut drafts, &mut effects, &mut broker);
            }
            Step::Wait => {}
            Step::External => {
                let mut taken = broker.snapshot(shell.head());
                taken.open_orders.push(common::broker_order(
                    &format!("b-foreign-{index}"),
                    Some("placed-in-the-broker-app"),
                    AAPL,
                    mandate_accounting::Side::Buy,
                    "1",
                    "0",
                    "new",
                ));
                record(
                    shell.run(Input::BrokerSnapshot(taken), &ports),
                    &mut drafts,
                    &mut effects,
                    &mut broker,
                );
            }
        }
        if index.saturating_add(1) == PREFIX.len() {
            prefix_head = shell.head();
        }
    }

    let first_id = ClientOrderId::for_intent(&IntentId(EventId(intent_named(0))))
        .map(|id| id.as_str().to_owned())
        .unwrap_or_default();
    assert_eq!(
        shell.connector.accepted_for(&first_id),
        1,
        "the prefix's plain opening on a clean account must reach the broker exactly once, or \
         nothing below is tested: {:?}",
        shell.connector.requests
    );
    assert!(
        drafts.iter().any(|d| d.event_type == "OrderSubmitted"
            && field(d, "client_order_id") == Some(first_id.as_str())),
        "and the journal names it"
    );
    Run {
        shell,
        drafts,
        effects,
        first_id,
        prefix_head,
        broker,
        kill_batches,
        kill_working,
    }
}

/// Whether anything rule 13 lets hold a protective order was in force before draft `at`, in the
/// protected lead's instrument: a kill switch anywhere in the script (its sells and cancels come
/// first), the agent `paused` or `stopped`, or an `Unknown` order in `CPHC` (DEC-129 item 22). A
/// correct executor may then decline to place the lead's legs, so a property must not demand them.
fn may_hold_protection(script: &[Step], drafts: &[EventDraft], at: usize) -> bool {
    let mut in_cphc: BTreeSet<&str> = BTreeSet::new();
    script.contains(&Step::KillSwitch)
        || drafts.iter().take(at).any(|d| {
            if d.event_type == "OrderSubmitted"
                && (field(d, "instrument_id") == Some(CPHC) || field(d, "instrument") == Some(CPHC))
                && let Some(id) = field(d, "client_order_id")
            {
                in_cphc.insert(id);
            }
            d.event_type == "KillSwitchActivated"
                || (d.event_type == "AgentModeApplied"
                    && matches!(field(d, "to"), Some("paused" | "stopped")))
                || (d.event_type == "OrderStateChanged"
                    && field(d, "state") == Some("unknown")
                    && field(d, "client_order_id").is_some_and(|id| in_cphc.contains(id)))
        })
}

/// Whether a script runs the protected lead: its entry is then a partly filled GTC bracket.
fn leads(script: &[Step]) -> bool {
    script.get(PREFIX.len()..PREFIX.len().saturating_add(PROTECTED_LEAD.len()))
        == Some(&PROTECTED_LEAD[..])
}

/// The exit intents a script hands over that no `GateDecided` names: an intent id whose first
/// handoff is an exit, which the executor must gate (allow, hold, or deny) rather than drop.
fn ungated_exits(script: &[Step], drafts: &[EventDraft]) -> Vec<String> {
    let mut first: BTreeMap<u8, bool> = BTreeMap::new();
    for one in script {
        if let Step::Intent { which, exiting, .. } = one {
            first.entry(*which).or_insert(*exiting);
        }
    }
    first
        .into_iter()
        .filter(|(_, exiting)| *exiting)
        .map(|(which, _)| intent_named(which))
        .filter(|intent| {
            !drafts.iter().any(|d| {
                d.event_type == "GateDecided" && field(d, "intent_id") == Some(intent.as_str())
            })
        })
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig {
        max_global_rejects: 16_384,
        ..ProptestConfig::default()
    })]

    /// Journal §5.2, `AGENTS.md` rule 5, DEC-07: nothing reaches the broker that the journal did
    /// not name first, in the same effect list.
    ///
    /// Both sides are compared as sets of **client order ids**: the broker's are the ids it
    /// accepted a submission for, and the journal's are the ids its `OrderSubmitted` drafts name.
    /// A §5.7 resubmission after a confirmed absence legitimately repeats an id and its body at the
    /// next attempt, so ids — not `(id, attempt)` pairs — are what the two sides share; the
    /// duplicate guarantee is the separate `accepted_for(id) <= 1`.
    #[test]
    fn every_submit_effect_follows_the_order_submitted_draft_that_names_it(script in scripted()) {
        let run = play(&script);
        let accepted: BTreeSet<String> = run
            .shell
            .connector
            .accepted
            .iter()
            .filter(|(_, count)| **count > 0)
            .map(|(id, _)| id.clone())
            .collect();
        let journaled: BTreeSet<String> = run
            .drafts
            .iter()
            .filter(|d| d.event_type == "OrderSubmitted")
            .filter_map(|d| field(d, "client_order_id").map(str::to_owned))
            .collect();
        prop_assert!(accepted.contains(&run.first_id));
        prop_assert_eq!(
            &accepted,
            &journaled,
            "the broker accepted {:?} and the journal names {:?}",
            accepted,
            journaled
        );
        for (id, count) in &run.shell.connector.accepted {
            prop_assert!(*count <= 1, "{} was accepted {} times", id, count);
        }
        let mut named: BTreeSet<String> = BTreeSet::new();
        for effect in &run.effects {
            match effect {
                Effect::Journal(draft) if draft.event_type == "OrderSubmitted" => {
                    if let Some(id) = field(draft, "client_order_id") {
                        named.insert(id.to_owned());
                    }
                }
                Effect::Broker(BrokerRequest::Submit(order)) => {
                    prop_assert!(
                        named.contains(order.client_order_id.as_str()),
                        "{} was sent before any draft named it (planted bug 2)",
                        order.client_order_id.as_str()
                    );
                }
                _ => {}
            }
        }
    }

    /// E7-2: the id is a function of the intent id alone, so two attempts for one intent carry
    /// one id and two intents never share one. The intent rides the submission's
    /// `OrderRequestRecorded` companion (journal spec §9.5, rule 45), and the doubted lead makes
    /// every script resubmit one intent at a second attempt, which is where an id that depends on
    /// the attempt would show. An exit's ladder rung is a new order under its own id, `-l{n}` for
    /// rung n ≥ 1, still carrying the exit's intent (trading-domain spec §2.3, §5.6, DEC-160
    /// (10)), so the id is judged per intent and rung, the rung read from the same companion
    /// (DEC-521 item 1).
    #[test]
    fn a_client_order_id_is_a_function_of_the_intent_id_alone(script in scripted_doubted()) {
        let run = play(&script);
        let mut by_intent: BTreeMap<(String, u64), BTreeSet<String>> = BTreeMap::new();
        let mut attempts: BTreeMap<String, usize> = BTreeMap::new();
        for submission in submissions_with_requests(&run.drafts) {
            let (Some(intent), Some(id)) = (
                submission.get("intent_id").and_then(Value::as_str),
                submission.get("client_order_id").and_then(Value::as_str),
            ) else {
                continue;
            };
            let rung = submission.get("rung").and_then(Value::as_int).unwrap_or(0);
            by_intent
                .entry((intent.to_owned(), rung))
                .or_default()
                .insert(id.to_owned());
            let count = attempts.entry(intent.to_owned()).or_insert(0);
            *count = count.saturating_add(1);
        }
        prop_assert!(
            attempts.get(&intent_named(3)).is_some_and(|count| *count >= 2),
            "the doubted lead's opening, confirmed absent, is submitted a second time (§5.7), so \
             there is a resubmission for this property to judge: {:?}",
            attempts
        );
        for ((intent, rung), ids) in &by_intent {
            prop_assert_eq!(
                ids.len(),
                1,
                "intent {} at rung {} produced {:?}, so the id depends on more than the intent \
                 and its rung (planted bug 7)",
                intent,
                rung,
                ids
            );
        }
    }

    /// E7-2: no two intents ever share a client order id, across restarts and across agents. The
    /// intent a submission carries rides its `OrderRequestRecorded` companion (§9.5, rule 45), so
    /// the loop reads the merged view, the way the fold does.
    #[test]
    fn distinct_intents_never_share_a_client_order_id(script in scripted()) {
        let run = play(&script);
        let book = ShadowBook::of(&run.drafts);
        let mut owner: BTreeMap<String, String> = BTreeMap::new();
        let mut companion: Option<Value> = None;
        let mut seen = 0usize;
        for draft in &run.drafts {
            if draft.event_type == "OrderRequestRecorded" {
                companion = Some(draft.payload.clone());
                continue;
            }
            if draft.event_type != "OrderSubmitted" {
                continue;
            }
            let merged = match &companion {
                Some(merge) => {
                    let mut merged = draft.payload.clone();
                    if let (Value::Object(merged), Value::Object(companion)) =
                        (&mut merged, merge)
                    {
                        for (key, value) in companion {
                            merged.entry(key.clone()).or_insert_with(|| value.clone());
                        }
                    }
                    merged
                }
                None => draft.payload.clone(),
            };
            let field_of = |name: &str| merged.get(name).and_then(Value::as_str);
            let Some(id) = field_of("client_order_id") else {
                continue;
            };
            let Some(intent) = field_of("intent_id") else {
                prop_assert!(
                    book.protective.contains(id),
                    "{} was submitted with no intent and is not a protective order the journal \
                     placed",
                    id
                );
                seen = seen.saturating_add(1);
                continue;
            };
            seen = seen.saturating_add(1);
            if let Some(previous) = owner.insert(id.to_owned(), intent.to_owned()) {
                prop_assert_eq!(
                    &previous,
                    intent,
                    "{} names two intents (planted bug 8)",
                    id
                );
            }
        }
        prop_assert_eq!(seen, book.submissions.len());
    }

    /// E7-3's acceptance clause, read off the broker-side counter rather than the journal: a
    /// crash at any point of any step, and a restart with the broker carried across, leaves every
    /// client order id accepted at most once — and the prefix's order still exactly once.
    #[test]
    fn no_crash_point_makes_the_broker_see_two_orders_for_one_intent(
        script in scripted(),
        point in prop::sample::select(&CrashPoint::ALL[..]),
    ) {
        let mut run = play(&script);
        let ids = TestIds;
        let mandates = FixedMandate::covering(&[AAPL, CPHC]);
        let instruments = FixedInstruments;
        let configuration = config();
        let ports = ports(&ids, &mandates, &instruments, &configuration);
        let head = run.shell.head();
        let crashed = run
            .shell
            .step_crashing(Input::BrokerSnapshot(run.broker.snapshot(head)), &ports, point);
        prop_assert!(crashed.is_ok(), "the reconciliation step refused: {:?}", crashed.err());
        let (mut after, started) = run.shell.restart_keeping_broker(&ports);
        prop_assert!(
            started.submissions().is_empty(),
            "Started never resubmits; it queries (journal §5.2, planted bug 1)"
        );
        let mut startup = run.broker.snapshot(after.head());
        startup.reason = ReconcileReason::Startup;
        after.run(Input::BrokerSnapshot(startup), &ports);
        for (id, count) in &after.connector.accepted {
            prop_assert!(
                *count <= 1,
                "the broker accepted {} distinct submissions for {} after a crash at {:?}",
                count,
                id,
                point
            );
        }
        prop_assert_eq!(after.connector.accepted_for(&run.first_id), 1);
    }

    /// Journal §5.2, §5.7: recovery queries, and never resubmits an order it has doubted without
    /// a confirmed absence. An absence is not a state: it is journaled as `OrderStateChanged` to
    /// `unknown` with `lookup: "absent"`, so the oracle counts those, per order, since the order
    /// last went `unknown` (DEC-133's ruling on absences), and measures their span on the
    /// `risk_clock` timestamps. The doubted lead puts a resubmission in every script.
    #[test]
    fn no_recovery_submits_without_a_confirmed_absence(script in scripted_doubted()) {
        let run = play(&script);
        let mut absences: BTreeMap<String, (u32, Option<i64>)> = BTreeMap::new();
        let mut doubted: BTreeSet<String> = BTreeSet::new();
        let mut resubmissions = 0usize;
        let window = config().unknown_absent_window_s;
        let needed = config().unknown_absent_lookups;
        for draft in &run.drafts {
            let Some(id) = field(draft, "client_order_id") else {
                continue;
            };
            let at = risk_seconds(draft);
            match draft.event_type.as_str() {
                "OrderStateChanged" if field(draft, "state") == Some("unknown") => {
                    if field(draft, "lookup") == Some("absent") {
                        let entry = absences.entry(id.to_owned()).or_insert((0, at));
                        entry.0 = entry.0.saturating_add(1);
                    } else {
                        doubted.insert(id.to_owned());
                        absences.remove(id);
                    }
                }
                "OrderSubmitted" if doubted.contains(id) => {
                    resubmissions = resubmissions.saturating_add(1);
                    let (count, first) = absences.get(id).copied().unwrap_or((0, None));
                    prop_assert!(
                        count >= needed,
                        "{} was resubmitted after {} absences, below the configured {}",
                        id,
                        count,
                        needed
                    );
                    let spanned = match (first, at) {
                        (Some(first), Some(now)) => now.saturating_sub(first),
                        _ => 0,
                    };
                    prop_assert!(
                        spanned >= window,
                        "{} was resubmitted after absences spanning {}s, inside the {}s window",
                        id,
                        spanned,
                        window
                    );
                    doubted.remove(id);
                    absences.remove(id);
                }
                _ => {}
            }
        }
        prop_assert!(
            resubmissions > 0,
            "the doubted lead's opening is resubmitted after its confirmed absence, so there is a \
             resubmission for this property to judge"
        );
        prop_assert!(
            resubmissions <= ShadowBook::of(&run.drafts).submissions.len(),
            "the count cannot exceed the submissions the journal carries"
        );
    }

    /// §5.7: filled quantity is non-decreasing, at most the order quantity, and equals the sum of
    /// unique fills.
    #[test]
    fn filled_quantity_equals_the_sum_of_unique_fills(script in scripted()) {
        let run = play(&script);
        let book = ShadowBook::of(&run.drafts);
        let live = ShadowBook::of_state(&run.shell.state);
        prop_assert!(book.orders.contains_key(&run.first_id));
        for (id, shadow) in &book.orders {
            let Some(actual) = live.orders.get(id) else {
                return Err(TestCaseError::fail(format!("{id} is missing from the state")));
            };
            prop_assert_eq!(
                shadow.filled_units,
                actual.filled_units,
                "{}: unique fills sum to {} and the state says {} (planted bug 16)",
                id,
                shadow.filled_units,
                actual.filled_units
            );
            if shadow.qty_units > 0 {
                prop_assert!(
                    actual.filled_units <= shadow.qty_units,
                    "{} filled {} of {}",
                    id,
                    actual.filled_units,
                    shadow.qty_units
                );
            }
        }
        let broker_filled: i128 = run
            .broker
            .orders
            .values()
            .map(|order| order.filled_units)
            .sum();
        let applied: i128 = book.orders.values().map(|order| order.filled_units).sum();
        prop_assert!(
            applied <= broker_filled,
            "the journal applied {} units of fills and the broker reported {} for our orders",
            applied,
            broker_filled
        );
    }

    /// §5.7: terminal states are final. A state name the oracle does not know is skipped, never
    /// read as a terminal one (DEC-133's ruling on absences: `unknown` with `lookup: "absent"` is a
    /// lookup result, not a state change).
    #[test]
    fn no_terminal_order_leaves_its_terminal_state(script in scripted()) {
        let run = play(&script);
        let mut terminal: BTreeMap<String, String> = BTreeMap::new();
        let mut transitions = 0usize;
        for draft in &run.drafts {
            let state = match draft.event_type.as_str() {
                "OrderStateChanged" => match field(draft, "state").and_then(shadow_state) {
                    Some(state) => state,
                    None => continue,
                },
                "OrderAbandoned" => "abandoned",
                _ => continue,
            };
            let Some(id) = field(draft, "client_order_id") else {
                continue;
            };
            transitions = transitions.saturating_add(1);
            if let Some(was) = terminal.get(id) {
                prop_assert_eq!(
                    was.as_str(),
                    state,
                    "{} left its terminal state {} for {}",
                    id,
                    was,
                    state
                );
            }
            if TERMINAL.contains(&state) {
                terminal.insert(id.to_owned(), state.to_owned());
            }
        }
        prop_assert!(
            transitions > 0,
            "the prefix's acknowledged order is at least one transition on the journal"
        );
    }

    /// §5.7 and interpretation 26: a reservation outlives everything but a terminal state.
    #[test]
    fn a_reservation_is_never_released_before_a_terminal_state(script in scripted()) {
        let run = play(&script);
        let book = ShadowBook::of(&run.drafts);
        let live = ShadowBook::of_state(&run.shell.state);
        prop_assert!(book.orders.contains_key(&run.first_id));
        for (id, shadow) in &book.orders {
            let Some(actual) = live.orders.get(id) else {
                return Err(TestCaseError::fail(format!("{id} is missing from the state")));
            };
            prop_assert_eq!(
                shadow.reserved,
                actual.reserved,
                "{}: the journal implies reserved={} in state {} and the state says {} \
                 (planted bugs 11 and 20)",
                id,
                shadow.reserved,
                shadow.state,
                actual.reserved
            );
        }
    }

    /// §5.7 and interpretation 26: every one of the six terminal states releases the reservation.
    #[test]
    fn every_terminal_state_releases_its_reservation(script in scripted()) {
        let run = play(&script);
        let book = ShadowBook::of(&run.drafts);
        let terminal: Vec<_> = book
            .orders
            .iter()
            .filter(|(_, order)| TERMINAL.contains(&order.state.as_str()))
            .collect();
        prop_assume!(!terminal.is_empty());
        for (id, order) in terminal {
            prop_assert!(
                !run.shell
                    .state
                    .reservations()
                    .keys()
                    .any(|held| held.as_str() == id),
                "{} is {} and still holds its reservation",
                id,
                order.state
            );
        }
    }

    /// ES-21, journal §8: replaying the drafts a run journaled reproduces its state.
    #[test]
    fn folding_the_journaled_drafts_reproduces_the_live_state(script in scripted()) {
        let run = play(&script);
        prop_assume!(!run.drafts.is_empty());
        let replayed = run
            .shell
            .replay()
            .map_err(|e| TestCaseError::fail(format!("the replay refused: {e}")))?;
        let book = ShadowBook::of(&run.drafts);
        book.agrees_with(&ShadowBook::of_state(&replayed))
            .map_err(TestCaseError::fail)?;
        book.agrees_with(&ShadowBook::of_state(&run.shell.state))
            .map_err(TestCaseError::fail)?;
        let mut ledger = ShadowLedger::of(&run.drafts);
        ledger.positions.retain(|_, held| *held != 0);
        prop_assert_eq!(
            ledger.positions,
            ShadowLedger::of_state(&replayed).positions,
            "the i128 ledger and the fold disagree about positions"
        );
    }

    /// ES-21, journal §8: a replay emits nothing.
    #[test]
    fn a_replay_emits_no_draft_and_no_broker_effect(script in scripted()) {
        let run = play(&script);
        prop_assume!(!run.shell.account_journal.is_empty());
        let before = run.shell.connector.total_accepted();
        let replayed = run
            .shell
            .replay()
            .map_err(|e| TestCaseError::fail(format!("the replay refused: {e}")))?;
        prop_assert_eq!(
            run.shell.connector.total_accepted(),
            before,
            "a replay cannot reach the broker; that is half of crash safety"
        );
        prop_assert!(
            replayed.unresolved().is_none(),
            "and it leaves no batch in doubt"
        );
    }

    /// ES-21: two runs of the same inputs give equal effect lists.
    #[test]
    fn two_runs_of_the_same_inputs_give_equal_effects(script in scripted()) {
        let first = play(&script);
        let second = play(&script);
        prop_assert_eq!(
            first.effects.len(),
            second.effects.len(),
            "two runs produced {} and {} effects",
            first.effects.len(),
            second.effects.len()
        );
        prop_assume!(!first.effects.is_empty());
        prop_assert_eq!(
            first.effects,
            second.effects,
            "determinism is asserted, not assumed (ES-21)"
        );
    }

    /// `AGENTS.md` rule 13, made unrepresentable: no agent-scoped effect names the account-wide
    /// endpoints. The script only ever fires an agent-scoped switch.
    #[test]
    fn no_agent_scoped_effect_can_name_the_account_wide_endpoints(script in scripted()) {
        let run = play(&script);
        prop_assume!(script.contains(&Step::KillSwitch));
        let switches = run
            .drafts
            .iter()
            .filter(|d| d.event_type == "KillSwitchActivated")
            .count();
        prop_assert!(switches > 0, "the script fired a kill switch and it was journaled");
        for effect in &run.effects {
            if let Effect::Broker(request) = effect {
                prop_assert!(
                    !common::is_account_wide(request),
                    "an agent-scoped run reached {:?} (planted bug 12)",
                    request
                );
            }
        }
    }

    /// §5.5: the final mode is journaled before any cancel and any sell of the same switch.
    #[test]
    fn the_mode_draft_precedes_every_cancel_and_every_sell(script in scripted()) {
        let run = play(&script);
        prop_assume!(script.contains(&Step::KillSwitch));
        prop_assert!(!run.kill_batches.is_empty());
        prop_assert!(
            run.kill_batches.first().is_some_and(|batch| batch
                .iter()
                .any(|e| matches!(e, Effect::Journal(d) if d.event_type == "AgentModeApplied"))),
            "the first kill switch journals the agent's final mode (§5.5): a switch that does \
             nothing orders nothing before it"
        );
        for batch in &run.kill_batches {
            let mode_at = batch
                .iter()
                .position(|e| matches!(e, Effect::Journal(d) if d.event_type == "AgentModeApplied"));
            for (index, effect) in batch.iter().enumerate() {
                let acts = match effect {
                    Effect::Broker(BrokerRequest::Cancel { .. }) => true,
                    Effect::Broker(BrokerRequest::Submit(order)) => {
                        order.side == mandate_accounting::Side::Sell
                    }
                    Effect::Journal(d) => d.event_type == "KillSwitchActivated",
                    _ => false,
                };
                if acts {
                    prop_assert!(
                        mode_at.is_some_and(|at| at < index),
                        "{:?} at {} in the kill switch's batch came before the final mode {:?} \
                         (planted bug 13)",
                        effect,
                        index,
                        mode_at
                    );
                }
            }
        }
    }

    /// §5.4: Σ protective sell quantity never exceeds the position, in every script, whether or
    /// not it reaches a protected position.
    #[test]
    #[ignore = "pending E7-2"]
    fn protective_sell_quantity_never_exceeds_the_position_in_any_script(script in scripted()) {
        let run = play(&script);
        let entry = ClientOrderId::for_intent(&IntentId(EventId(intent_named(1))))
            .map(|id| id.as_str().to_owned())
            .unwrap_or_default();
        let completed = run.drafts.iter().position(|d| {
            d.event_type == "OrderStateChanged"
                && field(d, "client_order_id") == Some(entry.as_str())
                && field(d, "state") == Some("filled")
        });
        prop_assert!(
            !leads(&script)
                || completed.is_none_or(|at| may_hold_protection(&script, &run.drafts, at)
                    || run.drafts.iter().skip(at).any(|d| {
                    d.event_type == "ProtectionChanged"
                        && field(d, "action") == Some("placed")
                        && protected_instrument(&d.payload) == Some(CPHC)
                })),
            "the protected lead's entry filled completely with the agent in no mode that may hold \
             protection (no kill switch, not paused or stopped: AGENTS.md rule 13, §5.5), so its \
             bracket legs are placed (§5.4), and there is protection for this property to judge"
        );
        let accountant = ProtectionAccountant::of(&run.drafts);
        let ledger = ShadowLedger::of(&run.drafts);
        for (name, covered) in &accountant.covered {
            let held = ledger.positions.get(name).copied().unwrap_or(0);
            prop_assert!(
                *covered <= held.max(0),
                "{} is protected for {} against a position of {}",
                name,
                covered,
                held
            );
        }
    }

    /// §5.4: a completely filled bracket entry activates its legs, and Σ protective sell quantity
    /// never exceeds the position.
    #[test]
    fn protective_sell_quantity_never_exceeds_the_position(
        script in scripted_protected_complete()
    ) {
        let run = play(&script);
        let accountant = ProtectionAccountant::of(&run.drafts);
        let ledger = ShadowLedger::of(&run.drafts);
        prop_assert!(
            !accountant.covered.is_empty(),
            "the protected lead's entry completed, so its bracket legs are active and protect it \
             (§5.4: legs are held until the entry is completely filled)"
        );
        for (name, covered) in &accountant.covered {
            let held = ledger.positions.get(name).copied().unwrap_or(0);
            prop_assert!(
                *covered <= held.max(0),
                "{} is protected for {} against a position of {}",
                name,
                covered,
                held
            );
        }
    }

    /// §5.4, E7-4: every unprotected interval is journaled from start to end.
    #[test]
    #[ignore = "pending E7-4"]
    fn every_unprotected_interval_has_a_journaled_start_and_end(script in scripted_protected()) {
        let run = play(&script);
        let accountant = ProtectionAccountant::of(&run.drafts);
        prop_assert!(
            !accountant.intervals.is_empty(),
            "the protected lead's partly filled bracket journals an unprotected interval (§5.4)"
        );
        let protected: Vec<_> = accountant
            .covered
            .iter()
            .filter(|(_, covered)| **covered > 0)
            .collect();
        let _ = protected;
        prop_assert_eq!(
            accountant.open(),
            0,
            "{} intervals were opened and never closed (planted bug 4)",
            accountant.open()
        );
    }

    /// §5.4, E7-4: no interval exceeds `max_unprotected_s` without an alert. An interval is
    /// measured on the drafts' `risk_clock` timestamps, to its end or, still open, to the run's
    /// last clock, and an alert counts for it when the draft the notification names is in the
    /// interval's instrument and inside the interval.
    #[test]
    fn no_interval_exceeds_the_limit_without_an_alert(
        script in prop_oneof![scripted_awaited(), scripted_awaited_first(), scripted_protected()],
    ) {
        let run = play(&script);
        let accountant = ProtectionAccountant::of(&run.drafts);
        prop_assert!(
            !accountant.intervals.is_empty(),
            "the protected lead's partly filled bracket journals an unprotected interval (§5.4)"
        );
        let subjects: BTreeMap<&str, (&str, i64)> = run
            .drafts
            .iter()
            .filter_map(|d| {
                let name = field(d, "instrument").or(field(d, "instrument_id"))?;
                Some((d.event_id.0.as_str(), (name, risk_seconds(d)?)))
            })
            .collect();
        let alerts: Vec<(&str, i64)> = run
            .effects
            .iter()
            .filter_map(|e| match e {
                Effect::Notify(note) => subjects.get(note.subject_event.0.as_str()).copied(),
                _ => None,
            })
            .collect();
        let bound = config().max_unprotected_s;
        for interval in &accountant.intervals {
            let end = interval.ended_at.unwrap_or(accountant.last_clock);
            let lasted = end.saturating_sub(interval.started_at);
            if lasted <= bound {
                continue;
            }
            prop_assert!(
                alerts.iter().any(|(name, at)| *name == interval.instrument
                    && *at >= interval.started_at
                    && *at <= end),
                "an interval in {} of {}s from {} exceeded the {}s bound with no alert (planted \
                 bug 14)",
                interval.instrument,
                lasted,
                interval.started_at,
                bound
            );
        }
    }

    /// §5.4: nothing is submitted in an instrument while a cancel in it is unconfirmed. A cancel is
    /// resolved by the order's own terminal state on the journal, or, for a protective order, by
    /// the `ProtectionChanged` that records it cancelled — never by the request being accepted.
    /// Two rulings bound the wait. Rule 5's wait ends when the cancel is journaled overdue
    /// (`cancel_overdue`, DEC-160 (7), (13), (18)), and an exit never waits twice on the same
    /// submission of an opening: once that opening's wait went overdue, even before its cancel
    /// could be asked (an unacknowledged opening is queried, never cancelled blind), a cancel asked
    /// later for the same submission holds no sell; a resubmission starts the wait afresh (DEC-532
    /// item 3). And an entry that turns terminal partly filled, "after that cancel or by any other
    /// path", gets its OCO for the filled quantity
    /// (DEC-346 item 6), so a protective submission does not wait on any buy's cancel, plain or
    /// bracket (DEC-521 item 2). A plain buy that fills only adds to the position. A bracket
    /// entry's legs are held until it is completely filled and are sized to its quantity (§5.4),
    /// so the fill that activates them adds as much to the position as they can sell, and the OCO
    /// is at most its own entry's fill. That bound needs no live protective order or exit to cover
    /// the filled shares of a bracket entry whose legs are held; a re-placement sized on the
    /// position breaks it today, because held legs are counted by no cap (backlog E1), and so does
    /// E5's overdue cancel. Every other sell still waits on every outstanding cancel: the sells are
    /// what §5.4's sequences order after a confirmation; an opening may be accepted while an exit
    /// waits, and the exit then asks its cancel too (DEC-160 (13)), so a buy is not judged here.
    #[test]
    #[ignore = "pending E7-4"]
    fn no_order_is_submitted_while_an_unconfirmed_cancel_is_outstanding(
        script in scripted_beside_a_step(),
    ) {
        let run = play(&script);
        if let (Some(batch), Some(true)) = (run.kill_batches.first(), run.kill_working.first()) {
            prop_assert!(
                batch
                    .iter()
                    .any(|e| matches!(e, Effect::Broker(BrokerRequest::Cancel { .. }))),
                "the first kill switch, with the agent's order working at the broker and no cancel \
                 yet asked for it, cancels it (§5.5), so cancels are on the table for this property \
                 to judge"
            );
        }
        let mut instrument_of: BTreeMap<String, String> = BTreeMap::new();
        let mut outstanding: BTreeMap<String, String> = BTreeMap::new();
        let mut buys: BTreeSet<String> = BTreeSet::new();
        let mut overdue_submissions: BTreeSet<String> = BTreeSet::new();
        for effect in &run.effects {
            if let Effect::Broker(BrokerRequest::Submit(order)) = effect
                && order.side == mandate_accounting::Side::Buy
            {
                buys.insert(order.client_order_id.as_str().to_owned());
            }
            match effect {
                Effect::Journal(draft) if draft.event_type == "OrderSubmitted" => {
                    let name = field(draft, "instrument_id").or(field(draft, "instrument"));
                    if let (Some(id), Some(name)) = (field(draft, "client_order_id"), name) {
                        instrument_of.insert(id.to_owned(), name.to_owned());
                        overdue_submissions.remove(id);
                    }
                }
                Effect::Journal(draft)
                    if draft.event_type == "ProtectionChanged"
                        && field(draft, "action") == Some("placed") =>
                {
                    let name = protected_instrument(&draft.payload).unwrap_or_default();
                    for id in named_orders(&draft.payload, "orders") {
                        instrument_of.insert(id, name.to_owned());
                    }
                }
                Effect::Broker(BrokerRequest::Cancel { client_order_id }) => {
                    let id = client_order_id.as_str().to_owned();
                    if !(buys.contains(&id) && overdue_submissions.contains(&id)) {
                        let name = instrument_of.get(&id).cloned().unwrap_or_default();
                        outstanding.insert(id, name);
                    }
                }
                Effect::Journal(draft) if draft.event_type == "OrderStateChanged" => {
                    let overdue = draft.payload.get("cancel_overdue") == Some(&Value::Bool(true));
                    if overdue && let Some(id) = field(draft, "client_order_id") {
                        overdue_submissions.insert(id.to_owned());
                    }
                    if (overdue
                        || field(draft, "state")
                            .and_then(shadow_state)
                            .is_some_and(|state| TERMINAL.contains(&state)))
                        && let Some(id) = field(draft, "client_order_id")
                    {
                        outstanding.remove(id);
                    }
                }
                Effect::Journal(draft) if draft.event_type == "OrderAbandoned" => {
                    if let Some(id) = field(draft, "client_order_id") {
                        outstanding.remove(id);
                    }
                }
                Effect::Journal(draft)
                    if draft.event_type == "ProtectionChanged"
                        && field(draft, "action") == Some("cancelled") =>
                {
                    for id in named_orders(&draft.payload, "orders") {
                        outstanding.remove(&id);
                    }
                }
                Effect::Broker(BrokerRequest::Submit(order))
                    if order.side == mandate_accounting::Side::Sell =>
                {
                    let protective = order.purpose == Purpose::Protective;
                    let blocking: Vec<&String> = outstanding
                        .iter()
                        .filter(|(_, name)| name.as_str() == order.instrument.as_str())
                        .filter(|(id, _)| !(protective && buys.contains(*id)))
                        .map(|(id, _)| id)
                        .collect();
                    prop_assert!(
                        blocking.is_empty(),
                        "{} was submitted with {:?} still unconfirmed in {} (planted bug 5)",
                        order.client_order_id.as_str(),
                        blocking,
                        order.instrument.as_str()
                    );
                }
                _ => {}
            }
        }
    }

    /// §5.4: "orders submitted while protection is canceled must be marketable at submission"
    /// (§5.6: the exit price ladder prices them). Protection is cancelled from an interval that
    /// an exit sequence or a re-placement opens (an `unprotected_start` that names no `bracket`;
    /// a partly filled bracket's interval has its legs held, not cancelled) until protection is
    /// placed again or the interval ends. Inside, every order but the protection itself is
    /// marketable against the script's one quote: a sell at or below the bid, a buy at or above
    /// the ask, or a market order. The exiting lead submits its exit inside such an interval in
    /// every script, so one is always judged.
    #[test]
    #[ignore = "pending E7-4"]
    fn no_resting_order_is_submitted_inside_an_unprotected_interval(script in scripted_exiting()) {
        let run = play(&script);
        let (Some(bid), Some(ask)) = (units(BID), units(ASK)) else {
            return Err(TestCaseError::fail("the script's quote parses"));
        };
        let mut cancelled: BTreeSet<String> = BTreeSet::new();
        let mut judged = 0usize;
        for effect in &run.effects {
            match effect {
                Effect::Journal(draft) if draft.event_type == "ProtectionChanged" => {
                    let (Some(name), Some(action)) =
                        (protected_instrument(&draft.payload), field(draft, "action"))
                    else {
                        continue;
                    };
                    match action {
                        "unprotected_start" if field(draft, "bracket").is_none() => {
                            cancelled.insert(name.to_owned());
                        }
                        "placed" | "unprotected_end" => {
                            cancelled.remove(name);
                        }
                        _ => {}
                    }
                }
                Effect::Broker(BrokerRequest::Submit(order))
                    if order.purpose != Purpose::Protective
                        && cancelled.contains(order.instrument.as_str()) =>
                {
                    judged = judged.saturating_add(1);
                    let limit = order.limit_price.map(|price| units(&price.to_string()));
                    let marketable = match (order.side, limit) {
                        (_, None) => true,
                        (mandate_accounting::Side::Sell, Some(Some(limit))) => limit <= bid,
                        (mandate_accounting::Side::Buy, Some(Some(limit))) => limit >= ask,
                        (_, Some(None)) => false,
                    };
                    prop_assert!(
                        marketable,
                        "{} ({:?} {:?} at {:?}) rests while protection in {} is cancelled \
                         (§5.4, §5.6)",
                        order.client_order_id.as_str(),
                        order.purpose,
                        order.side,
                        order.limit_price,
                        order.instrument.as_str()
                    );
                }
                _ => {}
            }
        }
        prop_assert!(
            judged > 0,
            "the exiting lead's exit is submitted once its protection's cancel is confirmed, inside \
             the interval, so there is an order for this property to judge"
        );
    }

    /// `AGENTS.md` rule 13: no risk-reducing submission is ever denied by a pacing control.
    #[test]
    fn no_risk_reducing_submission_is_ever_denied_by_a_pacing_control(script in scripted()) {
        let run = play(&script);
        let pacing = [
            "conduct",
            "eligibility",
            "day_trade_budget",
            "buying_power",
            "session",
        ];
        let mut denials = 0usize;
        for draft in &run.drafts {
            if draft.event_type != "GateDecided" || field(draft, "verdict") != Some("deny") {
                continue;
            }
            denials = denials.saturating_add(1);
            let purpose = field(draft, "purpose").unwrap_or("");
            if purpose == "risk_exit" || purpose == "protective" || purpose == "flatten" {
                let reason = field(draft, "reason_code").unwrap_or("");
                prop_assert!(
                    !pacing.iter().any(|control| reason.contains(control)),
                    "a {} was denied for {}",
                    purpose,
                    reason
                );
            }
        }
        prop_assert!(
            denials <= run.drafts.len(),
            "the denial count comes from the same draft list"
        );
        let ungated = ungated_exits(&script, &run.drafts);
        prop_assert!(
            ungated.is_empty(),
            "every exit the script hands over is gated, so there are exit verdicts for this \
             property to judge: {:?}",
            ungated
        );
    }

    /// `AGENTS.md` rule 13: the only holds on an exit are the four the rule names.
    #[test]
    fn the_only_holds_on_an_exit_are_the_four_the_rule_names(script in scripted()) {
        let run = play(&script);
        let allowed = ["agent_paused", "agent_stopped", "unknown_order_in_flight", "broker"];
        let mut holds = 0usize;
        for draft in &run.drafts {
            if draft.event_type != "GateDecided" {
                continue;
            }
            let purpose = field(draft, "purpose").unwrap_or("");
            if purpose != "risk_exit" && purpose != "owner_exit" {
                continue;
            }
            if field(draft, "verdict") == Some("hold") {
                holds = holds.saturating_add(1);
                let reason = field(draft, "reason_code").unwrap_or("");
                prop_assert!(
                    allowed.contains(&reason),
                    "an exit was held for {}, which is not one of the four",
                    reason
                );
            }
        }
        prop_assert!(holds <= run.drafts.len());
        let ungated = ungated_exits(&script, &run.drafts);
        prop_assert!(
            ungated.is_empty(),
            "every exit the script hands over is gated, so there are exit verdicts for this \
             property to judge: {:?}",
            ungated
        );
    }

    /// §5.7: the status map is total and never silently ignores.
    #[test]
    fn the_status_map_is_total_and_never_silently_ignores(
        status in prop::sample::select(vec![
            "new", "accepted", "pending_new", "accepted_for_bidding", "held", "partially_filled",
            "filled", "done_for_day", "stopped", "calculated", "pending_cancel", "canceled",
            "expired", "rejected", "suspended", "pending_replace", "replaced", "teleported",
            "", "NEW",
        ]),
    ) {
        let ids = TestIds;
        let mandates = FixedMandate::covering(&[AAPL]);
        let instruments = FixedInstruments;
        let configuration = config();
        let ports = ports(&ids, &mandates, &instruments, &configuration);
        let mut shell = Shell::new(1);
        shell
            .fold_one(&stream_opened())
            .map_err(|e| TestCaseError::fail(format!("the stream must open: {e}")))?;
        let mut shell = shell.restart_ready(&ports);
        let sent = shell.run(
            handoff("01JABCDEFGHJKMNPQRSTVWXYZ0", common::AGENT, opening(AAPL, "1", "150")),
            &ports,
        );
        let id = sent
            .submissions()
            .first()
            .map(|o| o.client_order_id.as_str().to_owned())
            .ok_or_else(|| TestCaseError::fail("the attempt is sent"))?;
        let ran = shell.run(
            Input::BrokerUpdate(BrokerUpdate::Order(common::broker_order(
                "b-1",
                Some(&id),
                AAPL,
                mandate_accounting::Side::Buy,
                "1",
                "0",
                status,
            ))),
            &ports,
        );
        prop_assert!(
            !ran.effects.is_empty(),
            "`{}` produced no effect at all, which is the silent no-op §5.7's last row forbids \
             (planted bug 15)",
            status
        );
    }

    /// §11: every order-set difference is adopted with a compensating event. The broker here holds
    /// no open order at all, so every order the journal shows working — computed from the drafts,
    /// not from the crate — differs, and each must be adopted with exactly one
    /// `CompensatingEvent`.
    #[test]
    fn every_order_difference_adopts_the_broker_with_a_compensating_event(script in scripted()) {
        let run = play(&script);
        let book = ShadowBook::of(&run.drafts);
        let working: BTreeSet<&String> = book
            .orders
            .iter()
            .filter(|(_, order)| {
                !TERMINAL.contains(&order.state.as_str())
                    && order.state != "unknown"
                    && order.state != "intent"
            })
            .map(|(id, _)| id)
            .collect();
        prop_assume!(!working.is_empty());
        let ids = TestIds;
        let mandates = FixedMandate::covering(&[AAPL, CPHC]);
        let instruments = FixedInstruments;
        let configuration = config();
        let ports = ports(&ids, &mandates, &instruments, &configuration);
        let mut taken = run.broker.snapshot(run.shell.head());
        taken.open_orders = Vec::new();
        let reconciliation = mandate_executor::reconcile(&run.shell.state, &taken, &ports)
            .map_err(|e| TestCaseError::fail(format!("the reconciliation refused: {e}")))?;
        let compensating: Vec<&EventDraft> = reconciliation
            .effects
            .iter()
            .filter_map(|e| match e {
                Effect::Journal(d) if d.event_type == "CompensatingEvent" => Some(d),
                _ => None,
            })
            .collect();
        for id in &working {
            let named = compensating
                .iter()
                .filter(|d| format!("{:?}", d.payload).contains(id.as_str()))
                .count();
            prop_assert_eq!(
                named,
                1,
                "{} is working on the journal and absent at the broker: the difference is adopted \
                 with exactly one compensating event naming it (planted bug 3)",
                id
            );
        }
        let adopted = reconciliation
            .differences
            .iter()
            .filter(|d| d.kind == mandate_executor::DifferenceKind::OrderState)
            .count();
        prop_assert_eq!(adopted, compensating.len());
    }

    /// §11: nothing outside the order set is ever adopted. The broker is the script's own truth,
    /// except that it holds one more `AAPL` share than the shadow ledger — a quantity no script can
    /// reach (a ledger position plus one) — so there is always a position difference. Read off the
    /// effects, never off the crate's own `adopted` flag: every `CompensatingEvent` must name one
    /// of our orders, and there is exactly one per order difference, so one written for the
    /// position — which would make the ledger agree with the broker and destroy the evidence —
    /// is caught (interpretation 13, planted bug 18).
    #[test]
    fn no_position_cash_or_fee_difference_is_ever_adopted(script in scripted()) {
        let run = play(&script);
        let ids = TestIds;
        let mandates = FixedMandate::covering(&[AAPL, CPHC]);
        let instruments = FixedInstruments;
        let configuration = config();
        let ports = ports(&ids, &mandates, &instruments, &configuration);
        let ledger = ShadowLedger::of(&run.drafts);
        let held = ledger.positions.get(AAPL).copied().unwrap_or(0);
        let mut taken = run.broker.snapshot(run.shell.head());
        taken.positions.retain(|position| position.instrument.as_str() != AAPL);
        taken.positions.push(common::broker_position(
            AAPL,
            &decimal(held.saturating_add(1_000_000_000)),
        ));
        let reconciliation = mandate_executor::reconcile(&run.shell.state, &taken, &ports)
            .map_err(|e| TestCaseError::fail(format!("the reconciliation refused: {e}")))?;
        let book = ShadowBook::of(&run.drafts);
        let ours: BTreeSet<&String> = book.orders.keys().chain(book.protective.iter()).collect();
        let compensating: Vec<&EventDraft> = reconciliation
            .effects
            .iter()
            .filter_map(|e| match e {
                Effect::Journal(d) if d.event_type == "CompensatingEvent" => Some(d),
                _ => None,
            })
            .collect();
        for draft in &compensating {
            let rendered = format!("{:?}", draft.payload);
            prop_assert!(
                ours.iter().any(|id| rendered.contains(id.as_str())),
                "a compensating event that names none of our orders adopts something outside the \
                 order set (planted bug 18): {}",
                rendered
            );
        }
        let order_differences = reconciliation
            .differences
            .iter()
            .filter(|d| d.kind == mandate_executor::DifferenceKind::OrderState)
            .count();
        prop_assert_eq!(
            compensating.len(),
            order_differences,
            "one compensating event per order difference and none for anything else"
        );
        prop_assert!(
            reconciliation.effects.iter().any(|e| matches!(
                e,
                Effect::Journal(d) if d.event_type == "BrokerPositionObserved"
            )),
            "the position is compared and recorded"
        );
        prop_assert_eq!(
            reconciliation.verdict,
            ReconciliationVerdict::Mismatch,
            "and the extra share is a mismatch, not an adoption"
        );
    }

    /// E7-3: a reconciliation leaves nothing unexplained and unpaused. The broker holds one more
    /// `AAPL` share than the shadow ledger, a quantity no script reaches, so there is always a
    /// position difference to pause on (planted bug 9).
    #[test]
    fn a_reconciliation_leaves_nothing_unexplained_and_unpaused(script in scripted()) {
        let run = play(&script);
        let ids = TestIds;
        let mandates = FixedMandate::covering(&[AAPL, CPHC]);
        let instruments = FixedInstruments;
        let configuration = config();
        let ports = ports(&ids, &mandates, &instruments, &configuration);
        let held = ShadowLedger::of(&run.drafts)
            .positions
            .get(AAPL)
            .copied()
            .unwrap_or(0);
        let mut taken = run.broker.snapshot(run.shell.head());
        taken.positions.retain(|position| position.instrument.as_str() != AAPL);
        taken.positions.push(common::broker_position(
            AAPL,
            &decimal(held.saturating_add(1_000_000_000)),
        ));
        let reconciliation = mandate_executor::reconcile(&run.shell.state, &taken, &ports)
            .map_err(|e| TestCaseError::fail(format!("the reconciliation refused: {e}")))?;
        prop_assert_eq!(
            reconciliation.verdict,
            ReconciliationVerdict::Mismatch,
            "a position the ledger does not have is a mismatch"
        );
        let paused = reconciliation
            .effects
            .iter()
            .filter(|e| matches!(
                e,
                Effect::Journal(d) if d.event_type == "AgentModeApplied"
                    && field(d, "to") == Some("paused")
            ))
            .count();
        prop_assert!(paused > 0, "and pauses the agents holding it (planted bug 9)");
        prop_assert!(
            reconciliation
                .effects
                .iter()
                .any(|e| matches!(e, Effect::Notify(_))),
            "and alerts the owner"
        );
    }

    /// §11, interpretation 12: the step order is part of the algorithm. The broker reports a fill
    /// the journal lacks and the position that fill explains, so the fill must be ingested before
    /// the position is compared (planted bug 10), and the run closes the batch.
    #[test]
    fn the_reconciliation_order_is_orders_then_fills_then_positions_then_cash_then_fees(
        script in scripted(),
    ) {
        let run = play(&script);
        let ids = TestIds;
        let mandates = FixedMandate::covering(&[AAPL, CPHC]);
        let instruments = FixedInstruments;
        let configuration = config();
        let ports = ports(&ids, &mandates, &instruments, &configuration);
        let mut taken = run.broker.snapshot(run.shell.head());
        let missing = common::broker_fill("f-missing", Some(&run.first_id), "1", "150");
        let already = run
            .broker
            .fills
            .iter()
            .any(|fill| fill.client_order_id.as_deref() == Some(run.first_id.as_str()));
        prop_assume!(!already);
        taken.fills.push(missing);
        taken.open_orders.retain(|open| open.client_order_id.as_deref() != Some(run.first_id.as_str()));
        let held = ShadowLedger::of(&run.drafts)
            .positions
            .get(AAPL)
            .copied()
            .unwrap_or(0);
        taken.positions.retain(|position| position.instrument.as_str() != AAPL);
        taken.positions.push(common::broker_position(
            AAPL,
            &decimal(held.saturating_add(1_000_000_000)),
        ));
        let reconciliation = mandate_executor::reconcile(&run.shell.state, &taken, &ports)
            .map_err(|e| TestCaseError::fail(format!("the reconciliation refused: {e}")))?;
        let position_of = |wanted: &str| {
            reconciliation.effects.iter().position(|e| match e {
                Effect::Journal(d) => d.event_type == wanted,
                _ => false,
            })
        };
        let fill = position_of("FillApplied")
            .or_else(|| position_of("LateFillApplied"))
            .ok_or_else(|| TestCaseError::fail("the missing fill is ingested"))?;
        let observed = position_of("BrokerPositionObserved")
            .ok_or_else(|| TestCaseError::fail("the positions are compared"))?;
        let finished = position_of("ReconciliationRun")
            .ok_or_else(|| TestCaseError::fail("the run is appended"))?;
        prop_assert!(
            fill < observed && observed < finished,
            "orders, then fills, then positions, then the run: the fill at {}, the position at \
             {}, the run at {} (planted bug 10)",
            fill,
            observed,
            finished
        );
        if let Some(adopted) = position_of("OrderStateChanged") {
            prop_assert!(adopted < fill, "order adoptions come first");
        }
        prop_assert!(
            reconciliation
                .differences
                .iter()
                .all(|d| d.kind != mandate_executor::DifferenceKind::Position),
            "the ingested fill explains the extra share, so nothing is a position mismatch: {:?}",
            reconciliation.differences
        );
    }

    /// §11: cash is compared against the last broker cash snapshot, within 0.01 × fills since it
    /// plus accrued unposted fees. With no fill and no fee since the broker last reported 20000,
    /// the band is zero: the same 20000 is no difference and pauses nobody, and any cent more is a
    /// difference that alerts.
    #[test]
    fn cash_within_the_band_never_pauses_and_outside_it_always_alerts(
        cents in 0u64..500,
    ) {
        let ids = TestIds;
        let mandates = FixedMandate::covering(&[AAPL]);
        let instruments = FixedInstruments;
        let configuration = config();
        let ports = ports(&ids, &mandates, &instruments, &configuration);
        let mut shell = Shell::new(1);
        shell
            .fold_one(&stream_opened())
            .map_err(|e| TestCaseError::fail(format!("the stream must open: {e}")))?;
        let (mut shell, _) = shell.restart(&ports);
        shell.run(
            Input::BrokerUpdate(BrokerUpdate::Account(common::broker_account())),
            &ports,
        );
        let cash = decimal(i128::from(cents).saturating_mul(10_000_000).saturating_add(20_000_000_000_000));
        let mut taken = snapshot(shell.head().0, ReconcileReason::Scheduled);
        taken.account = mandate_executor::BrokerAccount {
            cash: common::usd(&cash),
            ..common::broker_account()
        };
        let reconciliation = mandate_executor::reconcile(&shell.state, &taken, &ports)
            .map_err(|e| TestCaseError::fail(format!("the reconciliation refused: {e}")))?;
        let alerts = reconciliation
            .effects
            .iter()
            .filter(|e| matches!(e, Effect::Notify(_)))
            .count();
        let cash_difference = reconciliation
            .differences
            .iter()
            .any(|d| d.kind == mandate_executor::DifferenceKind::Cash);
        if cents == 0 {
            prop_assert!(!cash_difference, "the same 20000 is no difference");
            prop_assert!(
                !reconciliation.effects.iter().any(|e| matches!(
                    e,
                    Effect::Journal(d) if d.event_type == "AgentModeApplied"
                )),
                "and inside the band nobody is paused"
            );
        } else {
            prop_assert!(
                cash_difference,
                "{} is {} cents above the 20000 the broker last reported, outside a zero band",
                cash,
                cents
            );
            prop_assert!(alerts > 0, "a cash difference above the band always alerts (§11)");
        }
    }

    /// DEC-131 item 13, interpretation 15: a run is never positioned after a submission it did not
    /// cover. The snapshot is the one taken when the prefix had run and is published only now, so
    /// the run's expected head must be that older head — never the current one, which would let
    /// the append land after submissions the snapshot never saw (planted bug 17).
    #[test]
    fn no_reconciliation_run_is_appended_after_a_submission_it_did_not_cover(
        script in scripted(),
    ) {
        let run = play(&script);
        let ids = TestIds;
        let mandates = FixedMandate::covering(&[AAPL, CPHC]);
        let instruments = FixedInstruments;
        let configuration = config();
        let ports = ports(&ids, &mandates, &instruments, &configuration);
        let head = run.shell.head();
        prop_assume!(run.prefix_head.0 < head.0);
        let taken = snapshot(run.prefix_head.0, ReconcileReason::Scheduled);
        let reconciliation = mandate_executor::reconcile(&run.shell.state, &taken, &ports)
            .map_err(|e| TestCaseError::fail(format!("the reconciliation refused: {e}")))?;
        prop_assert_eq!(
            reconciliation.expected_head,
            run.prefix_head,
            "the run is appended at the head its snapshot was taken at, {:?}, not the current \
             {:?} (planted bug 17)",
            run.prefix_head,
            head
        );
        prop_assert!(
            reconciliation.effects.iter().any(|e| matches!(
                e,
                Effect::Journal(d) if d.event_type == "ReconciliationRun"
            )),
            "and the run is drafted"
        );
    }

    /// §11 and interpretation 14: only an owner acknowledgment clears a mismatch pause.
    #[test]
    fn no_input_but_an_acknowledged_owner_ack_clears_a_mismatch_pause(
        later in prop::collection::vec(0i64..500, 1..6),
    ) {
        let ids = TestIds;
        let mandates = FixedMandate::covering(&[AAPL]);
        let instruments = FixedInstruments;
        let configuration = config();
        let ports = ports(&ids, &mandates, &instruments, &configuration);
        let mut shell = Shell::new(1);
        shell
            .fold_one(&stream_opened())
            .map_err(|e| TestCaseError::fail(format!("the stream must open: {e}")))?;
        let (mut shell, _) = shell.restart(&ports);
        let mut bad = snapshot(shell.head().0, ReconcileReason::Startup);
        bad.positions = vec![common::broker_position(AAPL, "7")];
        shell.run(Input::BrokerSnapshot(bad), &ports);
        prop_assert!(
            !shell.state.mismatched().is_empty(),
            "the mismatch is recorded before anything else is tried"
        );
        let mut at = 1_000_i64;
        for step in later {
            at = at.saturating_add(step).saturating_add(1);
            shell.run(Input::Tick(RiskClock::from_secs(at)), &ports);
            shell.run(
                Input::BrokerSnapshot(snapshot(shell.head().0, ReconcileReason::Scheduled)),
                &ports,
            );
            prop_assert!(
                !shell.state.mismatched().is_empty(),
                "a tick or an agreeing reconciliation cleared the mismatch at {}",
                at
            );
        }
    }

    /// Journal §2: every risk input this crate appends carries a non-decreasing `risk_clock`, a
    /// §4.7 timestamp on a whole second ("never integer seconds"), read as one. A script whose run
    /// appends no risk input (no fill, so no `FillApplied` or `FeesCharged`) has none to judge.
    #[test]
    fn every_risk_input_draft_carries_a_non_decreasing_risk_clock(script in scripted()) {
        let run = play(&script);
        let risk_inputs = [
            "MarkUpdated",
            "FillApplied",
            "LateFillApplied",
            "FeesCharged",
            "CorporateActionApplied",
            "CashInLieuPosted",
            "CompensatingEvent",
            "MandateVersionApplied",
            "RiskDayStarted",
            "OwnerAcknowledged",
            "UniverseChanged",
        ];
        let mut last = i64::MIN;
        let mut checked = 0usize;
        prop_assume!(run.drafts.iter().any(|d| risk_inputs.contains(&d.event_type.as_str())));
        for draft in &run.drafts {
            if !risk_inputs.contains(&draft.event_type.as_str()) {
                continue;
            }
            checked = checked.saturating_add(1);
            let at = risk_seconds(draft).ok_or_else(|| {
                TestCaseError::fail(format!(
                    "{} carries no risk_clock timestamp (journal spec §4.7)",
                    draft.event_type
                ))
            })?;
            prop_assert!(
                at >= last,
                "{} went back from {} to {}",
                draft.event_type,
                last,
                at
            );
            last = at;
        }
        prop_assert!(checked > 0, "the run appended a risk input for this property to judge");
    }

    /// Journal §2: every copied fact cites its origin.
    #[test]
    fn every_copied_draft_cites_its_origin(script in scripted()) {
        let run = play(&script);
        let copied = [
            "AgentModeApplied",
            "TradingDayStarted",
            "ClockAdvanced",
            "OwnerAcknowledged",
            "UniverseChanged",
        ];
        let mut seen = 0usize;
        for draft in &run.drafts {
            if !copied.contains(&draft.event_type.as_str()) {
                continue;
            }
            if draft.payload.get("originated") == Some(&Value::Bool(true)) {
                continue;
            }
            seen = seen.saturating_add(1);
            prop_assert!(
                draft.causation_id.is_some(),
                "{} was copied without a causation id",
                draft.event_type
            );
        }
        prop_assert!(seen <= run.drafts.len());
    }

    /// DEC-131 item 6: an event id is a function of `(epoch, head, ordinal)` and nothing else.
    #[test]
    fn a_derived_event_id_is_a_function_of_epoch_head_and_ordinal(script in scripted()) {
        let first = play(&script);
        let second = play(&script);
        let ids_of = |run: &Run| -> Vec<String> {
            run.drafts.iter().map(|d| d.event_id.0.clone()).collect()
        };
        let mine = ids_of(&first);
        prop_assume!(!mine.is_empty());
        prop_assert_eq!(
            mine.clone(),
            ids_of(&second),
            "two runs of the same script derived different ids"
        );
        let unique: BTreeSet<_> = mine.iter().collect();
        prop_assert_eq!(
            unique.len(),
            mine.len(),
            "an id was derived twice within one run"
        );
    }

    /// DEC-85: every account-stream catalogue event is interpreted or named.
    #[test]
    fn every_catalogue_event_is_interpreted_or_named(
        event_type in prop::sample::select(vec![
            "StreamOpened", "IntentReceived", "GateDecided", "OrderSubmitted",
            "OrderStateChanged", "OrderAbandoned", "BrokerExchangeRecorded", "FillApplied",
            "LateFillApplied", "FeesCharged", "MarkUpdated", "SettlementPosted", "DividendPaid",
            "CashInLieuPosted", "CorporateActionPrepared", "CorporateActionApplied",
            "ProtectionChanged", "BrokerPositionObserved", "ReconciliationRun",
            "CompensatingEvent", "AccountSnapshotRecorded", "AccountStateObserved",
            "RejectObserved", "AccountRestrictionChanged", "ExternalActivityIngested",
            "RelatedAccountsCoordination", "ConductBreachDetected", "AgentModeApplied",
            "TradingDayStarted", "KillSwitchActivated", "OwnerAcknowledged",
            "MandateVersionApplied", "RiskDayStarted", "RiskLimitTriggered", "RiskLimitLifted",
            "HighWaterMarkReset", "PositionReleased", "InstrumentRestrictionChanged",
            "GoalCompleted", "UniverseChanged", "NobodyEverWroteThis",
        ]),
    ) {
        let mut state = ExecutorState::new(scope());
        let opened = common::event(
            ACCOUNT_STREAM,
            1,
            "StreamOpened",
            common::object(&[
                ("environment", common::text("paper")),
                ("stream_type", common::text("account")),
            ]),
        );
        if event_type != "StreamOpened" {
            mandate_executor::fold(&mut state, &opened)
                .map_err(|e| TestCaseError::fail(format!("the stream must open: {e}")))?;
        }
        let candidate = common::event(
            ACCOUNT_STREAM,
            if event_type == "StreamOpened" { 1 } else { 2 },
            event_type,
            common::with_clock(&[], 10),
        );
        const OTHER_STREAMS: [&str; 9] = [
            "RelatedAccountsCoordination",
            "MandateVersionApplied",
            "RiskLimitTriggered",
            "RiskLimitLifted",
            "HighWaterMarkReset",
            "PositionReleased",
            "InstrumentRestrictionChanged",
            "GoalCompleted",
            "UniverseChanged",
        ];
        /// The catalogue events a merged slice interprets. Every other event this crate owns
        /// answers its story's stub until the slice that implements it moves it here, in the same
        /// change, with live tests pinning what it does (DEC-137, #184 review finding 4).
        const INTERPRETED: [&str; 24] = [
            "StreamOpened",
            "IntentReceived",
            "GateDecided",
            "OrderSubmitted",
            "OrderStateChanged",
            "OrderAbandoned",
            "AgentModeApplied",
            "ClockAdvanced",
            "MarkUpdated",
            "FillApplied",
            "LateFillApplied",
            "FeesCharged",
            "ExternalActivityIngested",
            "AccountRestrictionChanged",
            "AccountStateObserved",
            "RejectObserved",
            "CompensatingEvent",
            "BrokerPositionObserved",
            "ReconciliationRun",
            "AccountSnapshotRecorded",
            "OwnerAcknowledged",
            "ProtectionChanged",
            "ConductBreachDetected",
            "TradingDayStarted",
        ];
        let stubbed = event_type != "NobodyEverWroteThis"
            && !OTHER_STREAMS.contains(&event_type)
            && !INTERPRETED.contains(&event_type);
        match mandate_executor::fold(&mut state, &candidate) {
            Ok(()) => {
                prop_assert!(
                    !stubbed,
                    "{} folded silently before any slice interprets it: it must answer its \
                     story's stub (DEC-137)",
                    event_type
                );
                prop_assert_ne!(
                    event_type,
                    "NobodyEverWroteThis",
                    "a name nobody wrote folded as if it meant something: the silent no-op DEC-85 \
                     forbids"
                );
                prop_assert!(
                    ![
                        "MandateVersionApplied",
                        "UniverseChanged",
                        "RiskLimitTriggered",
                        "RiskLimitLifted",
                        "HighWaterMarkReset",
                        "PositionReleased",
                        "InstrumentRestrictionChanged",
                        "GoalCompleted",
                    ]
                    .contains(&event_type),
                    "{} carries stream F's or stream J's values, which this crate does not \
                     interpret yet (task brief, Decisions needed 8), so it must refuse naming the \
                     owning story rather than fold it",
                    event_type
                );
            }
            Err(error) => {
                if event_type == "NobodyEverWroteThis" {
                    prop_assert_eq!(
                        error.code(),
                        "not_interpreted",
                        "a name nobody wrote is the one case that must be not_interpreted"
                    );
                } else {
                    prop_assert_ne!(
                        error.code(),
                        "foreign_stream",
                        "{} is on the account stream's own catalogue",
                        event_type
                    );
                    if stubbed {
                        prop_assert!(
                            error.code() == "unimplemented" && format!("{error}").contains("E7-"),
                            "{} is not interpreted by any merged slice, so it answers its \
                             story's stub, naming the story (DEC-137): {}",
                            event_type,
                            error
                        );
                    }
                    if error.code() == "not_interpreted" {
                        prop_assert!(
                            format!("{error}").contains('E'),
                            "an uninterpreted catalogue event names the story that owns it \
                             (DEC-85): {}",
                            error
                        );
                    }
                }
            }
        }
    }

    /// `AGENTS.md` rules 6 and 7, journal §6.4: nothing sensitive reaches a draft or an alert.
    #[test]
    fn no_draft_payload_holds_a_credential_or_an_account_number(script in scripted()) {
        let run = play(&script);
        prop_assume!(!run.drafts.is_empty());
        let forbidden = ["APCA-API", "apca-api", "secret", "account_number"];
        for draft in &run.drafts {
            let rendered = format!("{:?}", draft.payload);
            for needle in forbidden {
                prop_assert!(
                    !rendered.contains(needle),
                    "{} carries `{}`",
                    draft.event_type,
                    needle
                );
            }
        }
    }

    /// `AGENTS.md` rule 6, DEC-11: an alert carries an opaque id and a message key only.
    #[test]
    fn no_alert_payload_holds_an_instrument_a_price_or_a_quantity(script in scripted()) {
        let run = play(&script);
        let alerts: Vec<_> = run
            .effects
            .iter()
            .filter_map(|e| match e {
                Effect::Notify(reference) => Some(reference),
                _ => None,
            })
            .collect();
        prop_assume!(!alerts.is_empty());
        for alert in alerts {
            prop_assert!(
                !alert.message_key.contains(AAPL)
                    && !alert.message_key.contains(CPHC)
                    && !alert.message_key.contains("150"),
                "an alert named something it may not: {}",
                alert.message_key
            );
        }
    }

    /// ES-21, journal §8, §5.7: no submission carries an opening older than its maximum age,
    /// measured from the intent's first `IntentReceived` on the `risk_clock` timestamps. Only an
    /// opening ages out: an exit is never too old, it is held and priced at its release (rule 13,
    /// DEC-160 (12), the coordinator's ruling D2). The intent and purpose ride the submission's
    /// `OrderRequestRecorded` companion (journal spec §9.5, rule 45). The stale lead hands over an
    /// opening the startup reconciliation holds until it is past the age, which is where an age
    /// check made only on a resubmission would let it go (planted bug 19).
    #[test]
    fn no_submission_carries_an_intent_older_than_its_maximum_age(script in scripted_stale()) {
        let run = play(&script);
        let mut received: BTreeMap<String, i64> = BTreeMap::new();
        for draft in run.drafts.iter().filter(|d| d.event_type == "IntentReceived") {
            if let (Some(intent), Some(at)) = (field(draft, "intent_id"), risk_seconds(draft)) {
                received.entry(intent.to_owned()).or_insert(at);
            }
        }
        prop_assert!(
            received.contains_key(&intent_named(3)),
            "the stale lead's opening is received, so there is an aged intent to judge"
        );
        let clocks: BTreeMap<&str, i64> = run
            .drafts
            .iter()
            .filter(|d| d.event_type == "OrderSubmitted")
            .filter_map(|d| Some((d.event_id.0.as_str(), risk_seconds(d)?)))
            .collect();
        let max = config().max_intent_age_s;
        let mut openings = 0usize;
        let submitted = run.drafts.iter().filter(|d| d.event_type == "OrderSubmitted");
        for (draft, merged) in submitted.zip(submissions_with_requests(&run.drafts)) {
            let text = |name: &str| merged.get(name).and_then(Value::as_str);
            if !matches!(text("purpose"), Some("open" | "increase")) {
                continue;
            }
            openings = openings.saturating_add(1);
            let Some(intent) = text("intent_id") else {
                return Err(TestCaseError::fail(format!(
                    "{:?} is an opening with no intent on its companion",
                    text("client_order_id")
                )));
            };
            let (Some(born), Some(at)) =
                (received.get(intent), clocks.get(draft.event_id.0.as_str()))
            else {
                return Err(TestCaseError::fail(format!(
                    "{intent} was submitted with no IntentReceived or no risk_clock before it"
                )));
            };
            prop_assert!(
                at.saturating_sub(*born) <= max,
                "{} was submitted {}s after it was received (planted bug 19)",
                intent,
                at.saturating_sub(*born)
            );
        }
        prop_assert!(openings > 0, "the prefix's opening is submitted, so one is judged");
    }
}

/// #244 round 2, blocker 1: the reviewer's shrunk script. `External` leaves the second `CPHC` order
/// `Unknown` before the protected lead's entry completes, which rule 13 lets hold the entry's legs,
/// so `protective_sell_quantity_never_exceeds_the_position_in_any_script` must skip it rather than
/// demand a placement.
#[test]
fn an_unknown_order_in_the_leads_instrument_before_the_completion_is_skipped() {
    let script = [
        Step::Intent {
            which: 0,
            exiting: false,
            other: false,
            protected: false,
        },
        Step::Acknowledge,
        Step::Fill,
        Step::Intent {
            which: 1,
            exiting: false,
            other: true,
            protected: true,
        },
        Step::Acknowledge,
        Step::Fill,
        Step::Intent {
            which: 2,
            exiting: false,
            other: true,
            protected: false,
        },
        Step::External,
        Step::Fill,
        Step::Fill,
    ];
    let run = play(&script);
    let entry = ClientOrderId::for_intent(&IntentId(EventId(intent_named(1))))
        .map(|id| id.as_str().to_owned())
        .unwrap_or_default();
    let completed = run.drafts.iter().position(|d| {
        d.event_type == "OrderStateChanged"
            && field(d, "client_order_id") == Some(entry.as_str())
            && field(d, "state") == Some("filled")
    });
    assert!(leads(&script), "the script runs the protected lead");
    let at = completed.unwrap_or(run.drafts.len());
    assert!(
        may_hold_protection(&script, &run.drafts, at),
        "an Unknown CPHC order before the completion may hold the legs, so the case is skipped"
    );
    assert!(
        !may_hold_protection(&script, &run.drafts, 0),
        "and nothing holds protection before the first draft, so the scope is not a blanket skip"
    );
}

/// #244 round 3, major 1: the reviewer's script `PREFIX + PROTECTED_LEAD + [Wait, Wait, Cancelled,
/// Acknowledge, KillSwitch]`. At the switch the prefix's buy is filled, the lead's entry remainder is
/// cancelled at the bracket timeout, and the only working acknowledged order is the OCO §5.4 then
/// submits for the filled share. `play` cannot reach that state on this code (the OCO's producer is
/// E7-4's), so the broker the script leaves at the switch is built directly: it asks the switch to
/// cancel nothing, while the same broker holding the agent's own working buy does.
#[test]
fn a_resting_protective_order_is_not_one_the_kill_switch_must_cancel() {
    let side = |instrument: &str, side, qty_units, filled_units, cancelled, purpose| BrokerSide {
        instrument: instrument.to_owned(),
        side,
        qty_units,
        filled_units,
        acknowledged: true,
        cancelled,
        purpose,
    };
    let mut broker = BrokerModel::default();
    let unit = 1_000_000_000;
    for (id, order) in [
        (
            "md-prefix",
            side(
                AAPL,
                mandate_accounting::Side::Buy,
                unit,
                unit,
                false,
                Purpose::Open,
            ),
        ),
        (
            "md-entry",
            side(
                CPHC,
                mandate_accounting::Side::Buy,
                2 * unit,
                unit,
                true,
                Purpose::Open,
            ),
        ),
        (
            "md-entry-p1",
            side(
                CPHC,
                mandate_accounting::Side::Sell,
                unit,
                0,
                false,
                Purpose::Protective,
            ),
        ),
    ] {
        broker.orders.insert(id.to_owned(), order);
        broker.order_of_arrival.push(id.to_owned());
    }
    broker.cancels_asked.push("md-entry".to_owned());
    assert!(
        !broker.must_cancel_at_a_switch(),
        "only the OCO works, and an automated switch may leave it in place (§5.5)"
    );
    broker.orders.insert(
        "md-own".to_owned(),
        side(
            CPHC,
            mandate_accounting::Side::Buy,
            unit,
            0,
            false,
            Purpose::Open,
        ),
    );
    broker.order_of_arrival.push("md-own".to_owned());
    assert!(
        broker.must_cancel_at_a_switch(),
        "the agent's own working buy is still one the switch must cancel"
    );
}
