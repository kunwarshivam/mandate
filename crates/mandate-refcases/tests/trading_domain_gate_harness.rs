//! The `trading_domain` gate driver (E6-9, DEC-199) reads every key it claims and refuses what it
//! cannot read. RC-15's `status_not_active` variant, with the `agent_mode` and `actions`
//! expectations its later stories own taken out, passes as the founder wrote it; editing its
//! decision, its status or its flags makes it fail. Scenes built on RC-15's account show that each
//! `propose_order` member, each purpose, each exchange code, the fee reservation and the case's
//! `gate` configuration reach `mandate_risk::evaluate`, and that every other RC-15 variant and
//! every gate case that needs a later story still fails naming that story.

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

/// RC-15's `status_not_active` variant without the expectations E7-3 (`agent_mode`) and E7-4
/// (`actions`) own, then `edit` applied to the variant's steps.
fn status_not_active(edit: impl FnOnce(&mut Json)) -> Result<(), String> {
    let fixture = edited("RC-15", |c| {
        let steps = &mut c["variants"][0]["overrides"]["steps"];
        let first = steps[0]["expect"].as_object().unwrap();
        assert_eq!(
            first.keys().collect::<Vec<_>>(),
            ["actions", "agent_mode"],
            "RC-15::status_not_active's first step expects more than E7-3 and E7-4 own"
        );
        steps[0].as_object_mut().unwrap().remove("expect");
        edit(steps);
    });
    run(fixture, "RC-15::status_not_active")
}

/// RC-15's account (alpaca margin, $20,000 settled, 10 AAPL) with AAPL marked at 150.00 before
/// `steps`, then `edit` applied to the case.
fn scene(steps: Json, edit: impl FnOnce(&mut Json)) -> Result<(), String> {
    let fixture = edited("RC-15", |c| {
        c.as_object_mut().unwrap().remove("variants");
        let mut all = vec![json!({
            "at": "2026-09-22T09:59:00-04:00",
            "event": "mark",
            "data": { "instrument": "AAPL", "price": "150.00", "source": "quote" }
        })];
        all.extend(steps.as_array().unwrap().iter().cloned());
        c["steps"] = Json::Array(all);
        edit(c);
    });
    run(fixture, "RC-15")
}

fn proposal(at: &str, data: Json, decision: Json) -> Json {
    json!({ "at": at, "event": "propose_order", "data": data, "expect": { "decision": decision } })
}

const TEN_AM: &str = "2026-09-22T10:00:00-04:00";
const FIVE_PM: &str = "2026-09-22T17:00:00-04:00";

fn buy_msft(extra: Json) -> Json {
    let mut data = json!({
        "instrument": "MSFT", "side": "buy", "type": "limit",
        "qty": "5", "limit_price": "400.00", "purpose": "open"
    });
    for (k, v) in extra.as_object().unwrap() {
        data[k] = v.clone();
    }
    data
}

fn sell_aapl(purpose: &str) -> Json {
    json!({
        "instrument": "AAPL", "side": "sell", "type": "limit",
        "qty": "10", "limit_price": "155.00", "purpose": purpose
    })
}

fn allow() -> Json {
    json!({ "verdict": "allow" })
}

fn stop(verdict: &str, reason_code: &str) -> Json {
    json!({ "verdict": verdict, "reason_code": reason_code })
}

fn err(text: &str) -> Result<(), String> {
    Err(text.to_owned())
}

#[test]
fn rc_15_status_not_active_passes_without_the_expectations_later_stories_own() {
    assert_eq!(status_not_active(|_| {}), Ok(()));
    assert_eq!(
        run(fixture(), "RC-15::status_not_active"),
        err(
            "expectation `actions` not interpreted until E7-4; expectation `agent_mode` after `broker_account_update` not interpreted until E7-3"
        )
    );
}

#[test]
fn an_edited_decision_fails() {
    assert_eq!(
        status_not_active(|s| s[1]["expect"]["decision"]["verdict"] = json!("allow")),
        err("step 2: decision.verdict: expected allow, got deny")
    );
    assert_eq!(
        status_not_active(|s| s[1]["expect"]["decision"]["verdict"] = json!("hold")),
        err("step 2: decision.verdict: expected hold, got deny")
    );
    assert_eq!(
        status_not_active(|s| {
            s[1]["expect"]["decision"]["reason_code"] = json!("account_restricted");
        }),
        err(
            "step 2: decision.reason_code: expected account_restricted, got account_trading_blocked"
        )
    );
    assert_eq!(
        status_not_active(|s| s[1]["expect"]["decision"]["reason_code"] = json!("blocked")),
        err("step 2: reason code `blocked` is not registered in the case file")
    );
    assert_eq!(
        status_not_active(|s| s[1]["expect"]["decision"]["why"] = json!("status")),
        err("step 2: decision: unknown key `why`")
    );
    assert_eq!(
        status_not_active(|s| s[1]["expect"]["decision"]["verdict"] = json!(false)),
        err("step 2: `decision.verdict` is not a string")
    );
    assert_eq!(
        status_not_active(|s| s[1]["expect"]["decision"]["reason_code"] = json!(7)),
        err("step 2: `decision.reason_code` is not a string")
    );
    assert_eq!(
        status_not_active(|s| s[0]["expect"] = json!({ "decision": { "verdict": "deny" } })),
        err("step 1: `decision` expected on a step that made no gate decision")
    );
}

#[test]
fn a_verdict_alone_is_checked_and_a_reason_the_gate_did_not_give_fails() {
    assert_eq!(
        status_not_active(|s| {
            s[1]["expect"]["decision"] = json!({ "verdict": "deny" });
        }),
        Ok(())
    );
    assert_eq!(
        scene(
            json!([proposal(TEN_AM, buy_msft(json!({})), allow())]),
            |_| {}
        ),
        Ok(())
    );
    assert_eq!(
        scene(
            json!([proposal(
                TEN_AM,
                buy_msft(json!({})),
                stop("allow", "account_restricted")
            )]),
            |_| {}
        ),
        err("step 2: decision.reason_code: expected account_restricted, got none")
    );
}

#[test]
fn an_active_status_leaves_the_account_active_and_any_other_blocks_it() {
    assert_eq!(
        status_not_active(|s| s[0]["data"]["status"] = json!("ACTIVE")),
        err("step 2: decision.verdict: expected deny, got allow")
    );
    assert_eq!(
        status_not_active(|s| s[0]["data"]["status"] = json!("ACCOUNT_CLOSED")),
        Ok(())
    );
    assert_eq!(
        status_not_active(|s| s[0]["data"]["status"] = json!(1)),
        err("step 1: account `status` is not a string")
    );
}

#[test]
fn each_blocking_flag_is_read_true_and_false() {
    for flag in [
        "trading_blocked",
        "account_blocked",
        "trade_suspended_by_user",
    ] {
        assert_eq!(
            status_not_active(|s| {
                s[0]["data"] = json!({ "status": "ACTIVE", flag: true });
            }),
            Ok(()),
            "{flag} true"
        );
        assert_eq!(
            status_not_active(|s| {
                s[0]["data"] = json!({ "status": "ACTIVE", flag: false });
            }),
            err("step 2: decision.verdict: expected deny, got allow"),
            "{flag} false"
        );
        assert_eq!(
            status_not_active(|s| s[0]["data"] = json!({ flag: "yes" })),
            Err(format!("step 1: account `{flag}` is not a boolean")),
            "{flag} as text"
        );
    }
}

#[test]
fn a_later_active_status_never_lifts_a_detected_restriction() {
    assert_eq!(
        status_not_active(|s| {
            let steps = s.as_array_mut().unwrap();
            steps.insert(
                1,
                json!({
                    "at": "2026-09-22T10:01:00-04:00",
                    "event": "broker_account_update",
                    "data": { "status": "ACTIVE", "trading_blocked": false }
                }),
            );
        }),
        Ok(())
    );
}

#[test]
fn an_account_update_refuses_what_it_does_not_read() {
    assert_eq!(
        status_not_active(|s| s[0]["data"]["multiplier"] = json!("4")),
        err("account update field `multiplier` not interpreted until E6-6")
    );
    assert_eq!(
        status_not_active(|s| s[0]["data"]["crypto_status"] = json!("INACTIVE")),
        err("account update field `crypto_status` not interpreted until E6-10")
    );
    assert_eq!(
        status_not_active(|s| s[0]["data"]["reason"] = json!("margin call")),
        err("step 1: broker_account_update data: unknown key `reason`")
    );
    assert_eq!(
        status_not_active(|s| s[0]["data"] = json!(["ACCOUNT_UPDATED"])),
        err("step 1: broker_account_update data is not an object")
    );
}

#[test]
fn the_initial_account_status_is_read() {
    let initially = |status: Json| {
        run(
            edited("RC-15", |c| {
                let variant = &mut c["variants"][0]["overrides"];
                variant["steps"].as_array_mut().unwrap().remove(0);
                variant["initial"] = json!({
                    "account": { "cash": { "settled": "20000.00" }, "status": status },
                    "positions": [{ "instrument": "AAPL", "qty": "10", "cost_basis": "1500.00" }]
                });
            }),
            "RC-15::status_not_active",
        )
    };
    assert_eq!(initially(json!("ACCOUNT_UPDATED")), Ok(()));
    assert_eq!(
        initially(json!("ACTIVE")),
        err("step 1: decision.verdict: expected deny, got allow")
    );
    assert_eq!(
        initially(json!(true)),
        err("account `status` is not a string")
    );
    assert_eq!(
        run(
            edited("RC-15", |c| {
                let variant = &mut c["variants"][0]["overrides"];
                variant["steps"].as_array_mut().unwrap().remove(0);
                variant["initial"] = json!({
                    "account": { "cash": { "settled": "20000.00" }, "account_blocked": true },
                    "positions": [{ "instrument": "AAPL", "qty": "10", "cost_basis": "1500.00" }]
                });
            }),
            "RC-15::status_not_active",
        ),
        Ok(())
    );
}

#[test]
fn rc_03s_gate_variant_passes_and_its_quantity_side_and_instrument_are_read() {
    let variant = "RC-03::gate_rejects_zero_crossing_order";
    let with = |key: &str, value: Json| {
        run(
            edited("RC-03", |c| {
                c["variants"][0]["overrides"]["steps"][0]["data"][key] = value;
            }),
            variant,
        )
    };
    assert_eq!(run(fixture(), variant), Ok(()));
    assert_eq!(
        with("qty", json!("5")),
        err("step 1: decision.verdict: expected deny, got allow")
    );
    assert_eq!(
        with("side", json!("short")),
        err("step 1: unknown side `short`")
    );
    assert_eq!(
        with("instrument", json!("ZZZ")),
        err("step 1: unknown instrument `ZZZ`")
    );
    assert_eq!(
        with("type", json!("market")),
        err("step 1: order type `market` is not interpreted: every proposal is a limit order")
    );
    assert_eq!(
        with("tif", json!("ioc")),
        err("step 1: unknown tif Some(\"ioc\")")
    );
    assert_eq!(with("tif", json!(1)), err("step 1: unknown tif None"));
    assert_eq!(
        with("name", json!(1)),
        err("step 1: `name` is not a string")
    );
    assert_eq!(
        with("reason", json!("drawdown")),
        err("step 1: propose_order data: unknown key `reason`")
    );
    assert_eq!(
        with("purpose", json!("hedge")),
        err("step 1: `hedge` is not a purpose")
    );
    assert_eq!(
        with("qty", json!("eight")),
        err("step 1: `qty`: not a decimal number")
    );
    assert_eq!(with("name", json!("exit_1")), Ok(()));
    assert_eq!(with("tif", json!("day")), Ok(()));
}

#[test]
fn each_purpose_reaches_the_gate_as_its_proposer() {
    let at_five =
        |data: Json, decision: Json| scene(json!([proposal(FIVE_PM, data, decision)]), |_| {});
    assert_eq!(at_five(sell_aapl("risk_exit"), allow()), Ok(()));
    assert_eq!(at_five(sell_aapl("protective"), allow()), Ok(()));
    assert_eq!(
        at_five(
            sell_aapl("discretionary_exit"),
            stop("defer", "discretionary_exit_regular_session_only")
        ),
        Ok(())
    );
    assert_eq!(
        at_five(
            sell_aapl("owner_exit"),
            stop("defer", "owner_confirmation_required")
        ),
        Ok(())
    );
    assert_eq!(
        at_five(buy_msft(json!({})), stop("deny", "session_not_allowed")),
        Ok(())
    );
    assert_eq!(
        at_five(
            json!({
                "instrument": "AAPL", "side": "buy", "type": "limit",
                "qty": "1", "limit_price": "150.00", "purpose": "increase"
            }),
            stop("deny", "session_not_allowed")
        ),
        Ok(())
    );
    for purpose in ["discretionary_exit", "owner_exit"] {
        assert_eq!(
            scene(
                json!([proposal(TEN_AM, sell_aapl(purpose), allow())]),
                |_| {}
            ),
            Ok(()),
            "{purpose} in the regular session"
        );
    }
}

#[test]
fn a_stated_quote_is_the_market_the_collar_reads() {
    assert_eq!(
        scene(
            json!([proposal(
                TEN_AM,
                buy_msft(json!({ "quote": { "bid": "390.00", "ask": "391.00" } })),
                stop("deny", "price_outside_collar")
            )]),
            |_| {}
        ),
        Ok(())
    );
    assert_eq!(
        scene(
            json!([proposal(
                TEN_AM,
                buy_msft(json!({ "quote": { "bid": "399.00", "ask": "401.00" } })),
                allow()
            )]),
            |_| {}
        ),
        Ok(())
    );
    assert_eq!(
        scene(
            json!([proposal(
                TEN_AM,
                buy_msft(
                    json!({ "quote": { "bid": "399.00", "ask": "401.00", "last": "400.00" } })
                ),
                allow()
            )]),
            |_| {}
        ),
        err("step 2: quote: unknown key `last`")
    );
}

/// The listing's 90,000,000 median dollar volume puts MSFT in the liquid collar tier (DEC-199 item
/// 6): a buy at 400.00 against a 395.00 ask is above the liquid tier's 1% ceiling (398.95) and
/// inside the other tier's 2% (402.90). With the tier's threshold at the median the buy is denied
/// `price_outside_collar`, since the tier is `≥`; one cent above the median it is allowed. The
/// pair holds only for a median of exactly 90,000,000.
#[test]
fn the_filled_median_is_the_liquid_collar_tier_to_the_cent() {
    let at_threshold = |threshold: &str, decision: Json| {
        scene(
            json!([proposal(
                TEN_AM,
                buy_msft(json!({ "quote": { "bid": "394.00", "ask": "395.00" } })),
                decision
            )]),
            |c| {
                c["config_overrides"] =
                    json!({ "gate": { "collar": { "liquid_threshold_usd": threshold } } });
            },
        )
    };
    assert_eq!(
        at_threshold("90000000.00", stop("deny", "price_outside_collar")),
        Ok(())
    );
    assert_eq!(at_threshold("90000000.01", allow()), Ok(()));
}

/// The filled volumes decide the order-size participation cap for an opening (DEC-199 item 6):
/// 0.05 of the 1,000,000 trailing 5-minute volume is 50,000 shares, so a 50,000-share buy is
/// allowed and a 50,001-share one is denied `conduct_limit_breached`. The allowed buy also shows
/// the 20-day ADV is at least 1,000,000, or 0.05 of it would deny 50,000 shares under the daily
/// cap; its own 10,000,000 cannot bind first, so the unit test pins it. The account holds enough
/// cash that no other check reaches either size.
#[test]
fn the_filled_trailing_volume_is_the_order_size_cap_to_the_share() {
    let buy = |qty: &str, decision: Json| {
        scene(
            json!([proposal(TEN_AM, buy_msft(json!({ "qty": qty })), decision)]),
            |c| c["initial"]["account"]["cash"]["settled"] = json!("30000000.00"),
        )
    };
    assert_eq!(buy("50000", allow()), Ok(()));
    assert_eq!(buy("50001", stop("deny", "conduct_limit_breached")), Ok(()));
}

/// The listing's minimum order is one share (DEC-199 item 6): in a fractionable MSFT a day buy of
/// one share is allowed, and one of 0.999999999 is refused under §5.3 rule 2's minimum size, which
/// has no registered reason code yet (DEC-129 item 27).
#[test]
fn the_filled_minimum_order_is_one_share_to_the_last_place() {
    let buy = |qty: &str| {
        scene(
            json!([proposal(
                TEN_AM,
                buy_msft(json!({ "qty": qty, "tif": "day" })),
                allow()
            )]),
            |c| c["instruments"]["MSFT"]["fractionable"] = json!(true),
        )
    };
    assert_eq!(buy("1"), Ok(()));
    assert_eq!(
        buy("0.999999999"),
        err(
            "step 2: `mandate_risk::evaluate`: the reason code of §5.3 rule 2's minimum size is not implemented yet (pending DEC-129 item 27) (unimplemented)"
        )
    );
}

/// Each proposal is decided alone, with no trace of an earlier one (DEC-199 item 3), so a case
/// with a second `propose_order` step waits for the stories that give a submission its effects.
/// RC-08 passes with its one proposal and fails, naming them, with that proposal listed twice.
#[test]
fn a_second_proposal_in_one_case_waits_for_e7_4_and_e7_5() {
    let waits =
        err("a second `propose_order` step in one case not interpreted until E7-4 and E7-5");
    assert_eq!(run(fixture(), "RC-08"), Ok(()));
    assert_eq!(
        run(
            edited("RC-08", |c| {
                let steps = c["steps"].as_array_mut().unwrap();
                let proposal = steps[2].clone();
                assert_eq!(proposal["event"], "propose_order");
                steps.insert(3, proposal);
            }),
            "RC-08",
        ),
        waits
    );
    assert_eq!(
        scene(
            json!([
                proposal(TEN_AM, buy_msft(json!({})), allow()),
                proposal(TEN_AM, sell_aapl("risk_exit"), allow())
            ]),
            |_| {}
        ),
        waits
    );
}

/// An `agent_mode` expectation after an event the driver's `MODE_OWNERS` does not list, where
/// nothing moves the mode, is the account ledger's (E7-5), and fails naming it rather than as an
/// unknown key.
#[test]
fn an_agent_mode_where_nothing_moves_it_names_the_account_ledger() {
    assert_eq!(
        scene(
            json!([proposal(TEN_AM, buy_msft(json!({})), allow())]),
            |c| c["steps"][1]["expect"]["agent_mode"] = json!("normal"),
        ),
        err("step 2: expectation `agent_mode` not interpreted until E7-5")
    );
    assert_eq!(
        scene(json!([]), |c| {
            c["steps"][0]["expect"] = json!({ "agent_mode": "normal" });
        }),
        err("step 1: expectation `agent_mode` not interpreted until E7-5")
    );
}

#[test]
fn an_unmarked_or_short_position_is_refused_rather_than_valued() {
    let unmarked = |steps: Json| {
        run(
            edited("RC-15", |c| {
                c.as_object_mut().unwrap().remove("variants");
                c["steps"] = steps;
            }),
            "RC-15",
        )
    };
    assert_eq!(
        unmarked(json!([proposal(TEN_AM, buy_msft(json!({})), allow())])),
        err("step 1: `AAPL` is held with no mark, so its market value is unknown")
    );
    assert_eq!(
        unmarked(json!([proposal(
            TEN_AM,
            {
                let mut data = sell_aapl("risk_exit");
                data["quote"] = json!({ "bid": "154.00", "ask": "156.00" });
                data
            },
            allow()
        )])),
        err(
            "step 1: `AAPL` is held with no mark and its step states a quote, whose midpoint the harness does not compute"
        )
    );
    assert_eq!(
        unmarked(json!([proposal(TEN_AM, sell_aapl("risk_exit"), allow())])),
        Ok(())
    );
    assert_eq!(
        scene(
            json!([
                {
                    "at": "2026-09-22T09:59:30-04:00",
                    "event": "fill",
                    "data": { "instrument": "AAPL", "side": "sell", "qty_gross": "20", "price": "150.00" }
                },
                proposal(TEN_AM, buy_msft(json!({})), allow())
            ]),
            |_| {}
        ),
        err("step 3: `AAPL` is held short, which no v1 gate snapshot holds")
    );
}

#[test]
fn each_exchange_code_is_read_and_an_unknown_or_missing_one_is_refused() {
    let listed_on = |exchange: Option<Json>, decision: Json| {
        scene(
            json!([proposal(TEN_AM, buy_msft(json!({})), decision)]),
            |c| match exchange {
                Some(code) => c["instruments"]["MSFT"]["exchange"] = code,
                None => {
                    c["instruments"]["MSFT"]
                        .as_object_mut()
                        .unwrap()
                        .remove("exchange");
                }
            },
        )
    };
    for code in ["NASDAQ", "NYSE", "ARCA", "AMEX", "BATS"] {
        assert_eq!(listed_on(Some(json!(code)), allow()), Ok(()), "{code}");
    }
    assert_eq!(
        listed_on(Some(json!("OTC")), stop("deny", "ineligible_exchange")),
        Ok(())
    );
    assert_eq!(
        listed_on(Some(json!("LSE")), allow()),
        err("unknown exchange `LSE`")
    );
    assert_eq!(
        listed_on(Some(json!(1)), allow()),
        err("instrument `MSFT`: `exchange` is not a string")
    );
    assert_eq!(
        listed_on(None, allow()),
        err("step 2: instrument `MSFT` states no `exchange`, which the eligibility floor reads")
    );
}

/// A generic cash account with `settled` dollars, no positions, and one buy of 5 MSFT at `limit`.
fn cash_account_buys(settled: &str, limit: &str, config: &str) -> Result<(), String> {
    let fixture = edited("RC-15", |c| {
        c.as_object_mut().unwrap().remove("variants");
        c["broker_profile"] = json!("generic");
        c["config"] = json!(config);
        c["initial"] = json!({ "account": { "type": "cash", "cash": { "settled": settled } } });
        c["steps"] = json!([proposal(
            TEN_AM,
            buy_msft(json!({ "limit_price": limit })),
            allow()
        )]);
    });
    run(fixture, "RC-15")
}

#[test]
fn the_fee_reservation_and_the_limit_price_reach_buying_power() {
    assert_eq!(
        cash_account_buys("2000.01", "400.00", "test_default"),
        Ok(())
    );
    assert_eq!(
        cash_account_buys("2000.00", "400.00", "test_default"),
        err("step 1: decision.verdict: expected allow, got deny")
    );
    assert_eq!(cash_account_buys("2000.00", "400.00", "no_fees"), Ok(()));
    assert_eq!(
        cash_account_buys("2000.00", "400.01", "no_fees"),
        err("step 1: decision.verdict: expected allow, got deny")
    );
}

#[test]
fn the_cases_gate_configuration_is_read() {
    let with_gate = |gate: Json, decision: Json| {
        scene(
            json!([proposal(TEN_AM, buy_msft(json!({})), decision)]),
            |c| c["config_overrides"] = json!({ "gate": gate }),
        )
    };
    assert_eq!(
        with_gate(
            json!({ "price_floor": "400.01" }),
            stop("deny", "below_price_floor")
        ),
        Ok(())
    );
    assert_eq!(
        with_gate(json!({ "price_floor": "400.00" }), allow()),
        Ok(())
    );
    assert_eq!(
        with_gate(json!({ "max_notional": "1" }), allow()),
        err("config gate: unknown key `max_notional`")
    );
}

#[test]
fn a_fractional_proposal_needs_a_day_tif_and_a_fractionable_instrument() {
    let fractional = |tif: Option<&str>, fractionable: bool| {
        scene(
            json!([proposal(
                TEN_AM,
                {
                    let mut data = buy_msft(json!({ "qty": "1.5" }));
                    if let Some(tif) = tif {
                        data["tif"] = json!(tif);
                    }
                    data
                },
                allow()
            )]),
            |c| c["instruments"]["MSFT"]["fractionable"] = json!(fractionable),
        )
    };
    assert_eq!(
        fractional(None, true),
        err("step 2: a fractional proposal states no `tif`")
    );
    assert_eq!(fractional(Some("day"), true), Ok(()));
    assert_eq!(
        fractional(Some("gtc"), true),
        err(
            "step 2: `mandate_risk::evaluate`: the reason code of §5.3 rule 7 is not implemented yet (pending DEC-129 item 27) (unimplemented)"
        )
    );
    assert_eq!(
        fractional(Some("day"), false),
        err(
            "step 2: `mandate_risk::evaluate`: the reason code of §5.3 rule 2's increment is not implemented yet (pending DEC-129 item 27) (unimplemented)"
        )
    );
    assert_eq!(
        scene(
            json!([proposal(TEN_AM, buy_msft(json!({ "tif": "gtc" })), allow())]),
            |_| {}
        ),
        Ok(())
    );
}

/// RC-09 and RC-09B are not pinned here: E6-6's harness changes their pending reasons step by step,
/// and each case's own trial in `tests/refcases.rs`, gated by `status.toml`, is its test (the
/// coordinator's ruling on #370, item 1).
#[test]
fn every_other_rc_15_variant_and_gate_case_names_the_story_it_waits_for() {
    let pending = [
        (
            "RC-15",
            "`broker_order_update` steps not interpreted until E7-2; a second `propose_order` step in one case not interpreted until E7-4 and E7-5; expectation `actions` not interpreted until E7-4; expectation `agent_mode` after `broker_order_update` not interpreted until E7-3",
        ),
        (
            "RC-15::unexplained_403s",
            "`broker_order_update` steps not interpreted until E7-2; expectation `actions` not interpreted until E7-4; expectation `agent_mode` after `broker_order_update` not interpreted until E7-3",
        ),
        (
            "RC-15::external_order_detected",
            "`broker_order_update` steps not interpreted until E7-2; expectation `actions` not interpreted until E7-4; expectation `agent_mode` after `broker_order_update` not interpreted until E7-5",
        ),
        (
            "RC-11",
            "expectation `agent_mode` after `fill` not interpreted until E7-5; external fills (`source: external`) not interpreted until E7-5",
        ),
        (
            "RC-14::kill_switch",
            "`kill_switch` steps not interpreted until E6-5; expectation `actions` not interpreted until E7-4; expectation `agent_mode` after `kill_switch` not interpreted until E6-5; initial `open_orders` not interpreted until E7-4",
        ),
        (
            "RC-17",
            "`deploy_agent` steps not interpreted until E7-5; a second `propose_order` step in one case not interpreted until E7-4 and E7-5; expectation `buying_power` after `propose_order` not interpreted until E7-5; initial `agents` not interpreted until E7-5; proposal field `agent` not interpreted until E7-5",
        ),
        (
            "RC-22",
            "`broker_order_update` steps not interpreted until E7-2; `conduct_breach` steps not interpreted until E6-8; a second `propose_order` step in one case not interpreted until E7-4 and E7-5; expectation `actions` not interpreted until E7-4; expectation `agent_mode` after `conduct_breach` not interpreted until E6-11; instrument field `median_dollar_volume_20d` not interpreted until E6-7",
        ),
        (
            "RC-24::presumed_halt_regular_session",
            "expectation `actions` not interpreted until E7-4; instrument field `median_dollar_volume_20d` not interpreted until E6-7; proposal field `last_good_quote` not interpreted until E7-4; proposal field `pricing` not interpreted until E7-4; proposal field `status_feed` not interpreted until E7-4",
        ),
        (
            "RC-25",
            "a second `propose_order` step in one case not interpreted until E7-4 and E7-5; instrument field `median_dollar_volume_20d` not interpreted until E6-7; instrument field `prior_close` not interpreted until E6-7; proposal field `owner_confirmed_bid` not interpreted until E6-8",
        ),
    ];
    for (case, wanted) in pending {
        assert_eq!(run(fixture(), case), err(wanted), "{case}");
    }
    assert_eq!(
        scene(
            json!([proposal(TEN_AM, buy_msft(json!({})), allow())]),
            |c| {
                c["initial"]["account"]["crypto_status"] = json!("INACTIVE");
            }
        ),
        err("initial account `crypto_status` not interpreted until E6-10")
    );
}

#[test]
fn the_gate_cases_the_driver_completes_pass() {
    for case in [
        "RC-03::gate_rejects_zero_crossing_order",
        "RC-08",
        "RC-18::generic_cash_account",
    ] {
        assert_eq!(run(fixture(), case), Ok(()), "{case}");
    }
}
