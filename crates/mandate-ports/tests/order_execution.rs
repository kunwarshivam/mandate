//! `validate_order_execution` on its own: every purpose with both sides, both protection
//! mismatches, and every asset class with every time in force. Each expectation is written out
//! here, not derived from the function's own match, so the table is an independent oracle.

use mandate_accounting::{AssetClass, Side};
use mandate_num::Price;
use mandate_ports::{PortError, validate_order_execution};
use mandate_runtime::{OrderExecution, ProtectionPrices, Purpose, TimeInForce};

const DIRECTION: &str = "an order direction that cannot create a short position";
const PROTECTION: &str = "the mandate-required protective prices";
const TIF: &str = "a time in force allowed by the mandate's asset class";

fn plain() -> OrderExecution {
    OrderExecution {
        asset_class: AssetClass::UsEquity,
        tif: TimeInForce::Day,
        protection_required: false,
        protection: None,
    }
}

fn prices() -> ProtectionPrices {
    ProtectionPrices {
        stop: Price::parse("90").unwrap(),
        take_profit: None,
    }
}

fn refusal(result: Result<(), PortError>) -> Option<&'static str> {
    match result {
        Ok(()) => None,
        Err(PortError::Absent { what }) => Some(what),
        Err(other) => panic!("refused as {other:?}, not as an absent check"),
    }
}

/// Only an opening or an increase buys; every exit, protective order and flatten sells. Either
/// side the other way would open a short, which v1 never does (non-negotiable rule 12).
#[test]
fn each_purpose_takes_only_the_side_that_cannot_open_a_short() {
    let cases = [
        (Purpose::Open, Side::Buy),
        (Purpose::Increase, Side::Buy),
        (Purpose::RiskExit, Side::Sell),
        (Purpose::OwnerExit, Side::Sell),
        (Purpose::DiscretionaryExit, Side::Sell),
        (Purpose::Protective, Side::Sell),
        (Purpose::Flatten, Side::Sell),
    ];
    for (purpose, allowed) in cases {
        let other = if allowed == Side::Buy {
            Side::Sell
        } else {
            Side::Buy
        };
        assert_eq!(
            refusal(validate_order_execution(allowed, purpose, plain())),
            None,
            "{purpose:?} takes {allowed:?}"
        );
        assert_eq!(
            refusal(validate_order_execution(other, purpose, plain())),
            Some(DIRECTION),
            "{purpose:?} refuses {other:?}"
        );
    }
}

/// The protective prices are present exactly when the mandate requires them: missing ones and
/// unasked-for ones are both refused, and both agreeing ways pass.
#[test]
fn protective_prices_are_present_exactly_when_required() {
    let with = |required: bool, protection: Option<ProtectionPrices>| OrderExecution {
        protection_required: required,
        protection,
        ..plain()
    };
    let check = |execution| {
        refusal(validate_order_execution(
            Side::Buy,
            Purpose::Open,
            execution,
        ))
    };
    assert_eq!(check(with(true, Some(prices()))), None);
    assert_eq!(check(with(false, None)), None);
    assert_eq!(
        check(with(true, None)),
        Some(PROTECTION),
        "required and missing"
    );
    assert_eq!(
        check(with(false, Some(prices()))),
        Some(PROTECTION),
        "present but not required"
    );
}

/// US equities take `day` and `gtc`, never `ioc`; crypto takes `gtc` and `ioc`, never `day`.
#[test]
fn each_asset_class_takes_only_its_times_in_force() {
    let cases = [
        (AssetClass::UsEquity, TimeInForce::Day, None),
        (AssetClass::UsEquity, TimeInForce::Gtc, None),
        (AssetClass::UsEquity, TimeInForce::Ioc, Some(TIF)),
        (AssetClass::Crypto, TimeInForce::Day, Some(TIF)),
        (AssetClass::Crypto, TimeInForce::Gtc, None),
        (AssetClass::Crypto, TimeInForce::Ioc, None),
    ];
    for (asset_class, tif, expected) in cases {
        let execution = OrderExecution {
            asset_class,
            tif,
            ..plain()
        };
        assert_eq!(
            refusal(validate_order_execution(
                Side::Sell,
                Purpose::RiskExit,
                execution
            )),
            expected,
            "{asset_class:?} with {tif:?}"
        );
    }
}
