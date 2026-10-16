//! The rehearsal script (DEC-922 item 4, DEC-926): the accounts, the session, the quotes and the
//! scripted fills, read strictly. A member the script does not know, a number written as a JSON
//! number rather than decimal text, or a value outside its enumeration refuses the whole script,
//! so a typo can never become a silent default. Oracles: the core built and driven directly.

mod common;

use common::{AGENTIC, limit, price, qty};
use mandate_rh_sim::{Script, ScriptError, Session, Side, Sim, State};
use serde_json::{Value, json};

fn base() -> Value {
    json!({
        "accounts": [{"number": AGENTIC, "agentic_allowed": true, "cash": "750.25",
            "buying_power": "10000", "pattern_day_trader": false}],
        "session": "regular",
        "quotes": {"SPY": "500"},
        "fills": [
            {"symbol": "SPY", "side": "buy", "price": "499.5", "quantity": "1"},
            {"symbol": "SPY", "side": "sell", "price": "501"},
        ],
    })
}

fn parsed(script: &Value) -> Result<Script, ScriptError> {
    Script::parse(&script.to_string())
}

#[test]
fn a_script_builds_the_core_it_describes() {
    let script = parsed(&base()).unwrap();
    let sim = script.sim().unwrap();
    assert_eq!(sim.accounts().len(), 1);
    assert_eq!(sim.accounts()[0].number, AGENTIC);
    assert_eq!(sim.accounts()[0].cash.to_string(), "750.25");
    assert_eq!(
        sim.equity_quotes(&["SPY".to_owned()]).unwrap(),
        [("SPY".to_owned(), price("500"))]
    );
    let fills = script.fills();
    assert_eq!(fills.len(), 2);
    assert_eq!(
        (fills[0].symbol.as_str(), fills[0].side, fills[0].quantity),
        ("SPY", Side::Buy, Some(qty("1")))
    );
    assert_eq!(fills[0].price, price("499.5"));
    assert_eq!((fills[1].side, fills[1].quantity), (Side::Sell, None));
    let mut regular = sim.clone();
    let order = regular.place(&limit("buy", "1", "500", 1)).unwrap();
    assert_eq!(order.state, State::Confirmed, "the session is regular");
}

#[test]
fn every_session_name_is_read_and_fills_and_quotes_may_be_absent() {
    for (name, session) in [
        ("closed", Session::Closed),
        ("overnight", Session::Overnight),
        ("extended", Session::Extended),
        ("regular", Session::Regular),
    ] {
        let mut script = base();
        script["session"] = json!(name);
        let built = parsed(&script).unwrap().sim().unwrap();
        let mut expected = Sim::new(built.accounts().to_vec()).unwrap();
        expected
            .apply(mandate_rh_sim::Event::Quote("SPY".to_owned(), price("500")))
            .unwrap();
        expected
            .apply(mandate_rh_sim::Event::Session(session))
            .unwrap();
        let (mut built, mut expected) = (built, expected);
        let request = limit("buy", "1", "500", 1);
        assert_eq!(
            built.place(&request).map(|o| o.state),
            expected.place(&request).map(|o| o.state),
            "{name}"
        );
    }
    let mut bare = base();
    let fields = bare.as_object_mut().unwrap();
    fields.remove("fills");
    fields.remove("quotes");
    fields.remove("session");
    let script = parsed(&bare).unwrap();
    assert!(script.fills().is_empty());
    let mut sim = script.sim().unwrap();
    let closed = sim.place(&limit("buy", "1", "500", 1)).unwrap();
    assert_eq!(closed.state, State::Queued, "no session is the closed one");
}

#[test]
fn anything_the_script_does_not_define_refuses_it() {
    let edits: Vec<(&str, Box<dyn Fn(&mut Value)>)> = vec![
        ("top-level member", Box::new(|s| s["extra"] = json!(1))),
        (
            "account member",
            Box::new(|s| s["accounts"][0]["extra"] = json!(1)),
        ),
        (
            "fill member",
            Box::new(|s| s["fills"][0]["extra"] = json!(1)),
        ),
        (
            "cash as a number",
            Box::new(|s| s["accounts"][0]["cash"] = json!(750.25)),
        ),
        (
            "price as a number",
            Box::new(|s| s["fills"][0]["price"] = json!(499.5)),
        ),
        (
            "quote as a number",
            Box::new(|s| s["quotes"]["SPY"] = json!(500)),
        ),
        (
            "quantity as a number",
            Box::new(|s| s["fills"][0]["quantity"] = json!(1)),
        ),
        (
            "non-canonical decimal",
            Box::new(|s| s["quotes"]["SPY"] = json!("500.0")),
        ),
        (
            "missing number",
            Box::new(|s| {
                s["accounts"][0].as_object_mut().unwrap().remove("number");
            }),
        ),
        (
            "missing price",
            Box::new(|s| {
                s["fills"][0].as_object_mut().unwrap().remove("price");
            }),
        ),
        (
            "missing accounts",
            Box::new(|s| {
                s.as_object_mut().unwrap().remove("accounts");
            }),
        ),
        (
            "flag as text",
            Box::new(|s| s["accounts"][0]["agentic_allowed"] = json!("true")),
        ),
        (
            "unknown session",
            Box::new(|s| s["session"] = json!("lunch")),
        ),
        (
            "unknown side",
            Box::new(|s| s["fills"][0]["side"] = json!("short")),
        ),
        (
            "accounts not a list",
            Box::new(|s| s["accounts"] = json!({})),
        ),
        (
            "duplicate account",
            Box::new(|s| {
                let first = s["accounts"][0].clone();
                s["accounts"].as_array_mut().unwrap().push(first);
            }),
        ),
        (
            "zero fill",
            Box::new(|s| s["fills"][0]["quantity"] = json!("0")),
        ),
    ];
    for (what, edit) in edits {
        let mut script = base();
        edit(&mut script);
        let refused = parsed(&script).and_then(|s| s.sim().map(|_| ()).map_err(ScriptError::Sim));
        assert!(refused.is_err(), "{what}");
    }
    assert!(Script::parse("not json").is_err());
    assert!(Script::parse("[]").is_err());
}
