//! DP-4 and DEC-433 items 5 and 8 over random arrival orders and clock skews: after every event,
//! the opening quote and the last marked quote equal what an independent oracle computes from the
//! whole history at once.
//!
//! The cache folds one quote at a time against its watermark. The oracle never folds: it finds the
//! watermark as the earliest-taken of the latest-stamped quotes that are neither future-stamped nor
//! received at or before a connection loss taken earlier, then asks whether anything after it
//! withdrew it, all in whole milliseconds and cents, with its own sanity rule in integers. A vendor
//! time ahead of or behind the receive time, quotes still in flight when the connection is lost,
//! duplicates, replays, stamps far in the future, and quotes exactly at the spread and skew limits
//! all come from the generator.

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
    /// The connection was lost at `at_ms` by the host clock, which may be before quotes still
    /// to be taken were received.
    Lost {
        at_ms: i64,
    },
    Clock(bool),
    Query {
        now_ms: i64,
    },
}

fn message() -> impl Strategy<Value = Message> {
    let spread = prop_oneof![3 => (-20i64..600).prop_map(Some), 1 => Just(None)];
    (1u8..=2, 0i64..20_000, 495u64..505, spread).prop_map(
        |(instrument, vendor_ms, twentieths, spread)| {
            let bid = twentieths * 20;
            let at_limit = bid * SPREAD_BPS / 10_000;
            let ask = match spread {
                Some(spread) => bid.checked_add_signed(spread).unwrap(),
                None => bid + at_limit,
            };
            Message {
                instrument,
                vendor_ms,
                bid,
                ask,
            }
        },
    )
}

fn event(pool: usize) -> impl Strategy<Value = Event> {
    prop_oneof![
        8 => (0..pool, prop_oneof![4 => -2_500i64..4_000, 1 => Just(-SKEW_MS), 1 => Just(-SKEW_MS - 1)])
            .prop_map(|(message, delay_ms)| Event::Arrive { message, delay_ms }),
        1 => (-3_000i64..24_000).prop_map(|at_ms| Event::Lost { at_ms }),
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

/// Whether a connection loss taken before `seen` happened at or after its receipt, so it is not
/// a live quote since resync.
fn before_resync(events: &[Event], seen: &Seen) -> bool {
    events[..seen.index]
        .iter()
        .any(|e| matches!(*e, Event::Lost { at_ms } if at_ms >= seen.received_ms))
}

/// The arrivals that can hold the watermark: not future-stamped and not before resync.
fn candidates(pool: &[Message], events: &[Event], instrument: u8) -> Vec<Seen> {
    arrivals(pool, events, instrument)
        .into_iter()
        .filter(|s| !s.future() && !before_resync(events, s))
        .collect()
}

/// The quote that marks after `events`: the earliest-taken of the latest-stamped candidates, if
/// it is sane and neither a future-stamped arrival nor a connection loss came after it.
fn oracle_mark(pool: &[Message], events: &[Event], instrument: u8) -> Option<Seen> {
    let candidates = candidates(pool, events, instrument);
    let latest = candidates.iter().map(|s| s.message.vendor_ms).max()?;
    let holder = *candidates.iter().find(|s| s.message.vendor_ms == latest)?;
    let withdrawn = arrivals(pool, events, instrument)
        .iter()
        .any(|s| s.index > holder.index && s.future())
        || events[holder.index..]
            .iter()
            .any(|e| matches!(e, Event::Lost { .. }));
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

/// The last candidate that marked when it came: sane, and stamped after every earlier candidate.
fn oracle_last_marked(pool: &[Message], events: &[Event], instrument: u8) -> Option<Quote> {
    let seen = candidates(pool, events, instrument);
    seen.iter()
        .enumerate()
        .filter(|(n, s)| {
            s.sane()
                && seen[..*n]
                    .iter()
                    .all(|earlier| earlier.message.vendor_ms < s.message.vendor_ms)
        })
        .map(|(_, s)| s.quote())
        .next_back()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1_024))]

    #[test]
    fn the_cache_agrees_with_the_oracle_after_every_event((pool, events) in history()) {
        let mut cache = HotCache::new(freshness(), 2);
        cache.subscribe(asset(1), Feed::Iex, at(-10_000)).unwrap();
        cache.subscribe(asset(2), Feed::Iex, at(-10_000)).unwrap();
        for (n, event) in events.iter().enumerate() {
            match *event {
                Event::Arrive { message, delay_ms } => {
                    cache.intake(quote_of(pool[message], delay_ms)).unwrap();
                }
                Event::Lost { at_ms } => cache.connection_lost(at(at_ms)),
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

    /// Whatever arrives, an opening quote comes from an arrival that is sane, not future-stamped
    /// and received after every connection loss taken before it, judged by the oracle's integer
    /// rules, and is younger than the threshold: stale or disordered data only ever removes the
    /// quote, so it can never price or admit a larger opening.
    #[test]
    fn an_opening_quote_always_comes_from_a_sane_live_arrival((pool, events) in history()) {
        let mut cache = HotCache::new(freshness(), 2);
        cache.subscribe(asset(1), Feed::Iex, at(-10_000)).unwrap();
        cache.subscribe(asset(2), Feed::Iex, at(-10_000)).unwrap();
        cache.set_clock_in_tolerance(true);
        for (n, event) in events.iter().enumerate() {
            match *event {
                Event::Arrive { message, delay_ms } => {
                    cache.intake(quote_of(pool[message], delay_ms)).unwrap();
                }
                Event::Lost { at_ms } => cache.connection_lost(at(at_ms)),
                Event::Clock(_) | Event::Query { .. } => {}
            }
            let prefix = &events[..=n];
            for instrument in 1..=2 {
                for now_ms in [-3_000, 0, 5_000, 10_000, 20_000, 25_000] {
                    let Some(q) = cache.opening_quote(&asset(instrument), at(now_ms)) else {
                        continue;
                    };
                    let live = arrivals(&pool, prefix, instrument).into_iter().any(|s| {
                        let basis = s.message.vendor_ms.min(s.received_ms);
                        s.sane()
                            && !s.future()
                            && !before_resync(prefix, &s)
                            && price(s.message.bid) == q.bid
                            && price(s.message.ask) == q.ask
                            && at(basis) == q.at
                            && basis <= now_ms
                            && now_ms - basis <= MAX_AGE_MS
                    });
                    prop_assert!(live, "instrument {} at {} ms after event {}: {:?}", instrument, now_ms, n, q);
                }
            }
        }
    }
}
