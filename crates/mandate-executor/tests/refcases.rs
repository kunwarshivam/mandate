//! The account-stream reference cases this stream owns, loaded from the committed fixtures
//! (`fixtures/refcases/trading-domain.json`) in the `mandate-refcases` harness's shape.
//!
//! The harness crate itself is a `layer = "tool"` crate, which no product crate may depend on
//! (`xtask/layers.toml`), so this file reproduces its shape rather than importing it: one named
//! test per case, each loading its case by id and driving it through [`mandate_executor::fold`]
//! and [`mandate_executor::handle`]. Nothing here edits `crates/mandate-refcases/` or
//! `status.toml`: the cases move from pending to passing in a **status PR after the
//! implementation PR** (the DEC-105 and E4-1 precedent, task brief Scope).
//!
//! Each case's steps and expectations are read from the fixture rather than transcribed, so a
//! founder-approved change to the YAML reaches these tests through `cargo xtask refcases --write`
//! and nothing else.
//!
//! # What is asserted, and what is not
//!
//! Only the keys this stream owns are asserted (review round 1, coordinator ruling 7):
//!
//! - `actions`: the step's effects, in order, contain the listed actions as an ordered
//!   subsequence, and `actions: []` means no broker request and no alert at all. An alert and an
//!   account refresh are matched anywhere in the step: a notification is not ordered against the
//!   requests it reports on;
//! - `orders`: a named order's state, read off the executor's own state;
//! - `reconciliation`: the verdict of a reconciliation against the broker quantities the step
//!   names;
//! - `protective_sell_qty`: the executor's protective sell quantity in the instrument;
//! - `initial.open_orders` and `initial.positions`, which are **seeded** as the journal events
//!   that would have put them there (a `FillApplied`, a `ProtectionChanged` with `placed`).
//!
//! A gate `decision` is **never** asserted: it is stream I's and stream J's (`mandate-risk`), and
//! a case whose only expectations are decisions or accounting values is not driven here at all.
//! The dropped cases, each with its reason, are [`DROPPED`], and
//! `the_fixture_partition_is_driven_plus_dropped` checks that [`DRIVEN`] and [`DROPPED`] together
//! cover every executor- or reconciliation-scoped case and variant in the fixture, so a case added
//! to or renamed in the YAML fails here rather than leaving this suite silently.
//!
//! # How the steps become inputs
//!
//! - The risk clock is the UTC epoch second of the step's `at`, so the session and the trading
//!   date are the calendar's (`Ports::fees`), and each new New York date is a copied
//!   `TradingDayStarted` folded and handled before the step.
//! - A step's `quote` (or `last_good_quote`, aged by `age_s`) is observed before the step.
//! - A submission is acknowledged at the start of the next step unless that step rejects it; a
//!   cancel is confirmed only where the step's actions say `await_cancel_confirmed`, and the
//!   confirmation is recorded where it arrived, so "nothing is submitted before the cancel is
//!   confirmed" is checked rather than assumed.
//! - Steps of other streams' kinds (`fees_charged`, `conduct_breach`) are skipped by name; a step
//!   kind nobody named fails the case.
//! - Fixture instruments map onto the four the fixed ports know: `AAPL` to the liquid equity,
//!   crypto to `BTCUSD`, a fractionable equity to `FRAC`, and any other equity to `CPHC`.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{
    ACCOUNT_STREAM, CLOCK_STREAM, FixedInstruments, FixedMandate, Shell, TestIds, config, copied,
    event, fee_config, ports, stream_opened, text, with_clock,
};
use mandate_accounting::Side;
use mandate_executor::{
    BrokerOutcome, BrokerRequest, BrokerUpdate, Command, Effect, EventId, Initiator, Input,
    IntentBody, KillScope, OrderState, OrderType, Ports, ProtectionPrices, Purpose,
    ReconcileReason, ReconciliationVerdict, SubmitOrder, TimeInForce,
};
use mandate_num::{Bps, Qty, Rounding};
use mandate_time::UtcNanos;
use serde_json::Value as Json;

/// One case as the fixture carries it, with a variant's overrides already applied.
struct Case {
    id: String,
    instruments: Json,
    initial: Json,
    steps: Vec<Json>,
}

/// Loads one case, or one of its variants, from the committed fixture.
///
/// A missing case is a panic here rather than a silent skip: the fixture is founder-owned and
/// this stream's cases are named in the task brief, so a case that disappeared is a defect.
fn case(id: &str, variant: Option<&str>) -> Case {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/refcases/trading-domain.json"
    );
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let document: Json = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
    let found = document
        .get("cases")
        .and_then(Json::as_array)
        .and_then(|cases| {
            cases
                .iter()
                .find(|c| c.get("id").and_then(Json::as_str) == Some(id))
        })
        .unwrap_or_else(|| panic!("{path} has no case {id}"))
        .clone();
    let mut initial = found.get("initial").cloned().unwrap_or(Json::Null);
    let mut steps = found.get("steps").and_then(Json::as_array).cloned();
    if let Some(name) = variant {
        let overrides = found
            .get("variants")
            .and_then(Json::as_array)
            .and_then(|variants| {
                variants
                    .iter()
                    .find(|v| v.get("name").and_then(Json::as_str) == Some(name))
            })
            .and_then(|v| v.get("overrides"))
            .unwrap_or_else(|| panic!("{id} has no variant {name}"));
        if let (Some(base), Some(Json::Object(changed))) =
            (initial.as_object_mut(), overrides.get("initial"))
        {
            for (key, value) in changed {
                base.insert(key.clone(), value.clone());
            }
        }
        steps = overrides
            .get("steps")
            .and_then(Json::as_array)
            .cloned()
            .or(steps);
    }
    let named = match variant {
        Some(name) => format!("{id}::{name}"),
        None => id.to_owned(),
    };
    assert!(
        DRIVEN.contains(&named.as_str()),
        "{named} is driven here but missing from `DRIVEN`, so the partition check cannot see it"
    );
    Case {
        id: named,
        instruments: found.get("instruments").cloned().unwrap_or(Json::Null),
        initial,
        steps: steps.unwrap_or_else(|| panic!("{id} has no steps")),
    }
}

/// Reference-case decimals are written the way a trading desk writes them (`150.00`); every
/// value that crosses into `mandate-num` is canonical text (`150`), so the harness normalises
/// rather than letting a fixture fail a test before the crate does.
fn canonical(raw: &str) -> String {
    match raw.split_once('.') {
        None => raw.to_owned(),
        Some((whole, fraction)) => {
            let trimmed = fraction.trim_end_matches('0');
            if trimmed.is_empty() {
                whole.to_owned()
            } else {
                format!("{whole}.{trimmed}")
            }
        }
    }
}

/// Decimal text as integer units at nine places, parsed here rather than through `mandate-num`.
fn units(raw: &str) -> i128 {
    let (negative, digits) = raw
        .strip_prefix('-')
        .map_or((false, raw), |rest| (true, rest));
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    let padded = format!("{fraction:0<9}");
    let value = format!("{whole}{}", padded.get(..9).unwrap_or("000000000"))
        .parse::<i128>()
        .unwrap_or_else(|e| panic!("`{raw}` is not a decimal: {e}"));
    if negative { -value } else { value }
}

/// A seeded position's price, `cost ÷ quantity`, at nine places.
fn unit_price(cost: &str, held: &str) -> String {
    let each = units(cost)
        .saturating_mul(1_000_000_000)
        .checked_div(units(held))
        .unwrap_or_else(|| panic!("a seeded position of {held} has no price"));
    canonical(&format!(
        "{}.{:09}",
        each.div_euclid(1_000_000_000),
        each.rem_euclid(1_000_000_000)
    ))
}

fn price(raw: &str) -> mandate_num::Price {
    common::price(&canonical(raw))
}

fn qty(raw: &str) -> Qty {
    common::qty(&canonical(raw))
}

fn text_of<'a>(data: &'a Json, key: &str) -> Option<&'a str> {
    data.get(key).and_then(Json::as_str)
}

/// The risk clock of a fixture timestamp: its UTC epoch second.
fn clock_of(at: &str) -> i64 {
    UtcNanos::parse_rfc3339(at)
        .unwrap_or_else(|e| panic!("`{at}` is not an RFC 3339 instant: {e}"))
        .secs()
}

/// The fixture's instrument as one of the four the fixed ports know.
fn symbol(case: &Case, name: &str) -> &'static str {
    let described = case.instruments.get(name);
    let class = described.and_then(|d| text_of(d, "asset_class"));
    let fractionable = described
        .and_then(|d| d.get("fractionable"))
        .and_then(Json::as_bool)
        .unwrap_or(false);
    if class == Some("crypto") {
        FixedInstruments::CRYPTO
    } else if name == "AAPL" {
        FixedInstruments::LIQUID_EQUITY
    } else if fractionable {
        FixedInstruments::FRACTIONABLE
    } else {
        FixedInstruments::THIN_EQUITY
    }
}

/// A fixture name as a client order id the executor could have derived, for orders the case
/// seeds rather than submits (`oco_1` becomes `md-oco-1`).
fn seeded_id(name: &str) -> String {
    format!("md-{}", name.replace('_', "-"))
}

/// What one step did, in the order it happened: the executor's effects, and the harness's own
/// confirmations between them.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seen {
    Cancel(String),
    Confirmed(String),
    Submit(String),
    AccountWide,
    RefreshAccount,
    Journal(String),
    Gate,
    Alert(String),
}

/// One case being driven.
struct Drive<'p> {
    case: Case,
    shell: Shell,
    ports: &'p Ports<'p>,
    /// Fixture names bound to client order ids, by seeding or by the submission that carried them.
    names: BTreeMap<String, String>,
    /// Every submission, by client order id.
    submitted: BTreeMap<String, SubmitOrder>,
    /// Submissions the broker has not answered yet, in order.
    unanswered: Vec<String>,
    /// Protective prices already resting per instrument, which an add re-uses.
    resting: BTreeMap<String, ProtectionPrices>,
    date: Option<String>,
    days: u64,
    brokered: u64,
}

impl<'p> Drive<'p> {
    fn new(case: Case, ports: &'p Ports<'p>) -> Self {
        let mut drive = Self {
            case,
            shell: Shell::new(1),
            ports,
            names: BTreeMap::new(),
            submitted: BTreeMap::new(),
            unanswered: Vec::new(),
            resting: BTreeMap::new(),
            date: None,
            days: 0,
            brokered: 0,
        };
        drive.seed();
        drive
    }

    fn first_at(&self) -> i64 {
        self.case
            .steps
            .first()
            .and_then(|s| text_of(s, "at"))
            .map_or(0, clock_of)
    }

    fn first_date(&self) -> String {
        self.case
            .steps
            .first()
            .and_then(|s| text_of(s, "at"))
            .and_then(|at| at.get(..10))
            .unwrap_or("2026-09-22")
            .to_owned()
    }

    fn fold(&mut self, event_type: &str, pairs: &[(&str, mandate_canon::Value)], at: i64) {
        let seq = self.shell.head().0.saturating_add(1);
        let folded = event(ACCOUNT_STREAM, seq, event_type, with_clock(pairs, at));
        self.shell.fold_one(&folded).unwrap_or_else(|e| {
            panic!(
                "{}: the seeded {event_type} refused with {}: {e}",
                self.case.id,
                e.code()
            )
        });
    }

    /// `initial.positions` as the fills that made them, and `initial.open_orders` as the
    /// protection the executor placed, then a restart, which is how a process meets them.
    fn seed(&mut self) {
        self.shell
            .fold_one(&stream_opened())
            .unwrap_or_else(|e| panic!("{}: the stream must open: {e}", self.case.id));
        let before = self.first_at().saturating_sub(120);
        let created = self.first_date();
        let positions = self
            .case
            .initial
            .get("positions")
            .and_then(Json::as_array)
            .cloned()
            .unwrap_or_default();
        for (n, position) in positions.iter().enumerate() {
            let name = symbol(
                &self.case,
                text_of(position, "instrument").unwrap_or("AAPL"),
            );
            let held = qty(text_of(position, "qty").unwrap_or("0"));
            let each = unit_price(
                text_of(position, "cost_basis").unwrap_or("0"),
                text_of(position, "qty").unwrap_or("1"),
            );
            self.fold(
                "FillApplied",
                &[
                    ("fill_id", text(&format!("seed-{n}"))),
                    ("instrument", text(name)),
                    ("side", text("buy")),
                    ("qty_gross", text(&held.to_string())),
                    ("price", text(&each)),
                ],
                before,
            );
        }
        let open = self
            .case
            .initial
            .get("open_orders")
            .and_then(Json::as_array)
            .cloned()
            .unwrap_or_default();
        for order in &open {
            let fixture = text_of(order, "name").unwrap_or("protective");
            let name = symbol(&self.case, text_of(order, "instrument").unwrap_or("AAPL"));
            let id = seeded_id(fixture);
            let legs = order
                .get("legs")
                .and_then(Json::as_array)
                .cloned()
                .unwrap_or_default();
            let take_profit = legs
                .iter()
                .find(|l| text_of(l, "type") == Some("limit"))
                .and_then(|l| text_of(l, "limit_price"))
                .map(price);
            let stop = legs
                .iter()
                .find(|l| text_of(l, "type") == Some("stop"))
                .and_then(|l| text_of(l, "stop_price"))
                .or_else(|| text_of(order, "stop_price"))
                .map(price)
                .unwrap_or_else(|| panic!("{}: {fixture} has no stop", self.case.id));
            let covered = legs
                .first()
                .and_then(|l| text_of(l, "qty"))
                .or_else(|| text_of(order, "qty"))
                .unwrap_or("0");
            let mut pairs = vec![
                ("instrument", text(name)),
                ("action", text("placed")),
                ("orders", text(&id)),
                ("qty", text(&canonical(covered))),
                ("stop", text(&stop.to_string())),
                ("created_on", text(&created)),
                ("agent", text(common::AGENT)),
            ];
            if let Some(tp) = take_profit {
                pairs.push(("take_profit", text(&tp.to_string())));
            }
            if let Some(limit) = text_of(order, "limit_price") {
                pairs.push(("limit", text(&canonical(limit))));
            }
            self.fold("ProtectionChanged", &pairs, before.saturating_add(1));
            self.names.insert(fixture.to_owned(), id);
            self.resting
                .insert(name.to_owned(), ProtectionPrices { stop, take_profit });
        }
        let (restarted, _) = self.shell.restart(self.ports);
        self.shell = restarted;
    }

    /// Runs one input and records what it did.
    fn run(&mut self, input: Input, seen: &mut Vec<Seen>, what: &str) {
        let ran = self
            .shell
            .step(input, self.ports)
            .unwrap_or_else(|e| panic!("{}: {what} refused with {}: {e}", self.case.id, e.code()));
        for effect in &ran.effects {
            match effect {
                Effect::Broker(BrokerRequest::Cancel { client_order_id }) => {
                    seen.push(Seen::Cancel(client_order_id.as_str().to_owned()));
                }
                Effect::Broker(BrokerRequest::Submit(order)) => {
                    let id = order.client_order_id.as_str().to_owned();
                    self.submitted.insert(id.clone(), order.clone());
                    self.unanswered.push(id.clone());
                    seen.push(Seen::Submit(id));
                }
                Effect::Broker(BrokerRequest::CancelAll(_) | BrokerRequest::ClosePosition(..)) => {
                    seen.push(Seen::AccountWide);
                }
                Effect::Broker(BrokerRequest::GetAccount) => seen.push(Seen::RefreshAccount),
                Effect::Journal(draft) => match draft.event_type.as_str() {
                    "ProtectionChanged" => {
                        match draft
                            .payload
                            .get("action")
                            .and_then(mandate_canon::Value::as_str)
                        {
                            Some("unprotected_start") => {
                                seen.push(Seen::Journal("unprotected_window_start".to_owned()));
                            }
                            Some("unprotected_end") => {
                                seen.push(Seen::Journal("unprotected_window_end".to_owned()));
                            }
                            _ => {}
                        }
                    }
                    "ExternalActivityIngested" => {
                        seen.push(Seen::Journal("external_activity".to_owned()));
                    }
                    "GateDecided" => seen.push(Seen::Gate),
                    _ => {}
                },
                Effect::Notify(reference) => {
                    seen.push(Seen::Alert(reference.message_key.to_owned()))
                }
                Effect::Broker(_) | Effect::Timer(_) => {}
            }
        }
    }

    /// A new New York date is a copied `TradingDayStarted`, folded and handled.
    fn day(&mut self, date: &str, at: i64, seen: &mut Vec<Seen>) {
        self.days = self.days.saturating_add(1);
        let started = copied(
            ACCOUNT_STREAM,
            self.shell.head().0.saturating_add(1),
            "TradingDayStarted",
            with_clock(&[("date", text(date))], at),
            &EventId(format!("{CLOCK_STREAM}-{}", self.days.saturating_add(100))),
        );
        self.shell.fold_one(&started).unwrap_or_else(|e| {
            panic!(
                "{}: the trading day {date} refused with {}: {e}",
                self.case.id,
                e.code()
            )
        });
        self.run(Input::Journal(started), seen, "the trading day");
    }

    fn broker_order(
        &mut self,
        id: &str,
        status: &str,
        filled: &str,
    ) -> mandate_executor::BrokerOrder {
        self.brokered = self.brokered.saturating_add(1);
        let order = self.submitted.get(id);
        mandate_executor::BrokerOrder {
            limit_price: order.and_then(|o| o.limit_price),
            stop_price: order.and_then(|o| o.stop_price),
            ..common::broker_order(
                &format!("b-{}", self.brokered),
                Some(id),
                order.map_or(FixedInstruments::LIQUID_EQUITY, |o| o.instrument.as_str()),
                order.map_or(Side::Buy, |o| o.side),
                &order.map_or_else(|| "1".to_owned(), |o| o.qty.to_string()),
                &canonical(filled),
                status,
            )
        }
    }

    /// Answers every submission the broker has not answered, except `refused`, which the step
    /// itself rejects.
    fn acknowledge(&mut self, refused: Option<&str>, seen: &mut Vec<Seen>) {
        let waiting = std::mem::take(&mut self.unanswered);
        for id in waiting {
            if Some(id.as_str()) == refused {
                continue;
            }
            let accepted = self.broker_order(&id, "accepted", "0");
            self.run(
                Input::Broker(Ok(BrokerOutcome::Submitted(accepted))),
                seen,
                "the acknowledgment",
            );
        }
    }

    /// Confirms every cancel the step has sent and not yet seen confirmed, recording each
    /// confirmation where it arrived, until a round sends no new cancel.
    fn confirm_cancels(&mut self, seen: &mut Vec<Seen>) {
        loop {
            let confirmed: BTreeSet<String> = seen
                .iter()
                .filter_map(|s| match s {
                    Seen::Confirmed(id) => Some(id.clone()),
                    _ => None,
                })
                .collect();
            let open: Vec<String> = seen
                .iter()
                .filter_map(|s| match s {
                    Seen::Cancel(id) if !confirmed.contains(id) => Some(id.clone()),
                    _ => None,
                })
                .collect();
            if open.is_empty() {
                return;
            }
            for id in open {
                seen.push(Seen::Confirmed(id.clone()));
                self.run(
                    Input::Broker(Ok(BrokerOutcome::CancelAccepted {
                        client_order_id: id,
                    })),
                    seen,
                    "the cancel confirmation",
                );
            }
        }
    }

    fn intent_body(&self, data: &Json) -> IntentBody {
        let name = symbol(&self.case, text_of(data, "instrument").unwrap_or("AAPL"));
        let entry = data.get("entry").unwrap_or(data);
        let field = |key: &str| text_of(entry, key).or_else(|| text_of(data, key));
        let quantity = field("qty").unwrap_or("1");
        let limit = field("limit_price")
            .or_else(|| data.get("quote").and_then(|q| text_of(q, "bid")))
            .or_else(|| data.get("last_good_quote").and_then(|q| text_of(q, "bid")))
            .unwrap_or_else(|| panic!("{}: a proposal with no price and no quote", self.case.id));
        let purpose = match text_of(data, "purpose").unwrap_or("open") {
            "risk_exit" => Purpose::RiskExit,
            "owner_exit" => Purpose::OwnerExit,
            "discretionary_exit" => Purpose::DiscretionaryExit,
            "increase" => Purpose::Increase,
            _ => Purpose::Open,
        };
        let side = if field("side") == Some("sell") {
            Side::Sell
        } else {
            Side::Buy
        };
        let bracket = data
            .get("stop_loss")
            .and_then(|s| text_of(s, "stop_price"))
            .map(|stop| ProtectionPrices {
                stop: price(stop),
                take_profit: data
                    .get("take_profit")
                    .and_then(|t| text_of(t, "limit_price"))
                    .map(price),
            });
        let protection = match (bracket, purpose) {
            (Some(prices), _) => Some(prices),
            (None, Purpose::Increase) => self.resting.get(name).copied(),
            (None, _) => None,
        };
        IntentBody::Order {
            instrument: common::instrument(name),
            side,
            qty: qty(quantity),
            limit: price(limit),
            purpose,
            protection,
        }
    }

    /// Drives one step and returns what it did.
    fn step(&mut self, index: usize, step: &Json) -> Vec<Seen> {
        let mut seen = Vec::new();
        let kind = text_of(step, "event")
            .unwrap_or_else(|| panic!("{} step {index} has no `event`", self.case.id))
            .to_owned();
        let at_text = text_of(step, "at")
            .unwrap_or_else(|| panic!("{} step {index} has no `at`", self.case.id))
            .to_owned();
        let at = clock_of(&at_text);
        let data = step.get("data").cloned().unwrap_or(Json::Null);
        let refused = (kind == "broker_order_update"
            && text_of(&data, "status") == Some("rejected"))
        .then(|| {
            text_of(&data, "name")
                .and_then(|n| self.names.get(n))
                .cloned()
        })
        .flatten();
        self.acknowledge(refused.as_deref(), &mut seen);
        let date = at_text.get(..10).unwrap_or("").to_owned();
        if self.date.as_deref() != Some(date.as_str()) {
            self.day(&date, at, &mut seen);
            self.date = Some(date);
        }
        self.run(Input::Tick(common::clock(at)), &mut seen, "the clock");
        let name = symbol(&self.case, text_of(&data, "instrument").unwrap_or("AAPL"));
        if let Some(quote) = data.get("quote") {
            let observed = common::quote(
                name,
                &canonical(text_of(quote, "bid").unwrap_or("150")),
                &canonical(text_of(quote, "ask").unwrap_or("150")),
                at,
            );
            self.run(Input::Market(observed), &mut seen, "the quote");
        }
        if let Some(quote) = data.get("last_good_quote") {
            let age = quote.get("age_s").and_then(Json::as_i64).unwrap_or(0);
            let observed = common::quote(
                name,
                &canonical(text_of(quote, "bid").unwrap_or("150")),
                &canonical(text_of(quote, "ask").unwrap_or("150")),
                at.saturating_sub(age),
            );
            self.run(Input::Market(observed), &mut seen, "the last good quote");
        }
        match kind.as_str() {
            "propose_order" => {
                let body = self.intent_body(&data);
                self.run(
                    common::handoff(&format!("01JREFCASE{index:016}"), common::AGENT, body),
                    &mut seen,
                    "the proposal",
                );
            }
            "broker_order_update" => self.order_update(index, &data, &mut seen),
            "broker_account_update" => {
                let account = mandate_executor::BrokerAccount {
                    status: text_of(&data, "status").unwrap_or("ACTIVE").to_owned(),
                    ..common::broker_account()
                };
                self.run(
                    Input::BrokerUpdate(BrokerUpdate::Account(account)),
                    &mut seen,
                    "the account update",
                );
            }
            "kill_switch" => self.run(
                Input::Command(Command::KillSwitch {
                    scope: KillScope::Agent(common::agent(common::AGENT)),
                    initiator: Initiator::RiskLimit,
                    confirmation: None,
                }),
                &mut seen,
                "the kill switch",
            ),
            "mark" => {
                let mark = canonical(text_of(&data, "price").unwrap_or("150"));
                self.run(
                    Input::Market(common::quote(name, &mark, &mark, at)),
                    &mut seen,
                    "the mark",
                );
            }
            "advance_clock" => {}
            "corporate_action_prepare" | "corporate_action_applied" => {
                self.corporate_action(&kind, name, &data, at, &mut seen);
            }
            "reconciliation" | "broker_position_update" => {
                let taken = self.snapshot(&data);
                self.run(
                    Input::BrokerSnapshot(taken),
                    &mut seen,
                    "the reconciliation",
                );
            }
            "fill" => self.accounting_fill(index, name, &data, at),
            "fees_charged" | "conduct_breach" => {}
            other => panic!(
                "{} step {index}: `{other}` is a step kind this harness does not know, so the \
                 case cannot pass by skipping it",
                self.case.id
            ),
        }
        if expected_actions(step)
            .iter()
            .any(|(kind, _)| kind == "await_cancel_confirmed")
        {
            self.confirm_cancels(&mut seen);
        }
        seen
    }

    fn order_update(&mut self, index: usize, data: &Json, seen: &mut Vec<Seen>) {
        let status = text_of(data, "status").unwrap_or("new");
        let fill = data.get("fill");
        if let Some(named) = text_of(data, "name") {
            let id = self.names.get(named).cloned().unwrap_or_else(|| {
                panic!("{} step {index}: `{named}` names no order", self.case.id)
            });
            if status == "rejected" {
                let reject = mandate_executor::BrokerReject {
                    code: data
                        .get("reject")
                        .and_then(|r| text_of(r, "mapped_as"))
                        .map(str::to_owned),
                    ..common::broker_reject(
                        Some(&id),
                        u16::try_from(
                            data.get("http_status")
                                .and_then(Json::as_u64)
                                .unwrap_or(422),
                        )
                        .unwrap_or(422),
                        data.get("reject")
                            .and_then(|r| text_of(r, "message"))
                            .unwrap_or("rejected"),
                    )
                };
                self.run(
                    Input::Broker(Ok(BrokerOutcome::Rejected(reject))),
                    seen,
                    "the reject",
                );
                return;
            }
            let order = self.submitted.get(&id).cloned();
            if let (Some(fill), Some(order)) = (fill, &order) {
                let reported = mandate_executor::BrokerFill {
                    instrument: order.instrument.clone(),
                    side: order.side,
                    ..common::broker_fill(
                        &format!("f-{index}"),
                        Some(&id),
                        &canonical(text_of(fill, "qty").unwrap_or("0")),
                        &canonical(text_of(fill, "price").unwrap_or("0")),
                    )
                };
                self.run(
                    Input::BrokerUpdate(BrokerUpdate::Fill(reported)),
                    seen,
                    "the fill",
                );
            }
            let filled = fill.and_then(|f| text_of(f, "qty")).unwrap_or("0");
            let update = self.broker_order(&id, status, filled);
            self.run(
                Input::BrokerUpdate(BrokerUpdate::Order(update)),
                seen,
                "the order update",
            );
            return;
        }
        let foreign = text_of(data, "client_order_id").unwrap_or("foreign");
        if status == "rejected" {
            let reject = common::broker_reject(
                Some(foreign),
                u16::try_from(
                    data.get("http_status")
                        .and_then(Json::as_u64)
                        .unwrap_or(422),
                )
                .unwrap_or(422),
                data.get("reject")
                    .and_then(|r| text_of(r, "message"))
                    .unwrap_or("forbidden"),
            );
            self.run(
                Input::BrokerUpdate(BrokerUpdate::Reject(reject)),
                seen,
                "the reject",
            );
            return;
        }
        let name = symbol(&self.case, text_of(data, "instrument").unwrap_or("AAPL"));
        let side = if text_of(data, "side") == Some("sell") {
            Side::Sell
        } else {
            Side::Buy
        };
        let quantity = canonical(fill.and_then(|f| text_of(f, "qty")).unwrap_or("1"));
        let order = common::broker_order(
            &format!("b-foreign-{index}"),
            Some(foreign),
            name,
            side,
            &quantity,
            &quantity,
            status,
        );
        self.run(
            Input::BrokerUpdate(BrokerUpdate::Order(order)),
            seen,
            "the foreign order",
        );
        if let Some(fill) = fill {
            let reported = mandate_executor::BrokerFill {
                instrument: common::instrument(name),
                side,
                ..common::broker_fill(
                    &format!("f-foreign-{index}"),
                    Some(foreign),
                    &quantity,
                    &canonical(text_of(fill, "price").unwrap_or("0")),
                )
            };
            self.run(
                Input::BrokerUpdate(BrokerUpdate::Fill(reported)),
                seen,
                "the foreign fill",
            );
        }
    }

    /// A corporate action, as the journal event stream F writes for it, folded and handled.
    fn corporate_action(
        &mut self,
        kind: &str,
        name: &str,
        data: &Json,
        at: i64,
        seen: &mut Vec<Seen>,
    ) {
        let event_type = if kind == "corporate_action_prepare" {
            "CorporateActionPrepared"
        } else {
            "CorporateActionApplied"
        };
        let mut pairs = vec![
            ("instrument", text(name)),
            ("action", text(text_of(data, "type").unwrap_or("split"))),
            (
                "ex_date",
                text(text_of(data, "ex_date").unwrap_or("2026-09-22")),
            ),
        ];
        if let Some(ratio) = data.get("ratio") {
            let new = ratio.get("new").and_then(Json::as_u64).unwrap_or(1);
            let old = ratio.get("old").and_then(Json::as_u64).unwrap_or(1);
            assert_eq!(old, 1, "{}: only an n-for-1 split is seeded", self.case.id);
            pairs.push(("ratio", text(&new.to_string())));
        }
        if let Some(amount) = text_of(data, "amount_per_share") {
            pairs.push(("amount", text(&canonical(amount))));
        }
        let seq = self.shell.head().0.saturating_add(1);
        let prepared = event(ACCOUNT_STREAM, seq, event_type, with_clock(&pairs, at));
        self.shell.fold_one(&prepared).unwrap_or_else(|e| {
            panic!(
                "{}: {event_type} refused with {}: {e}",
                self.case.id,
                e.code()
            )
        });
        self.run(Input::Journal(prepared), seen, event_type);
    }

    /// A fill the accounting stream applied, with the crypto asset fee it accrued at the
    /// configured taker rate, folded as stream F journals them.
    fn accounting_fill(&mut self, index: usize, name: &str, data: &Json, at: i64) {
        let gross = qty(text_of(data, "qty_gross").unwrap_or("0"));
        let side = text_of(data, "side").unwrap_or("buy");
        self.fold(
            "FillApplied",
            &[
                ("fill_id", text(&format!("f-{index}"))),
                ("instrument", text(name)),
                ("side", text(side)),
                ("qty_gross", text(&gross.to_string())),
                (
                    "price",
                    text(&canonical(text_of(data, "price").unwrap_or("0"))),
                ),
            ],
            at,
        );
        if name == FixedInstruments::CRYPTO && side == "buy" {
            let fee = asset_fee(gross);
            self.fold(
                "FeesCharged",
                &[
                    ("family", text("crypto_asset")),
                    ("accrued", text(&fee.to_string())),
                    ("charged", text("0")),
                    ("instrument", text(name)),
                ],
                at,
            );
        }
    }

    /// The broker's side for a reconciliation step: the positions it names, and the orders the
    /// harness knows are still working.
    fn snapshot(&self, data: &Json) -> mandate_executor::BrokerSnapshot {
        let mut taken = common::snapshot(self.shell.head().0, ReconcileReason::Scheduled);
        if let Some(Json::Object(positions)) = data.get("broker_positions") {
            for (fixture, held) in positions {
                let held = held.as_str().unwrap_or("0");
                taken.positions.push(common::broker_position(
                    symbol(&self.case, fixture),
                    &canonical(held),
                ));
            }
        }
        taken
    }
}

/// The crypto asset fee a taker buy accrues at `Ports::fees`' rate (§6.3), computed here from
/// the configuration rather than read from the case's expectation.
fn asset_fee(gross: Qty) -> Qty {
    let rate: Bps = fee_config().crypto.taker;
    gross
        .times_bps(rate, Rounding::HalfEven)
        .unwrap_or_else(|e| panic!("the asset fee: {e}"))
}

/// The actions one step's `expect.actions` lists, each as its name and argument.
fn expected_actions(step: &Json) -> Vec<(String, Json)> {
    step.get("expect")
        .and_then(|e| e.get("actions"))
        .and_then(Json::as_array)
        .map(|actions| {
            actions
                .iter()
                .filter_map(|action| match action {
                    Json::String(name) => Some((name.clone(), Json::Null)),
                    Json::Object(map) => map.iter().next().map(|(k, v)| (k.clone(), v.clone())),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn tif(raw: &str) -> TimeInForce {
    match raw {
        "gtc" => TimeInForce::Gtc,
        "ioc" => TimeInForce::Ioc,
        _ => TimeInForce::Day,
    }
}

impl Drive<'_> {
    fn id_of(&self, name: &str, index: usize) -> String {
        self.names
            .get(name)
            .cloned()
            .unwrap_or_else(|| panic!("{} step {index}: `{name}` names no order yet", self.case.id))
    }

    /// Whether one observed submission is the one an expected `submit` or `submit_protective`
    /// describes. Unnamed or already-bound submissions to another name never match.
    fn describes(&self, id: &str, kind: &str, argument: &Json) -> bool {
        let Some(order) = self.submitted.get(id) else {
            return false;
        };
        let wanted = match argument {
            Json::String(name) => Some(name.as_str()),
            _ => text_of(argument, "name"),
        };
        let bound_elsewhere = self
            .names
            .iter()
            .any(|(name, bound)| bound == id && Some(name.as_str()) != wanted);
        if bound_elsewhere {
            return false;
        }
        if let Some(name) = wanted
            && let Some(bound) = self.names.get(name)
            && bound != id
        {
            return false;
        }
        if kind == "submit" {
            return match argument {
                Json::Object(_) => {
                    text_of(argument, "type")
                        .is_none_or(|t| t != "limit" || order.order_type == OrderType::Limit)
                        && text_of(argument, "limit_price")
                            .is_none_or(|p| order.limit_price == Some(price(p)))
                        && argument
                            .get("extended_hours")
                            .and_then(Json::as_bool)
                            .is_none_or(|e| order.extended_hours == e)
                        && text_of(argument, "tif").is_none_or(|t| order.tif == tif(t))
                }
                _ => true,
            };
        }
        let quantity_matches = text_of(argument, "qty")
            .is_none_or(|q| order.oco.as_ref().map_or(order.qty, |legs| legs.qty) == qty(q));
        let tif_matches = text_of(argument, "tif").is_none_or(|t| order.tif == tif(t));
        let shape = match text_of(argument, "type") {
            Some("oco") => order.oco.as_ref().is_some_and(|legs| {
                text_of(argument, "take_profit").is_none_or(|p| legs.take_profit == price(p))
                    && text_of(argument, "stop").is_none_or(|p| legs.stop == price(p))
            }),
            Some("stop_limit") => {
                order.order_type == OrderType::StopLimit
                    && text_of(argument, "stop").is_none_or(|p| order.stop_price == Some(price(p)))
                    && text_of(argument, "limit")
                        .is_none_or(|p| order.limit_price == Some(price(p)))
            }
            _ => false,
        };
        order.side == Side::Sell && quantity_matches && tif_matches && shape
    }

    /// Whether one observed thing is the expected action, binding a submission's name when it is.
    fn matches(&mut self, kind: &str, argument: &Json, seen: &Seen, index: usize) -> bool {
        let named = argument.as_str();
        match (kind, seen) {
            ("cancel" | "cancel_remainder", Seen::Cancel(id)) => {
                named.is_some_and(|n| self.id_of(n, index) == *id)
            }
            ("cancel_all_orders", Seen::Cancel(_)) => true,
            ("await_cancel_confirmed", Seen::Confirmed(id)) => {
                named == Some("all") || named.is_some_and(|n| self.id_of(n, index) == *id)
            }
            ("journal", Seen::Journal(what)) => named == Some(what.as_str()),
            ("rerun_gate", Seen::Gate) => true,
            ("submit" | "submit_protective", Seen::Submit(id)) => {
                if !self.describes(id, kind, argument) {
                    return false;
                }
                let wanted = match argument {
                    Json::String(name) => Some(name.clone()),
                    _ => text_of(argument, "name").map(str::to_owned),
                };
                if let Some(name) = wanted {
                    self.names.insert(name, id.clone());
                }
                true
            }
            ("close_all_positions", Seen::Submit(id)) => self
                .submitted
                .get(id)
                .is_some_and(|order| order.side == Side::Sell),
            ("alert_owner", Seen::Alert(key)) => named == Some(key.as_str()),
            ("refresh_account", Seen::RefreshAccount) => true,
            _ => false,
        }
    }

    /// `actions`: the listed actions occur in this order within what the step did.
    fn assert_actions(&mut self, index: usize, step: &Json, seen: &[Seen]) -> bool {
        let Some(listed) = step.get("expect").and_then(|e| e.get("actions")) else {
            return false;
        };
        let expected = expected_actions(step);
        if listed.as_array().is_some_and(Vec::is_empty) {
            let acted: Vec<&Seen> = seen
                .iter()
                .filter(|s| {
                    matches!(
                        s,
                        Seen::Cancel(_) | Seen::Submit(_) | Seen::AccountWide | Seen::Alert(_)
                    )
                })
                .collect();
            assert!(
                acted.is_empty(),
                "{} step {index}: `actions: []`, and the step did {acted:?}",
                self.case.id
            );
            return true;
        }
        let known = [
            "cancel",
            "cancel_remainder",
            "cancel_all_orders",
            "await_cancel_confirmed",
            "journal",
            "rerun_gate",
            "submit",
            "submit_protective",
            "close_all_positions",
            "alert_owner",
            "refresh_account",
        ];
        let mut from = 0usize;
        let mut last_cancel: Option<usize> = None;
        for (kind, argument) in &expected {
            assert!(
                known.contains(&kind.as_str()),
                "{} step {index}: the harness does not know the action `{kind}`",
                self.case.id
            );
            let unordered = kind == "alert_owner" || kind == "refresh_account";
            let start = if unordered { 0 } else { from };
            let found = (start..seen.len()).find(|&at| {
                seen.get(at)
                    .is_some_and(|s| self.matches(kind, argument, s, index))
            });
            let at = found.unwrap_or_else(|| {
                panic!(
                    "{} step {index}: expected `{kind}` {argument} after position {from} of \
                     {seen:?}",
                    self.case.id
                )
            });
            if kind.starts_with("cancel") {
                last_cancel = Some(at);
            }
            if kind == "await_cancel_confirmed"
                && let Some(cancelled) = last_cancel
            {
                let early: Vec<&Seen> = seen
                    .get(cancelled..at)
                    .unwrap_or_default()
                    .iter()
                    .filter(|s| matches!(s, Seen::Submit(_)))
                    .collect();
                assert!(
                    early.is_empty(),
                    "{} step {index}: {early:?} went before the cancel was confirmed (§5.4)",
                    self.case.id
                );
            }
            if !unordered {
                from = at.saturating_add(1);
            }
        }
        if expected.iter().any(|(kind, argument)| {
            kind == "cancel_all_orders" && argument.as_str() == Some("agent")
        }) {
            assert!(
                !seen.contains(&Seen::AccountWide),
                "{} step {index}: an agent-scoped kill switch never uses cancel-all or \
                 close-position (`AGENTS.md` rule 13): {seen:?}",
                self.case.id
            );
        }
        true
    }

    /// `orders`: each named order's state.
    fn assert_orders(&self, index: usize, step: &Json) -> bool {
        let Some(Json::Object(orders)) = step.get("expect").and_then(|e| e.get("orders")) else {
            return false;
        };
        for (name, expected) in orders {
            let id = self.id_of(name, index);
            let wanted = text_of(expected, "state").unwrap_or("");
            let client = mandate_executor::ClientOrderId::parse(&id).unwrap_or_else(|e| {
                panic!("{}: `{id}` is not a client order id: {e}", self.case.id)
            });
            let state = self.shell.state.order(&client).map(|o| o.state);
            let resting = self
                .shell
                .state
                .orders()
                .values()
                .map(|o| o.instrument.clone())
                .chain(self.resting.keys().map(|n| common::instrument(n)))
                .any(|instrument| {
                    self.shell
                        .state
                        .protection(&instrument)
                        .expect("the protection accessor answers")
                        .is_some_and(|p| p.resting.contains(&client))
                });
            match wanted {
                "Accepted" => assert!(
                    state == Some(OrderState::Accepted) || (state.is_none() && resting),
                    "{} step {index}: {name} is still working, and it is {state:?} (resting: \
                     {resting})",
                    self.case.id
                ),
                "Canceled" => assert!(
                    state == Some(OrderState::Canceled) || (state.is_none() && !resting),
                    "{} step {index}: {name} is cancelled, and it is {state:?} (resting: \
                     {resting})",
                    self.case.id
                ),
                other => panic!(
                    "{}: the harness does not know the state `{other}`",
                    self.case.id
                ),
            }
        }
        true
    }

    /// `reconciliation`: the verdict against the broker quantities the step names.
    fn assert_reconciliation(
        &self,
        index: usize,
        step: &Json,
        before: &mandate_executor::ExecutorState,
    ) -> bool {
        let Some(expected) = step.get("expect").and_then(|e| e.get("reconciliation")) else {
            return false;
        };
        let data = step.get("data").cloned().unwrap_or(Json::Null);
        let name = symbol(&self.case, text_of(&data, "instrument").unwrap_or("AAPL"));
        let status = text_of(expected, "status").unwrap_or("ok");
        let (state, taken) = match text_of(expected, "broker_qty_before_fee_posting") {
            Some(broker) => {
                let mut taken = common::snapshot(self.shell.head().0, ReconcileReason::Scheduled);
                taken.positions = vec![common::broker_position(name, &canonical(broker))];
                (&self.shell.state, taken)
            }
            None => (before, self.snapshot(&data)),
        };
        let run = mandate_executor::reconcile(state, &taken, self.ports).unwrap_or_else(|e| {
            panic!(
                "{} step {index}: the reconciliation refused with {}: {e}",
                self.case.id,
                e.code()
            )
        });
        let paused = run.effects.iter().any(|e| {
            matches!(
                e,
                Effect::Journal(d) if d.event_type == "AgentModeApplied"
                    && d.payload.get("to").and_then(mandate_canon::Value::as_str) == Some("paused")
            )
        });
        match status {
            "ok" | "pending_corporate_action" => {
                assert_ne!(
                    run.verdict,
                    ReconciliationVerdict::Mismatch,
                    "{} step {index}: `{status}` is no mismatch: {:?}",
                    self.case.id,
                    run.differences
                );
                assert!(
                    !paused,
                    "{} step {index}: and nobody is paused",
                    self.case.id
                );
            }
            other => panic!(
                "{}: the harness does not know the status `{other}`",
                self.case.id
            ),
        }
        if let (Some(broker), Some(tolerance)) = (
            text_of(expected, "broker_qty_before_fee_posting"),
            text_of(expected, "tolerance_qty"),
        ) {
            let beyond = qty(broker)
                .checked_add(qty(tolerance))
                .unwrap_or_else(|e| panic!("{}: {e}", self.case.id));
            let mut off = common::snapshot(self.shell.head().0, ReconcileReason::Scheduled);
            off.positions = vec![common::broker_position(name, &beyond.to_string())];
            let run = mandate_executor::reconcile(&self.shell.state, &off, self.ports)
                .unwrap_or_else(|e| panic!("{}: {e}", self.case.id));
            assert_eq!(
                run.verdict,
                ReconciliationVerdict::Mismatch,
                "{} step {index}: the unposted fee explains exactly {tolerance}, so {beyond} is a \
                 mismatch",
                self.case.id
            );
        }
        true
    }

    /// `protective_sell_qty`: the executor's protective sell quantity in the step's instrument.
    fn assert_protection(&self, index: usize, step: &Json) -> bool {
        let Some(expected) = step
            .get("expect")
            .and_then(|e| e.get("protective_sell_qty"))
            .and_then(Json::as_str)
        else {
            return false;
        };
        let data = step.get("data").cloned().unwrap_or(Json::Null);
        let instrument = text_of(&data, "name")
            .and_then(|n| self.names.get(n))
            .and_then(|id| self.submitted.get(id))
            .map(|o| o.instrument.clone())
            .unwrap_or_else(|| {
                common::instrument(symbol(
                    &self.case,
                    text_of(&data, "instrument").unwrap_or("AAPL"),
                ))
            });
        assert_eq!(
            self.shell
                .state
                .protective_sell_qty(&instrument)
                .expect("the protective-quantity accessor answers"),
            qty(expected),
            "{} step {index}: the protective sell quantity (§5.4)",
            self.case.id
        );
        true
    }
}

/// Every case and variant this file drives, one test each, named as `case` names them.
const DRIVEN: [&str; 16] = [
    "RC-04",
    "RC-06::protective_orders_kept_through_dividend",
    "RC-07",
    "RC-14",
    "RC-14::add_via_bracket",
    "RC-14::kill_switch",
    "RC-14::passive_exit_becomes_oco_take_profit",
    "RC-15",
    "RC-15::external_order_detected",
    "RC-15::status_not_active",
    "RC-15::unexplained_403s",
    "RC-20",
    "RC-21",
    "RC-22",
    "RC-24",
    "RC-24::presumed_halt_regular_session",
];

/// The cases the task brief assigned to this stream that are not driven, each with its reason:
/// none of them expects a key this stream owns (coordinator ruling 7).
const DROPPED: [(&str, &str); 9] = [
    (
        "RC-08",
        "settlement and buying power: stream F's and the gate's values only",
    ),
    (
        "RC-09",
        "the day-trade budget: gate decisions and counts only",
    ),
    (
        "RC-09B",
        "the day-trade budget: gate decisions and counts only",
    ),
    (
        "RC-11",
        "trade and settlement dates and a mode: stream F's calendar only",
    ),
    ("RC-14::plain_add_blocked", "one gate decision"),
    (
        "RC-17",
        "instrument claims and shared buying power: gate decisions only",
    ),
    (
        "RC-18",
        "settlement and buying power: stream F's and the gate's values only",
    ),
    (
        "RC-23",
        "a fractionable split residual: stream F's arithmetic only (coordinator ruling 6)",
    ),
    (
        "RC-25",
        "close-window and session decisions only; steps 0-1 are one second apart and only the \
         gate's close-window rule tells them apart",
    ),
];

/// The fixture's cases and variants as `case` names them, with each case's `scope` list.
fn fixture_entries() -> BTreeMap<String, Vec<String>> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/refcases/trading-domain.json"
    );
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let document: Json = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut entries = BTreeMap::new();
    for found in document
        .get("cases")
        .and_then(Json::as_array)
        .unwrap_or_else(|| panic!("{path} has no `cases` array"))
    {
        let id = text_of(found, "id").unwrap_or_else(|| panic!("a case in {path} has no id"));
        let scope: Vec<String> = found
            .get("scope")
            .and_then(Json::as_array)
            .map(|s| {
                s.iter()
                    .filter_map(Json::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        entries.insert(id.to_owned(), scope.clone());
        for variant in found
            .get("variants")
            .and_then(Json::as_array)
            .cloned()
            .unwrap_or_default()
        {
            if let Some(name) = text_of(&variant, "name") {
                entries.insert(format!("{id}::{name}"), scope.clone());
            }
        }
    }
    entries
}

/// The driven and dropped sets partition this stream's share of the fixture: each names a case or
/// variant the fixture carries, no entry is both, and every executor- or reconciliation-scoped
/// case and variant is one or the other. Not pending: it reads the fixture only.
#[test]
fn the_fixture_partition_is_driven_plus_dropped() {
    let entries = fixture_entries();
    let driven: BTreeSet<&str> = DRIVEN.iter().copied().collect();
    let dropped: BTreeSet<&str> = DROPPED.iter().map(|(id, _)| *id).collect();
    assert_eq!(driven.len(), DRIVEN.len(), "no case is driven twice");
    assert_eq!(dropped.len(), DROPPED.len(), "no case is dropped twice");
    assert!(
        driven.is_disjoint(&dropped),
        "a case is driven or dropped, never both: {:?}",
        driven.intersection(&dropped).collect::<Vec<_>>()
    );
    for (id, reason) in DROPPED {
        assert!(!reason.is_empty(), "{id} is dropped without a reason");
    }
    for id in driven.iter().chain(dropped.iter()) {
        assert!(
            entries.contains_key(*id),
            "{id} is named here and the fixture does not carry it"
        );
    }
    let owned: BTreeSet<&str> = entries
        .iter()
        .filter(|(_, scope)| {
            scope
                .iter()
                .any(|s| s == "executor" || s == "reconciliation")
        })
        .map(|(id, _)| id.as_str())
        .collect();
    let missing: Vec<&&str> = owned
        .iter()
        .filter(|id| !driven.contains(**id) && !dropped.contains(**id))
        .collect();
    assert!(
        missing.is_empty(),
        "the fixture carries executor or reconciliation cases this suite neither drives nor \
         drops: {missing:?}"
    );
}

/// Drives one case's steps against a fresh executor and asserts the keys this stream owns.
fn drive(case: Case) {
    assert!(
        !case.steps.is_empty(),
        "{} has no steps to drive, so the case would assert nothing",
        case.id
    );
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[
        FixedInstruments::LIQUID_EQUITY,
        FixedInstruments::THIN_EQUITY,
        FixedInstruments::CRYPTO,
        FixedInstruments::FRACTIONABLE,
    ]);
    let instruments = FixedInstruments;
    let configuration = config();
    let ports = ports(&ids, &mandates, &instruments, &configuration);
    let steps = case.steps.clone();
    let mut drive = Drive::new(case, &ports);
    let mut asserted = 0usize;
    for (index, step) in steps.iter().enumerate() {
        let before = drive.shell.state.clone();
        let seen = drive.step(index, step);
        let checks = [
            drive.assert_actions(index, step, &seen),
            drive.assert_orders(index, step),
            drive.assert_reconciliation(index, step, &before),
            drive.assert_protection(index, step),
        ];
        asserted = asserted.saturating_add(checks.iter().filter(|c| **c).count());
    }
    assert!(
        asserted > 0,
        "{} asserted nothing: every case this stream drives names at least one key it owns",
        drive.case.id
    );
}

#[test]
#[ignore = "pending E7-4"]
fn trading_domain_rc_14_equity_exit_sequence_with_protective_oco() {
    drive(case("RC-14", None));
}

#[test]
#[ignore = "pending E7-4"]
fn trading_domain_rc_14_passive_exit_becomes_oco_take_profit() {
    drive(case("RC-14", Some("passive_exit_becomes_oco_take_profit")));
}

#[test]
#[ignore = "pending E7-4"]
fn trading_domain_rc_14_add_via_bracket() {
    drive(case("RC-14", Some("add_via_bracket")));
}

#[test]
#[ignore = "pending E7-4"]
fn trading_domain_rc_14_kill_switch() {
    drive(case("RC-14", Some("kill_switch")));
}

#[test]
#[ignore = "pending E7-4"]
fn trading_domain_rc_04_split_cancels_the_oco_and_re_derives_protection() {
    drive(case("RC-04", None));
}

#[test]
#[ignore = "pending E7-4"]
fn trading_domain_rc_06_protective_orders_kept_through_dividend() {
    drive(case(
        "RC-06",
        Some("protective_orders_kept_through_dividend"),
    ));
}

#[test]
#[ignore = "pending E7-3"]
fn trading_domain_rc_07_unposted_crypto_fees_reconcile() {
    drive(case("RC-07", None));
}

#[test]
#[ignore = "pending E7-4"]
fn trading_domain_rc_20_crypto_stop_limit_add_and_exit_sequences() {
    drive(case("RC-20", None));
}

#[test]
#[ignore = "pending E7-4"]
fn trading_domain_rc_21_bracket_partly_filled_and_re_placed_before_expiry() {
    drive(case("RC-21", None));
}

#[test]
#[ignore = "pending E7-4"]
fn trading_domain_rc_22_resting_buys_cancelled_before_the_exit() {
    drive(case("RC-22", None));
}

#[test]
#[ignore = "pending E7-4"]
fn trading_domain_rc_24_exit_price_ladder_in_extended_hours() {
    drive(case("RC-24", None));
}

#[test]
#[ignore = "pending E7-4"]
fn trading_domain_rc_24_presumed_halt_regular_session() {
    drive(case("RC-24", Some("presumed_halt_regular_session")));
}

#[test]
#[ignore = "pending E7-3"]
fn trading_domain_rc_15_restriction_from_a_closing_only_reject() {
    drive(case("RC-15", None));
}

#[test]
#[ignore = "pending E7-3"]
fn trading_domain_rc_15_status_not_active() {
    drive(case("RC-15", Some("status_not_active")));
}

#[test]
#[ignore = "pending E7-3"]
fn trading_domain_rc_15_external_order_detected() {
    drive(case("RC-15", Some("external_order_detected")));
}

#[test]
#[ignore = "pending E7-3"]
fn trading_domain_rc_15_unexplained_403s() {
    drive(case("RC-15", Some("unexplained_403s")));
}
