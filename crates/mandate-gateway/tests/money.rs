//! Fixed-point USD (inference spec §7.1, §7.2): exact text, the maximum rounded up, the cost rounded
//! half up, and cached tokens never priced above fresh ones.

use mandate_canon::Digest;
use mandate_gateway::money::{MoneyError, PriceError, PriceTable, TokenPrice, Usage, Usd6};
use proptest::prelude::*;

fn usd(text: &str) -> Usd6 {
    Usd6::parse(text).unwrap()
}

fn table(input: &str, output: &str, cached: &str) -> PriceTable {
    PriceTable::new(
        TokenPrice::per_million(usd(input)),
        TokenPrice::per_million(usd(output)),
        TokenPrice::per_million(usd(cached)),
        Digest::of(b"prices"),
    )
    .unwrap()
}

#[test]
fn usd6_reads_and_writes_the_journals_decimal_text() {
    for (text, micros, shown) in [
        ("0", 0, "0"),
        ("50", 50_000_000, "50"),
        ("0.0117", 11_700, "0.0117"),
        ("0.000001", 1, "0.000001"),
        ("12.5", 12_500_000, "12.5"),
        ("7.100000", 7_100_000, "7.1"),
    ] {
        let value = usd(text);
        assert_eq!(value.micros(), micros, "{text}");
        assert_eq!(value.to_string(), shown, "{text}");
        assert_eq!(Usd6::from_micros(micros), value);
    }
    for bad in [
        "",
        ".",
        "1.",
        ".5",
        "-1",
        "1e3",
        "0.0000001",
        "1,5",
        " 1",
        "+1",
        "0x1",
        "18446744073709.551616",
    ] {
        assert!(Usd6::parse(bad).is_err(), "{bad:?}");
    }
    assert_eq!(Usd6::parse("0.0000001"), Err(MoneyError::NotUsd6));
    assert_eq!(
        Usd6::parse("99999999999999999999"),
        Err(MoneyError::Overflow)
    );
    assert_eq!(Usd6::dollars(50), Ok(usd("50")));
    assert_eq!(Usd6::dollars(u64::MAX), Err(MoneyError::Overflow));
    assert_eq!(usd("1").checked_add(usd("0.5")), Ok(usd("1.5")));
    assert_eq!(
        Usd6::from_micros(u64::MAX).checked_add(Usd6::from_micros(1)),
        Err(MoneyError::Overflow)
    );
    assert_eq!(usd("1").saturating_sub(usd("3")), Usd6::ZERO);
    assert_eq!(usd("3").saturating_sub(usd("1")), usd("2"));
}

#[test]
fn the_maximum_prices_every_prompt_byte_as_a_fresh_token_and_rounds_up() {
    let t = table("3", "15", "0.3");
    assert_eq!(
        t.max_cost(1_000, 1_000),
        Ok(usd("0.018")),
        "1000 × $3/M + 1000 × $15/M"
    );
    assert_eq!(t.max_cost(1, 0), Ok(usd("0.000003")));
    let cheap = table("0.15", "0.6", "0.075");
    assert_eq!(
        cheap.max_cost(1, 0),
        Ok(usd("0.000001")),
        "0.00000015 rounds up to one micro-dollar"
    );
    assert_eq!(cheap.max_cost(0, 0), Ok(Usd6::ZERO));
    assert_eq!(
        cheap.max_cost(10, 0),
        Ok(usd("0.000002")),
        "0.0000015 rounds up"
    );
    assert_eq!(t.max_cost(u64::MAX, u32::MAX), Err(MoneyError::Overflow));
}

#[test]
fn the_cost_prices_cached_tokens_at_the_cached_rate_and_rounds_half_up() {
    let t = table("3", "15", "0.3");
    let u = Usage {
        input: 1_000,
        output: 100,
        cached: 400,
    };
    assert_eq!(
        t.cost(u),
        Ok(usd("0.00342")),
        "600 × 3 + 400 × 0.3 + 100 × 15, per million"
    );
    let cheap = table("0.15", "0.6", "0.075");
    assert_eq!(
        cheap.cost(Usage {
            input: 10,
            output: 0,
            cached: 0
        }),
        Ok(usd("0.000002")),
        "1.5 micro rounds half up"
    );
    assert_eq!(
        cheap.cost(Usage {
            input: 3,
            output: 0,
            cached: 0
        }),
        Ok(Usd6::ZERO),
        "0.45 micro rounds down"
    );
    assert_eq!(
        t.cost(Usage {
            input: 10,
            output: 0,
            cached: 50
        }),
        Ok(usd("0.000003")),
        "cached beyond the input count is capped at it"
    );
    assert_eq!(
        t.cost(Usage {
            input: u64::MAX,
            output: u64::MAX,
            cached: 0
        }),
        Err(MoneyError::Overflow)
    );
}

#[test]
fn a_cached_token_is_never_priced_above_a_fresh_one() {
    let err = PriceTable::new(
        TokenPrice::per_million(usd("1")),
        TokenPrice::per_million(usd("1")),
        TokenPrice::per_million(usd("1.000001")),
        Digest::of(b"p"),
    );
    assert_eq!(err.err(), Some(PriceError::CachedAboveInput));
    assert!(
        PriceTable::new(
            TokenPrice::per_million(usd("1")),
            TokenPrice::per_million(usd("2")),
            TokenPrice::per_million(usd("1")),
            Digest::of(b"p")
        )
        .is_ok()
    );
    assert_eq!(TokenPrice::per_million(usd("2")).amount(), usd("2"));
    assert_eq!(table("1", "2", "0.5").version(), Digest::of(b"prices"));
}

proptest! {
    /// The maximum bounds the cost of every reply within the request's bounds: no more prompt
    /// tokens than bytes and no more output tokens than allowed (journal spec §9.19).
    #[test]
    fn the_maximum_bounds_every_honest_cost(
        bytes in 0u64..200_000, max_out in 0u32..20_000, input_frac in 0u64..=100, output_frac in 0u64..=100,
        cached_frac in 0u64..=100, input in 0u64..100_000_000, output in 0u64..100_000_000, cached in 0u64..=100_000_000,
    ) {
        let cached = cached.min(input);
        let t = PriceTable::new(
            TokenPrice::per_million(Usd6::from_micros(input)),
            TokenPrice::per_million(Usd6::from_micros(output)),
            TokenPrice::per_million(Usd6::from_micros(cached)),
            Digest::of(b"p"),
        ).unwrap();
        let used = Usage {
            input: bytes * input_frac / 100,
            output: u64::from(max_out) * output_frac / 100,
            cached: bytes * input_frac / 100 * cached_frac / 100,
        };
        let max = t.max_cost(bytes, max_out).unwrap();
        let cost = t.cost(used).unwrap();
        prop_assert!(cost <= max, "{cost} > {max}");
        let exact = u128::from(used.input - used.cached) * u128::from(input)
            + u128::from(used.cached) * u128::from(cached)
            + u128::from(used.output) * u128::from(output);
        prop_assert_eq!(u128::from(cost.micros()), (exact * 2 + 1_000_000) / 2_000_000, "half up");
        let bound = u128::from(bytes) * u128::from(input) + u128::from(max_out) * u128::from(output);
        prop_assert_eq!(u128::from(max.micros()), bound.div_ceil(1_000_000), "ceiling");
    }

    #[test]
    fn usd6_text_round_trips(micros in 0u64..u64::MAX / 2) {
        let value = Usd6::from_micros(micros);
        prop_assert_eq!(Usd6::parse(&value.to_string()), Ok(value));
    }
}
