//! Builders the data service's tests share: instants as milliseconds after a fixed base, prices
//! in whole cents, and one default profile.

#![allow(
    dead_code,
    reason = "each test crate uses its own subset of the builders"
)]

use std::time::Duration;

use mandate_dataservice::{Feed, Freshness, Quote};
use mandate_domain::AssetId;
use mandate_num::{Fraction, Price, Qty};
use mandate_time::UtcNanos;

/// 2025-10-09T08:53:20Z, a Thursday.
pub const BASE_SECS: i64 = 1_760_000_000;
/// The default profile's staleness threshold, in milliseconds.
pub const MAX_AGE_MS: i64 = 2_000;
/// The default `future_skew_ms`.
pub const SKEW_MS: i64 = 1_000;
/// The default spread limit, in basis points of the bid.
pub const SPREAD_BPS: u64 = 500;

/// `ms` milliseconds plus `ns` nanoseconds after the base.
pub fn at_ns(ms: i64, ns: u32) -> UtcNanos {
    let secs = BASE_SECS + ms.div_euclid(1_000);
    let nanos = u32::try_from(ms.rem_euclid(1_000) * 1_000_000).unwrap() + ns;
    UtcNanos::from_parts(secs, nanos).unwrap()
}

pub fn at(ms: i64) -> UtcNanos {
    at_ns(ms, 0)
}

pub fn asset(n: u8) -> AssetId {
    AssetId::parse(&format!("00000000-0000-4000-8000-{n:012x}")).unwrap()
}

pub fn price(cents: u64) -> Price {
    Price::parse(&canonical(&format!("{}.{:02}", cents / 100, cents % 100))).unwrap()
}

/// A decimal's canonical text: no trailing zeros after the point, and no bare point.
fn canonical(text: &str) -> String {
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

pub fn freshness() -> Freshness {
    Freshness {
        max_age: Duration::from_millis(u64::try_from(MAX_AGE_MS).unwrap()),
        future_skew: Duration::from_millis(u64::try_from(SKEW_MS).unwrap()),
        max_spread: Fraction::parse(&canonical(&format!("0.{SPREAD_BPS:04}"))).unwrap(),
    }
}

/// A quote on IEX for instrument 1 with sizes of 100.
pub fn quote(bid: u64, ask: u64, vendor: UtcNanos, received: UtcNanos) -> Quote {
    Quote {
        instrument: asset(1),
        feed: Feed::Iex,
        bid: price(bid),
        bid_size: Qty::parse("100").unwrap(),
        ask: price(ask),
        ask_size: Qty::parse("100").unwrap(),
        vendor_time: vendor,
        received_at: received,
    }
}
