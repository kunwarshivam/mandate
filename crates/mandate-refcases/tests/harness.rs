//! The `trading_domain` harness checks every key of every interpreted expectation (DEC-85). RC-08
//! and RC-18's cash variant, run without their `propose_order` step (E6-3), show that the account
//! type and the `buying_power` expectation are read (DEC-105): the founder's values pass, and a
//! wrong account type, a wrong buying power, or an account type the profile forbids fails.
//!
//! The backtest cases do the same for E4-1 (DEC-106 item 10): RC-10, RC-12, and RC-19 pass as the
//! founder wrote them, and editing a fill's price, bar, or liquidity, a bar's session label, an
//! order's decision time, its `first_bar_reference_volume`, its position, or a canceled leg makes
//! the case fail. Six bugs planted in the backtest interpretation, one per key it reads, were each
//! caught by one of these two tests (`crates/mandate-sim/tests/properties.rs` lists all twenty).

use std::path::Path;
use std::sync::Arc;

use mandate_refcases::{Json, read_fixture, trading_domain};
use serde_json::json;

fn fixture() -> Json {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases");
    Arc::unwrap_or_clone(read_fixture(&dir, "trading-domain.json").unwrap())
}

fn run(fixture: Json, case: &str) -> Result<(), String> {
    let wanted = format!("trading_domain::{case}");
    let case = trading_domain::cases(&Arc::new(fixture))
        .into_iter()
        .find(|c| c.id == wanted)
        .unwrap_or_else(|| panic!("no case {wanted}"));
    (case.run)()
}

/// The fixture with case `id` edited: its `propose_order` steps removed, then `edit` applied.
fn accounting_steps_of(id: &str, edit: impl FnOnce(&mut Json)) -> Json {
    let mut fixture = fixture();
    let case = fixture["cases"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|c| c["id"] == id)
        .unwrap();
    without_gate_steps(&mut case["steps"]);
    for variant in case["variants"].as_array_mut().into_iter().flatten() {
        if let Some(steps) = variant
            .get_mut("overrides")
            .and_then(|o| o.get_mut("steps"))
        {
            without_gate_steps(steps);
        }
    }
    edit(case);
    fixture
}

fn without_gate_steps(steps: &mut Json) {
    if let Some(steps) = steps.as_array_mut() {
        steps.retain(|s| s["event"] != "propose_order");
    }
}

#[test]
fn rc_08_accounting_steps_pass_and_a_wrong_account_type_or_buying_power_fails() {
    assert_eq!(run(accounting_steps_of("RC-08", |_| {}), "RC-08"), Ok(()));

    let as_margin = run(
        accounting_steps_of("RC-08", |c| {
            c["initial"]["account"]["type"] = json!("margin");
        }),
        "RC-08",
    );
    assert_eq!(
        as_margin,
        Err("step 2: buying_power: expected 499.98, got 1049.98".to_owned())
    );

    let wrong_value = run(
        accounting_steps_of("RC-08", |c| {
            c["steps"][0]["expect"]["buying_power"] = json!("500.00");
        }),
        "RC-08",
    );
    assert_eq!(
        wrong_value,
        Err("step 1: buying_power: expected 500, got 499.99".to_owned())
    );

    let untyped = run(
        accounting_steps_of("RC-08", |c| {
            c["initial"]["account"]
                .as_object_mut()
                .unwrap()
                .remove("type");
        }),
        "RC-08",
    );
    assert_eq!(
        untyped,
        Err("a generic account needs `initial.account.type`".to_owned())
    );

    let alpaca_cash = run(
        accounting_steps_of("RC-08", |c| c["broker_profile"] = json!("alpaca")),
        "RC-08",
    );
    assert_eq!(
        alpaca_cash,
        Err("an alpaca account is never a cash account".to_owned())
    );
}

#[test]
fn rc_18_cash_variant_accounting_steps_pass_and_the_margin_case_reads_the_default_type() {
    let variant = "RC-18::generic_cash_account";
    assert_eq!(run(accounting_steps_of("RC-18", |_| {}), variant), Ok(()));
    let as_margin = run(
        accounting_steps_of("RC-18", |c| {
            c["variants"][0]["overrides"]["initial"]["account"]["type"] = json!("margin");
        }),
        variant,
    );
    assert_eq!(
        as_margin,
        Err("step 2: buying_power: expected 499.98, got 1049.98".to_owned())
    );
    let main_case = run(accounting_steps_of("RC-18", |_| {}), "RC-18");
    assert!(
        main_case
            .as_ref()
            .is_err_and(|e| e.contains("`broker_order_update` steps not interpreted until E7-2")),
        "{main_case:?}"
    );
}

/// The fixture with case `id` edited in memory.
fn edited(id: &str, edit: impl FnOnce(&mut Json)) -> Json {
    let mut fixture = fixture();
    let case = fixture["cases"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|c| c["id"] == id)
        .unwrap();
    edit(case);
    fixture
}

fn failure(fixture: Json, case: &str, wanted: &str) {
    let result = run(fixture, case);
    assert!(
        result.as_ref().is_err_and(|e| e.contains(wanted)),
        "expected a failure naming {wanted:?}, got {result:?}"
    );
}

#[test]
#[ignore = "pending E4-1"]
fn rc_10_and_rc_19_pass_and_a_wrong_fill_or_decision_time_fails() {
    assert_eq!(run(edited("RC-10", |_| {}), "RC-10"), Ok(()));
    assert_eq!(run(edited("RC-19", |_| {}), "RC-19"), Ok(()));

    failure(
        edited("RC-10", |c| {
            c["orders"][0]["expect"]["fills"][0]["price"] = json!("99.98");
        }),
        "RC-10",
        "fills[0].price: expected 99.98, got 99.97",
    );
    failure(
        edited("RC-10", |c| {
            c["orders"][0]["expect"]["fills"][0]["bar"] = json!(2);
        }),
        "RC-10",
        "fills[0].bar",
    );
    failure(
        edited("RC-10", |c| {
            c["orders"][1]["expect"]["fills"][0]["liquidity"] = json!("taker");
        }),
        "RC-10",
        "fills[0].liquidity",
    );
    failure(
        edited("RC-10", |c| {
            c["orders"][0]["initial_position"]["qty"] = json!("50");
        }),
        "RC-10",
        "DEC-32",
    );
    failure(
        edited("RC-19", |c| {
            c["orders"][0]["decided_at"] = json!("2026-09-21T09:42:00-04:00");
        }),
        "RC-19",
        "fills: count: expected 1, got 0",
    );
}

#[test]
#[ignore = "pending E4-1"]
fn rc_12_passes_and_its_session_labels_median_and_canceled_leg_are_read() {
    assert_eq!(run(edited("RC-12", |_| {}), "RC-12"), Ok(()));

    failure(
        edited("RC-12", |c| {
            c["orders"][6]["bars"][0]["session"] = json!("pre_market");
        }),
        "RC-12",
        "bar session",
    );
    failure(
        edited("RC-12", |c| {
            c["orders"][6]
                .as_object_mut()
                .unwrap()
                .remove("first_bar_reference_volume");
        }),
        "RC-12",
        "fills: count: expected 1, got 0",
    );
    failure(
        edited("RC-12", |c| {
            c["orders"][3]["expect"]["canceled_legs"] = json!(["stop"]);
        }),
        "RC-12",
        "canceled_legs",
    );
    failure(
        edited("RC-12", |c| {
            c["isolation"] = json!("per_account");
        }),
        "RC-12",
        "unknown isolation",
    );
}
