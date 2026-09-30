//! RC-15's restricted arm, which its own case cannot reach in this harness: the closing-only
//! restriction arrives in step 2 as a `broker_order_update` reject, and that event is E7-2's. The
//! test runs the founder's steps 3 and 4 unchanged, with the account in the state §7.3's second row
//! gives that reject (`closing_only`), and with AAPL marked at step 4's own limit price, since the
//! case holds AAPL with no mark and the harness refuses to value an unmarked position it is not
//! deciding on (DEC-199). The same steps under an active account show the state is what decides.

use std::path::Path;
use std::sync::Arc;

use mandate_risk::AccountState;
use serde_json::json;

use super::Gate;
use crate::trading_domain::{BrokerProfile, config, initial, instruments, run_step};
use crate::{Json, ensure, list_at, read_fixture, str_at};

fn rc_15() -> Result<(Json, Json), String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases");
    let fixture = read_fixture(&dir, "trading-domain.json").map(Arc::unwrap_or_clone)?;
    let case = list_at(&fixture, "cases")?
        .iter()
        .find(|c| c.get("id").and_then(Json::as_str) == Some("RC-15"))
        .cloned()
        .ok_or("no case RC-15")?;
    Ok((fixture, case))
}

/// A step's stated `decision`, and the outcome of comparing the gate's decision with it.
type Compared = (Json, Result<(), String>);

/// Steps 3 and 4 of RC-15, each decided in `state`, as the decision the gate made on each.
fn steps_3_and_4(state: AccountState) -> Result<Vec<Compared>, String> {
    let (fixture, case) = rc_15()?;
    let config = config(&fixture, &case)?;
    let instruments = instruments(&case)?;
    let mut account = initial(
        &case,
        BrokerProfile::parse(str_at(&case, "broker_profile")?)?,
        &instruments,
    )?;
    let mut gate = Gate::read(&fixture, &case)?;
    gate.restrict(state);
    let mark = json!({
        "at": "2026-09-22T10:00:30-04:00",
        "event": "mark",
        "data": { "instrument": "AAPL", "price": "155.00", "source": "quote" }
    });
    run_step(&mut account, &mut gate, &mark, 1, &instruments, &config)?;
    let steps = list_at(&case, "steps")?;
    let mut out = Vec::new();
    for n in [2, 3] {
        let step = steps.get(n).ok_or("RC-15 has four steps")?;
        let expected = crate::at(step, "expect.decision")?.clone();
        let (_, decision) = run_step(&mut account, &mut gate, step, n, &instruments, &config)?;
        out.push((
            expected.clone(),
            gate.check_decision(decision.as_ref(), &expected),
        ));
    }
    Ok(out)
}

#[test]
fn rc_15_denies_an_opening_and_allows_a_risk_exit_in_a_closing_only_account() -> Result<(), String>
{
    let decided = steps_3_and_4(AccountState::ClosingOnly)?;
    let wanted = [
        json!({ "verdict": "deny", "reason_code": "account_restricted" }),
        json!({ "verdict": "allow" }),
    ];
    for ((expected, outcome), wanted) in decided.into_iter().zip(wanted) {
        ensure(expected == wanted, || {
            format!("RC-15 changed: its step states {expected}, the test was written for {wanted}")
        })?;
        outcome?;
    }
    Ok(())
}

#[test]
fn the_same_steps_in_an_active_account_allow_the_opening() -> Result<(), String> {
    let decided = steps_3_and_4(AccountState::Active)?;
    let outcomes: Vec<Result<(), String>> = decided.into_iter().map(|(_, o)| o).collect();
    ensure(
        outcomes
            == vec![
                Err("decision.verdict: expected deny, got allow".to_owned()),
                Ok(()),
            ],
        || format!("{outcomes:?}"),
    )
}

#[test]
fn a_blocked_account_denies_both_steps_account_trading_blocked() -> Result<(), String> {
    let decided = steps_3_and_4(AccountState::Blocked)?;
    let outcomes: Vec<Result<(), String>> = decided.into_iter().map(|(_, o)| o).collect();
    ensure(
        outcomes
            == vec![
                Err(
                    "decision.reason_code: expected account_restricted, got account_trading_blocked"
                        .to_owned(),
                ),
                Err("decision.verdict: expected allow, got deny".to_owned()),
            ],
        || format!("{outcomes:?}"),
    )
}
