//! A property of the exits over random runs (E21-30, DEC-2033 item 5, DEC-2037 item 7,
//! `AGENTS.md` rule 13; DEC-2040): the process hands every exit, an owner's at the market at its
//! confirmed floor, with no limit, or stepped up with its confirmation journaled at that floor, holds no exit at all while the agent is `normal` and no order is
//! `Unknown`, and the gate, given its facts, never fails an owner's exit on eligibility, session,
//! conduct, buying power or the day-trade budget. The oracle reads only what the journal holds and
//! the alerts the process raised.

mod support;

use mandate_canon::Value;
use mandate_process::State;
use proptest::prelude::*;
use support::{
    AGENT, Error, ExitFacts, NOON, Paper, TwoPercentStop, at, deployed, owner_exit,
    owner_exit_stepped_up, propose, propose_owner_market_exit, start_wired, wired,
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
    /// An owner's exit at the market whose bid was confirmed with valid step-up, at this floor.
    SteppedUp(u32),
    /// A risk or discretionary exit at this limit.
    Other(&'static str, u32),
}

fn owner_exits() -> impl Strategy<Value = Vec<Exit>> {
    proptest::collection::vec(
        prop_oneof![
            (100u32..200).prop_map(Exit::Limit),
            proptest::option::of(100u32..200).prop_map(Exit::Market),
            (100u32..200).prop_map(Exit::SteppedUp),
            (
                prop_oneof![Just("risk_exit"), Just("discretionary_exit")],
                100u32..200
            )
                .prop_map(|(purpose, limit)| Exit::Other(purpose, limit)),
        ],
        1..5,
    )
}

/// Each exit handed once, in order, at its own limit, at its confirmed floor, or with none; the
/// process up and holding nothing; and no gate check of `NEVER_FAIL` failed.
fn check(exits: &[Exit]) -> Result<(), Error> {
    let journal = deployed()?;
    let mut handed = Vec::new();
    let mut limits = Vec::new();
    let mut floors = Vec::new();
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
                limits.push(Value::Str(limit.to_string()));
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
                limits.push(Value::Str(limit.to_string()));
            }
            Exit::Market(floor) => {
                let request = format!("01JOWNREQS{n:016}");
                let floor = floor.map(|floor| floor.to_string());
                owner_exit(&journal, AGENT, &request, floor.as_deref())?;
                propose_owner_market_exit(&journal, AGENT, &intent, &request)?;
                limits.push(floor.map_or(Value::Null, Value::Str));
            }
            Exit::SteppedUp(floor) => {
                let request = format!("01JOWNREQS{n:016}");
                owner_exit_stepped_up(
                    &journal,
                    AGENT,
                    &request,
                    &floor.to_string(),
                    "2026-09-21T14:59:00.000000000Z",
                )?;
                propose_owner_market_exit(&journal, AGENT, &intent, &request)?;
                limits.push(Value::Null);
                floors.push(Value::Str(floor.to_string()));
            }
        }
        handed.push(intent);
    }
    let broker = Paper::new(&journal);
    broker.holding("AAPL", "1000")?;
    let wiring = wired(Box::new(ExitFacts), Box::new(TwoPercentStop))?;
    let mut process = start_wired(&journal, &broker, wiring)?;
    let mut held = Vec::new();
    for _ in 0..3 {
        for alert in process.step(at(NOON)?) {
            if matches!(alert.key, "exit_held" | "owner_exit_unpriced") {
                held.extend(alert.subject);
            }
        }
    }
    assert!(held.is_empty(), "exits held in normal mode: {held:?}");
    assert!(!matches!(process.lifecycle().state(), State::Exited(_)));
    assert_eq!(journal.received(), handed);
    let received: Vec<Value> = journal
        .payloads("IntentReceived")
        .iter()
        .map(|payload| payload.get("limit_price").cloned().unwrap_or(Value::Null))
        .collect();
    assert_eq!(received, limits, "each at its limit, its floor, or none");
    let confirmed: Vec<Value> = journal
        .payloads("OwnerConfirmationReceived")
        .iter()
        .map(|payload| payload.get("floor").cloned().unwrap_or(Value::Null))
        .collect();
    assert_eq!(
        confirmed, floors,
        "a stepped-up exit's confirmation journaled at its floor, and no other"
    );
    let decisions = journal.payloads("GateDecided");
    assert!(!decisions.is_empty());
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
