//! The transport's pure rules (DEC-2200 items 4, 6 and 8): which host and path an attempt may go
//! to, how much of its deadline is left, and how a `Retry-After` is read; and the system clock.

#![allow(
    clippy::disallowed_methods,
    reason = "tests time the transport against the wall clock it runs on"
)]

mod common;

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::after;
use mandate_gateway::provider::Clock;
use mandate_gateway_http::{MAX_BODY_BYTES, SystemClock, budget, retry_after_ms, url_for};
use mandate_time::UtcNanos;

#[test]
fn an_allowed_host_and_path_give_exactly_that_url() {
    for (host, path, url) in [
        (
            "openrouter.ai",
            "/api/v1/chat/completions",
            "https://openrouter.ai/api/v1/chat/completions",
        ),
        ("localhost:8443", "/v1", "https://localhost:8443/v1"),
        ("10.0.0.7:1", "/", "https://10.0.0.7:1/"),
        ("a-b.example:65535", "/x/y", "https://a-b.example:65535/x/y"),
    ] {
        assert_eq!(
            url_for(host, path).map(|u| u.to_string()),
            Some(url.to_owned()),
            "{host} {path}"
        );
    }
}

#[test]
fn a_host_outside_the_rules_is_refused() {
    for host in [
        "",
        "OpenRouter.ai",
        "openrouter.ai.",
        ".openrouter.ai",
        "open..router.ai",
        "user@openrouter.ai",
        "openrouter.ai@evil.example",
        "evil.example#.openrouter.ai",
        "openrouter.ai/x",
        "open router.ai",
        "openrouter_ai",
        "openrouter.ai:",
        "openrouter.ai:0",
        "openrouter.ai:65536",
        "openrouter.ai:443:1",
        "openrouter.ai:+443",
        "openrouter.ai:44a",
        "[::1]:443",
        "ex\u{e9}mple.com",
    ] {
        assert_eq!(url_for(host, "/v1"), None, "{host:?}");
    }
}

#[test]
fn a_path_outside_the_rules_is_refused() {
    for path in [
        "",
        "v1",
        "/a#b",
        "/a\\b",
        "/a b",
        "/a\tb",
        "/caf\u{e9}",
        "/a\u{7f}",
        "/v1?x=1",
    ] {
        assert_eq!(url_for("openrouter.ai", path), None, "{path:?}");
    }
    assert!(
        url_for("openrouter.ai", "/a/../b").is_none(),
        "a path the URL parser would rewrite is refused"
    );
}

#[test]
fn the_body_cap_is_four_mebibytes() {
    assert_eq!(MAX_BODY_BYTES, 4_194_304, "DEC-2200 item 7");
}

#[test]
fn only_delay_seconds_are_read_from_retry_after() {
    assert_eq!(retry_after_ms("7"), Some(7_000));
    assert_eq!(retry_after_ms("0"), Some(0));
    assert_eq!(retry_after_ms("120"), Some(120_000));
    assert_eq!(
        retry_after_ms("18446744073709551"),
        Some(18_446_744_073_709_551_000)
    );
    for value in [
        "",
        " 7",
        "7 ",
        "+7",
        "-1",
        "7.5",
        "Wed, 21 Oct 2026 07:28:00 GMT",
        "18446744073709552",
        "99999999999999999999",
    ] {
        assert_eq!(retry_after_ms(value), None, "{value:?}");
    }
}

fn at(secs: i64, nanos: u32) -> UtcNanos {
    UtcNanos::from_parts(secs, nanos).unwrap()
}

#[test]
fn the_budget_is_the_time_left_before_the_deadline() {
    assert_eq!(budget(at(10, 0), at(10, 1)), Some(Duration::from_nanos(1)));
    assert_eq!(
        budget(at(10, 999_999_999), at(12, 1)),
        Some(Duration::from_nanos(1_000_000_002))
    );
    assert_eq!(
        budget(at(10, 5), at(10, 5)),
        None,
        "none left at the deadline"
    );
    assert_eq!(budget(at(10, 6), at(10, 5)), None, "none left past it");
    assert_eq!(budget(at(11, 0), at(10, 999_999_999)), None);
    assert_eq!(
        budget(at(0, 0), at(4_000_000_000, 0)),
        Some(Duration::from_secs(4_000_000_000)),
        "a span longer than i32 seconds"
    );
}

#[test]
fn the_system_clock_reads_the_system_time_and_waits_until_a_moment() {
    let mut clock = SystemClock;
    let now = clock.now();
    let system = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let skew = i128::from(now.secs()) * 1_000_000_000 + i128::from(now.nanos())
        - i128::try_from(system.as_nanos()).unwrap();
    assert!(skew.abs() < 1_000_000_000, "skew {skew} ns");
    let started = Instant::now();
    let soon = after(clock.now(), Duration::from_millis(60));
    clock.wait_until(soon);
    assert!(started.elapsed() >= Duration::from_millis(55));
    let started = Instant::now();
    clock.wait_until(at(0, 0));
    assert!(
        started.elapsed() < Duration::from_millis(50),
        "a moment already past is no wait"
    );
}
