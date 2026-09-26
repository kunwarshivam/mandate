//! The `trading_domain` harness checks every key of every interpreted expectation (DEC-85). RC-08
//! and RC-18's cash variant, run without their `propose_order` step (E6-3), show that the account
//! type and the `buying_power` expectation are read (DEC-105): the founder's values pass, and a
//! wrong account type, a wrong buying power, or an account type the profile forbids fails.

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
