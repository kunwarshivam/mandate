//! The shared fund-movement name check (DEC-839 item 3, as DEC-676 amends it; E7-12 with E7-16).
//!
//! The oracle is typed here from the decision's text, never computed by the code under test: each
//! row gives a name, its parts, and its verdict. The token set is typed a second time in
//! [`SET`] so the property's oracle does not read [`FUND_MOVEMENT_TOKENS`]. Every test but the one
//! pinning the set is pending: the functions are `todo!()` stubs until E7-12's implementation PR.

use mandate_domain::fund_movement::{FUND_MOVEMENT_TOKENS, is_fund_movement_name, name_tokens};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};

/// DEC-839 item 3's ten tokens and DEC-676's five, typed from the decisions.
const SET: [&str; 15] = [
    "transfer",
    "transfers",
    "withdraw",
    "withdrawal",
    "withdrawals",
    "wire",
    "ach",
    "send",
    "payout",
    "disburse",
    "deposit",
    "deposits",
    "fund",
    "funds",
    "funding",
];

/// A name, its parts as DEC-839 item 3 splits them, and whether it moves funds.
const TABLE: &[(&str, &[&str], bool)] = &[
    ("transfer", &["transfer"], true),
    ("transfers", &["transfers"], true),
    ("withdraw", &["withdraw"], true),
    ("withdrawal", &["withdrawal"], true),
    ("withdrawals", &["withdrawals"], true),
    ("wire", &["wire"], true),
    ("ach", &["ach"], true),
    ("send", &["send"], true),
    ("payout", &["payout"], true),
    ("disburse", &["disburse"], true),
    ("deposit", &["deposit"], true),
    ("deposits", &["deposits"], true),
    ("fund", &["fund"], true),
    ("funds", &["funds"], true),
    ("funding", &["funding"], true),
    ("create_transfer", &["create", "transfer"], true),
    ("list_transfers", &["list", "transfers"], true),
    ("request_withdraw", &["request", "withdraw"], true),
    ("new-withdrawal-v2", &["new", "withdrawal", "v2"], true),
    ("cancel.withdrawals", &["cancel", "withdrawals"], true),
    ("Wire Out", &["wire", "out"], true),
    (
        "initiate-ach-transfer",
        &["initiate", "ach", "transfer"],
        true,
    ),
    ("sendMoney", &["send", "money"], true),
    ("createPayout", &["create", "payout"], true),
    ("DisburseNow", &["disburse", "now"], true),
    ("makeDeposit", &["make", "deposit"], true),
    ("get_deposits", &["get", "deposits"], true),
    ("addFund", &["add", "fund"], true),
    ("withdrawFunds", &["withdraw", "funds"], true),
    ("instant_funding", &["instant", "funding"], true),
    (
        "createAchRelationship",
        &["create", "ach", "relationship"],
        true,
    ),
    ("getACH", &["get", "ach"], true),
    ("ACH_Debit", &["ach", "debit"], true),
    ("get_transfers", &["get", "transfers"], true),
    ("wire.out", &["wire", "out"], true),
    ("Withdraw Funds", &["withdraw", "funds"], true),
    ("TRANSFER", &["transfer"], true),
    ("x/wire:y", &["x", "wire", "y"], true),
    ("wireé", &["wire"], true),
    ("fundéx", &["fund", "x"], true),
    ("café_send", &["caf", "send"], true),
    ("ACHDebit", &["ach", "debit"], true),
    ("wireXfer", &["wire", "xfer"], true),
    ("getACHStatus", &["get", "ach", "status"], true),
    ("URLParser", &["url", "parser"], false),
    ("HTTPSend", &["http", "send"], true),
    ("HTTPSEnd", &["https", "end"], false),
    ("ACHWIRE", &["achwire"], false),
    ("v2Transfer", &["v2transfer"], false),
    ("ach2", &["ach2"], false),
    ("wire2wire", &["wire2wire"], false),
    ("wіre", &["w", "re"], false),
    ("", &[], false),
    ("-_. /", &[], false),
    ("é", &[], false),
    ("get_fundamentals", &["get", "fundamentals"], false),
    ("refund_status", &["refund", "status"], false),
    ("wireless", &["wireless"], false),
    ("resend", &["resend"], false),
    ("teacher", &["teacher"], false),
    ("get_quote", &["get", "quote"], false),
    ("place_equity_order", &["place", "equity", "order"], false),
    ("getRefunds", &["get", "refunds"], false),
    ("Transferable", &["transferable"], false),
];

#[test]
fn the_token_set_is_exactly_the_fifteen_sorted() {
    assert_eq!(
        FUND_MOVEMENT_TOKENS,
        [
            "ach",
            "deposit",
            "deposits",
            "disburse",
            "fund",
            "funding",
            "funds",
            "payout",
            "send",
            "transfer",
            "transfers",
            "wire",
            "withdraw",
            "withdrawal",
            "withdrawals",
        ],
        "DEC-839 item 3's ten and DEC-676's five, sorted by byte value"
    );
    let mut typed = SET;
    typed.sort_unstable();
    assert_eq!(
        FUND_MOVEMENT_TOKENS, typed,
        "the set typed from the decisions"
    );
}

#[test]
#[ignore = "pending E7-12"]
fn every_name_splits_into_the_parts_the_decision_names() {
    for &(name, parts, _) in TABLE {
        assert_eq!(name_tokens(name), parts, "the parts of {name:?}");
    }
}

#[test]
#[ignore = "pending E7-12"]
fn every_name_gets_the_verdict_the_decision_names() {
    for &(name, _, moves_funds) in TABLE {
        assert_eq!(
            is_fund_movement_name(name),
            moves_funds,
            "whether {name:?} moves funds"
        );
    }
}

#[test]
#[ignore = "pending E7-12"]
fn every_token_is_refused_whole_in_any_case_and_inside_a_name() {
    for token in SET {
        let upper = token.to_ascii_uppercase();
        let (head, tail) = token.split_at(1);
        let capital = format!("{}{tail}", head.to_ascii_uppercase());
        for name in [
            token.to_owned(),
            upper.clone(),
            capital.clone(),
            format!("get_{token}"),
            format!("{token}-now"),
            format!("a.{upper}.b"),
            format!("doThe{capital}Now"),
            format!("x9 {token}"),
            format!("{upper}Now"),
            format!("GET{capital}"),
        ] {
            assert!(is_fund_movement_name(&name), "{name:?} names {token:?}");
        }
        for name in [
            format!("{token}x"),
            format!("re{token}"),
            format!("{token}9"),
            format!("{upper}NOW"),
        ] {
            assert!(
                !is_fund_movement_name(&name),
                "{name:?} has {token:?} only inside a longer part"
            );
        }
    }
}

/// A word of the generated name: its text, and the part the rule makes of it.
fn word() -> impl Strategy<Value = (String, String)> {
    let base = prop_oneof![
        proptest::sample::select(SET.to_vec()).prop_map(str::to_owned),
        "[a-z][a-z0-9]{0,7}",
    ];
    (base, 0..3u8).prop_map(|(lower, style)| {
        let text = match style {
            0 => lower.clone(),
            1 => lower.to_ascii_uppercase(),
            _ => {
                let (head, tail) = lower.split_at(1);
                format!("{}{tail}", head.to_ascii_uppercase())
            }
        };
        (text, lower)
    })
}

/// Words joined by runs of separators, or by nothing where a lowercase letter meets a capital or a
/// capital meets a capital followed by a lowercase letter (DEC-676's acronym split). The separators
/// include non-ASCII letters, which the ASCII reading also splits at.
#[test]
#[ignore = "pending E7-12"]
fn a_name_built_from_words_splits_into_them_and_is_refused_iff_one_is_in_the_set() {
    let names = proptest::collection::vec((word(), "[-_. /:é·і]{0,3}"), 1..6);
    let mut runner = TestRunner::new(Config {
        cases: 512,
        failure_persistence: None,
        ..Config::default()
    });
    let outcome = runner.run(&names, |words| {
        let mut name = String::new();
        let mut parts: Vec<String> = Vec::new();
        for ((text, part), separator) in words {
            let mut next = text.chars();
            let opens_capital = next.next().is_some_and(|c| c.is_ascii_uppercase());
            let then_lower = next.next().is_some_and(|c| c.is_ascii_lowercase());
            let camel = opens_capital
                && (name.ends_with(|c: char| c.is_ascii_lowercase())
                    || (then_lower && name.ends_with(|c: char| c.is_ascii_uppercase())));
            if separator.is_empty() && !name.is_empty() && !camel {
                continue;
            }
            name.push_str(&separator);
            name.push_str(&text);
            parts.push(part);
        }
        let moves_funds = parts.iter().any(|part| SET.contains(&part.as_str()));
        prop_assert_eq!(name_tokens(&name), parts, "the parts of {:?}", name);
        prop_assert_eq!(is_fund_movement_name(&name), moves_funds, "{:?}", name);
        Ok(())
    });
    assert_eq!(outcome, Ok(()));
}
