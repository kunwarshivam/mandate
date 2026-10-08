//! Maintenance excess, as trading-domain spec §9.2's `intraday_margin` regime reads it: the
//! broker's equity less its maintenance margin, positive an excess and negative a deficit (the first
//! paper trade brief's X-9, slice A1; DEC-524).
//!
//! The hand cases compute each expectation by hand. The property's oracle is whole cents in `i128`,
//! formatted as decimal text and parsed once, so it shares no arithmetic with `mandate-num`.

mod common;

use common::{broker_account, usd};
use mandate_executor::{BrokerAccount, ExecutorError};
use mandate_num::{NumError, Usd};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

/// The largest magnitude a [`Usd`] holds: 2^96 − 1 units at scale 0.
const USD_LIMIT: &str = "79228162514264337593543950335";

fn account(equity: &str, maintenance_margin: &str) -> BrokerAccount {
    BrokerAccount {
        equity: usd(equity),
        maintenance_margin: usd(maintenance_margin),
        ..broker_account()
    }
}

#[test]
fn maintenance_excess_is_equity_less_the_maintenance_margin() {
    for (equity, margin, excess) in [
        ("20000", "0", "20000"),
        ("20000", "4999.99", "15000.01"),
        ("1000000", "12345.67", "987654.33"),
        ("20000", "20000", "0"),
    ] {
        assert_eq!(
            account(equity, margin).maintenance_excess(),
            Ok(usd(excess)),
            "{equity} − {margin} = {excess}"
        );
    }
}

#[test]
fn a_requirement_above_equity_is_a_negative_excess_a_deficit() {
    for (equity, margin, deficit) in [
        ("20000", "25000.5", "-5000.5"),
        ("0", "0.01", "-0.01"),
        ("-100", "50", "-150"),
    ] {
        assert_eq!(
            account(equity, margin).maintenance_excess(),
            Ok(usd(deficit)),
            "{equity} − {margin} = {deficit}: §9.2's deficit is reported, never floored at zero"
        );
    }
}

#[test]
fn the_excess_reads_neither_the_prior_close_nor_the_buying_power() {
    let given = BrokerAccount {
        equity: usd("50000"),
        last_equity: usd("70000"),
        cash: usd("30000"),
        buying_power: usd("200000"),
        non_marginable_buying_power: usd("30000"),
        maintenance_margin: usd("12500"),
        ..broker_account()
    };
    assert_eq!(
        given.maintenance_excess(),
        Ok(usd("37500")),
        "50000 − 12500, the current equity's excess: not the prior close's 57500, the cash's \
         17500, or any buying-power figure"
    );
}

#[test]
fn a_negative_maintenance_margin_is_refused_rather_than_raising_the_excess() {
    for margin in ["-0.01", "-12500"] {
        assert_eq!(
            account("20000", margin).maintenance_excess(),
            Err(ExecutorError::Num(NumError::Negative)),
            "a requirement of {margin} would raise the excess above equity, so it is refused \
             (DEC-524 item 2, AGENTS.md rule 3)"
        );
    }
}

#[test]
fn an_excess_beyond_the_usd_range_is_refused_not_wrapped() {
    let given = account(&format!("-{USD_LIMIT}"), "1");
    assert_eq!(
        given.maintenance_excess(),
        Err(ExecutorError::Num(NumError::Overflow)),
        "−(2^96 − 1) − 1 does not fit a Usd, so the difference is refused, never clamped"
    );
}

fn check<S>(strategy: S, body: impl Fn(S::Value) -> Result<(), TestCaseError>)
where
    S: Strategy,
    S::Value: std::fmt::Debug,
{
    let config = ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    if let Err(failure) = proptest::test_runner::TestRunner::new(config).run(&strategy, body) {
        panic!("{failure}");
    }
}

/// Whole cents as canonical decimal text, written without `mandate-num`: a sign, the whole
/// dollars, and the cents with trailing zeros dropped.
fn cents_text(cents: i128) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let magnitude = cents.unsigned_abs();
    let (dollars, rest) = (magnitude / 100, magnitude % 100);
    match rest {
        0 => format!("{sign}{dollars}"),
        r if r % 10 == 0 => format!("{sign}{dollars}.{}", r / 10),
        r => format!("{sign}{dollars}.{r:02}"),
    }
}

/// For every equity and every non-negative requirement in whole cents, the excess is the oracle's
/// `i128` difference: an excess when the requirement is below equity, zero at it, and a deficit
/// above it, with no clamp and no rounding.
#[test]
fn the_excess_is_the_whole_cent_difference_for_every_account() {
    let equity = -10_000_000_000_i64..=10_000_000_000_000_i64;
    let margin = 0_i64..=10_000_000_000_000_i64;
    check((equity, margin), |(equity, margin)| {
        let expected = Usd::parse(&cents_text(i128::from(equity) - i128::from(margin)))
            .map_err(|error| TestCaseError::fail(format!("the oracle's text: {error}")))?;
        let given = BrokerAccount {
            equity: usd(&cents_text(i128::from(equity))),
            maintenance_margin: usd(&cents_text(i128::from(margin))),
            ..broker_account()
        };
        prop_assert_eq!(given.maintenance_excess(), Ok(expected));
        Ok(())
    });
}
