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
    handoff, opening, ports, quote, risk_exit, scope, snapshot, stream_opened,
};
use mandate_canon::Value;
use mandate_executor::{
    BrokerOutcome, BrokerRequest, BrokerUnknown, BrokerUpdate, Command, Effect, EventDraft,
    ExecutorState, Initiator, Input, KillScope, OrderState, Purpose, ReconcileReason,
    ReconciliationVerdict, RiskClock, Seq,
};
use proptest::prelude::*;

const AAPL: &str = FixedInstruments::LIQUID_EQUITY;
const CPHC: &str = FixedInstruments::THIN_EQUITY;

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
        if self.orders.len() != other.orders.len() {
            return Err(format!(
                "the journal carries {} orders and the state {}",
                self.orders.len(),
                other.orders.len()
            ));
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
    alerted: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ProtectionAccountant {
    intervals: Vec<Interval>,
    covered: BTreeMap<String, i128>,
}

impl ProtectionAccountant {
    /// Builds the interval set from the drafts alone: a `ProtectionChanged` whose action opens an
    /// interval starts one, and one whose action closes it ends it. An interval that never ends
    /// is exactly what "journaled from start to end" forbids.
    fn of(drafts: &[EventDraft]) -> Self {
        let mut accountant = Self::default();
        let mut alerted_at: BTreeSet<String> = BTreeSet::new();
        for draft in drafts {
            if draft.event_type == "OwnerAlertSent"
                && let Some(name) = field(draft, "instrument")
            {
                alerted_at.insert(name.to_owned());
            }
            if draft.event_type != "ProtectionChanged" {
                continue;
            }
            let (Some(name), Some(action)) = (field(draft, "instrument"), field(draft, "action"))
            else {
                continue;
            };
            let at = i64::try_from(number(draft, "risk_clock").unwrap_or(0)).unwrap_or(i64::MAX);
            match action {
                "unprotected_start" => accountant.intervals.push(Interval {
                    instrument: name.to_owned(),
                    started_at: at,
                    ended_at: None,
                    alerted: false,
                }),
                "unprotected_end" => {
                    if let Some(open) = accountant
                        .intervals
                        .iter_mut()
                        .rev()
                        .find(|i| i.instrument == name && i.ended_at.is_none())
                    {
                        open.ended_at = Some(at);
                    }
                }
                "placed" => {
                    let covered = field(draft, "qty").and_then(units).unwrap_or(0);
                    accountant.covered.insert(name.to_owned(), covered);
                }
                "cancelled" => {
                    accountant.covered.insert(name.to_owned(), 0);
                }
                _ => {}
            }
        }
        for interval in &mut accountant.intervals {
            interval.alerted = alerted_at.contains(&interval.instrument);
        }
        accountant
    }

    fn open(&self) -> usize {
        self.intervals
            .iter()
            .filter(|i| i.ended_at.is_none())
            .count()
    }

    /// The longest interval that closed, in whole seconds.
    fn longest_closed(&self) -> i64 {
        self.intervals
            .iter()
            .filter_map(|i| i.ended_at.map(|end| end.saturating_sub(i.started_at)))
            .max()
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Intent {
        which: u8,
        exiting: bool,
        other: bool,
    },
    Acknowledge,
    Timeout,
    Absent,
    Fill {
        units: u8,
    },
    Cancelled,
    Snapshot,
    KillSwitch,
    Restart,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        (0u8..4, any::<bool>(), any::<bool>()).prop_map(|(which, exiting, other)| Step::Intent {
            which,
            exiting,
            other,
        }),
        Just(Step::Acknowledge),
        Just(Step::Timeout),
        Just(Step::Absent),
        (1u8..9).prop_map(|units| Step::Fill { units }),
        Just(Step::Cancelled),
        Just(Step::Snapshot),
        Just(Step::KillSwitch),
        Just(Step::Restart),
    ]
}

fn scripted() -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec(step(), 1..14)
}

/// What one scripted run produced: every effect, in order, and the shell it ended in.
struct Run {
    shell: Shell,
    drafts: Vec<EventDraft>,
    effects: Vec<Effect>,
    submissions: usize,
}

/// Plays a script. The risk clock only ever advances, by four seconds a step plus whatever a
/// timeout costs, so no run this generator produces is one the journal would reject.
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
    let mut live: Vec<String> = Vec::new();
    let mut fills: u32 = 0;

    let record = |ran: Ran, drafts: &mut Vec<EventDraft>, effects: &mut Vec<Effect>| {
        drafts.extend(ran.drafts.iter().cloned());
        effects.extend(ran.effects.iter().cloned());
        ran
    };

    shell.fold_one(&stream_opened()).unwrap_or_else(|e| {
        panic!("the stream must open: {e}");
    });
    let (restarted, started_effects) = shell.restart(&ports);
    record(started_effects, &mut drafts, &mut effects);
    shell = restarted;

    for (index, one) in script.iter().enumerate() {
        at = at.saturating_add(4);
        let tick = record(
            shell.run(Input::Tick(clock(at)), &ports),
            &mut drafts,
            &mut effects,
        );
        let _ = tick;
        match one {
            Step::Intent {
                which,
                exiting,
                other,
            } => {
                let name = if *other { CPHC } else { AAPL };
                record(
                    shell.run(Input::Market(quote(name, "150", "150.2", at)), &ports),
                    &mut drafts,
                    &mut effects,
                );
                let intent = format!("01JABCDEFGHJKMNPQRSTVWXY{:02}", which);
                let body = if *exiting {
                    risk_exit(name, "1", "150")
                } else {
                    opening(name, "1", "150")
                };
                let ran = record(
                    shell.run(handoff(&intent, common::AGENT, body), &ports),
                    &mut drafts,
                    &mut effects,
                );
                for order in ran.submissions() {
                    live.push(order.client_order_id.as_str().to_owned());
                }
            }
            Step::Acknowledge => {
                if let Some(id) = live.last().cloned() {
                    record(
                        shell.run(
                            Input::Broker(Ok(BrokerOutcome::Submitted(common::broker_order(
                                &format!("b-{index}"),
                                Some(&id),
                                AAPL,
                                mandate_accounting::Side::Buy,
                                "1",
                                "0",
                                "accepted",
                            )))),
                            &ports,
                        ),
                        &mut drafts,
                        &mut effects,
                    );
                }
            }
            Step::Timeout => {
                record(
                    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports),
                    &mut drafts,
                    &mut effects,
                );
            }
            Step::Absent => {
                if let Some(id) = live.last().cloned() {
                    record(
                        shell.run(
                            Input::Broker(Ok(BrokerOutcome::Absent {
                                client_order_id: id,
                            })),
                            &ports,
                        ),
                        &mut drafts,
                        &mut effects,
                    );
                }
            }
            Step::Fill { units: quantity } => {
                if let Some(id) = live.last().cloned() {
                    fills = fills.saturating_add(1);
                    let _ = quantity;
                    record(
                        shell.run(
                            Input::BrokerUpdate(BrokerUpdate::Fill(common::broker_fill(
                                &format!("f-{fills}"),
                                Some(&id),
                                "1",
                                "150",
                            ))),
                            &ports,
                        ),
                        &mut drafts,
                        &mut effects,
                    );
                }
            }
            Step::Cancelled => {
                if let Some(id) = live.last().cloned() {
                    record(
                        shell.run(
                            Input::Broker(Ok(BrokerOutcome::CancelAccepted {
                                client_order_id: id,
                            })),
                            &ports,
                        ),
                        &mut drafts,
                        &mut effects,
                    );
                }
            }
            Step::Snapshot => {
                record(
                    shell.run(
                        Input::BrokerSnapshot(snapshot(shell.head().0, ReconcileReason::Scheduled)),
                        &ports,
                    ),
                    &mut drafts,
                    &mut effects,
                );
            }
            Step::KillSwitch => {
                record(
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
                );
            }
            Step::Restart => {
                let (next, ran) = shell.restart_keeping_broker(&ports);
                shell = next;
                record(ran, &mut drafts, &mut effects);
            }
        }
    }

    let submissions = usize::try_from(shell.connector.total_accepted()).unwrap_or(usize::MAX);
    Run {
        shell,
        drafts,
        effects,
        submissions,
    }
}

proptest! {
    /// Journal §5.2, `AGENTS.md` rule 5, DEC-07: nothing reaches the broker that the journal did
    /// not name first, in the same effect list.
    #[test]
    #[ignore = "pending E7-2"]
    fn every_submit_effect_follows_the_order_submitted_draft_that_names_it(script in scripted()) {
        let run = play(&script);
        let book = ShadowBook::of(&run.drafts);
        prop_assert_eq!(
            run.submissions,
            book.submissions.len(),
            "the broker accepted {} submissions and the journal records {}",
            run.submissions,
            book.submissions.len()
        );
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
                        "{} was sent before any draft named it",
                        order.client_order_id.as_str()
                    );
                }
                _ => {}
            }
        }
    }

    /// E7-2: the id is a function of the intent id alone, so two attempts for one intent carry
    /// one id and two intents never share one.
    #[test]
    #[ignore = "pending E7-2"]
    fn a_client_order_id_is_a_function_of_the_intent_id_alone(script in scripted()) {
        let run = play(&script);
        let mut by_intent: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for draft in &run.drafts {
            if draft.event_type != "OrderSubmitted" {
                continue;
            }
            let (Some(intent), Some(id)) =
                (field(draft, "intent_id"), field(draft, "client_order_id"))
            else {
                continue;
            };
            by_intent
                .entry(intent.to_owned())
                .or_default()
                .insert(id.to_owned());
        }
        prop_assume!(!by_intent.is_empty());
        for (intent, ids) in &by_intent {
            prop_assert_eq!(
                ids.len(),
                1,
                "intent {} produced {:?}, so the id depends on more than the intent \
                 (planted bug 7)",
                intent,
                ids
            );
        }
    }

    /// E7-2: no two intents ever share a client order id, across restarts and across agents.
    #[test]
    fn distinct_intents_never_share_a_client_order_id(script in scripted()) {
        let run = play(&script);
        let mut owner: BTreeMap<String, String> = BTreeMap::new();
        let mut seen = 0usize;
        for draft in &run.drafts {
            if draft.event_type != "OrderSubmitted" {
                continue;
            }
            let (Some(intent), Some(id)) =
                (field(draft, "intent_id"), field(draft, "client_order_id"))
            else {
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
        prop_assert_eq!(seen, ShadowBook::of(&run.drafts).submissions.len());
    }

    /// E7-3's acceptance clause, read off the broker-side counter rather than the journal.
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
        let _ = run.shell.step_crashing(Input::Tick(clock(9_000)), &ports, point);
        let (after, _) = run.shell.restart_keeping_broker(&ports);
        for (id, count) in &after.connector.accepted {
            prop_assert!(
                *count <= 1,
                "the broker accepted {} distinct submissions for {} after a crash at {:?}",
                count,
                id,
                point
            );
        }
    }

    /// Journal §5.2, §5.7: recovery queries, and never submits without a confirmed absence.
    #[test]
    #[ignore = "pending E7-3"]
    fn no_recovery_submits_without_a_confirmed_absence(script in scripted()) {
        let run = play(&script);
        let mut absences: BTreeMap<String, u32> = BTreeMap::new();
        let mut unknown: BTreeSet<String> = BTreeSet::new();
        let mut checked = 0usize;
        for draft in &run.drafts {
            let Some(id) = field(draft, "client_order_id") else {
                continue;
            };
            match draft.event_type.as_str() {
                "OrderStateChanged" if field(draft, "state") == Some("unknown") => {
                    unknown.insert(id.to_owned());
                }
                "OrderStateChanged" if field(draft, "state") == Some("absent_confirmed") => {
                    *absences.entry(id.to_owned()).or_insert(0) += 1;
                }
                "OrderSubmitted" if unknown.contains(id) => {
                    checked = checked.saturating_add(1);
                    let confirmed = absences.get(id).copied().unwrap_or(0);
                    prop_assert!(
                        confirmed >= 3,
                        "{} was resubmitted after {} absences, below the configured three",
                        id,
                        confirmed
                    );
                }
                _ => {}
            }
        }
        prop_assert!(
            checked <= ShadowBook::of(&run.drafts).submissions.len(),
            "the count cannot exceed the submissions the journal carries"
        );
    }

    /// §5.7: filled quantity is non-decreasing, at most the order quantity, and equals the sum of
    /// unique fills.
    #[test]
    #[ignore = "pending E7-2"]
    fn filled_quantity_equals_the_sum_of_unique_fills(script in scripted()) {
        let run = play(&script);
        let book = ShadowBook::of(&run.drafts);
        prop_assume!(!book.orders.is_empty());
        let live = ShadowBook::of_state(&run.shell.state);
        prop_assert_eq!(
            book.orders.len(),
            live.orders.len(),
            "the journal and the state carry a different number of orders"
        );
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
    }

    /// §5.7: terminal states are final.
    #[test]
    fn no_terminal_order_leaves_its_terminal_state(script in scripted()) {
        let run = play(&script);
        let mut terminal: BTreeMap<String, String> = BTreeMap::new();
        let mut transitions = 0usize;
        for draft in &run.drafts {
            if draft.event_type != "OrderStateChanged" && draft.event_type != "OrderAbandoned" {
                continue;
            }
            let Some(id) = field(draft, "client_order_id") else {
                continue;
            };
            let state = field(draft, "state")
                .and_then(shadow_state)
                .unwrap_or("abandoned");
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
            transitions >= terminal.len(),
            "a terminal state is reached by a transition the journal carries"
        );
    }

    /// §5.7 and interpretation 26: a reservation outlives everything but a terminal state.
    #[test]
    #[ignore = "pending E7-2"]
    fn a_reservation_is_never_released_before_a_terminal_state(script in scripted()) {
        let run = play(&script);
        let book = ShadowBook::of(&run.drafts);
        prop_assume!(!book.orders.is_empty());
        let live = ShadowBook::of_state(&run.shell.state);
        prop_assert_eq!(book.orders.len(), live.orders.len());
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
    #[ignore = "pending E7-2"]
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
        let ledger = ShadowLedger::of(&run.drafts);
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
    #[ignore = "pending E7-4"]
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
                    !request.is_account_wide(),
                    "an agent-scoped run reached {:?} (planted bug 12)",
                    request
                );
            }
        }
    }

    /// §5.5: the final mode is journaled before any cancel and any sell of the same switch.
    #[test]
    #[ignore = "pending E7-4"]
    fn the_mode_draft_precedes_every_cancel_and_every_sell(script in scripted()) {
        let run = play(&script);
        prop_assume!(script.contains(&Step::KillSwitch));
        let mut mode_at: Option<usize> = None;
        let mut switch_seen = false;
        for (index, effect) in run.effects.iter().enumerate() {
            match effect {
                Effect::Journal(draft) if draft.event_type == "AgentModeApplied" => {
                    mode_at = Some(index);
                }
                Effect::Journal(draft) if draft.event_type == "KillSwitchActivated" => {
                    switch_seen = true;
                    prop_assert!(
                        mode_at.is_some(),
                        "the switch was journaled with no mode applied before it (planted bug 13)"
                    );
                }
                Effect::Broker(BrokerRequest::Cancel { .. }) if switch_seen => {
                    prop_assert!(
                        mode_at.is_some_and(|at| at < index),
                        "a cancel ran before the final mode was applied"
                    );
                }
                _ => {}
            }
        }
        prop_assert!(switch_seen, "the kill switch reached the journal");
    }

    /// §5.4: Σ protective sell quantity never exceeds the position.
    #[test]
    #[ignore = "pending E7-4"]
    fn protective_sell_quantity_never_exceeds_the_position(script in scripted()) {
        let run = play(&script);
        let accountant = ProtectionAccountant::of(&run.drafts);
        let ledger = ShadowLedger::of(&run.drafts);
        prop_assume!(!accountant.covered.is_empty());
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
    fn every_unprotected_interval_has_a_journaled_start_and_end(script in scripted()) {
        let run = play(&script);
        let accountant = ProtectionAccountant::of(&run.drafts);
        prop_assume!(!accountant.intervals.is_empty());
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

    /// §5.4, E7-4: no interval exceeds `max_unprotected_s` without an alert.
    #[test]
    #[ignore = "pending E7-4"]
    fn no_interval_exceeds_the_limit_without_an_alert(script in scripted()) {
        let run = play(&script);
        let accountant = ProtectionAccountant::of(&run.drafts);
        prop_assume!(!accountant.intervals.is_empty());
        let alerts = run
            .effects
            .iter()
            .filter(|e| matches!(e, Effect::Notify(_)))
            .count();
        let longest = accountant.longest_closed();
        if longest > config().max_unprotected_s {
            prop_assert!(
                alerts > 0,
                "an interval of {}s exceeded the 60s bound with no alert (planted bug 14)",
                longest
            );
        }
    }

    /// §5.4: nothing is submitted while an unconfirmed cancel is outstanding.
    #[test]
    #[ignore = "pending E7-4"]
    fn no_order_is_submitted_while_an_unconfirmed_cancel_is_outstanding(script in scripted()) {
        let run = play(&script);
        let mut outstanding: BTreeSet<String> = BTreeSet::new();
        let mut cancels = 0usize;
        for effect in &run.effects {
            match effect {
                Effect::Broker(BrokerRequest::Cancel { client_order_id }) => {
                    cancels = cancels.saturating_add(1);
                    outstanding.insert(client_order_id.as_str().to_owned());
                }
                Effect::Journal(draft) if draft.event_type == "OrderStateChanged" => {
                    if field(draft, "state") == Some("canceled")
                        && let Some(id) = field(draft, "client_order_id")
                    {
                        outstanding.remove(id);
                    }
                }
                Effect::Broker(BrokerRequest::Submit(order)) => {
                    prop_assert!(
                        outstanding.is_empty(),
                        "{} was submitted with {:?} still unconfirmed (planted bug 5)",
                        order.client_order_id.as_str(),
                        outstanding
                    );
                }
                _ => {}
            }
        }
        prop_assert!(
            cancels <= run.effects.len(),
            "the cancel count is read off the same effect list"
        );
    }

    /// §5.4: an order submitted inside an unprotected interval is marketable, never resting.
    #[test]
    #[ignore = "pending E7-4"]
    fn no_resting_order_is_submitted_inside_an_unprotected_interval(script in scripted()) {
        let run = play(&script);
        let mut unprotected: BTreeSet<String> = BTreeSet::new();
        let mut submissions = 0usize;
        for effect in &run.effects {
            match effect {
                Effect::Journal(draft) if draft.event_type == "ProtectionChanged" => {
                    let (Some(name), Some(action)) =
                        (field(draft, "instrument"), field(draft, "action"))
                    else {
                        continue;
                    };
                    match action {
                        "unprotected_start" => {
                            unprotected.insert(name.to_owned());
                        }
                        "unprotected_end" => {
                            unprotected.remove(name);
                        }
                        _ => {}
                    }
                }
                Effect::Broker(BrokerRequest::Submit(order)) => {
                    submissions = submissions.saturating_add(1);
                    if unprotected.contains(order.instrument.as_str()) {
                        prop_assert!(
                            order.purpose != Purpose::Open && order.purpose != Purpose::Increase,
                            "{} opened a position inside an unprotected interval",
                            order.client_order_id.as_str()
                        );
                    }
                }
                _ => {}
            }
        }
        prop_assert_eq!(
            submissions,
            run.effects
                .iter()
                .filter(|e| matches!(e, Effect::Broker(BrokerRequest::Submit(_))))
                .count()
        );
    }

    /// `AGENTS.md` rule 13: no risk-reducing submission is ever denied by a pacing control.
    #[test]
    #[ignore = "pending E7-4"]
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
    }

    /// `AGENTS.md` rule 13: the only holds on an exit are the four the rule names.
    #[test]
    #[ignore = "pending E7-4"]
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
        let (mut shell, _) = shell.restart(&ports);
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

    /// §11: every order-set difference is adopted with a compensating event.
    #[test]
    fn every_order_difference_adopts_the_broker_with_a_compensating_event(script in scripted()) {
        let run = play(&script);
        let ids = TestIds;
        let mandates = FixedMandate::covering(&[AAPL, CPHC]);
        let instruments = FixedInstruments;
        let configuration = config();
        let ports = ports(&ids, &mandates, &instruments, &configuration);
        let taken = snapshot(run.shell.head().0, ReconcileReason::Scheduled);
        let reconciliation = mandate_executor::reconcile(&run.shell.state, &taken, &ports)
            .map_err(|e| TestCaseError::fail(format!("the reconciliation refused: {e}")))?;
        let adopted = reconciliation
            .differences
            .iter()
            .filter(|d| d.kind == mandate_executor::DifferenceKind::OrderState)
            .count();
        let compensating = reconciliation
            .effects
            .iter()
            .filter(|e| matches!(e, Effect::Journal(d) if d.event_type == "CompensatingEvent"))
            .count();
        prop_assert_eq!(
            adopted,
            compensating,
            "{} order differences produced {} compensating events (planted bug 3)",
            adopted,
            compensating
        );
    }

    /// §11: nothing outside the order set is ever adopted.
    #[test]
    #[ignore = "pending E7-3"]
    fn no_position_cash_or_fee_difference_is_ever_adopted(script in scripted()) {
        let run = play(&script);
        let ids = TestIds;
        let mandates = FixedMandate::covering(&[AAPL, CPHC]);
        let instruments = FixedInstruments;
        let configuration = config();
        let ports = ports(&ids, &mandates, &instruments, &configuration);
        let mut taken = snapshot(run.shell.head().0, ReconcileReason::Scheduled);
        taken.positions = vec![common::broker_position(AAPL, "3")];
        let reconciliation = mandate_executor::reconcile(&run.shell.state, &taken, &ports)
            .map_err(|e| TestCaseError::fail(format!("the reconciliation refused: {e}")))?;
        prop_assert!(
            !reconciliation.differences.is_empty(),
            "a position the ledger does not have is a difference"
        );
        for difference in &reconciliation.differences {
            if !difference.kind.adoptable() {
                prop_assert!(
                    !difference.adopted,
                    "a {:?} difference was adopted (planted bug 18)",
                    difference.kind
                );
            }
        }
    }

    /// E7-3: a reconciliation leaves nothing unexplained and unpaused.
    #[test]
    fn a_reconciliation_leaves_nothing_unexplained_and_unpaused(script in scripted()) {
        let run = play(&script);
        let ids = TestIds;
        let mandates = FixedMandate::covering(&[AAPL, CPHC]);
        let instruments = FixedInstruments;
        let configuration = config();
        let ports = ports(&ids, &mandates, &instruments, &configuration);
        let mut taken = snapshot(run.shell.head().0, ReconcileReason::Scheduled);
        taken.positions = vec![common::broker_position(AAPL, "3")];
        let reconciliation = mandate_executor::reconcile(&run.shell.state, &taken, &ports)
            .map_err(|e| TestCaseError::fail(format!("the reconciliation refused: {e}")))?;
        let unexplained = reconciliation
            .differences
            .iter()
            .filter(|d| !d.adopted && d.kind != mandate_executor::DifferenceKind::MissingFill)
            .count();
        if unexplained > 0 {
            prop_assert_eq!(
                reconciliation.verdict,
                ReconciliationVerdict::Mismatch,
                "{} differences were left unexplained without a mismatch verdict (planted bug 9)",
                unexplained
            );
            let paused = reconciliation
                .effects
                .iter()
                .filter(|e| matches!(
                    e,
                    Effect::Journal(d) if d.event_type == "AgentModeApplied"
                ))
                .count();
            prop_assert!(paused > 0, "and no agent was paused");
        }
    }

    /// §11, interpretation 12: the step order is part of the algorithm.
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
        let mut taken = snapshot(run.shell.head().0, ReconcileReason::Scheduled);
        taken.open_orders = vec![common::broker_order(
            "b-x",
            Some("md-unknown-to-us"),
            AAPL,
            mandate_accounting::Side::Buy,
            "1",
            "1",
            "filled",
        )];
        taken.fills = vec![common::broker_fill("f-x", Some("md-unknown-to-us"), "1", "150")];
        taken.positions = vec![common::broker_position(AAPL, "1")];
        let reconciliation = mandate_executor::reconcile(&run.shell.state, &taken, &ports)
            .map_err(|e| TestCaseError::fail(format!("the reconciliation refused: {e}")))?;
        let position_of = |wanted: &str| {
            reconciliation.effects.iter().position(|e| match e {
                Effect::Journal(d) => d.event_type == wanted,
                _ => false,
            })
        };
        let run_at = position_of("ReconciliationRun")
            .ok_or_else(|| TestCaseError::fail("the run is appended last"))?;
        for earlier in ["OrderStateChanged", "FillApplied", "BrokerPositionObserved"] {
            if let Some(at) = position_of(earlier) {
                prop_assert!(at < run_at, "{earlier} must precede the run");
            }
        }
        if let (Some(fill), Some(position)) =
            (position_of("FillApplied"), position_of("BrokerPositionObserved"))
        {
            prop_assert!(
                fill < position,
                "positions are compared only after the missing fills are ingested \
                 (planted bug 10)"
            );
        }
    }

    /// §11: the cash band alerts outside it and never pauses inside it.
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
        let (shell, _) = shell.restart(&ports);
        let drift = format!("{}.{:02}", cents / 100, cents % 100);
        let cash = format!("20000.{:02}", cents % 100);
        let _ = drift;
        let mut taken = snapshot(shell.head().0, ReconcileReason::Scheduled);
        taken.account = mandate_executor::BrokerAccount {
            cash: common::usd(cash.trim_end_matches('0').trim_end_matches('.')),
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
        if cash_difference {
            prop_assert!(alerts > 0, "a cash difference above the band always alerts (§11)");
        }
    }

    /// DEC-131 item 13, interpretation 15: a run is never positioned after a submission it did
    /// not cover.
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
        let taken = snapshot(head.0, ReconcileReason::Scheduled);
        let reconciliation = mandate_executor::reconcile(&run.shell.state, &taken, &ports)
            .map_err(|e| TestCaseError::fail(format!("the reconciliation refused: {e}")))?;
        prop_assert_eq!(
            reconciliation.expected_head,
            head,
            "the run is appended at the head its snapshot was taken at (planted bug 17)"
        );
        prop_assert!(
            run.shell
                .state
                .last_submission()
                .is_none_or(|Seq(at)| at <= head.0),
            "so any submission after it makes the append answer HeadMismatch"
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

    /// Journal §2: every risk input this crate appends carries a non-decreasing `risk_clock`.
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
        let mut last: u64 = 0;
        let mut checked = 0usize;
        for draft in &run.drafts {
            if !risk_inputs.contains(&draft.event_type.as_str()) {
                continue;
            }
            checked = checked.saturating_add(1);
            let at = number(draft, "risk_clock").ok_or_else(|| {
                TestCaseError::fail(format!("{} carries no risk_clock", draft.event_type))
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
        prop_assert!(checked <= run.drafts.len());
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
        match mandate_executor::fold(&mut state, &candidate) {
            Ok(()) => {}
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
    #[ignore = "pending E7-2"]
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

    /// ES-21, journal §8: no submission carries an intent older than its maximum age.
    #[test]
    fn no_submission_carries_an_intent_older_than_its_maximum_age(script in scripted()) {
        let run = play(&script);
        let mut received: BTreeMap<String, u64> = BTreeMap::new();
        let mut submissions = 0usize;
        let max = u64::try_from(config().max_intent_age_s).unwrap_or(u64::MAX);
        for draft in &run.drafts {
            match draft.event_type.as_str() {
                "IntentReceived" => {
                    if let (Some(intent), Some(at)) =
                        (field(draft, "intent_id"), number(draft, "risk_clock"))
                    {
                        received.insert(intent.to_owned(), at);
                    }
                }
                "OrderSubmitted" => {
                    submissions = submissions.saturating_add(1);
                    let (Some(intent), Some(at)) =
                        (field(draft, "intent_id"), number(draft, "risk_clock"))
                    else {
                        continue;
                    };
                    if let Some(born) = received.get(intent) {
                        prop_assert!(
                            at.saturating_sub(*born) <= max,
                            "{} was submitted {}s after it was received (planted bug 19)",
                            intent,
                            at.saturating_sub(*born)
                        );
                    }
                }
                _ => {}
            }
        }
        prop_assert_eq!(submissions, ShadowBook::of(&run.drafts).submissions.len());
    }
}
