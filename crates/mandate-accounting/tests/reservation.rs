//! The fee reservation (trading-domain spec §2.1, §7.2, §9.5), hand-calculated under the reference
//! cases' `test_default` schedule: SEC 0.00003 of a sell's notional, TAF 0.0002 a share sold, and
//! CAT 0.00001 a share either way.

mod common;

use common::{at, id, no_fees, test_default, usd};
use mandate_accounting::{AccountingError, AssetClass, ProspectiveOrder, Side, fee_reservation};
use mandate_num::{Price, Qty};

fn order(
    instrument: &str,
    asset_class: AssetClass,
    side: Side,
    qty: &str,
    limit: &str,
) -> ProspectiveOrder {
    ProspectiveOrder {
        instrument: id(instrument),
        asset_class,
        side,
        qty: Qty::parse(qty).unwrap(),
        limit: Price::parse(limit).unwrap(),
        at: at("2026-09-28T17:00:00Z"),
    }
}

fn buy(qty: &str, limit: &str) -> ProspectiveOrder {
    order("AAPL", AssetClass::UsEquity, Side::Buy, qty, limit)
}

/// An equity buy pays CAT only: 7 × 0.00001 = 0.00007, which rounds up to a cent; 1,000,000 shares
/// accrue exactly 10, which stays 10; and 1,000,001 accrue 10.00001, which a ceiling takes to 10.01
/// where half-even would leave 10.00.
#[test]
fn an_equity_buy_reserves_its_estimated_fees_rounded_up_to_the_cent() {
    let config = test_default();
    for (qty, expected) in [("7", "0.01"), ("1000000", "10"), ("1000001", "10.01")] {
        assert_eq!(
            fee_reservation(&buy(qty, "255.2"), &config),
            Ok(usd(expected)),
            "{qty} shares"
        );
    }
}

/// A schedule that charges nothing reserves nothing: the estimate is the schedule's, not a floor.
#[test]
fn a_buy_under_a_free_schedule_reserves_zero() {
    assert_eq!(
        fee_reservation(&buy("7", "255.2"), &no_fees()),
        Ok(usd("0"))
    );
}

/// A sell of 1,000 at 100 would accrue SEC 3, TAF 0.2 and CAT 0.01, yet reserves zero: only an
/// opening meets the buying-power check (§9.5). A crypto buy reserves zero because its fee is paid
/// in the asset (§7.2), and it needs no liquidity flag to say so.
#[test]
fn a_sell_and_a_crypto_buy_reserve_zero() {
    let config = test_default();
    let sell = order("AAPL", AssetClass::UsEquity, Side::Sell, "1000", "100");
    assert_eq!(fee_reservation(&sell, &config), Ok(usd("0")));
    let crypto = order("BTC/USD", AssetClass::Crypto, Side::Buy, "1", "60000");
    assert_eq!(fee_reservation(&crypto, &config), Ok(usd("0")));
}

/// An order the fold cannot price reserves nothing at all rather than zero.
#[test]
fn a_zero_quantity_buy_is_refused() {
    assert_eq!(
        fee_reservation(&buy("0", "255.2"), &test_default()),
        Err(AccountingError::ZeroQuantity)
    );
}
