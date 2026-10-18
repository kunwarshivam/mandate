//! A property of the exits over random runs (E21-30, DEC-2033 items 4 and 5, `AGENTS.md` rule 13):
//! the process never drops, refuses or silently holds an owner's exit, holds no exit at all while
//! the agent is `normal` and no order is `Unknown`, and the gate never fails an owner's exit on
//! eligibility, session, conduct, buying power or the day-trade budget. The oracle reads only what
//! the journal holds and the alerts the process raised.

mod support;

use std::collections::BTreeSet;

use mandate_canon::Value;
use mandate_process::State;
use proptest::prelude::*;
use support::{
    AGENT, Error, NOON, Paper, at, deployed, owner_exit, propose, propose_owner_market_exit, start,
};

/// The checks that never refuse an owner's exit (`AGENTS.md` rule 13).
const NEVER_FAIL: [&str; 5] = [
    "eligibility",
    "session",
    "conduct",
    "buying_power",
    "day_trade",
];

#[derive(Debug, Clone)]
enum Exit {
    Limit(u32),
    Market(Option<u32>),
    /// A risk or discretionary exit at this limit.
    Other(&'static str, u32),
}

fn owner_exits() -> impl Strategy<Value = Vec<Exit>> {
    proptest::collection::vec(
        prop_oneof![
            (100u32..200).prop_map(Exit::Limit),
            proptest::option::of(100u32..200).prop_map(Exit::Market),
            (
                prop_oneof![Just("risk_exit"), Just("discretionary_exit")],
                100u32..200
            )
                .prop_map(|(purpose, limit)| Exit::Other(purpose, limit)),
        ],
        1..5,
    )
}

/// Each exit handed or alerted, the alerts once each, the process up, and no gate check of
/// `NEVER_FAIL` failed.
fn check(exits: &[Exit]) -> Result<(), Error> {
    let journal = deployed()?;
    let mut priced = Vec::new();
    let mut unpriced = BTreeSet::new();
    for (n, exit) in exits.iter().enumerate() {
        let intent = format!("01JOWNEXIT{n:016}");
        match exit {
            Exit::Limit(limit) => {
                propose(
                    &journal,
                    AGENT,
                    &intent,
                    "sell",
                    "10",
                    &limit.to_string(),
                    "owner_exit",
                )?;
                priced.push(intent);
            }
            Exit::Other(purpose, limit) => {
                propose(
                    &journal,
                    AGENT,
                    &intent,
                    "sell",
                    "10",
                    &limit.to_string(),
                    purpose,
                )?;
                priced.push(intent);
            }
            Exit::Market(floor) => {
                let request = format!("01JOWNREQS{n:016}");
                let floor = floor.map(|floor| floor.to_string());
                owner_exit(&journal, AGENT, &request, floor.as_deref())?;
                propose_owner_market_exit(&journal, AGENT, &intent, &request)?;
                if floor.is_some() {
                    priced.push(intent);
                } else {
                    unpriced.insert(intent);
                }
            }
        }
    }
    let broker = Paper::new(&journal);
    broker.holding("AAPL", "1000")?;
    let mut process = start(&journal, &broker)?;
    let mut alerted = Vec::new();
    let mut held = Vec::new();
    for _ in 0..3 {
        for alert in process.step(at(NOON)?) {
            match alert.key {
                "owner_exit_unpriced" => alerted.extend(alert.subject),
                "exit_held" => held.extend(alert.subject),
                _ => {}
            }
        }
    }
    assert!(held.is_empty(), "exits held in normal mode: {held:?}");
    assert!(!matches!(process.lifecycle().state(), State::Exited(_)));
    assert_eq!(journal.received(), priced);
    assert_eq!(alerted.len(), unpriced.len());
    assert_eq!(alerted.into_iter().collect::<BTreeSet<_>>(), unpriced);
    let decisions = journal.payloads("GateDecided");
    assert_eq!(decisions.is_empty(), journal.received().is_empty());
    for decision in decisions {
        let Some(Value::Array(checks)) = decision.get("checks") else {
            return Err("a gate decision without its checks".into());
        };
        for check in checks {
            let id = check.get("id").and_then(Value::as_str).unwrap_or_default();
            let result = check.get("result").and_then(Value::as_str);
            assert!(
                !(NEVER_FAIL.contains(&id) && result == Some("fail")),
                "{id} failed an owner exit: {decision:?}"
            );
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn no_exit_is_held_and_no_owner_exit_dropped_or_refused(exits in owner_exits()) {
        check(&exits).map_err(|error| TestCaseError::fail(error.to_string()))?;
    }
}
