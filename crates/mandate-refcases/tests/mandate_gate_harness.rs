//! Families G (`gate`) and F (`agent_flatten`) of the `mandate` harness read every key of every case,
//! compare every expectation, and never let a case pass for the wrong reason (DEC-85, DEC-178).
//!
//! The arms live in `src/mandate/risk_gate.rs` and drive `mandate_risk::evaluate` and
//! `mandate_risk::agent_flatten`. Each test here has two halves that need each other: a case passes as
//! the fixture states it, which an arm that failed everything could not do, and fails, naming the
//! member, when that member is edited, dropped, or added, which an arm that compared nothing could not
//! do.

use std::path::Path;
use std::sync::Arc;

use mandate_refcases::{Json, mandate, read_fixture};
use serde_json::json;

fn fixture() -> Json {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases");
    Arc::unwrap_or_clone(read_fixture(&dir, "mandate.json").expect("the mandate fixture"))
}

fn run(fixture: Json, id: &str) -> Result<(), String> {
    let wanted = format!("mandate::{id}");
    let case = mandate::cases(&Arc::new(fixture))
        .into_iter()
        .find(|c| c.id == wanted)
        .unwrap_or_else(|| panic!("no case {wanted}"));
    (case.run)()
}

fn ids_of(fixture: &Json, kind: &str) -> Vec<String> {
    fixture["cases"]
        .as_array()
        .expect("a case list")
        .iter()
        .filter(|c| c["kind"] == kind)
        .map(|c| c["id"].as_str().expect("an id").to_owned())
        .collect()
}

fn case_mut<'a>(fixture: &'a mut Json, id: &str) -> &'a mut Json {
    fixture["cases"]
        .as_array_mut()
        .expect("a case list")
        .iter_mut()
        .find(|c| c["id"] == id)
        .expect("the case")
}

fn case_of(fixture: &Json, id: &str) -> Json {
    fixture["cases"]
        .as_array()
        .expect("a case list")
        .iter()
        .find(|c| c["id"] == id)
        .expect("the case")
        .clone()
}

/// Applies `doctor` to a copy of the fixture and runs `id`, which must fail with a message that
/// starts with `named`. Every needle names the member or case it is about and the arms' messages
/// start with what they compared, so a failure for another reason, even one that mentions the
/// member further on, does not match.
fn fails_naming(fixture: &Json, id: &str, named: &str, doctor: impl FnOnce(&mut Json)) {
    let mut doctored = fixture.clone();
    doctor(case_mut(&mut doctored, id));
    let failure = run(doctored, id).expect_err("the doctored case must fail");
    assert!(
        failure.starts_with(named),
        "{id}: the failure must start with `{named}`, got: {failure}"
    );
}

/// The sweep's message for a member planted at `path`, naming the level and the member.
fn planted_at(path: &[&str], planted: &str) -> String {
    let level = match path {
        [] => "case keys",
        ["expect"] => "expectations",
        ["state"] => "`state` members",
        ["proposed"] => "`proposed` members",
        ["state", "working_opening_orders", _] => "working opening order members",
        ["expect", "computed"] => "`computed` members",
        ["input"] => "`input` members",
        ["input", "open_orders", _] => "open order members",
        ["input", "agent_positions", _] => "agent position members",
        ["input", "broker_positions", _] => "broker position members",
        ["expect", "sells", _] => "sell members",
        ["expect", "deferred_sells", _] => "deferred sell members",
        other => panic!("no sweep message for {other:?}"),
    };
    format!("{level} not interpreted: {planted}")
}

/// Every member of every `gate` and `agent_flatten` case, at every level the arms read, is one they
/// know: a member planted beside the real ones fails its case at the sweep of its own level, naming
/// it.
#[test]
fn every_gate_and_flatten_member_is_read() {
    let fixture = fixture();
    let planted = "a_member_the_harness_does_not_read";
    let mut sites = 0;
    let mut plant = |id: &str, path: &[&str]| {
        fails_naming(&fixture, id, &planted_at(path, planted), |case| {
            let mut slot = case;
            for step in path {
                slot = match step.parse::<usize>() {
                    Ok(index) => &mut slot[index],
                    Err(_) => &mut slot[*step],
                };
            }
            slot.as_object_mut()
                .unwrap_or_else(|| panic!("{id}: {path:?} is not an object"))
                .insert(planted.to_owned(), Json::Null);
        });
        sites += 1;
    };
    for id in ids_of(&fixture, "gate") {
        let case = case_of(&fixture, &id);
        for path in [&[][..], &["expect"], &["state"], &["proposed"]] {
            plant(&id, path);
        }
        if case["state"]["working_opening_orders"]
            .as_array()
            .is_some_and(|orders| !orders.is_empty())
        {
            plant(&id, &["state", "working_opening_orders", "0"]);
        }
        if case["expect"].get("computed").is_some() {
            plant(&id, &["expect", "computed"]);
        }
    }
    for id in ids_of(&fixture, "agent_flatten") {
        let case = case_of(&fixture, &id);
        for path in [
            &[][..],
            &["expect"],
            &["input"],
            &["input", "open_orders", "0"],
            &["input", "agent_positions", "0"],
            &["input", "broker_positions", "0"],
            &["expect", "sells", "0"],
        ] {
            plant(&id, path);
        }
        if case["expect"]["deferred_sells"]
            .as_array()
            .is_some_and(|sells| !sells.is_empty())
        {
            plant(&id, &["expect", "deferred_sells", "0"]);
        }
    }
    assert_eq!(
        sites,
        16 * 4 + 14 + 12 + 4 * 7 + 2,
        "four sites in each of the 16 gate cases, the first working order of the 14 that have one, \
         the `computed` of the 12 that state it; seven sites in each of the four flatten cases, and \
         the first deferred sell of the two that defer one"
    );
}

/// An edited verdict: every denial becomes an allow, and an allow a denial.
fn other_verdict(verdict: &Json) -> Json {
    if verdict == "allow" {
        json!("deny")
    } else {
        json!("allow")
    }
}

/// An edited reason: a registered code other than the stated one, so the arm reads it and compares.
fn other_reason(reason: &Json) -> Json {
    if reason == "concentration_limit" {
        json!("max_order_size")
    } else {
        json!("concentration_limit")
    }
}

/// A `computed` figure moved to a value no gate case can produce, in its own type.
fn other_figure(key: &str, value: &Json) -> Json {
    match key {
        "orders_today" => json!(value.as_u64().expect("a count").saturating_add(1)),
        "last_exit_fill_at" => json!("2000-01-01T00:00:00.000000000Z"),
        "instrument" => json!("7b4a1c2e-9999-4a2b-9c3d-000000000009"),
        _ => {
            let whole: u64 = value
                .as_str()
                .and_then(|text| text.parse().ok())
                .expect("a whole dollar figure");
            json!(whole.saturating_add(1).to_string())
        }
    }
}

/// Every figure `computed` can state, with a value no gate case can produce, for adding where the case
/// states none.
const EVERY_FIGURE: [(&str, &str); 8] = [
    ("instrument_total", "1"),
    ("cap", "1"),
    ("order_usd", "1"),
    ("gross", "1"),
    ("gross_limit", "1"),
    ("orders_today", "999"),
    ("last_exit_fill_at", "2000-01-01T00:00:00.000000000Z"),
    ("instrument", "7b4a1c2e-9999-4a2b-9c3d-000000000009"),
];

/// Every `gate` case passes as the fixture states it, and fails, naming the member, when its verdict
/// or reason is edited or dropped, when an opening's `proposed.purpose` is flipped between `open`
/// and `increase`, when any `computed` figure it states is edited, and when a figure it does not
/// state is added.
///
/// The purpose flip is the one edit outside `expect`: the harness picks the same `Origin` and side
/// for both purposes, so only the gate's own assignment, from whether the agent holds the
/// instrument, can tell them apart, and the arm must compare it.
///
/// The added figure is the other direction: a case states only the figures its title is about, so
/// the arm compares the ones stated, and one the gate did not report, or reported with another value,
/// must fail rather than be skipped. MC-G02's unreached figures are pinned in its `FULL_GATE_ONLY`
/// entry, so editing them fails there.
#[test]
fn every_gate_case_passes_and_fails_on_each_edited_expectation() {
    let fixture = fixture();
    let ids = ids_of(&fixture, "gate");
    assert_eq!(ids.len(), 16, "family G is 16 cases");
    let mut edits = 0;
    for id in &ids {
        if let Err(failure) = run(fixture.clone(), id) {
            panic!("{id} must pass as the fixture states it: {failure}");
        }
        let case = case_of(&fixture, id);
        let expect = case["expect"].clone();
        fails_naming(&fixture, id, "verdict: ", |case| {
            case["expect"]["verdict"] = other_verdict(&expect["verdict"]);
        });
        fails_naming(&fixture, id, "reason: ", |case| {
            case["expect"]["reason"] = other_reason(&expect["reason"]);
        });
        for member in ["verdict", "reason"] {
            fails_naming(
                &fixture,
                id,
                &format!("fixture has no `{member}`"),
                |case| {
                    case["expect"]
                        .as_object_mut()
                        .expect("an expectation")
                        .remove(member);
                },
            );
        }
        edits += 4;
        let flipped = match case["proposed"]["purpose"].as_str() {
            Some("open") => Some("increase"),
            Some("increase") => Some("open"),
            _ => None,
        };
        if let Some(flipped) = flipped {
            fails_naming(&fixture, id, "purpose: ", |case| {
                case["proposed"]["purpose"] = json!(flipped);
            });
            edits += 1;
        }
        let stated = expect
            .get("computed")
            .and_then(Json::as_object)
            .cloned()
            .unwrap_or_default();
        for (key, value) in &stated {
            fails_naming(&fixture, id, &format!("computed.{key}: "), |case| {
                case["expect"]["computed"][key] = other_figure(key, value);
            });
            edits += 1;
        }
        for (key, value) in EVERY_FIGURE {
            if stated.contains_key(key) {
                continue;
            }
            let figure = if key == "orders_today" {
                json!(999)
            } else {
                json!(value)
            };
            fails_naming(&fixture, id, &format!("computed.{key}: "), |case| {
                case["expect"]["computed"][key] = figure;
            });
            edits += 1;
        }
    }
    assert_eq!(
        edits,
        16 * 4 + 12 + 28 + (16 * 8 - 28),
        "verdict and reason edited and dropped in each of the 16 cases, the purpose flipped in the 12 \
         openings, the 28 figures they state edited, and each of the 100 figures they do not state \
         added"
    );
}

/// MC-G02's `FULL_GATE_ONLY` entry expires when the case's own verdict is the full gate's: stated as
/// denied `working_order_limit`, the case fails, naming the entry, instead of passing through it.
#[test]
fn the_full_gate_only_entry_expires_when_the_verdicts_agree() {
    let fixture = fixture();
    fails_naming(
        &fixture,
        "MC-G02",
        "MC-G02: the full gate now gives the case's own verdict",
        |case| {
            case["expect"]["verdict"] = json!("deny");
            case["expect"]["reason"] = json!("working_order_limit");
        },
    );
}

/// MC-G13, the allowed reopen, passes through all eight checks, conduct included: E6-8 made checks
/// 5 and 6 whole, so the case the harness held pending on them is decided by the gate. The same
/// reopen five minutes before the close is denied `close_window` at check 6, so the case's path
/// runs through the conduct controls rather than around them; stated that way, with the check-7
/// figures the denial no longer reaches taken out, it passes.
#[test]
fn the_allowed_reopen_passes_the_conduct_checks_e6_8_made_whole() {
    let fixture = fixture();
    if let Err(failure) = run(fixture.clone(), "MC-G13") {
        panic!("MC-G13 must pass as the fixture states it: {failure}");
    }
    let mut in_the_close_window = fixture;
    let case = case_mut(&mut in_the_close_window, "MC-G13");
    case["state"]["now"] = json!("2026-09-21T19:55:00.000000000Z");
    case["expect"]["verdict"] = json!("deny");
    case["expect"]["reason"] = json!("close_window");
    let computed = case["expect"]["computed"]
        .as_object_mut()
        .expect("MC-G13 states computed figures");
    computed.remove("gross");
    computed.remove("gross_limit");
    if let Err(failure) = run(in_the_close_window, "MC-G13") {
        panic!("MC-G13 in the close window must be denied at check 6: {failure}");
    }
}

/// No `gate` case states a pacing, so the arm requires the gate to put none on any of them. MC-G08,
/// an allowed discretionary exit, passes at 15:00Z; moved five minutes before the close it is
/// still allowed, but the gate now paces it as a marketable limit (trading-domain §9.6's close
/// window), and the case fails naming `pacing`.
#[test]
fn an_allowed_exit_the_gate_paces_fails_since_no_case_states_a_pacing() {
    let fixture = fixture();
    if let Err(failure) = run(fixture.clone(), "MC-G08") {
        panic!("MC-G08 must pass as the fixture states it: {failure}");
    }
    fails_naming(
        &fixture,
        "MC-G08",
        "pacing: expected None, got Some(",
        |case| {
            case["state"]["now"] = json!("2026-09-21T19:55:00.000000000Z");
        },
    );
}

/// The `index`th member of the expected plan's `list`.
fn nth<'a>(case: &'a mut Json, list: &str, index: usize) -> &'a mut Json {
    &mut case["expect"][list][index]
}

/// A quantity or price moved by prefixing a digit, which keeps it valid and changes its value.
fn other_amount(value: &Json) -> Json {
    json!(format!("1{}", value.as_str().expect("a decimal string")))
}

/// Every `agent_flatten` case passes as the fixture states it, and fails, naming the member, when any
/// member of its plan is edited: the mode and purpose swapped, the cancels reordered, shortened, or
/// lengthened by another agent's order, each endpoint flipped, every member of every sell and deferred
/// sell edited, and a sell or deferred sell dropped, added, or (where there are two) reordered.
///
/// A sell's `floor_price` and `remainder` are optional in the fixture: where one is stated it is
/// edited or dropped, and where it is not it is added, so a plan that grew a floor the case does not
/// state fails.
#[test]
fn every_flatten_case_passes_and_fails_on_each_edited_expectation() {
    let fixture = fixture();
    let ids = ids_of(&fixture, "agent_flatten");
    assert_eq!(ids.len(), 4, "family F is four cases");
    let mut edits = 0;
    for id in &ids {
        if let Err(failure) = run(fixture.clone(), id) {
            panic!("{id} must pass as the fixture states it: {failure}");
        }
        let expect = case_of(&fixture, id)["expect"].clone();
        fails_naming(&fixture, id, "mode_applied_first: ", |case| {
            let mode = &mut case["expect"]["mode_applied_first"];
            *mode = if *mode == "paused" {
                json!("stopped")
            } else {
                json!("paused")
            };
        });
        fails_naming(&fixture, id, "purpose: ", |case| {
            let purpose = &mut case["expect"]["purpose"];
            *purpose = if *purpose == "risk_exit" {
                json!("owner_exit")
            } else {
                json!("risk_exit")
            };
        });
        edits += 2;

        let cancels = expect["cancel_client_order_ids"]
            .as_array()
            .expect("a cancel list")
            .clone();
        let mut reversed = cancels.clone();
        reversed.reverse();
        let mut shorter = cancels.clone();
        shorter.pop();
        let mut longer = cancels.clone();
        longer.push(json!("b-1"));
        for list in [reversed, shorter, longer] {
            fails_naming(&fixture, id, "cancel_client_order_ids: ", |case| {
                case["expect"]["cancel_client_order_ids"] = Json::Array(list);
            });
            edits += 1;
        }
        for endpoint in ["cancel_all_endpoint", "close_position_endpoint"] {
            fails_naming(&fixture, id, &format!("{endpoint}: "), |case| {
                let flag = &mut case["expect"][endpoint];
                *flag = json!(!flag.as_bool().expect("a boolean"));
            });
            edits += 1;
        }

        let sells = expect["sells"].as_array().expect("a sell list").clone();
        for index in 0..sells.len() {
            let member = |name: &str| format!("sells[{index}].{name}: ");
            fails_naming(&fixture, id, &member("instrument"), |case| {
                nth(case, "sells", index)["instrument"] =
                    json!("7b4a1c2e-9999-4a2b-9c3d-000000000009");
            });
            fails_naming(&fixture, id, &member("qty"), |case| {
                let qty = &mut nth(case, "sells", index)["qty"];
                *qty = other_amount(qty);
            });
            fails_naming(&fixture, id, &member("pricing"), |case| {
                let pricing = &mut nth(case, "sells", index)["pricing"];
                *pricing = if *pricing == "market_or_ladder" {
                    json!("exit_price_ladder")
                } else {
                    json!("market_or_ladder")
                };
            });
            fails_naming(&fixture, id, &member("floor_price"), |case| {
                let members = nth(case, "sells", index).as_object_mut().expect("a sell");
                match members.get("floor_price").cloned() {
                    Some(floor) => members.insert("floor_price".to_owned(), other_amount(&floor)),
                    None => members.insert("floor_price".to_owned(), json!("97")),
                };
            });
            fails_naming(&fixture, id, &member("remainder"), |case| {
                let members = nth(case, "sells", index).as_object_mut().expect("a sell");
                if members.remove("remainder").is_none() {
                    members.insert(
                        "remainder".to_owned(),
                        json!("rests_at_floor_then_waits_for_open"),
                    );
                }
            });
            edits += 5;
        }
        let mut dropped = sells.clone();
        dropped.pop();
        let mut added = sells.clone();
        added.push(sells.first().expect("at least one sell").clone());
        let mut lists = vec![("sells: ", dropped), ("sells: ", added)];
        if sells.len() > 1 {
            let mut swapped = sells.clone();
            swapped.reverse();
            lists.push(("sells[0].", swapped));
        }
        for (named, list) in lists {
            fails_naming(&fixture, id, named, |case| {
                case["expect"]["sells"] = Json::Array(list);
            });
            edits += 1;
        }

        let deferred = expect["deferred_sells"]
            .as_array()
            .expect("a deferred list")
            .clone();
        for index in 0..deferred.len() {
            let member = |name: &str| format!("deferred_sells[{index}].{name}: ");
            fails_naming(&fixture, id, &member("instrument"), |case| {
                nth(case, "deferred_sells", index)["instrument"] =
                    json!("7b4a1c2e-9999-4a2b-9c3d-000000000009");
            });
            fails_naming(&fixture, id, &member("qty"), |case| {
                let qty = &mut nth(case, "deferred_sells", index)["qty"];
                *qty = other_amount(qty);
            });
            fails_naming(&fixture, id, &member("until"), |case| {
                nth(case, "deferred_sells", index)["until"] = json!("next_risk_day");
            });
            edits += 3;
            fails_naming(&fixture, id, "deferred_sells: ", |case| {
                case["expect"]["deferred_sells"]
                    .as_array_mut()
                    .expect("a deferred list")
                    .remove(index);
            });
            edits += 1;
        }
        fails_naming(&fixture, id, "deferred_sells: ", |case| {
            case["expect"]["deferred_sells"]
                .as_array_mut()
                .expect("a deferred list")
                .push(
                    json!({"instrument": "7b4a1c2e-2222-4a2b-9c3d-000000000002", "qty": "10",
                             "until": "regular_session_open"}),
                );
        });
        edits += 1;
    }
    assert_eq!(
        edits,
        4 * 7 + 6 * 5 + (4 * 2 + 2) + 2 * 4 + 4,
        "seven plan-level edits in each case; five members of each of the six sells; a sell dropped \
         and added in each case and the two pairs reordered; three members of each of the two \
         deferred sells and each dropped; and a deferred sell added to each case"
    );
}

/// The flatten arm refuses an input it would otherwise have to guess at: an owner's confirmed bid with
/// no offset to floor it at, a bid stated that the owner did not confirm, and an initiator the plan
/// has no rule for.
#[test]
fn the_flatten_arm_refuses_inputs_it_cannot_read() {
    let fixture = fixture();
    fails_naming(
        &fixture,
        "MC-F03",
        "a confirmed bid needs the `max_exit_offset`",
        |case| {
            case["input"]
                .as_object_mut()
                .expect("an input")
                .remove("max_exit_offset");
        },
    );
    fails_naming(
        &fixture,
        "MC-F04",
        "the case states a `confirmed_bid` the owner did not confirm",
        |case| {
            case["input"]["confirmed_bid"] = json!("100");
        },
    );
    fails_naming(
        &fixture,
        "MC-F01",
        "`someone_else` is not a flatten initiator",
        |case| {
            case["input"]["initiator"] = json!("someone_else");
        },
    );
}
