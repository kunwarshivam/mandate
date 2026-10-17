//! Each rule of spec §3.3 and §3.4 and DEC-1008, one at a time.

mod common;

use common::{asset, at, at_ns, freshness, price, quote};
use mandate_dataservice::{CacheFull, Feed, HotCache, Intake, Refused};
use mandate_risk::SaneQuote;

/// A cache with instrument 1 on IEX and the clock in tolerance.
fn cache() -> HotCache {
    let mut cache = HotCache::new(freshness(), 4);
    cache.subscribe(asset(1), Feed::Iex, early()).unwrap();
    cache.set_clock_in_tolerance(true);
    cache
}

/// A subscription instant before every quote these cases send.
fn early() -> mandate_time::UtcNanos {
    at(-10_000)
}

fn opening(cache: &HotCache, now_ms: i64) -> Option<SaneQuote> {
    cache.opening_quote(&asset(1), at(now_ms))
}

#[test]
fn a_sane_in_order_quote_marks_and_is_fresh_for_an_opening() {
    let mut cache = cache();
    assert_eq!(
        cache.intake(quote(10_000, 10_010, at(0), at(5))),
        Ok(Intake::Marked)
    );
    assert_eq!(
        opening(&cache, 10),
        Some(SaneQuote {
            bid: price(10_000),
            ask: price(10_010),
            at: at(0)
        }),
        "the opening quote carries the earlier of vendor and receive time"
    );
    assert_eq!(
        cache.last_marked(&asset(1)).map(|q| q.received_at),
        Some(at(5))
    );
}

#[test]
fn age_counts_from_the_earlier_of_vendor_and_receive_time() {
    let mut cache = cache();
    cache.intake(quote(10_000, 10_010, at(500), at(0))).unwrap();
    assert_eq!(
        opening(&cache, 2_000).map(|q| q.at),
        Some(at(0)),
        "vendor ahead: receive time"
    );
    assert_eq!(
        opening(&cache, 2_001),
        None,
        "2,001 ms after receipt is stale"
    );
    let mut cache = self::cache();
    cache.intake(quote(10_000, 10_010, at(0), at(900))).unwrap();
    assert_eq!(
        opening(&cache, 2_000).map(|q| q.at),
        Some(at(0)),
        "slow delivery: vendor time"
    );
    assert_eq!(
        opening(&cache, 2_001),
        None,
        "2,001 ms after the vendor stamp is stale"
    );
}

#[test]
fn an_age_of_exactly_the_threshold_is_fresh_and_one_nanosecond_more_is_not() {
    let mut cache = cache();
    cache.intake(quote(10_000, 10_010, at(0), at(0))).unwrap();
    assert!(cache.opening_quote(&asset(1), at(2_000)).is_some());
    assert_eq!(cache.opening_quote(&asset(1), at_ns(2_000, 1)), None);
}

#[test]
fn a_quote_stamped_after_now_is_not_fresh() {
    let mut cache = cache();
    cache
        .intake(quote(10_000, 10_010, at(100), at(100)))
        .unwrap();
    assert_eq!(cache.opening_quote(&asset(1), at_ns(99, 999_999)), None);
    assert!(cache.opening_quote(&asset(1), at(100)).is_some());
}

#[test]
fn no_quote_is_fresh_until_the_clock_is_known_in_tolerance_or_while_it_is_out() {
    let mut cache = HotCache::new(freshness(), 4);
    cache.subscribe(asset(1), Feed::Iex, early()).unwrap();
    cache.intake(quote(10_000, 10_010, at(0), at(0))).unwrap();
    assert_eq!(
        opening(&cache, 1),
        None,
        "a new cache starts out of tolerance"
    );
    cache.set_clock_in_tolerance(true);
    assert!(opening(&cache, 1).is_some());
    cache.set_clock_in_tolerance(false);
    assert_eq!(opening(&cache, 1), None);
    assert!(
        cache.last_marked(&asset(1)).is_some(),
        "exits keep their mark source"
    );
}

#[test]
fn a_future_stamped_quote_withdraws_the_mark_and_leaves_the_watermark() {
    let mut cache = cache();
    cache.intake(quote(10_000, 10_010, at(0), at(0))).unwrap();
    assert_eq!(
        cache.intake(quote(10_000, 10_010, at_ns(2_000, 1), at(1_000))),
        Ok(Intake::FutureStamped)
    );
    assert_eq!(opening(&cache, 1_000), None);
    assert_eq!(
        cache.intake(quote(10_001, 10_011, at(1_500), at(1_500))),
        Ok(Intake::Marked),
        "a quote before the future stamp but after the watermark still marks"
    );
    assert_eq!(opening(&cache, 1_500).map(|q| q.bid), Some(price(10_001)));
    assert_eq!(
        cache.last_marked(&asset(1)).map(|q| q.bid),
        Some(price(10_001))
    );
}

#[test]
fn a_vendor_stamp_exactly_the_skew_ahead_is_not_future_stamped() {
    let mut cache = cache();
    assert_eq!(
        cache.intake(quote(10_000, 10_010, at(2_000), at(1_000))),
        Ok(Intake::Marked)
    );
    assert!(freshness().future_stamped(&quote(1, 1, at_ns(2_000, 1), at(1_000))));
    assert!(!freshness().future_stamped(&quote(1, 1, at(2_000), at(1_000))));
}

#[test]
fn a_quote_at_or_before_the_watermark_never_marks() {
    let mut cache = cache();
    cache
        .intake(quote(10_000, 10_010, at(100), at(100)))
        .unwrap();
    assert_eq!(
        cache.intake(quote(10_050, 10_060, at(99), at(150))),
        Ok(Intake::OutOfOrder)
    );
    assert_eq!(
        cache.intake(quote(10_050, 10_060, at(100), at(150))),
        Ok(Intake::OutOfOrder)
    );
    assert_eq!(opening(&cache, 150).map(|q| q.bid), Some(price(10_000)));
}

#[test]
fn the_same_message_again_is_a_duplicate() {
    let mut cache = cache();
    cache
        .intake(quote(10_000, 10_010, at(100), at(100)))
        .unwrap();
    assert_eq!(
        cache.intake(quote(10_000, 10_010, at(100), at(300))),
        Ok(Intake::Duplicate)
    );
    assert_eq!(
        opening(&cache, 2_100).map(|q| q.at),
        Some(at(100)),
        "a copy does not refresh it"
    );
    let mut other_size = quote(10_000, 10_010, at(100), at(300));
    other_size.ask_size = mandate_num::Qty::parse("200").unwrap();
    assert!(!other_size.same_message(&quote(10_000, 10_010, at(100), at(100))));
    assert_eq!(cache.intake(other_size), Ok(Intake::OutOfOrder));
}

#[test]
fn a_crossed_or_too_wide_quote_withdraws_the_mark_and_moves_the_watermark() {
    let mut cache = cache();
    cache.intake(quote(10_000, 10_010, at(0), at(0))).unwrap();
    assert_eq!(
        cache.intake(quote(10_010, 10_000, at(10), at(10))),
        Ok(Intake::NotSane)
    );
    assert_eq!(opening(&cache, 10), None);
    assert_eq!(
        cache.intake(quote(10_000, 10_010, at(5), at(20))),
        Ok(Intake::OutOfOrder)
    );
    assert_eq!(opening(&cache, 20), None);
    assert_eq!(
        cache.last_marked(&asset(1)).map(|q| q.vendor_time),
        Some(at(0))
    );
    assert_eq!(
        cache.intake(quote(10_000, 10_501, at(30), at(30))),
        Ok(Intake::NotSane)
    );
    assert_eq!(
        cache.intake(quote(10_000, 10_500, at(40), at(40))),
        Ok(Intake::Marked)
    );
    assert_eq!(
        cache.intake(quote(10_000, 10_000, at(50), at(50))),
        Ok(Intake::Marked)
    );
    assert_eq!(
        opening(&cache, 50).map(|q| q.ask),
        Some(price(10_000)),
        "locked is sane"
    );
}

#[test]
fn a_connection_loss_withdraws_every_mark_until_a_quote_received_after_it() {
    let mut cache = cache();
    cache.subscribe(asset(2), Feed::Iex, early()).unwrap();
    cache.intake(quote(10_000, 10_010, at(0), at(0))).unwrap();
    let mut second = quote(5_000, 5_001, at(0), at(0));
    second.instrument = asset(2);
    cache.intake(second).unwrap();
    cache.connection_lost(at(5));
    assert_eq!(opening(&cache, 5), None);
    assert_eq!(cache.opening_quote(&asset(2), at(5)), None);
    assert_eq!(
        cache.intake(quote(10_001, 10_011, at(3), at(4))),
        Ok(Intake::BeforeResync),
        "a quote received before the loss and taken after it never marks"
    );
    assert_eq!(
        cache.intake(quote(10_002, 10_012, at(5), at(5))),
        Ok(Intake::BeforeResync),
        "received at the instant of the loss is not after it"
    );
    assert_eq!(opening(&cache, 5), None);
    assert_eq!(
        cache.intake(quote(10_000, 10_010, at(0), at(6))),
        Ok(Intake::Duplicate),
        "a replay received after the loss is still the watermark's message"
    );
    assert_eq!(
        cache.intake(quote(10_003, 10_013, at(2), at(7))),
        Ok(Intake::Marked),
        "the in-flight quotes did not move the watermark"
    );
    assert_eq!(opening(&cache, 7).map(|q| q.bid), Some(price(10_003)));
    assert!(cache.last_marked(&asset(2)).is_some());
    assert_eq!(cache.opening_quote(&asset(2), at(7)), None);
    cache.connection_lost(at(1));
    assert_eq!(
        cache.intake(quote(10_004, 10_014, at(6), at(5))),
        Ok(Intake::BeforeResync),
        "an earlier loss instant never moves the resync point back"
    );
}

#[test]
fn only_quotes_received_after_the_subscription_mark() {
    let mut cache = HotCache::new(freshness(), 4);
    cache.set_clock_in_tolerance(true);
    cache.subscribe(asset(1), Feed::Iex, at(10)).unwrap();
    assert_eq!(
        cache.intake(quote(10_000, 10_010, at(9), at(10))),
        Ok(Intake::BeforeResync)
    );
    assert_eq!(
        cache.intake(quote(10_000, 10_010, at(9), at(11))),
        Ok(Intake::Marked)
    );
    cache.unsubscribe(&asset(1));
    cache.subscribe(asset(1), Feed::Iex, at(20)).unwrap();
    assert_eq!(
        cache.intake(quote(10_001, 10_011, at(15), at(19))),
        Ok(Intake::BeforeResync),
        "a resubscription forgets the watermark but takes nothing received before it"
    );
    assert_eq!(opening(&cache, 20), None);
    cache.subscribe(asset(1), Feed::Iex, at(30)).unwrap();
    assert_eq!(
        cache.intake(quote(10_001, 10_011, at(21), at(25))),
        Ok(Intake::Marked),
        "subscribing again on the same feed keeps the first subscription's instant"
    );
}

#[test]
fn a_feed_change_keeps_a_later_loss_instant() {
    let mut cache = cache();
    cache.connection_lost(at(50));
    cache.subscribe(asset(1), Feed::Sip, at(40)).unwrap();
    let mut sip = quote(10_000, 10_010, at(45), at(45));
    sip.feed = Feed::Sip;
    assert_eq!(cache.intake(sip.clone()), Ok(Intake::BeforeResync));
    sip.received_at = at(51);
    assert_eq!(cache.intake(sip), Ok(Intake::Marked));
}

#[test]
fn only_subscribed_instruments_on_their_feed_are_taken() {
    let mut cache = cache();
    let mut other = quote(10_000, 10_010, at(0), at(0));
    other.instrument = asset(9);
    assert_eq!(cache.intake(other), Err(Refused::NotSubscribed));
    let mut sip = quote(10_000, 10_010, at(0), at(0));
    sip.feed = Feed::Sip;
    assert_eq!(cache.intake(sip.clone()), Err(Refused::OtherFeed));
    assert_eq!(opening(&cache, 0), None);
    cache.intake(quote(10_000, 10_010, at(0), at(0))).unwrap();
    cache.subscribe(asset(1), Feed::Iex, at(0)).unwrap();
    assert!(
        opening(&cache, 0).is_some(),
        "subscribing again on the same feed keeps the mark"
    );
    cache.subscribe(asset(1), Feed::Sip, early()).unwrap();
    assert_eq!(opening(&cache, 0), None, "another feed starts over");
    assert_eq!(cache.last_marked(&asset(1)), None);
    assert_eq!(cache.intake(sip), Ok(Intake::Marked));
    cache.unsubscribe(&asset(1));
    assert_eq!(opening(&cache, 0), None);
    assert_eq!(
        cache.intake(quote(1, 1, at(9), at(9))),
        Err(Refused::NotSubscribed)
    );
}

#[test]
fn the_cache_holds_at_most_its_capacity() {
    let mut cache = HotCache::new(freshness(), 2);
    assert_eq!(cache.subscribe(asset(1), Feed::Iex, early()), Ok(()));
    assert_eq!(cache.subscribe(asset(2), Feed::Iex, early()), Ok(()));
    assert_eq!(
        cache.subscribe(asset(3), Feed::Iex, early()),
        Err(CacheFull)
    );
    assert_eq!(
        cache.subscribe(asset(2), Feed::Crypto, early()),
        Ok(()),
        "a feed change adds nothing"
    );
    cache.unsubscribe(&asset(1));
    assert_eq!(cache.subscribe(asset(3), Feed::Iex, early()), Ok(()));
    assert_eq!(
        HotCache::new(freshness(), 0).subscribe(asset(1), Feed::Iex, early()),
        Err(CacheFull)
    );
}

#[test]
fn refusals_say_what_was_refused() {
    assert_eq!(
        Refused::NotSubscribed.to_string(),
        "quote for an instrument not subscribed"
    );
    assert_eq!(
        Refused::OtherFeed.to_string(),
        "quote from a feed the instrument is not subscribed on"
    );
    assert_eq!(
        CacheFull.to_string(),
        "the hot cache holds its capacity of instruments"
    );
}
