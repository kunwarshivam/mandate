//! The live opening's cross-check over the preflight's own quotes (DEC-933 item 2; DEC-529 item
//! 12): Robinhood's ask, the broker's own, must sit within the registered collar of the IEX ask
//! Alpaca's data client read, both of one instrument, or the opening refuses. The quotes are
//! literals; nothing reaches a host (`AGENTS.md` rule 8).

use mandate_accounting::InstrumentId;
use mandate_alpaca::{Feed, LatestQuote};
use mandate_num::{Fraction, Price, Qty, Usd};
use mandate_paper::{CollarMembers, PaperError, QuoteRefusal, robinhood_cross_check};
use mandate_robinhood::Quote;
use mandate_time::UtcNanos;

fn collar() -> CollarMembers {
    CollarMembers {
        liquid_x: Fraction::parse("0.01").expect("the liquid collar parses"),
        other_x: Fraction::parse("0.02").expect("the other collar parses"),
        crypto_x: Fraction::parse("0.005").expect("the crypto collar parses"),
        liquid_threshold_usd: Usd::parse("50000000").expect("the threshold parses"),
    }
}

fn price(text: &str) -> Price {
    Price::parse(text).expect("the price parses")
}

fn robinhood(symbol: &str, bid: &str, ask: &str) -> Quote {
    Quote {
        symbol: InstrumentId::new(symbol).expect("the symbol parses"),
        bid: price(bid),
        ask: price(ask),
    }
}

/// The IEX quote of SPY at an ask of 100.
fn iex() -> LatestQuote {
    LatestQuote {
        instrument: InstrumentId::new("SPY").expect("the symbol parses"),
        at: UtcNanos::parse_rfc3339("2026-10-16T15:00:00Z").expect("the instant parses"),
        bid: price("99.9"),
        bid_size: Qty::parse("1").expect("the size parses"),
        ask: price("100"),
        ask_size: Qty::parse("1").expect("the size parses"),
        feed: Feed::Iex,
    }
}

fn liquid() -> Option<Usd> {
    Some(Usd::parse("50000000").expect("the volume parses"))
}

fn illiquid() -> Option<Usd> {
    Some(Usd::parse("49999999").expect("the volume parses"))
}

/// Robinhood's ask is compared, not its bid, with the IEX ask, within the liquid tier's 1% band
/// at both ends: 101 and 99 agree, 101.01 and 98.99 do not.
#[test]
fn robinhoods_ask_must_sit_within_the_collar_of_the_iex_ask() {
    for ask in ["100", "101", "99"] {
        let agreed =
            robinhood_cross_check(&collar(), &robinhood("SPY", "1", ask), &iex(), liquid());
        assert!(matches!(agreed, Ok(())), "{ask}: {agreed:?}");
    }
    for ask in ["101.01", "98.99"] {
        let refused =
            robinhood_cross_check(&collar(), &robinhood("SPY", "99", ask), &iex(), liquid());
        assert!(
            matches!(
                refused,
                Err(PaperError::CrossCheck(QuoteRefusal::Disagreement))
            ),
            "{ask}: {refused:?}"
        );
    }
}

/// The band is the registered collar's own tier for the instrument: below the liquid threshold
/// the other member's 2% admits a 1.5% gap the liquid member refuses, an unknown volume reads the
/// narrower liquid member, and an equity never reads the crypto member.
#[test]
fn the_band_is_the_registered_tier_for_the_instruments_volume() {
    let gap = robinhood("SPY", "101", "101.5");
    let admitted = robinhood_cross_check(&collar(), &gap, &iex(), illiquid());
    assert!(matches!(admitted, Ok(())), "{admitted:?}");
    for volume in [liquid(), None] {
        let refused = robinhood_cross_check(&collar(), &gap, &iex(), volume);
        assert!(
            matches!(
                refused,
                Err(PaperError::CrossCheck(QuoteRefusal::Disagreement))
            ),
            "{volume:?}: {refused:?}"
        );
    }
    let near = robinhood("SPY", "100", "100.75");
    let equity = robinhood_cross_check(&collar(), &near, &iex(), liquid());
    assert!(
        matches!(equity, Ok(())),
        "{equity:?}: not the crypto member's 0.5%"
    );
}

/// Quotes of two instruments cannot be compared, so the opening refuses as a missing quote.
#[test]
fn quotes_of_two_instruments_refuse_as_absent() {
    let refused =
        robinhood_cross_check(&collar(), &robinhood("QQQ", "100", "100"), &iex(), liquid());
    assert!(
        matches!(refused, Err(PaperError::CrossCheck(QuoteRefusal::Absent))),
        "{refused:?}"
    );
}
