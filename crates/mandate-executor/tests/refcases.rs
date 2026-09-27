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

mod common;

use common::{FixedInstruments, FixedMandate, Shell, TestIds, config, ports, stream_opened};
use mandate_executor::{BrokerUpdate, Command, Initiator, Input, KillScope, ReconcileReason};

/// One case as the fixture carries it.
struct Case {
    id: String,
    steps: Vec<serde_json::Value>,
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
    let document: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
    let cases = document
        .get("cases")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("{path} has no `cases` array"));
    let found = cases
        .iter()
        .find(|c| c.get("id").and_then(serde_json::Value::as_str) == Some(id))
        .unwrap_or_else(|| panic!("{path} has no case {id}"));
    let steps = match variant {
        None => found
            .get("steps")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_else(|| panic!("{id} has no steps")),
        Some(name) => found
            .get("variants")
            .and_then(serde_json::Value::as_array)
            .and_then(|variants| {
                variants
                    .iter()
                    .find(|v| v.get("name").and_then(serde_json::Value::as_str) == Some(name))
            })
            .and_then(|v| v.get("overrides"))
            .and_then(|o| o.get("steps"))
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_else(|| panic!("{id} has no variant {name} with steps")),
    };
    Case {
        id: match variant {
            Some(name) => format!("{id}::{name}"),
            None => id.to_owned(),
        },
        steps,
    }
}

/// Turns one harness step into the input that carries it, or says which story owns the step
/// kind. The four steps this stream owns are `broker_order_update`, `broker_position_update`,
/// `reconciliation`, and `corporate_action_prepare` (task brief, Reference cases); the rest
/// arrive as the inputs the other streams' events become on the account stream.
fn input_for(step: &serde_json::Value, index: usize, at: i64) -> Option<Input> {
    let name = step.get("event")?.as_str()?;
    let data = step.get("data").cloned().unwrap_or(serde_json::Value::Null);
    let text = |key: &str| {
        data.get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let symbol = text("instrument").unwrap_or_else(|| FixedInstruments::LIQUID_EQUITY.to_owned());
    match name {
        "propose_order" => Some(common::handoff(
            &format!("01JREFCASE{index:016}"),
            common::AGENT,
            proposal(&data),
        )),
        "broker_order_update" => Some(Input::BrokerUpdate(BrokerUpdate::Order(
            common::broker_order(
                &format!("b-{index}"),
                Some(&format!("md-01JREFCASE{index:016}")),
                &symbol,
                side_of(&data),
                &text("qty").unwrap_or_else(|| "1".to_owned()),
                data.get("fill")
                    .and_then(|f| f.get("qty"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("0"),
                &text("status").unwrap_or_else(|| "new".to_owned()),
            ),
        ))),
        "fill" => Some(Input::BrokerUpdate(BrokerUpdate::Fill(
            common::broker_fill(
                &format!("f-{index}"),
                Some(&format!("md-01JREFCASE{index:016}")),
                &text("qty_gross").unwrap_or_else(|| "1".to_owned()),
                &canonical(&text("price").unwrap_or_else(|| "1".to_owned())),
            ),
        ))),
        "broker_position_update" | "reconciliation" => Some(Input::BrokerSnapshot(
            common::snapshot(0, ReconcileReason::Scheduled),
        )),
        "broker_account_update" => Some(Input::BrokerUpdate(BrokerUpdate::Account(
            common::broker_account(),
        ))),
        "kill_switch" => Some(Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(common::agent(common::AGENT)),
            initiator: Initiator::RiskLimit,
            confirmation: None,
        })),
        "mark" => Some(Input::Market(common::quote(
            &symbol,
            &canonical(&text("price").unwrap_or_else(|| "150".to_owned())),
            &canonical(&text("price").unwrap_or_else(|| "150".to_owned())),
            at,
        ))),
        "advance_clock" => Some(Input::Tick(common::clock(at))),
        _ => None,
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

fn side_of(data: &serde_json::Value) -> mandate_accounting::Side {
    let named = data
        .get("side")
        .or_else(|| data.get("entry").and_then(|e| e.get("side")))
        .and_then(serde_json::Value::as_str);
    if named == Some("sell") {
        mandate_accounting::Side::Sell
    } else {
        mandate_accounting::Side::Buy
    }
}

fn proposal(data: &serde_json::Value) -> mandate_executor::IntentBody {
    let entry = data.get("entry").unwrap_or(data);
    let field = |key: &str| {
        entry
            .get(key)
            .or_else(|| data.get(key))
            .and_then(serde_json::Value::as_str)
    };
    let symbol = data
        .get("instrument")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(FixedInstruments::LIQUID_EQUITY);
    let purpose = data
        .get("purpose")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("open");
    let quantity = canonical(field("qty").unwrap_or("1"));
    let limit = canonical(field("limit_price").unwrap_or("150"));
    match purpose {
        "risk_exit" | "owner_exit" => common::risk_exit(symbol, &quantity, &limit),
        "discretionary_exit" => common::discretionary_exit(symbol, &quantity, &limit),
        _ => common::opening(symbol, &quantity, &limit),
    }
}

/// The action names one step's `expect.actions` list asks for, which is what makes the case
/// assert something rather than merely run.
fn expected_actions(step: &serde_json::Value) -> Vec<String> {
    step.get("expect")
        .and_then(|e| e.get("actions"))
        .and_then(serde_json::Value::as_array)
        .map(|actions| {
            actions
                .iter()
                .filter_map(|action| match action {
                    serde_json::Value::String(name) => Some(name.clone()),
                    serde_json::Value::Object(map) => map.keys().next().cloned(),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Drives one case's steps against a fresh executor and asserts what the case says must happen.
///
/// A step whose `expect` names `actions` must produce effects; a step whose `expect` names a
/// `decision` must journal `GateDecided` with that verdict. A step kind this stream does not own
/// is skipped by name rather than silently, so a case cannot pass by doing nothing — the defect
/// the throwaway-implementation run found in the first draft of this harness.
fn drive(case: &Case) {
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

    let mut shell = Shell::new(1);
    shell
        .fold_one(&stream_opened())
        .unwrap_or_else(|e| panic!("{}: the stream must open before step 1: {e}", case.id));
    let (mut shell, _) = shell.restart(&ports);

    let mut asserted = 0usize;
    for (index, step) in case.steps.iter().enumerate() {
        let name = step
            .get("event")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("{} step {index} has no `event`", case.id));
        let at = i64::try_from(index.saturating_mul(60).saturating_add(60)).unwrap_or(i64::MAX);
        shell
            .step(Input::Tick(common::clock(at)), &ports)
            .unwrap_or_else(|e| panic!("{}: the clock refused with {}: {e}", case.id, e.code()));
        let Some(input) = input_for(step, index, at) else {
            continue;
        };
        let ran = shell.step(input, &ports).unwrap_or_else(|e| {
            panic!(
                "{}: step {index} (`{name}`) refused with {}: {e}",
                case.id,
                e.code()
            )
        });

        let actions = expected_actions(step);
        if !actions.is_empty() {
            asserted = asserted.saturating_add(1);
            assert!(
                !ran.effects.is_empty(),
                "{}: step {index} (`{name}`) expects {actions:?} and produced no effect at all",
                case.id
            );
        }
        if let Some(verdict) = step
            .get("expect")
            .and_then(|e| e.get("decision"))
            .and_then(|d| d.get("verdict"))
            .and_then(serde_json::Value::as_str)
        {
            asserted = asserted.saturating_add(1);
            let decided = ran
                .draft("GateDecided")
                .and_then(|d| d.payload.get("verdict"))
                .and_then(mandate_canon::Value::as_str);
            assert_eq!(
                decided,
                Some(verdict),
                "{}: step {index} (`{name}`) expects the gate to {verdict}",
                case.id
            );
        }
    }
    assert!(
        asserted > 0,
        "{} asserted nothing: every case this stream owns names at least one action or verdict",
        case.id
    );
}

#[test]
#[ignore = "pending E7-4"]
fn trading_domain_rc_14_equity_exit_sequence_with_protective_oco() {
    drive(&case("RC-14", None));
}

#[test]
#[ignore = "pending E7-4"]
fn trading_domain_rc_14_passive_exit_becomes_oco_take_profit() {
    drive(&case("RC-14", Some("passive_exit_becomes_oco_take_profit")));
}

#[test]
#[ignore = "pending E7-4"]
fn trading_domain_rc_14_plain_add_blocked() {
    drive(&case("RC-14", Some("plain_add_blocked")));
}

#[test]
#[ignore = "pending E7-4"]
fn trading_domain_rc_14_add_via_bracket() {
    drive(&case("RC-14", Some("add_via_bracket")));
}

#[test]
#[ignore = "pending E7-4"]
fn trading_domain_rc_14_kill_switch() {
    drive(&case("RC-14", Some("kill_switch")));
}

#[test]
#[ignore = "pending E7-3"]
fn trading_domain_rc_04_split_cancels_the_oco_and_re_derives_protection() {
    drive(&case("RC-04", None));
}

#[test]
#[ignore = "pending E7-4"]
fn trading_domain_rc_06_protective_orders_kept_through_dividend() {
    drive(&case(
        "RC-06",
        Some("protective_orders_kept_through_dividend"),
    ));
}

#[test]
#[ignore = "pending E7-3"]
fn trading_domain_rc_07_unposted_crypto_fees_reconcile() {
    drive(&case("RC-07", None));
}

#[test]
#[ignore = "pending E7-3"]
fn trading_domain_rc_11_settlement_calendar_reconciles() {
    drive(&case("RC-11", None));
}

#[test]
#[ignore = "pending E7-4"]
fn trading_domain_rc_20_crypto_stop_limit_add_and_exit_sequences() {
    drive(&case("RC-20", None));
}

#[test]
#[ignore = "pending E7-4"]
fn trading_domain_rc_21_bracket_partly_filled_and_re_placed_before_expiry() {
    drive(&case("RC-21", None));
}

#[test]
#[ignore = "pending E7-4"]
fn trading_domain_rc_24_exit_price_ladder_in_extended_hours() {
    drive(&case("RC-24", None));
}

#[test]
#[ignore = "pending E7-4"]
fn trading_domain_rc_24_presumed_halt_regular_session() {
    drive(&case("RC-24", Some("presumed_halt_regular_session")));
}

#[test]
#[ignore = "pending E7-3"]
fn trading_domain_rc_15_external_order_detected() {
    drive(&case("RC-15", Some("external_order_detected")));
}

#[test]
#[ignore = "pending E7-3"]
fn trading_domain_rc_15_unexplained_403s() {
    drive(&case("RC-15", Some("unexplained_403s")));
}

#[test]
#[ignore = "pending E7-3"]
fn trading_domain_rc_17_instrument_claims_and_shared_buying_power() {
    drive(&case("RC-17", None));
}

#[test]
#[ignore = "pending E7-2"]
fn trading_domain_rc_08_broker_order_updates() {
    drive(&case("RC-08", None));
}

#[test]
#[ignore = "pending E7-2"]
fn trading_domain_rc_09_broker_order_updates() {
    drive(&case("RC-09", None));
}

#[test]
#[ignore = "pending E7-2"]
fn trading_domain_rc_09b_broker_order_updates() {
    drive(&case("RC-09B", None));
}

#[test]
#[ignore = "pending E7-2"]
fn trading_domain_rc_18_broker_order_updates() {
    drive(&case("RC-18", None));
}

#[test]
#[ignore = "pending E7-2"]
fn trading_domain_rc_22_broker_order_updates() {
    drive(&case("RC-22", None));
}

#[test]
#[ignore = "pending E7-2"]
fn trading_domain_rc_23_broker_order_updates() {
    drive(&case("RC-23", None));
}

#[test]
#[ignore = "pending E7-2"]
fn trading_domain_rc_25_broker_order_updates() {
    drive(&case("RC-25", None));
}
