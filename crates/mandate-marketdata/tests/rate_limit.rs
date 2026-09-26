//! Pacing against the host's rate limit, driven by scripted responses and by fake hosts that model
//! the limiter: no network and no real waiting. Each oracle recounts what the host allowed from its
//! own request log.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{
    FakeTransport, RecordingPause, START_SECS, day, ok, ok_rated, rate, spy_sip_1hour, status,
};
use mandate_marketdata::client::{
    Client, FetchError, Pause, RateHeaders, RatePolicy, Response, RetryCause, RetryPolicy,
    TokioPause, Transport, TransportError,
};
use mandate_time::UtcNanos;

const MINUTE: i128 = 60_000_000_000;

fn secs(values: &[u64]) -> Vec<Duration> {
    values.iter().copied().map(Duration::from_secs).collect()
}

fn at(secs: i64, nanos: u32) -> UtcNanos {
    UtcNanos::from_parts(secs, nanos).unwrap()
}

fn nanos(t: UtcNanos) -> i128 {
    i128::from(t.secs()) * 1_000_000_000 + i128::from(t.nanos())
}

/// Page `index` of a `pages`-page day of SPY bars: one bar a minute, token `p<index>` to the next.
fn bars_page(index: usize, pages: usize) -> Vec<u8> {
    let (hour, minute) = (index / 60, index % 60);
    let token = if index + 1 < pages {
        format!("\"p{index}\"")
    } else {
        "null".to_owned()
    };
    format!(
        r#"{{"bars":{{"SPY":[{{"c":1,"h":1,"l":1,"n":1,"o":1,"t":"2026-09-24T{hour:02}:{minute:02}:00Z","v":1,"vw":1}}]}},"next_page_token":{token}}}"#
    )
    .into_bytes()
}

fn policy(limit: u32, low_water: u32) -> RatePolicy {
    RatePolicy {
        limit,
        low_water,
        ..RatePolicy::default()
    }
}

#[derive(Debug, Clone, Copy)]
enum Limiter {
    /// Windows aligned to the minute of `limit` requests each; the reset is the window's end.
    FixedWindow,
    /// A bucket of `limit` requests refilled evenly over a minute, as Alpaca's headers show: the
    /// reset is when the bucket is full again, truncated to the second.
    Bucket,
    /// Never refuses.
    Unlimited,
}

#[derive(Debug, Clone)]
enum Headers {
    Accurate,
    Absent,
    Fixed(RateHeaders),
}

#[derive(Default)]
struct HostLog {
    /// Arrival time and status of every request.
    requests: Vec<(i128, u16)>,
    accepted_in_window: BTreeMap<i128, u32>,
    bucket: Option<(u128, i128)>,
}

struct HostInner {
    limiter: Limiter,
    headers: Headers,
    limit: u32,
    pages: usize,
    yields: bool,
    clock: RecordingPause,
    log: Mutex<HostLog>,
}

/// A fake data host: it serves `pages` pages of SPY bars per day, limits requests like the real
/// host, and answers a refused request with a 429 and no headers.
#[derive(Clone)]
struct Host(Arc<HostInner>);

impl Host {
    fn new(
        limiter: Limiter,
        headers: Headers,
        limit: u32,
        pages: usize,
        clock: &RecordingPause,
    ) -> Self {
        Self(Arc::new(HostInner {
            limiter,
            headers,
            limit,
            pages,
            yields: false,
            clock: clock.clone(),
            log: Mutex::default(),
        }))
    }

    /// Replies only after yielding once, so concurrent requests are in flight together.
    fn yielding(self) -> Self {
        let inner = Arc::into_inner(self.0).unwrap();
        Self(Arc::new(HostInner {
            yields: true,
            ..inner
        }))
    }

    fn requests(&self) -> Vec<(i128, u16)> {
        self.0.log.lock().unwrap().requests.clone()
    }

    fn refused(&self) -> usize {
        self.requests().iter().filter(|(_, s)| *s == 429).count()
    }

    fn accepted_times(&self) -> Vec<i128> {
        self.requests()
            .iter()
            .filter(|(_, s)| *s == 200)
            .map(|(t, _)| *t)
            .collect()
    }

    /// Whether the limiter lets a request in at `now`; if so, the remaining count and reset.
    fn admit(&self, log: &mut HostLog, now: i128) -> Option<(u32, i64)> {
        let limit = self.0.limit;
        match self.0.limiter {
            Limiter::Unlimited => Some((limit, i64::try_from(now / 1_000_000_000).unwrap())),
            Limiter::FixedWindow => {
                let window = now.div_euclid(MINUTE);
                let count = log.accepted_in_window.entry(window).or_default();
                if *count >= limit {
                    return None;
                }
                *count += 1;
                let reset = (window + 1) * MINUTE / 1_000_000_000;
                Some((limit - *count, i64::try_from(reset).unwrap()))
            }
            Limiter::Bucket => {
                let cost = u128::try_from(MINUTE).unwrap();
                let capacity = u128::from(limit) * cost;
                let (units, since) = log.bucket.unwrap_or((capacity, now));
                let elapsed = u128::try_from(now - since).unwrap();
                let units = (units + elapsed * u128::from(limit)).min(capacity);
                if units < cost {
                    log.bucket = Some((units, now));
                    return None;
                }
                let units = units - cost;
                log.bucket = Some((units, now));
                let full_in =
                    i128::try_from((capacity - units).div_ceil(u128::from(limit))).unwrap();
                let remaining = u32::try_from(units / cost).unwrap();
                Some((
                    remaining,
                    i64::try_from((now + full_in) / 1_000_000_000).unwrap(),
                ))
            }
        }
    }

    fn page_index(path: &str) -> usize {
        path.split('&')
            .find_map(|pair| pair.strip_prefix("page_token=p"))
            .map_or(0, |n| n.parse::<usize>().unwrap() + 1)
    }
}

impl Transport for Host {
    async fn get(&self, path_and_query: &str) -> Result<Response, TransportError> {
        let reply = {
            let now = nanos(self.0.clock.now());
            let mut log = self.0.log.lock().unwrap();
            let reply = match self.admit(&mut log, now) {
                None => Response {
                    status: 429,
                    rate: RateHeaders::default(),
                    body: br#"{"message":"too many requests."}"#.to_vec(),
                },
                Some((remaining, reset)) => Response {
                    status: 200,
                    rate: match &self.0.headers {
                        Headers::Accurate => rate(self.0.limit, remaining, reset),
                        Headers::Absent => RateHeaders::default(),
                        Headers::Fixed(headers) => headers.clone(),
                    },
                    body: bars_page(Self::page_index(path_and_query), self.0.pages),
                },
            };
            log.requests.push((now, reply.status));
            reply
        };
        if self.0.yields {
            tokio::task::yield_now().await;
        }
        Ok(reply)
    }
}

/// The most requests any span of one minute holds, counted from each request forward.
fn busiest_minute(times: &[i128]) -> usize {
    let mut busiest = 0;
    let mut end = 0;
    for (start, &t) in times.iter().enumerate() {
        while end < times.len() && times[end] < t + MINUTE {
            end += 1;
        }
        busiest = busiest.max(end - start);
    }
    busiest
}

/// Requests per minute from the first to the last.
fn per_minute(times: &[i128]) -> i128 {
    let span = times.last().unwrap() - times.first().unwrap();
    i128::try_from(times.len() - 1).unwrap() * MINUTE / span
}

#[tokio::test(flavor = "current_thread")]
async fn header_budget_is_spent_to_the_low_water_mark_then_waits_for_the_reset() {
    let clock = RecordingPause::default();
    let host = Host::new(Limiter::FixedWindow, Headers::Accurate, 200, 1000, &clock);
    let client = Client::new(host.clone(), clock.clone());
    let records = client
        .fetch_day(&spy_sip_1hour(), day("2026-09-24"))
        .await
        .unwrap();
    assert_eq!(records.len(), 1000);
    assert_eq!(host.refused(), 0);
    let mut per_window: BTreeMap<i128, usize> = BTreeMap::new();
    for t in host.accepted_times() {
        *per_window.entry(t.div_euclid(MINUTE)).or_default() += 1;
    }
    let counts: Vec<usize> = per_window.into_values().collect();
    assert_eq!(
        counts,
        [195, 195, 195, 195, 195, 25],
        "limit 200 less the low-water 5"
    );
    assert_eq!(
        clock.pauses(),
        secs(&[60, 60, 60, 60, 60]),
        "each wait ends at a reset"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn against_alpacas_bucket_the_client_keeps_near_the_limit_without_a_429() {
    let clock = RecordingPause::default();
    let host = Host::new(Limiter::Bucket, Headers::Accurate, 200, 1200, &clock);
    let client = Client::new(host.clone(), clock.clone());
    let records = client
        .fetch_day(&spy_sip_1hour(), day("2026-09-24"))
        .await
        .unwrap();
    assert_eq!(records.len(), 1200);
    assert_eq!(host.refused(), 0);
    let times = host.accepted_times();
    let rate = per_minute(&times);
    assert!(rate >= 195, "{rate} requests a minute");
    assert!(
        clock
            .pauses()
            .iter()
            .all(|p| *p <= Duration::from_millis(60_500))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn without_headers_the_bucket_alone_never_trips_a_fixed_window() {
    let clock = RecordingPause::default();
    let host = Host::new(Limiter::FixedWindow, Headers::Absent, 200, 600, &clock);
    let client = Client::new(host.clone(), clock.clone());
    client
        .fetch_day(&spy_sip_1hour(), day("2026-09-24"))
        .await
        .unwrap();
    assert_eq!(host.refused(), 0);
    let times = host.accepted_times();
    assert!(busiest_minute(&times) <= 200, "{}", busiest_minute(&times));
}

#[tokio::test(flavor = "current_thread")]
async fn malformed_or_missing_headers_fall_back_to_the_bucket() {
    let future = (START_SECS + 3600).to_string();
    let headers = |limit: Option<&str>, remaining: Option<&str>, reset: Option<&str>| RateHeaders {
        limit: limit.map(str::to_owned),
        remaining: remaining.map(str::to_owned),
        reset: reset.map(str::to_owned),
    };
    let cases = [
        RateHeaders::default(),
        headers(None, Some("0"), Some(&future)),
        headers(Some("many"), Some("0"), Some(&future)),
        headers(Some("200"), None, Some(&future)),
        headers(Some("200"), Some("-1"), Some(&future)),
        headers(Some("200"), Some("none"), Some(&future)),
        headers(Some("200"), Some("201"), Some(&future)),
        headers(Some("200"), Some("0"), None),
        headers(Some("200"), Some("0"), Some("soon")),
    ];
    for case in cases {
        let clock = RecordingPause::default();
        let host = Host::new(
            Limiter::Unlimited,
            Headers::Fixed(case.clone()),
            200,
            500,
            &clock,
        );
        let client = Client::new(host.clone(), clock.clone());
        client
            .fetch_day(&spy_sip_1hour(), day("2026-09-24"))
            .await
            .unwrap();
        let times = host.accepted_times();
        assert_eq!(times.len(), 500, "{case:?}");
        let start = nanos(at(START_SECS, 0));
        assert!(
            times[..20].iter().all(|t| *t == start),
            "a burst of a tenth: {case:?}"
        );
        assert!(times[20] > start, "{case:?}");
        assert!(busiest_minute(&times) <= 200, "{case:?}");
        let rate = per_minute(&times[20..]);
        assert!(
            (179..=180).contains(&rate),
            "the rest at 180 a minute: {rate} {case:?}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_429_without_headers_waits_a_full_window() {
    let transport = FakeTransport::serving([status(429), ok(&bars_page(0, 1))]);
    let clock = RecordingPause::default();
    let client = Client::new(transport.clone(), clock.clone());
    client
        .fetch_day(&spy_sip_1hour(), day("2026-09-24"))
        .await
        .unwrap();
    assert_eq!(clock.pauses(), secs(&[60]));
    assert_eq!(transport.requested().len(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn a_429_waits_until_the_last_known_reset() {
    let transport = FakeTransport::serving([
        ok_rated(&bars_page(0, 2), 200, 50, START_SECS + 20),
        status(429),
        ok(&bars_page(1, 2)),
    ]);
    let clock = RecordingPause::default();
    let client = Client::new(transport, clock.clone());
    client
        .fetch_day(&spy_sip_1hour(), day("2026-09-24"))
        .await
        .unwrap();
    assert_eq!(clock.pauses(), secs(&[20]));

    let transport = FakeTransport::serving([
        ok_rated(&bars_page(0, 3), 200, 5, START_SECS + 20),
        status(429),
        ok(&bars_page(1, 3)),
        ok(&bars_page(2, 3)),
    ]);
    let clock = RecordingPause::default();
    let client = Client::new(transport, clock.clone());
    client
        .fetch_day(&spy_sip_1hour(), day("2026-09-24"))
        .await
        .unwrap();
    assert_eq!(
        clock.pauses(),
        secs(&[20, 60]),
        "a 429 at the reset itself waits a full window"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn rate_limit_retries_span_several_windows_before_giving_up() {
    let transport = FakeTransport::serving((0..4).map(|_| status(429)));
    let clock = RecordingPause::default();
    let client = Client::new(transport.clone(), clock.clone());
    let result = client.fetch_day(&spy_sip_1hour(), day("2026-09-24")).await;
    assert_eq!(
        result,
        Err(FetchError::Exhausted {
            attempts: 4,
            last: RetryCause::Status(429)
        })
    );
    assert_eq!(clock.pauses(), secs(&[60, 60, 60]));
    assert!(
        clock.elapsed() > Duration::from_secs(120),
        "more than two windows"
    );
    assert_eq!(transport.unused(), 0);

    let transport =
        FakeTransport::serving([status(429), status(429), status(429), ok(&bars_page(0, 1))]);
    let client = Client::new(transport, RecordingPause::default());
    assert!(
        client
            .fetch_day(&spy_sip_1hour(), day("2026-09-24"))
            .await
            .is_ok(),
        "a window that slips twice still succeeds"
    );

    let transport = FakeTransport::serving([status(429)]);
    let clock = RecordingPause::default();
    let no_waiting = RetryPolicy {
        rate_limit_windows: 0,
        ..RetryPolicy::default()
    };
    let client = Client::new(transport, clock.clone()).with_retry(no_waiting);
    assert_eq!(
        client.fetch_day(&spy_sip_1hour(), day("2026-09-24")).await,
        Err(FetchError::Exhausted {
            attempts: 1,
            last: RetryCause::Status(429)
        })
    );
    assert!(clock.pauses().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a_429_and_server_errors_count_against_separate_budgets() {
    let transport = FakeTransport::serving([
        status(503),
        status(429),
        status(502),
        status(429),
        ok(&bars_page(0, 1)),
    ]);
    let clock = RecordingPause::default();
    let client = Client::new(transport, clock.clone()).with_retry(RetryPolicy {
        max_attempts: 3,
        rate_limit_windows: 2,
        ..RetryPolicy::default()
    });
    client
        .fetch_day(&spy_sip_1hour(), day("2026-09-24"))
        .await
        .unwrap();
    assert_eq!(clock.pauses(), secs(&[1, 60, 2, 60]));

    let transport = FakeTransport::serving([status(429), status(503), ok(&bars_page(0, 1))]);
    let clock = RecordingPause::default();
    let client = Client::new(transport, clock.clone()).with_retry(RetryPolicy {
        rate_limit_windows: 1,
        ..RetryPolicy::default()
    });
    client
        .fetch_day(&spy_sip_1hour(), day("2026-09-24"))
        .await
        .unwrap();
    assert_eq!(
        clock.pauses(),
        secs(&[60, 1]),
        "a server error after the last allowed 429 still retries"
    );

    let transport = FakeTransport::serving([status(429), status(503), status(503)]);
    let client = Client::new(transport, RecordingPause::default()).with_retry(RetryPolicy {
        max_attempts: 2,
        ..RetryPolicy::default()
    });
    assert_eq!(
        client.fetch_day(&spy_sip_1hour(), day("2026-09-24")).await,
        Err(FetchError::Exhausted {
            attempts: 3,
            last: RetryCause::Status(503)
        })
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_reset_in_the_past_or_now_is_ignored() {
    for reset in [START_SECS - 5, START_SECS] {
        let transport = FakeTransport::serving([
            ok_rated(&bars_page(0, 2), 200, 0, reset),
            ok(&bars_page(1, 2)),
        ]);
        let clock = RecordingPause::default();
        let client = Client::new(transport, clock.clone());
        client
            .fetch_day(&spy_sip_1hour(), day("2026-09-24"))
            .await
            .unwrap();
        assert!(clock.pauses().is_empty(), "{reset}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_full_budget_is_spent_down_to_the_low_water_mark() {
    let transport = FakeTransport::serving(
        std::iter::once(ok_rated(&bars_page(0, 7), 10, 10, START_SECS + 30))
            .chain((1..7).map(|i| ok(&bars_page(i, 7)))),
    );
    let clock = RecordingPause::default();
    let client = Client::new(transport, clock.clone()).with_rate(policy(200, 5));
    client
        .fetch_day(&spy_sip_1hour(), day("2026-09-24"))
        .await
        .unwrap();
    assert_eq!(
        clock.pauses(),
        secs(&[30]),
        "requests 2 to 6 spend 10 down to 5"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn remaining_is_never_spent_below_zero() {
    let transport = FakeTransport::serving([
        ok_rated(&bars_page(0, 3), 3, 1, START_SECS + 30),
        ok(&bars_page(1, 3)),
        ok(&bars_page(2, 3)),
    ]);
    let clock = RecordingPause::default();
    let client = Client::new(transport, clock.clone()).with_rate(policy(200, 0));
    client
        .fetch_day(&spy_sip_1hour(), day("2026-09-24"))
        .await
        .unwrap();
    assert_eq!(clock.pauses(), secs(&[30]));
}

#[tokio::test(flavor = "current_thread")]
async fn a_stale_response_never_raises_the_remaining_count() {
    let transport = FakeTransport::serving([
        ok_rated(&bars_page(0, 3), 200, 6, START_SECS + 50),
        ok_rated(&bars_page(1, 3), 200, 9, START_SECS + 50),
        ok(&bars_page(2, 3)),
    ]);
    let clock = RecordingPause::default();
    let client = Client::new(transport, clock.clone());
    client
        .fetch_day(&spy_sip_1hour(), day("2026-09-24"))
        .await
        .unwrap();
    assert_eq!(
        clock.pauses(),
        secs(&[50]),
        "the same reset keeps the lower count"
    );

    let transport = FakeTransport::serving([
        ok_rated(&bars_page(0, 3), 200, 100, START_SECS + 50),
        ok_rated(&bars_page(1, 3), 200, 3, START_SECS + 50),
        ok(&bars_page(2, 3)),
    ]);
    let clock = RecordingPause::default();
    let client = Client::new(transport, clock.clone());
    client
        .fetch_day(&spy_sip_1hour(), day("2026-09-24"))
        .await
        .unwrap();
    assert_eq!(
        clock.pauses(),
        secs(&[50]),
        "a lower count for the same reset is another client's spending"
    );

    let transport = FakeTransport::serving([
        ok_rated(&bars_page(0, 3), 200, 6, START_SECS + 50),
        ok_rated(&bars_page(1, 3), 200, 100, START_SECS + 20),
        ok(&bars_page(2, 3)),
    ]);
    let clock = RecordingPause::default();
    let client = Client::new(transport, clock.clone());
    client
        .fetch_day(&spy_sip_1hour(), day("2026-09-24"))
        .await
        .unwrap();
    assert_eq!(
        clock.pauses(),
        secs(&[50]),
        "an earlier reset is an older response"
    );

    let transport = FakeTransport::serving([
        ok_rated(&bars_page(0, 3), 200, 6, START_SECS + 50),
        ok_rated(&bars_page(1, 3), 200, 100, START_SECS + 80),
        ok(&bars_page(2, 3)),
    ]);
    let clock = RecordingPause::default();
    let client = Client::new(transport, clock.clone());
    client
        .fetch_day(&spy_sip_1hour(), day("2026-09-24"))
        .await
        .unwrap();
    assert!(clock.pauses().is_empty(), "a later reset is a new budget");
}

#[tokio::test(flavor = "current_thread")]
async fn a_wait_for_the_reset_is_bounded_by_a_window_and_jittered() {
    let cases = [
        (0, START_SECS + 10, Duration::from_secs(10)),
        (
            250_000_000,
            START_SECS + 10,
            Duration::from_millis(9_750 + 125),
        ),
        (
            500_000_000,
            START_SECS + 3600,
            Duration::from_millis(60_000 + 250),
        ),
    ];
    for (start_nanos, reset, expected) in cases {
        let transport = FakeTransport::serving([
            ok_rated(&bars_page(0, 2), 200, 5, reset),
            ok(&bars_page(1, 2)),
        ]);
        let clock = RecordingPause::starting_at(at(START_SECS, start_nanos));
        let client = Client::new(transport, clock.clone());
        client
            .fetch_day(&spy_sip_1hour(), day("2026-09-24"))
            .await
            .unwrap();
        assert_eq!(clock.pauses(), [expected], "{start_nanos} {reset}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_limit_below_two_paces_as_two() {
    let transport = FakeTransport::serving((0..3).map(|i| ok(&bars_page(i, 3))));
    let clock = RecordingPause::default();
    let client = Client::new(transport, clock.clone()).with_rate(policy(0, 5));
    client
        .fetch_day(&spy_sip_1hour(), day("2026-09-24"))
        .await
        .unwrap();
    assert_eq!(
        clock.pauses(),
        secs(&[60, 60]),
        "a burst of one, refilled at one a minute"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_partly_refilled_bucket_waits_only_for_the_rest_of_a_request() {
    let transport =
        FakeTransport::serving([ok(&bars_page(0, 2)), status(503), ok(&bars_page(1, 2))]);
    let clock = RecordingPause::default();
    let client = Client::new(transport, clock.clone()).with_rate(policy(2, 5));
    client
        .fetch_day(&spy_sip_1hour(), day("2026-09-24"))
        .await
        .unwrap();
    assert_eq!(
        clock.pauses(),
        secs(&[60, 1, 59]),
        "the second of backoff refilled a sixtieth of a request"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn concurrent_requests_through_one_client_share_the_budget() {
    let clock = RecordingPause::default();
    let host = Host::new(Limiter::FixedWindow, Headers::Accurate, 20, 10, &clock).yielding();
    let client = Client::new(host.clone(), clock.clone()).with_rate(policy(20, 2));
    let dataset = spy_sip_1hour();
    let fetch = || client.fetch_day(&dataset, day("2026-09-24"));
    let results = tokio::join!(
        fetch(),
        fetch(),
        fetch(),
        fetch(),
        fetch(),
        fetch(),
        fetch(),
        fetch()
    );
    for result in [
        results.0, results.1, results.2, results.3, results.4, results.5, results.6, results.7,
    ] {
        assert_eq!(result.unwrap().len(), 10);
    }
    assert_eq!(
        host.refused(),
        0,
        "in-flight requests count against the budget"
    );
    let mut per_window: BTreeMap<i128, u32> = BTreeMap::new();
    for t in host.accepted_times() {
        *per_window.entry(t.div_euclid(MINUTE)).or_default() += 1;
    }
    assert!(per_window.values().all(|n| *n <= 20), "{per_window:?}");
}

#[test]
fn the_system_clock_is_read_as_utc() {
    let now = TokioPause.now();
    assert!(now > at(START_SECS - 86_400, 0), "{now:?}");
    assert!(now < at(START_SECS + 100 * 365 * 86_400, 0), "{now:?}");
}
