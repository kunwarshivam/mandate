//! C2 tests (E7-6; connections spec CN-8 and §6.2 rule 6; DEC-875): the connector's reads reach
//! only the agentic account. Every read checks `get_accounts` first and calls its own tool only
//! when the list shows exactly one agentic account, under the recorded number, listed once; a read
//! whose arguments name another account is refused with nothing called; records naming another
//! account are dropped; a record naming none refuses the read; and none of it holds an exit on
//! the agentic account (`AGENTS.md` rule 13). Oracles: outcomes computed from the generated
//! account choices, and a recording tool double. The account fingerprint (CN-5) is not C2's.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use mandate_accounting::{InstrumentId, Side};
use mandate_executor::{
    BrokerConnector, BrokerOutcome, BrokerRequest, ClientOrderId, EventId, IntentId, OrderType,
    Purpose, SubmitOrder, TimeInForce,
};
use mandate_mcp::{CallClass, McpError};
use mandate_num::{Price, Qty};
use mandate_robinhood::{AccountRead, RobinhoodConnector, RobinhoodError, Tools};
use proptest::collection::vec;
use proptest::prelude::any;
use proptest::test_runner::{Config, TestRunner};
use serde_json::{Map, Value, json};

/// The recorded agentic account, and three other accounts of the same customer: no number is a
/// part of another, so finding one in an output means its data got through.
const NUMBERS: [&str; 4] = ["5QR00001", "9ZX00002", "9ZX00003", "9ZX00004"];
const OURS: &str = NUMBERS[0];
const OTHER: RobinhoodError = RobinhoodError::OtherAccount;
const READS: [AccountRead; 3] = [
    AccountRead::Portfolio,
    AccountRead::Positions,
    AccountRead::Orders,
];

type Calls = Rc<RefCell<Vec<(CallClass, &'static str, Value)>>>;

/// Answers each tool with its scripted tool result, or loses the answer when none is scripted.
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

fn connector(answers: &[(&'static str, Value)]) -> (RobinhoodConnector<Scripted>, Calls) {
    let calls = Calls::default();
    let answers = answers.iter().cloned().collect();
    let tools = Scripted {
        answers,
        calls: Rc::clone(&calls),
    };
    (RobinhoodConnector::new(tools, OURS.to_owned()), calls)
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

fn content(structured: Value) -> Value {
    json!({"structuredContent": structured, "isError": false})
}

/// `get_accounts`'s answer for `(account, agentic_allowed)` choices, each record tagged.
fn accounts(list: &[(usize, bool)]) -> Value {
    let records: Vec<Value> = list
        .iter()
        .enumerate()
        .map(|(at, (account, allowed))| account_record(at, *account, *allowed))
        .collect();
    content(json!({ "accounts": records }))
}

fn account_record(at: usize, account: usize, allowed: bool) -> Value {
    json!({"account_number": NUMBERS[account], "agentic_allowed": allowed, "tag": at})
}

/// The account check, as CN-8 and DEC-875 item 1 state it, counted over the generated choices.
fn expected_check(list: &[(usize, bool)]) -> Result<usize, RobinhoodError> {
    let agentic: Vec<usize> = (0..list.len()).filter(|at| list[*at].1).collect();
    let listed = list.iter().filter(|(account, _)| *account == 0).count();
    match (agentic.as_slice(), listed) {
        ([at], 1) if list[*at].0 == 0 => Ok(*at),
        (several, _) if several.len() > 1 => Err(RobinhoodError::AmbiguousAgenticAccount),
        (_, several) if several > 1 => Err(RobinhoodError::AmbiguousAgenticAccount),
        _ => Err(RobinhoodError::NoAgenticAccount),
    }
}

fn tool_of(read: AccountRead) -> &'static str {
    match read {
        AccountRead::Portfolio => "get_portfolio",
        AccountRead::Positions => "get_equity_positions",
        AccountRead::Orders => "get_equity_orders",
    }
}

/// The read's answer for generated records: one portfolio record, or a list of them.
fn read_answer(read: AccountRead, owners: &[usize]) -> Value {
    let record = |at: usize, account: usize| json!({"account_number": NUMBERS[account], "tag": at});
    let records: Vec<Value> = owners
        .iter()
        .enumerate()
        .map(|(at, a)| record(at, *a))
        .collect();
    match read {
        AccountRead::Portfolio => content(record(0, owners.first().copied().unwrap_or(0))),
        AccountRead::Positions => content(json!({ "positions": records })),
        AccountRead::Orders => content(json!({ "orders": records })),
    }
}

fn unreadable<T: std::fmt::Debug>(outcome: &Result<T, RobinhoodError>, why: &str) {
    let refused = matches!(outcome, Err(RobinhoodError::Unreadable { .. }));
    assert!(refused, "{why}: {outcome:?}");
}

fn object(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

#[test]
#[ignore = "pending E7-6"]
fn only_the_agentic_accounts_records_reach_the_caller() {
    let mut runner = TestRunner::new(Config::with_cases(512));
    let lists = vec((0usize..4, any::<bool>()), 0..4);
    let reads = (0usize..3, lists, vec(0usize..4, 1..6));
    let verdict = runner.run(&reads, |(read, list, owners)| {
        let read = READS[read];
        let script = [
            ("get_accounts", accounts(&list)),
            (tool_of(read), read_answer(read, &owners)),
        ];
        let (c, calls) = connector(&script);
        let outcome = ready(c.read(read, &Map::new()));
        let mut sent = vec![(CallClass::Ordinary, "get_accounts", json!({}))];
        let expected = expected_check(&list).map(|_| {
            sent.push((
                CallClass::Ordinary,
                tool_of(read),
                json!({"account_number": OURS}),
            ));
            let owners = match read {
                AccountRead::Portfolio => &owners[..1],
                AccountRead::Positions | AccountRead::Orders => &owners[..],
            };
            let ours = (0..owners.len()).filter(|at| owners[*at] == 0);
            ours.map(|at| object(json!({"account_number": OURS, "tag": at})))
                .collect::<Vec<_>>()
        });
        let leaked = NUMBERS[1..]
            .iter()
            .any(|n| format!("{outcome:?}").contains(n));
        proptest::prop_assert!(!leaked, "another account's data got through: {outcome:?}");
        proptest::prop_assert_eq!(outcome, expected);
        proptest::prop_assert_eq!(&*calls.borrow(), &sent);
        Ok(())
    });
    verdict.unwrap();
}

#[test]
#[ignore = "pending E7-6"]
fn zero_or_several_agentic_accounts_fail_closed_with_no_guess() {
    let none = RobinhoodError::NoAgenticAccount;
    let two = RobinhoodError::AmbiguousAgenticAccount;
    let cases = [
        (vec![], &none, "an empty list"),
        (vec![(0, false)], &none, "ours is not agentic"),
        (
            vec![(0, false), (1, true)],
            &none,
            "another's is never adopted",
        ),
        (vec![(1, true)], &none, "ours is not listed"),
        (vec![(0, true), (1, true)], &two, "two agentic accounts"),
        (vec![(0, true), (0, true)], &two, "ours listed twice"),
        (vec![(0, true), (0, false)], &two, "ours read two ways"),
    ];
    for (list, error, why) in cases {
        let positions = read_answer(AccountRead::Positions, &[0, 1]);
        let (c, calls) = connector(&[
            ("get_accounts", accounts(&list)),
            ("get_equity_positions", positions),
        ]);
        assert_eq!(ready(c.agentic_account()), Err(error.clone()), "{why}");
        let read = ready(c.read(AccountRead::Positions, &Map::new()));
        assert_eq!(read, Err(error.clone()), "{why}");
        let tools: Vec<&str> = calls.borrow().iter().map(|(_, tool, _)| *tool).collect();
        assert_eq!(tools, ["get_accounts"; 2], "{why}: no position was read");
    }
}

#[test]
#[ignore = "pending E7-6"]
fn a_read_naming_another_account_is_refused_with_nothing_called() {
    let list = [(0, true), (1, false)];
    let orders = read_answer(AccountRead::Orders, &[0]);
    let (c, calls) = connector(&[
        ("get_accounts", accounts(&list)),
        ("get_equity_orders", orders),
    ]);
    for read in READS {
        for named in [
            json!(NUMBERS[1]),
            json!(5),
            Value::Null,
            json!(""),
            json!([OURS]),
        ] {
            let filters = object(json!({"account_number": named, "symbol": "SPY"}));
            let refused = ready(c.read(read, &filters));
            assert_eq!(refused, Err(OTHER), "{read:?} {named}");
        }
    }
    assert!(calls.borrow().is_empty(), "{:?}", calls.borrow());
    let filters = object(json!({"account_number": OURS, "symbol": "SPY"}));
    let read = ready(c.read(AccountRead::Orders, &filters));
    let ours = object(json!({"account_number": OURS, "tag": 0}));
    assert_eq!(read, Ok(vec![ours]));
    let last = calls.borrow().last().cloned();
    let sent = (CallClass::Ordinary, "get_equity_orders", filters.into());
    assert_eq!(last, Some(sent), "the recorded account, named once");
}

#[test]
#[ignore = "pending E7-6"]
fn an_answer_that_cannot_be_attributed_refuses_the_whole_read() {
    let ours = json!({"account_number": OURS, "tag": 0});
    let list = |records: Value| content(json!({ "positions": records }));
    let unattributed = [
        list(json!([ours, {"tag": 1}])),
        list(json!([ours, {"account_number": 5, "tag": 1}])),
        list(json!([ours, {"account_number": null, "tag": 1}])),
        list(json!([ours, "9ZX00002"])),
        list(json!({"account_number": OURS})),
        content(json!({"orders": [ours]})),
        json!({"structuredContent": {"positions": [ours]}, "isError": true}),
        json!({"content": []}),
    ];
    for answer in unattributed {
        let (c, _) = connector(&[
            ("get_accounts", accounts(&[(0, true)])),
            ("get_equity_positions", answer.clone()),
        ]);
        let read = ready(c.read(AccountRead::Positions, &Map::new()));
        unreadable(&read, &answer.to_string());
    }
    let lists = [
        content(json!({"accounts": [{"account_number": OURS}]})),
        content(json!({"accounts": [{"account_number": OURS, "agentic_allowed": true}, {}]})),
        content(json!({"accounts": [{"account_number": OURS, "agentic_allowed": "true"}]})),
    ];
    for answer in lists {
        let (c, calls) = connector(&[("get_accounts", answer.clone())]);
        let check = ready(c.agentic_account());
        unreadable(&check, &answer.to_string());
        let read = ready(c.read(AccountRead::Positions, &Map::new()));
        unreadable(&read, &answer.to_string());
        assert_eq!(calls.borrow().len(), 2, "{answer}: no position was read");
    }
    let (c, _) = connector(&[]);
    let lost = ready(c.read(AccountRead::Positions, &Map::new()));
    unreadable(&lost, "a lost answer");
}

/// `AGENTS.md` rule 13: a failed account check and a refused read hold no exit, protective order
/// or cancel on the agentic account, each sent on the reserved budget and naming that account.
#[test]
#[ignore = "pending E7-6"]
fn a_failed_account_check_holds_no_exit_on_the_agentic_account() {
    let order = json!({"id": "rh-1", "state": "confirmed", "quantity": "2",
        "filled_quantity": "0", "limit_price": "475", "stop_price": null});
    let mut cancelled = order.clone();
    cancelled["state"] = json!("cancelled");
    let script = [
        ("get_accounts", accounts(&[(0, true), (1, true)])),
        ("review_equity_order", content(json!({"alerts": ["halt"]}))),
        ("place_equity_order", content(order)),
        ("cancel_equity_order", content(cancelled)),
    ];
    let (mut c, calls) = connector(&script);
    let check = ready(c.read(AccountRead::Positions, &Map::new()));
    assert_eq!(check, Err(RobinhoodError::AmbiguousAgenticAccount));
    let named = object(json!({"account_number": NUMBERS[1]}));
    let other = ready(c.read(AccountRead::Orders, &named));
    assert_eq!(other, Err(OTHER));
    calls.borrow_mut().clear();
    let (stop_limit, limit) = (OrderType::StopLimit, OrderType::Limit);
    let exits = [
        (
            "01JPROTECT",
            Purpose::Protective,
            stop_limit,
            TimeInForce::Gtc,
        ),
        ("01JRISKEXIT", Purpose::RiskExit, limit, TimeInForce::Day),
        ("01JFLATTEN", Purpose::Flatten, limit, TimeInForce::Day),
    ];
    for (intent, purpose, order_type, tif) in exits {
        let key = ClientOrderId::for_intent(&IntentId(EventId(intent.to_owned()))).unwrap();
        let stop = (order_type == OrderType::StopLimit).then(|| Price::parse("480").unwrap());
        let exit = SubmitOrder {
            client_order_id: key.clone(),
            instrument: InstrumentId::new("SPY").unwrap(),
            side: Side::Sell,
            qty: Qty::parse("2").unwrap(),
            order_type,
            tif,
            limit_price: Some(Price::parse("475").unwrap()),
            stop_price: stop,
            bracket: None,
            oco: None,
            extended_hours: false,
            purpose,
        };
        let sent = ready(c.call(&BrokerRequest::Submit(exit)));
        let submitted = matches!(sent, Ok(BrokerOutcome::Submitted(_)));
        assert!(submitted, "{purpose:?}: {sent:?}");
        let cancel = BrokerRequest::Cancel {
            client_order_id: key.clone(),
        };
        let cancelled = ready(c.call(&cancel));
        let accepted = BrokerOutcome::CancelAccepted {
            client_order_id: key.as_str().to_owned(),
        };
        assert_eq!(cancelled, Ok(accepted), "{purpose:?}");
    }
    let calls = calls.borrow();
    let tools: Vec<&str> = calls.iter().map(|(_, tool, _)| *tool).collect();
    let one = [
        "review_equity_order",
        "place_equity_order",
        "cancel_equity_order",
    ];
    assert_eq!(tools, one.repeat(3), "no read stands before an exit");
    for (class, tool, arguments) in calls.iter() {
        assert_eq!(*class, CallClass::RiskReducing, "{tool}");
        assert_eq!(arguments["account_number"], json!(OURS), "{tool}");
    }
}
