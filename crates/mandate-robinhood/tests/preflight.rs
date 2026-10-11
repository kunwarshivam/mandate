//! C4 tests (E7-6; DEC-470 item 1, DEC-875 item 6, DEC-902): the preflight facts and the account
//! snapshot from contract-shaped records. Cash, buying power, every position record and every
//! working order are mapped as the broker wrote them, each money and quantity figure as the exact
//! decimal text it is, and each working order with the side and the id its record names; a field
//! missing, a field the assumed shape does not list, a value of another type, a number that is
//! not canonical decimal text (trailing zeroes included), or an order record with an empty id
//! refuses the whole read (`AGENTS.md` rule 3); the quote and the tradability are the one
//! symbol's; and every account read goes through the scoped `read`. Oracles: the expected values
//! written out from the fixtures' literals, the assumed shapes' keys and the working states
//! listed here, and a recording tool double.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use mandate_accounting::{InstrumentId, Side};
use mandate_executor::{BrokerOrder, BrokerPosition};
use mandate_mcp::{CallClass, McpError};
use mandate_num::{Price, Qty, SignedQty, Usd};
use mandate_robinhood::{
    AccountSnapshot, PreflightFacts, Quote, RobinhoodConnector, RobinhoodError, Tools,
};
use proptest::collection::vec;
use proptest::prelude::{Just, Strategy, any, prop_oneof};
use proptest::test_runner::{Config, TestRunner};
use serde_json::{Value, json};

const OURS: &str = "5QR00001";
const THEIRS: &str = "9ZX00002";
const SYMBOL: &str = "VTI";
const ACCOUNT_READS: [&str; 4] = [
    "get_portfolio",
    "get_equity_positions",
    "get_equity_orders",
    "get_equity_tradability",
];
const STATES: [&str; 10] = [
    "new",
    "queued",
    "confirmed",
    "unconfirmed",
    "partially_filled",
    "filled",
    "cancelled",
    "rejected",
    "failed",
    "voided",
];
/// The working states and the status each maps to (connections spec §6.2, DEC-860 item 3).
const WORKING: [(&str, &str); 5] = [
    ("new", "accepted"),
    ("queued", "accepted"),
    ("confirmed", "accepted"),
    ("unconfirmed", "accepted"),
    ("partially_filled", "partially_filled"),
];

type Calls = Rc<RefCell<Vec<(CallClass, &'static str, Value)>>>;

struct Scripted {
    answers: BTreeMap<&'static str, Value>,
    calls: Calls,
}

impl Tools for Scripted {
    async fn call_tool(
        &self,
        class: CallClass,
        tool: &'static str,
        arguments: &Value,
    ) -> Result<String, McpError> {
        self.calls
            .borrow_mut()
            .push((class, tool, arguments.clone()));
        let answer = self.answers.get(tool).ok_or(McpError::Network)?;
        Ok(answer.to_string())
    }
}

fn ready<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("the connector awaited something other than the seam"),
    }
}

/// The five answers as `structuredContent`, before they are wrapped as tool results.
#[derive(Clone)]
struct Answers {
    accounts: Value,
    portfolio: Value,
    positions: Value,
    orders: Value,
    tradability: Value,
    quotes: Value,
}

impl Answers {
    fn flat() -> Self {
        Self {
            accounts: json!({"accounts": [
                {"account_number": OURS, "agentic_allowed": true},
                {"account_number": THEIRS, "agentic_allowed": false}]}),
            portfolio: json!({"account_number": OURS, "cash": "250.75", "buying_power": "1000"}),
            positions: json!({"positions": []}),
            orders: json!({"orders": []}),
            tradability: json!({"account_number": OURS, "symbol": SYMBOL, "tradable": true}),
            quotes: json!({"quotes": [{"symbol": SYMBOL, "bid_price": "85.1", "ask_price": "85.13"}]}),
        }
    }

    fn connector(&self) -> (RobinhoodConnector<Scripted>, Calls) {
        let wrap = |content: &Value| json!({"structuredContent": content, "isError": false});
        let answers = [
            ("get_accounts", &self.accounts),
            ("get_portfolio", &self.portfolio),
            ("get_equity_positions", &self.positions),
            ("get_equity_orders", &self.orders),
            ("get_equity_tradability", &self.tradability),
            ("get_equity_quotes", &self.quotes),
        ];
        let calls = Calls::default();
        let tools = Scripted {
            answers: answers.iter().map(|(t, c)| (*t, wrap(c))).collect(),
            calls: Rc::clone(&calls),
        };
        (RobinhoodConnector::new(tools, OURS.to_owned()), calls)
    }

    fn snapshot(&self) -> Result<AccountSnapshot, RobinhoodError> {
        ready(self.connector().0.account_snapshot())
    }

    fn preflight(&self) -> Result<PreflightFacts, RobinhoodError> {
        ready(self.connector().0.preflight_facts(&id(SYMBOL)))
    }
}

fn position(account: &str, symbol: &str, quantity: &str, average: &str) -> Value {
    json!({"account_number": account, "symbol": symbol, "quantity": quantity,
        "average_buy_price": average})
}

/// An order record, of the side its text names; its `type` follows the prices it carries.
fn order(
    account: &str,
    side: &str,
    at: &str,
    state: &str,
    prices: (Option<&str>, Option<&str>),
) -> Value {
    let kind = match prices {
        (None, None) => "market",
        (Some(_), None) => "limit",
        (None, Some(_)) => "stop_market",
        (Some(_), Some(_)) => "stop_limit",
    };
    json!({"id": at, "account_number": account, "symbol": SYMBOL, "side": side, "type": kind,
        "quantity": "2", "limit_price": prices.0, "stop_price": prices.1, "time_in_force": "gtc",
        "market_hours": "regular_hours", "state": state, "filled_quantity": "0.5"})
}

fn id(symbol: &str) -> InstrumentId {
    InstrumentId::new(symbol).unwrap()
}

fn held(symbol: &str, quantity: &str, average: &str) -> BrokerPosition {
    BrokerPosition {
        instrument: id(symbol),
        qty: SignedQty::parse(quantity).unwrap(),
        avg_entry_price: Price::parse(average).unwrap(),
    }
}

fn working(
    at: &str,
    side: Side,
    status: &str,
    prices: (Option<&str>, Option<&str>),
) -> BrokerOrder {
    BrokerOrder {
        broker_order_id: at.to_owned(),
        client_order_id: None,
        instrument: id(SYMBOL),
        side,
        qty: Qty::parse("2").unwrap(),
        filled_qty: Qty::parse("0.5").unwrap(),
        limit_price: prices.0.map(|p| Price::parse(p).unwrap()),
        stop_price: prices.1.map(|p| Price::parse(p).unwrap()),
        status: status.to_owned(),
        reject_code: None,
        replaced_by_broker_order_id: None,
        legs: Vec::new(),
        created_on: None,
    }
}

fn usd(text: &str) -> Usd {
    Usd::parse(text).unwrap()
}

fn unreadable<T: std::fmt::Debug>(outcome: &Result<T, RobinhoodError>, code: &str, why: &str) {
    let refused = matches!(outcome, Err(RobinhoodError::Unreadable { code: c }) if *c == code);
    assert!(
        refused,
        "{why}: expected Unreadable({code}), got {outcome:?}"
    );
}

#[test]
#[ignore = "pending E7-6"]
fn the_snapshot_maps_cash_buying_power_positions_and_working_orders() {
    let mut answers = Answers::flat();
    answers.positions = json!({"positions": [position(OURS, SYMBOL, "3", "85.12"),
        position(THEIRS, SYMBOL, "9", "1"), position(OURS, "QQQ", "0", "400.5")]});
    let limit = (Some("84.5"), None);
    let stop = (Some("80"), Some("81"));
    answers.orders = json!({"orders": [order(OURS, "buy", "o1", "new", limit),
        order(OURS, "buy", "o2", "partially_filled", stop), order(OURS, "buy", "o3", "filled", limit),
        order(THEIRS, "buy", "o4", "new", limit), order(OURS, "buy", "o5", "unconfirmed", (None, None))]});
    let expected = AccountSnapshot {
        cash: usd("250.75"),
        buying_power: usd("1000"),
        positions: vec![held(SYMBOL, "3", "85.12"), held("QQQ", "0", "400.5")],
        open_orders: vec![
            working("o1", Side::Buy, "accepted", limit),
            working("o2", Side::Buy, "partially_filled", stop),
            working("o5", Side::Buy, "accepted", (None, None)),
        ],
    };
    assert_eq!(answers.snapshot().unwrap(), expected);
}

#[test]
#[ignore = "pending E7-6"]
fn only_the_working_states_are_open_orders_and_a_flat_account_maps_to_none() {
    for state in STATES {
        let mut answers = Answers::flat();
        answers.orders = json!({"orders": [order(OURS, "buy", "o1", state, (Some("84.5"), None))]});
        let open = answers.snapshot().unwrap().open_orders;
        let status = WORKING.iter().find(|(s, _)| *s == state).map(|(_, m)| *m);
        let expected: Vec<BrokerOrder> = status
            .map(|m| working("o1", Side::Buy, m, (Some("84.5"), None)))
            .into_iter()
            .collect();
        assert_eq!(open, expected, "state {state}");
    }
    let flat = Answers::flat().snapshot().unwrap();
    assert!(
        flat.positions.is_empty() && flat.open_orders.is_empty(),
        "{flat:?}"
    );
}

/// DEC-902 item 4 / `BrokerOrder`'s side: the connector maps the record's `side` text (`buy`,
/// `sell`) to the executor's own two sides, as `mandate-alpaca`'s wire mapping does: `"sell"` to
/// `Side::Sell` and `"buy"` to `Side::Buy`.
#[test]
#[ignore = "pending E7-6"]
fn sell_orders_map_to_sell_and_buy_orders_to_buy() {
    let mut answers = Answers::flat();
    answers.orders = json!({"orders": [order(OURS, "sell", "o1", "new", (Some("84.5"), None)),
        order(OURS, "buy", "o2", "partially_filled", (None, None))]});
    let open = answers.snapshot().unwrap().open_orders;
    assert_eq!(
        open,
        vec![
            working("o1", Side::Sell, "accepted", (Some("84.5"), None)),
            working("o2", Side::Buy, "partially_filled", (None, None)),
        ],
        "the record's side text is the order's side, sell and buy both"
    );
}

/// Each read's code, the member holding its record (none for a one-record answer), and the
/// record that the base fixture carries there.
fn targets() -> [(&'static str, Option<&'static str>, Value); 5] {
    [
        ("portfolio", None, Answers::flat().portfolio),
        (
            "positions",
            Some("positions"),
            position(OURS, SYMBOL, "3", "85.12"),
        ),
        (
            "orders",
            Some("orders"),
            order(OURS, "buy", "o1", "new", (Some("84.5"), None)),
        ),
        ("tradability", None, Answers::flat().tradability),
        (
            "quotes",
            Some("quotes"),
            Answers::flat().quotes["quotes"][0].clone(),
        ),
    ]
}

/// The fixture with `record` in the read named by `code`.
fn with_record(code: &str, record: Value) -> Answers {
    let mut answers = Answers::flat();
    let slot = match code {
        "portfolio" => &mut answers.portfolio,
        "positions" => &mut answers.positions,
        "orders" => &mut answers.orders,
        "tradability" => &mut answers.tradability,
        _ => &mut answers.quotes,
    };
    *slot = match targets()
        .into_iter()
        .find(|t| t.0 == code)
        .and_then(|t| t.1)
    {
        Some(member) => json!({ member: [record] }),
        None => record,
    };
    answers
}

#[test]
#[ignore = "pending E7-6"]
fn any_unknown_or_missing_field_or_unexpected_type_refuses() {
    for (code, member, record) in targets() {
        let fields = record.as_object().unwrap().clone();
        let intact = with_record(code, record.clone()).preflight();
        assert!(intact.is_ok(), "{code}: the base record reads: {intact:?}");
        for key in fields.keys() {
            let mut missing = fields.clone();
            missing.remove(key);
            let outcome = with_record(code, Value::Object(missing)).preflight();
            unreadable(&outcome, code, &format!("{code} without {key}"));
            for odd in [json!(1.5), json!(["x"]), json!({"v": "1"})] {
                let mut typed = fields.clone();
                typed.insert(key.clone(), odd.clone());
                let outcome = with_record(code, Value::Object(typed)).preflight();
                unreadable(&outcome, code, &format!("{code}.{key} = {odd}"));
            }
        }
        let mut extra = fields.clone();
        extra.insert("margin_enabled".to_owned(), json!(false));
        let outcome = with_record(code, Value::Object(extra)).preflight();
        unreadable(
            &outcome,
            code,
            &format!("{code} with a field it does not list"),
        );
        if let Some(member) = member {
            let mut answers = with_record(code, record.clone());
            let slot = match code {
                "positions" => &mut answers.positions,
                "orders" => &mut answers.orders,
                _ => &mut answers.quotes,
            };
            *slot = json!({ member: [record.clone()], "next": "c2" });
            unreadable(
                &answers.preflight(),
                code,
                &format!("{code} with a next page"),
            );
        }
    }
    let odd_values = [
        ("portfolio", "cash", json!("250.70")),
        ("portfolio", "cash", json!("-1")),
        ("portfolio", "buying_power", json!("-0.01")),
        ("portfolio", "buying_power", json!("1e3")),
        ("portfolio", "buying_power", json!("1000.0")),
        ("positions", "symbol", json!("")),
        ("positions", "average_buy_price", json!("0")),
        ("positions", "quantity", json!("3.0")),
        ("positions", "average_buy_price", json!("85.120")),
        ("orders", "state", json!("in_doubt")),
        ("orders", "side", json!("short")),
        ("orders", "type", json!("trailing_stop")),
        ("orders", "time_in_force", json!("ioc")),
        ("orders", "market_hours", json!("overnight")),
        ("orders", "quantity", json!("2.0")),
        ("orders", "limit_price", json!("84.50")),
        ("orders", "stop_price", json!("81.00")),
        ("orders", "filled_quantity", json!("0.50")),
        ("orders", "ref_id", json!(7)),
        ("tradability", "tradable", json!("true")),
        ("quotes", "bid_price", json!("85.10")),
        ("quotes", "ask_price", json!("85.130")),
    ];
    for (code, key, value) in odd_values {
        let (_, _, mut record) = targets().into_iter().find(|t| t.0 == code).unwrap();
        record[key] = value.clone();
        let outcome = with_record(code, record).preflight();
        unreadable(&outcome, code, &format!("{code}.{key} = {value}"));
    }
}

/// DEC-902 item 4's `id` (non-empty text), read fail-closed like the other rejects: a record
/// with no id cannot be named, re-read or cancelled, so the whole orders read refuses rather
/// than carry an order the executor could never refer to again.
#[test]
#[ignore = "pending E7-6"]
fn an_order_record_with_an_empty_id_refuses() {
    let record = order(OURS, "buy", "", "new", (Some("84.5"), None));
    let outcome = with_record("orders", record).snapshot();
    unreadable(&outcome, "orders", "an order record with an empty id");
}

#[test]
#[ignore = "pending E7-6"]
fn the_quote_and_the_tradability_are_the_one_symbols() {
    let quote =
        |bid: &str, ask: &str| json!({"symbol": SYMBOL, "bid_price": bid, "ask_price": ask});
    let facts = Answers::flat().preflight().unwrap();
    let expected = Quote {
        symbol: id(SYMBOL),
        bid: Price::parse("85.1").unwrap(),
        ask: Price::parse("85.13").unwrap(),
    };
    assert_eq!(facts.quote, expected);
    assert_eq!(facts.account, Answers::flat().snapshot().unwrap());
    let locked = with_record("quotes", quote("85.1", "85.1"))
        .preflight()
        .unwrap();
    assert_eq!(
        locked.quote.bid, locked.quote.ask,
        "a locked quote is uncrossed"
    );
    let crossed = with_record("quotes", quote("85.2", "85.1")).preflight();
    unreadable(&crossed, "quotes", "a crossed quote");
    let other = json!({"symbol": "VT", "bid_price": "1", "ask_price": "2"});
    let mut answers = Answers::flat();
    for quotes in [
        json!([]),
        json!([other]),
        json!([quote("1", "2"), quote("1", "2")]),
    ] {
        answers.quotes = json!({ "quotes": quotes });
        unreadable(&answers.preflight(), "quotes", &format!("quotes {quotes}"));
    }
    let mut answers = Answers::flat();
    answers.tradability["tradable"] = json!(false);
    assert_eq!(answers.preflight(), Err(RobinhoodError::NotTradable));
    answers.tradability = json!({"account_number": OURS, "symbol": "VT", "tradable": true});
    unreadable(
        &answers.preflight(),
        "tradability",
        "another symbol's tradability",
    );
    answers.tradability = json!({"account_number": THEIRS, "symbol": SYMBOL, "tradable": true});
    unreadable(
        &answers.preflight(),
        "tradability",
        "only another account's tradability",
    );
}

#[test]
#[ignore = "pending E7-6"]
fn every_account_read_goes_through_the_scoped_read() {
    let (connector, calls) = Answers::flat().connector();
    ready(connector.preflight_facts(&id(SYMBOL))).unwrap();
    let calls = calls.borrow();
    let mut checked = false;
    let mut read: Vec<&str> = Vec::new();
    for (class, tool, arguments) in calls.iter() {
        assert_eq!(
            *class,
            CallClass::Ordinary,
            "{tool} draws on the ordinary budget"
        );
        match *tool {
            "get_accounts" => checked = true,
            "get_equity_quotes" => assert_eq!(arguments, &json!({"symbols": [SYMBOL]})),
            scoped if ACCOUNT_READS.contains(&scoped) => {
                assert!(checked, "{scoped} ran without a fresh account check");
                checked = false;
                let mut named = json!({"account_number": OURS});
                if scoped == "get_equity_tradability" {
                    named["symbol"] = json!(SYMBOL);
                }
                assert_eq!(arguments, &named, "{scoped}'s arguments");
                read.push(scoped);
            }
            other => panic!("{other} is not a preflight read"),
        }
    }
    read.sort_unstable();
    let mut expected = ACCOUNT_READS.to_vec();
    expected.sort_unstable();
    assert_eq!(read, expected, "each account read once");
    let refusals = [
        (json!([]), RobinhoodError::NoAgenticAccount),
        (
            json!([{"account_number": OURS, "agentic_allowed": true},
                {"account_number": THEIRS, "agentic_allowed": true}]),
            RobinhoodError::AmbiguousAgenticAccount,
        ),
    ];
    for (list, refusal) in refusals {
        let mut answers = Answers::flat();
        answers.accounts = json!({ "accounts": list });
        let (connector, calls) = answers.connector();
        assert_eq!(
            ready(connector.preflight_facts(&id(SYMBOL))),
            Err(refusal.clone())
        );
        assert_eq!(ready(connector.account_snapshot()), Err(refusal));
        let reached = calls.borrow().iter().any(|c| ACCOUNT_READS.contains(&c.1));
        assert!(!reached, "an account read went out without a passed check");
    }
}

#[test]
#[ignore = "pending E7-6"]
fn no_number_is_invented() {
    let mut answers = Answers::flat();
    answers.orders = json!({"orders": [order(OURS, "buy", "o1", "new", (None, None))]});
    let open = answers.snapshot().unwrap().open_orders;
    assert_eq!(
        open,
        vec![working("o1", Side::Buy, "accepted", (None, None))],
        "null stays none"
    );
    for key in ["limit_price", "stop_price", "filled_quantity"] {
        let mut record = order(OURS, "buy", "o1", "new", (Some("84.5"), None));
        record.as_object_mut().unwrap().remove(key);
        let outcome = with_record("orders", record).snapshot();
        unreadable(
            &outcome,
            "orders",
            &format!("an order without {key} is not defaulted"),
        );
    }
    let mut zero = Answers::flat();
    zero.portfolio["cash"] = json!("0");
    zero.positions = json!({"positions": [position(OURS, SYMBOL, "0", "85")]});
    let snapshot = zero.snapshot().unwrap();
    assert_eq!(snapshot.cash, Usd::ZERO);
    assert_eq!(
        snapshot.positions,
        vec![held(SYMBOL, "0", "85")],
        "a zero row is kept"
    );
}

#[test]
#[ignore = "pending E7-6"]
fn money_is_exact_decimal_text_and_a_json_number_refuses() {
    let mut answers = Answers::flat();
    answers.portfolio["cash"] = json!("1234567890123.123456789");
    answers.portfolio["buying_power"] = json!("0.000000001");
    answers.positions = json!({"positions": [position(OURS, SYMBOL,
        "123456789.123456789", "99999999.999999999")]});
    let prices = (Some("0.123456789"), Some("0.987654321"));
    let mut record = order(OURS, "buy", "o1", "new", prices);
    record["quantity"] = json!("123456789.123456789");
    record["filled_quantity"] = json!("0.135792468");
    answers.orders = json!({"orders": [record]});
    answers.quotes = json!({"quotes": [{"symbol": SYMBOL, "bid_price": "0.000000002",
        "ask_price": "12345678.123456789"}]});
    let snapshot = answers.snapshot().unwrap();
    assert_eq!(snapshot.cash.to_string(), "1234567890123.123456789");
    assert_eq!(snapshot.buying_power.to_string(), "0.000000001");
    assert_eq!(
        snapshot.positions,
        vec![held(SYMBOL, "123456789.123456789", "99999999.999999999")],
        "a position's fields are the exact values their text names"
    );
    let mut expected = working("o1", Side::Buy, "accepted", prices);
    expected.qty = Qty::parse("123456789.123456789").unwrap();
    expected.filled_qty = Qty::parse("0.135792468").unwrap();
    assert_eq!(
        snapshot.open_orders,
        vec![expected],
        "an order's every money and quantity field is the exact value its text names"
    );
    let facts = answers.preflight().unwrap();
    assert_eq!(
        facts.quote,
        Quote {
            symbol: id(SYMBOL),
            bid: Price::parse("0.000000002").unwrap(),
            ask: Price::parse("12345678.123456789").unwrap(),
        },
        "the quote's prices are the exact values their text names"
    );
    let numbers = [
        ("portfolio", "cash", json!(250.75)),
        ("portfolio", "buying_power", json!(1000)),
        ("positions", "quantity", json!(3)),
        ("positions", "average_buy_price", json!(85.12)),
        ("orders", "limit_price", json!(84.5)),
        ("orders", "stop_price", json!(81)),
        ("orders", "filled_quantity", json!(0.5)),
        ("quotes", "bid_price", json!(85.1)),
        ("quotes", "ask_price", json!(85.13)),
    ];
    for (code, key, number) in numbers {
        let (_, _, mut record) = targets().into_iter().find(|t| t.0 == code).unwrap();
        record[key] = number.clone();
        let outcome = with_record(code, record).preflight();
        unreadable(
            &outcome,
            code,
            &format!("{code}.{key} as the JSON number {number}"),
        );
    }
}

/// A generated record: ours or another account's, its fields, and a defect to plant in it.
#[derive(Debug, Clone)]
struct Drawn {
    ours: bool,
    fields: Value,
    defect: Option<(usize, u8)>,
}

/// `units` hundredths (or thousandths, ...) written as canonical decimal text.
fn decimal(units: u64, places: usize) -> String {
    let scale = 10u64.pow(u32::try_from(places).unwrap());
    let text = format!("{}.{:0places$}", units / scale, units % scale);
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn drawn(fields: impl Strategy<Value = Value>) -> impl Strategy<Value = Drawn> {
    let defect = proptest::option::weighted(0.1, (0usize..16, 0u8..3));
    (any::<bool>(), fields, defect).prop_map(|(ours, fields, defect)| Drawn {
        ours,
        fields,
        defect,
    })
}

fn position_fields() -> impl Strategy<Value = Value> {
    let symbol = prop_oneof![Just("VTI"), Just("QQQ"), Just("SPY")];
    (symbol, 0u64..5_000_000, 1u64..10_000_000)
        .prop_map(|(s, q, p)| position(OURS, s, &decimal(q, 3), &decimal(p, 4)))
}

fn order_fields() -> impl Strategy<Value = Value> {
    let limit = proptest::option::of(1u64..1_000_000);
    (0usize..10, 0u32..1000, any::<bool>(), limit).prop_map(|(state, at, sell, limit)| {
        let limit = limit.map(|l| decimal(l, 2));
        let side = if sell { "sell" } else { "buy" };
        order(
            OURS,
            side,
            &format!("o{at}"),
            STATES[state],
            (limit.as_deref(), None),
        )
    })
}

/// Plants `defect` in a record of ours: a key removed, an unlisted key added, or a key's value
/// made a JSON number. Returns whether it planted one.
fn plant(record: &mut Drawn) -> bool {
    let (Some((key, kind)), true) = (record.defect, record.ours) else {
        return false;
    };
    let fields = record.fields.as_object_mut().unwrap();
    let keys: Vec<String> = fields.keys().cloned().collect();
    let key = keys[key % keys.len()].clone();
    match kind {
        0 => {
            fields.remove(&key);
        }
        1 => {
            fields.insert("cost_basis".to_owned(), json!("1"));
        }
        _ => {
            fields.insert(key, json!(1.5));
        }
    }
    true
}

fn owned(mut record: Drawn) -> (Value, bool) {
    let planted = plant(&mut record);
    if !record.ours {
        record.fields["account_number"] = json!(THEIRS);
    }
    (record.fields, planted)
}

#[test]
#[ignore = "pending E7-6"]
fn the_snapshot_matches_an_independent_oracle_over_random_records() {
    let mut runner = TestRunner::new(Config::with_cases(256));
    let records = (
        (0u64..10_000_000_000, 0u64..10_000_000_000),
        vec(drawn(position_fields()), 0..5),
        vec(drawn(order_fields()), 0..6),
    );
    let verdict = runner.run(&records, |((cash, power), positions, orders)| {
        let mut answers = Answers::flat();
        answers.portfolio["cash"] = json!(decimal(cash, 2));
        answers.portfolio["buying_power"] = json!(decimal(power, 2));
        let positions: Vec<(Value, bool)> = positions.into_iter().map(owned).collect();
        let orders: Vec<(Value, bool)> = orders.into_iter().map(owned).collect();
        answers.positions =
            json!({"positions": positions.iter().map(|p| &p.0).collect::<Vec<_>>()});
        answers.orders = json!({"orders": orders.iter().map(|o| &o.0).collect::<Vec<_>>()});
        let ours = |r: &&(Value, bool)| r.0["account_number"] == json!(OURS);
        let planted: Vec<&str> = [("positions", &positions), ("orders", &orders)]
            .into_iter()
            .filter(|(_, records)| records.iter().any(|r| r.1))
            .map(|(code, _)| code)
            .collect();
        let outcome = answers.snapshot();
        if !planted.is_empty() {
            let refused = matches!(&outcome,
                Err(RobinhoodError::Unreadable { code }) if planted.contains(code));
            proptest::prop_assert!(refused, "planted in {planted:?}, got {outcome:?}");
            return Ok(());
        }
        let text = |r: &Value, k: &str| r[k].as_str().unwrap().to_owned();
        let open = orders.iter().filter(ours).filter_map(|(r, _)| {
            let state = text(r, "state");
            let status = WORKING.iter().find(|(s, _)| *s == state)?.1;
            let side = if text(r, "side") == "sell" {
                Side::Sell
            } else {
                Side::Buy
            };
            Some(working(
                &text(r, "id"),
                side,
                status,
                (r["limit_price"].as_str(), None),
            ))
        });
        let held = positions.iter().filter(ours).map(|(r, _)| {
            held(
                &text(r, "symbol"),
                &text(r, "quantity"),
                &text(r, "average_buy_price"),
            )
        });
        let expected = AccountSnapshot {
            cash: usd(&decimal(cash, 2)),
            buying_power: usd(&decimal(power, 2)),
            positions: held.collect(),
            open_orders: open.collect(),
        };
        proptest::prop_assert_eq!(outcome, Ok(expected));
        Ok(())
    });
    verdict.unwrap();
}
