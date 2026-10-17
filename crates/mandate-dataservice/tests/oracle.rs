//! DP-4 and DEC-433 items 5 and 8 over random arrival orders and clock skews: after every event,
//! the opening quote and the last marked quote equal what an independent oracle computes from the
//! whole history at once.
//!
//! The cache folds one quote at a time against its watermark. The oracle never folds: it finds the
//! watermark as the earliest-received of the latest-stamped quotes that are not future-stamped,
//! then asks whether anything after it withdrew it, all in whole milliseconds and cents, with its
//! own sanity rule in integers. A vendor time ahead of or behind the receive time, duplicates,
//! replays and stamps far in the future all come from the generator.

mod common;

use common::{MAX_AGE_MS, SKEW_MS, SPREAD_BPS, asset, at, freshness, price};
use mandate_dataservice::{Feed, HotCache, Quote};
use mandate_num::Qty;
use mandate_risk::SaneQuote;
use proptest::prelude::*;

/// One vendor message in the pool the arrivals draw from, so the same message can arrive twice.
#[derive(Debug, Clone, Copy)]
struct Message {
    instrument: u8,
    vendor_ms: i64,
    bid: u64,
    ask: u64,
}

#[derive(Debug, Clone, Copy)]
enum Event {
    /// A pool message received `delay_ms` after its vendor stamp; negative is a vendor clock
    /// ahead of ours.
    Arrive {
        message: usize,
        delay_ms: i64,
    },
    Lost,
    Clock(bool),
    Query {
        now_ms: i64,
    },
}

fn message() -> impl Strategy<Value = Message> {
    (1u8..=2, 0i64..20_000, 9_900u64..10_100, -20i64..600).prop_map(
        |(instrument, vendor_ms, bid, spread)| Message {
            instrument,
            vendor_ms,
            bid,
            ask: bid.checked_add_signed(spread).unwrap(),
        },
    )
}

fn event(pool: usize) -> impl Strategy<Value = Event> {
    prop_oneof![
        8 => (0..pool, -2_500i64..4_000)
            .prop_map(|(message, delay_ms)| Event::Arrive { message, delay_ms }),
        1 => Just(Event::Lost),
        2 => any::<bool>().prop_map(Event::Clock),
        4 => (-3_000i64..26_000).prop_map(|now_ms| Event::Query { now_ms }),
    ]
}

fn history() -> impl Strategy<Value = (Vec<Message>, Vec<Event>)> {
    prop::collection::vec(message(), 1..12).prop_flat_map(|pool| {
        let events = prop::collection::vec(event(pool.len()), 1..80);
        (Just(pool), events)
    })
}

fn quote_of(message: Message, delay_ms: i64) -> Quote {
    Quote {
        instrument: asset(message.instrument),
        feed: Feed::Iex,
        bid: price(message.bid),
        bid_size: Qty::parse("100").unwrap(),
        ask: price(message.ask),
        ask_size: Qty::parse("100").unwrap(),
        vendor_time: at(message.vendor_ms),
        received_at: at(message.vendor_ms + delay_ms),
    }
}

/// An arrival as the oracle sees it: where it came in the history, and its times in milliseconds.
#[derive(Debug, Clone, Copy)]
struct Seen {
    index: usize,
    message: Message,
    received_ms: i64,
}

impl Seen {
    fn future(&self) -> bool {
        self.message.vendor_ms - self.received_ms > SKEW_MS
    }

    fn sane(&self) -> bool {
        let (bid, ask) = (u128::from(self.message.bid), u128::from(self.message.ask));
        bid <= ask && ask * 10_000 <= bid * u128::from(10_000 + SPREAD_BPS)
    }

    fn quote(&self) -> Quote {
        quote_of(self.message, self.received_ms - self.message.vendor_ms)
    }
}

fn arrivals(pool: &[Message], events: &[Event], instrument: u8) -> Vec<Seen> {
    events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match *event {
            Event::Arrive { message, delay_ms } => {
                let message = pool[message];
                (message.instrument == instrument).then_some(Seen {
                    index,
                    message,
                    received_ms: message.vendor_ms + delay_ms,
                })
            }
            _ => None,
        })
        .collect()
}

/// The quote that marks after `events`: the earliest-received of the latest-stamped arrivals that
/// are not future-stamped, if it is sane and neither a future-stamped arrival nor a connection
/// loss came after it.
fn oracle_mark(pool: &[Message], events: &[Event], instrument: u8) -> Option<Seen> {
    let seen = arrivals(pool, events, instrument);
    let latest = seen
        .iter()
        .filter(|s| !s.future())
        .map(|s| s.message.vendor_ms)
        .max()?;
    let holder = *seen
        .iter()
        .find(|s| !s.future() && s.message.vendor_ms == latest)?;
    let withdrawn = seen.iter().any(|s| s.index > holder.index && s.future())
        || events[holder.index..]
            .iter()
            .any(|e| matches!(e, Event::Lost));
    (holder.sane() && !withdrawn).then_some(holder)
}

fn oracle_opening(
    pool: &[Message],
    events: &[Event],
    instrument: u8,
    now_ms: i64,
) -> Option<SaneQuote> {
    let clock = events.iter().rev().find_map(|e| match e {
        Event::Clock(ok) => Some(*ok),
        _ => None,
    });
    let mark = oracle_mark(pool, events, instrument)?;
    let basis = mark.message.vendor_ms.min(mark.received_ms);
    let fresh = clock == Some(true) && basis <= now_ms && now_ms - basis <= MAX_AGE_MS;
    fresh.then(|| SaneQuote {
        bid: price(mark.message.bid),
        ask: price(mark.message.ask),
        at: at(basis),
    })
}

/// The last arrival that marked when it came: not future-stamped, sane, and stamped after every
/// earlier arrival that was not future-stamped.
fn oracle_last_marked(pool: &[Message], events: &[Event], instrument: u8) -> Option<Quote> {
    let seen = arrivals(pool, events, instrument);
    seen.iter()
        .enumerate()
        .filter(|(n, s)| {
            !s.future()
                && s.sane()
                && seen[..*n].iter().all(|earlier| {
                    earlier.future() || earlier.message.vendor_ms < s.message.vendor_ms
                })
        })
        .map(|(_, s)| s.quote())
        .next_back()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1_024))]

    #[test]
    fn the_cache_agrees_with_the_oracle_after_every_event((pool, events) in history()) {
        let mut cache = HotCache::new(freshness(), 2);
        cache.subscribe(asset(1), Feed::Iex).unwrap();
        cache.subscribe(asset(2), Feed::Iex).unwrap();
        for (n, event) in events.iter().enumerate() {
            match *event {
                Event::Arrive { message, delay_ms } => {
                    cache.intake(quote_of(pool[message], delay_ms)).unwrap();
                }
                Event::Lost => cache.connection_lost(),
                Event::Clock(ok) => cache.set_clock_in_tolerance(ok),
                Event::Query { .. } => {}
            }
            let prefix = &events[..=n];
            let now_ms = match *event {
                Event::Query { now_ms } => now_ms,
                _ => 10_000,
            };
            for instrument in 1..=2 {
                prop_assert_eq!(
                    cache.opening_quote(&asset(instrument), at(now_ms)),
                    oracle_opening(&pool, prefix, instrument, now_ms),
                    "opening quote of instrument {} after event {}", instrument, n
                );
                prop_assert_eq!(
                    cache.last_marked(&asset(instrument)).cloned(),
                    oracle_last_marked(&pool, prefix, instrument),
                    "last marked quote of instrument {} after event {}", instrument, n
                );
            }
        }
    }

    /// Whatever arrives, an opening quote is sane, younger than the threshold and from a quote
    /// stamped no later than `future_skew` after its receipt: stale or disordered data only ever
    /// removes the quote, so it can never price or admit a larger opening.
    #[test]
    fn an_opening_quote_is_always_sane_fresh_and_not_future_stamped((pool, events) in history()) {
        let mut cache = HotCache::new(freshness(), 2);
        cache.subscribe(asset(1), Feed::Iex).unwrap();
        cache.subscribe(asset(2), Feed::Iex).unwrap();
        cache.set_clock_in_tolerance(true);
        for event in &events {
            if let Event::Arrive { message, delay_ms } = *event {
                cache.intake(quote_of(pool[message], delay_ms)).unwrap();
            }
            for instrument in 1..=2 {
                for now_ms in [-3_000, 0, 5_000, 10_000, 20_000, 25_000] {
                    let Some(q) = cache.opening_quote(&asset(instrument), at(now_ms)) else {
                        continue;
                    };
                    let source = cache.last_marked(&asset(instrument)).unwrap();
                    prop_assert!(q.bid <= q.ask);
                    prop_assert!(freshness().sane(source) && !freshness().future_stamped(source));
                    prop_assert!(q.at <= at(now_ms) && at(now_ms - MAX_AGE_MS) <= q.at);
                    prop_assert_eq!(q.at, source.vendor_time.min(source.received_at));
                }
            }
        }
    }
}
