//! The two maintenance figures trading-domain spec §7.2 and §9.2 read from the broker, parsed from
//! Alpaca's account answer (the first paper trade brief's X-9, slice A1; DEC-524).
//!
//! `last_equity` is the prior close's equity (§7.2 names it); `maintenance_margin` is the
//! requirement from which the executor derives the maintenance excess §9.2's `intraday_margin`
//! regime checks. The recorded accounts carry both, at the values a flat 1,000,000 USD paper
//! account answers, so the cases that must tell the members apart edit the recorded body to
//! distinct values and compute each expectation by hand.

mod common;

use common::scenario;
use mandate_accounting::AccountType;
use mandate_alpaca::error::WireError;
use mandate_alpaca::{AccountRules, DeclaredRegime, alpaca_account_rules, wire};
use mandate_num::Usd;
use serde_json::{Value, json};

fn recorded(name: &str) -> Vec<u8> {
    scenario(name)
        .exchanges
        .first()
        .map(|exchange| exchange.response.clone())
        .unwrap_or_else(|| panic!("{name} has a recorded answer"))
}

fn usd(text: &str) -> Usd {
    Usd::parse(text).unwrap_or_else(|error| panic!("{text} is canonical: {error}"))
}

/// The recorded `account_active` answer with `edits` applied, member by member.
fn edited(edits: &[(&str, Option<Value>)]) -> Vec<u8> {
    let mut account: Value =
        serde_json::from_slice(&recorded("account_active")).unwrap_or_else(|e| panic!("{e}"));
    let object = account
        .as_object_mut()
        .unwrap_or_else(|| panic!("the recorded account is an object"));
    for (member, value) in edits {
        match value {
            Some(value) => object.insert((*member).to_owned(), value.clone()),
            None => object.remove(*member),
        };
    }
    serde_json::to_vec(&account).unwrap_or_else(|e| panic!("{e}"))
}

/// Equity 1,000,000; the prior close 987,654.32; a maintenance requirement of 12,345.67, beside
/// `last_maintenance_margin` and `initial_margin` values that a parser reading the wrong member
/// would return instead.
fn distinct() -> Vec<u8> {
    edited(&[
        ("equity", Some(json!("1000000"))),
        ("last_equity", Some(json!("987654.32"))),
        ("maintenance_margin", Some(json!("12345.67"))),
        ("last_maintenance_margin", Some(json!("999.99"))),
        ("initial_margin", Some(json!("55555.55"))),
    ])
}

#[test]
fn the_recorded_accounts_carry_the_prior_close_equity_and_the_maintenance_margin() {
    for name in ["account_active", "account_blocked"] {
        let account =
            wire::account(&recorded(name)).unwrap_or_else(|error| panic!("{name} parses: {error}"));
        assert_eq!(
            (account.last_equity, account.maintenance_margin),
            (usd("1000000"), Usd::ZERO),
            "{name} records `last_equity` \"1000000\" and `maintenance_margin` \"0\""
        );
    }
}

#[test]
fn each_figure_is_read_from_its_own_member() {
    let account = wire::account(&distinct()).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(account.equity, usd("1000000"));
    assert_eq!(
        account.last_equity,
        usd("987654.32"),
        "the prior close is `last_equity`, not `equity`"
    );
    assert_eq!(
        account.maintenance_margin,
        usd("12345.67"),
        "the requirement is `maintenance_margin`, not `last_maintenance_margin` or \
         `initial_margin`"
    );
}

#[test]
fn an_account_missing_either_figure_is_refused() {
    for member in ["last_equity", "maintenance_margin"] {
        assert_eq!(
            wire::account(&edited(&[(member, None)])),
            Err(WireError::MissingField { field: member }),
            "an account answer without `{member}` is refused, never read as zero (AGENTS.md \
             rule 3)"
        );
    }
}

#[test]
fn a_figure_sent_as_a_json_number_is_refused() {
    for member in ["last_equity", "maintenance_margin"] {
        assert_eq!(
            wire::account(&edited(&[(member, Some(json!(12345.67)))])),
            Err(WireError::FloatNumber { field: member }),
            "`{member}` as a JSON number has been through a float (ES-23)"
        );
    }
}

#[test]
fn a_recorded_flat_accounts_maintenance_excess_is_its_whole_equity() {
    for name in ["account_active", "account_blocked"] {
        let account =
            wire::account(&recorded(name)).unwrap_or_else(|error| panic!("{name} parses: {error}"));
        assert_eq!(
            account.maintenance_excess(),
            Ok(usd("1000000")),
            "{name}: equity 1000000 less maintenance margin 0"
        );
    }
}

#[test]
fn the_parsed_accounts_maintenance_excess_is_equity_less_its_maintenance_margin() {
    let account = wire::account(&distinct()).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        account.maintenance_excess(),
        Ok(usd("987654.33")),
        "1000000 − 12345.67 = 987654.33: neither the prior close (987654.32) nor the last \
         requirement (1000000 − 999.99) nor the initial margin (1000000 − 55555.55)"
    );
}

/// Trading spec §7.2 states Alpaca's account rules per broker: "Account type: Alpaca: always
/// margin" and "Day-trading regime: Alpaca: `intraday_margin`". The connector declares exactly
/// those, and the shell takes them from here (DEC-840).
#[test]
fn alpacas_declared_account_rules_are_section_7_2s() {
    assert_eq!(
        alpaca_account_rules(),
        AccountRules {
            account_type: AccountType::Margin,
            regime: DeclaredRegime::IntradayMargin,
        }
    );
}
