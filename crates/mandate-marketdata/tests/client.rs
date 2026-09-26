//! Pagination, retries, and backoff against recorded and scripted responses; no network.

mod common;

use std::time::Duration;

use common::{
    FakeTransport, RecordingPause, btc_trades, day, ok, scenario, shy_iex_trades, spy_sip_1hour,
    status,
};
use mandate_marketdata::client::{Client, FetchError, RetryCause, RetryPolicy, TransportError};
use mandate_marketdata::model::DatasetId;

fn secs(values: &[u64]) -> Vec<Duration> {
    values.iter().copied().map(Duration::from_secs).collect()
}

#[tokio::test(flavor = "current_thread")]
async fn pagination_follows_tokens_until_null_in_order() {
    let cases: [(&str, DatasetId, u32, usize); 3] = [
        (
            "stock-bars-sip-spy-1hour-2026-09-24-paged",
            spy_sip_1hour(),
            5,
            16,
        ),
        (
            "stock-trades-iex-shy-2026-09-24-paged",
            shy_iex_trades(),
            300,
            860,
        ),
        (
            "crypto-trades-btcusd-2026-09-24-paged",
            btc_trades(),
            300,
            878,
        ),
    ];
    for (name, dataset, limit, rows) in cases {
        let s = scenario(name);
        assert!(s.requests.len() > 1, "{name} must span several pages");
        let transport = FakeTransport::serving(s.bodies.iter().map(|b| ok(b)));
        let pause = RecordingPause::default();
        let client = Client::new(transport.clone(), pause.clone()).with_page_limit(limit);
        let records = client.fetch_day(&dataset, day("2026-09-24")).await.unwrap();
        assert_eq!(records.len(), rows, "{name}");
        assert_eq!(transport.requested(), s.requests, "{name}");
        assert_eq!(transport.unused(), 0, "{name}");
        assert!(pause.pauses().is_empty(), "{name}");
        let times = records.times();
        assert!(times.windows(2).all(|w| w[0] <= w[1]), "{name}");
    }
}

fn page(times: &[&str], token: Option<&str>) -> Vec<u8> {
    let bars: Vec<String> = times
        .iter()
        .map(|t| format!(r#"{{"c":1,"h":1,"l":1,"n":1,"o":1,"t":"{t}","v":1,"vw":1}}"#))
        .collect();
    let token = token.map_or("null".to_owned(), |t| format!("\"{t}\""));
    format!(
        r#"{{"bars":{{"SPY":[{}]}},"next_page_token":{token}}}"#,
        bars.join(",")
    )
    .into_bytes()
}

#[tokio::test(flavor = "current_thread")]
async fn a_repeated_page_token_is_an_error() {
    let transport = FakeTransport::serving([
        ok(&page(&["2026-09-24T08:00:00Z"], Some("A"))),
        ok(&page(&["2026-09-24T09:00:00Z"], Some("B"))),
        ok(&page(&["2026-09-24T10:00:00Z"], Some("A"))),
    ]);
    let client = Client::new(transport.clone(), RecordingPause::default());
    let result = client.fetch_day(&spy_sip_1hour(), day("2026-09-24")).await;
    assert_eq!(result, Err(FetchError::RepeatedPageToken { page: 3 }));
    assert_eq!(transport.requested().len(), 3);
}

#[tokio::test(flavor = "current_thread")]
async fn records_out_of_time_order_across_pages_are_an_error() {
    let transport = FakeTransport::serving([
        ok(&page(&["2026-09-24T09:00:00Z"], Some("A"))),
        ok(&page(&["2026-09-24T08:00:00Z"], None)),
    ]);
    let client = Client::new(transport, RecordingPause::default());
    let result = client.fetch_day(&spy_sip_1hour(), day("2026-09-24")).await;
    assert!(matches!(result, Err(FetchError::OutOfOrder { .. })));
}

#[tokio::test(flavor = "current_thread")]
async fn server_errors_back_off_exponentially_and_a_rate_limit_waits_a_window() {
    let body = page(&["2026-09-24T08:00:00Z"], None);
    let transport = FakeTransport::serving([
        status(429),
        status(503),
        Err(TransportError::Timeout),
        status(500),
        ok(&body),
    ]);
    let pause = RecordingPause::default();
    let client = Client::new(transport.clone(), pause.clone());
    let records = client
        .fetch_day(&spy_sip_1hour(), day("2026-09-24"))
        .await
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(pause.pauses(), secs(&[60, 1, 2, 4]));
    let requested = transport.requested();
    assert_eq!(requested.len(), 5);
    assert!(
        requested.iter().all(|r| r == &requested[0]),
        "a retry repeats the same request"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn retries_stop_after_the_attempt_budget() {
    let transport = FakeTransport::serving((0..6).map(|_| status(503)));
    let pause = RecordingPause::default();
    let client = Client::new(transport.clone(), pause.clone());
    let result = client.fetch_day(&spy_sip_1hour(), day("2026-09-24")).await;
    assert_eq!(
        result,
        Err(FetchError::Exhausted {
            attempts: 6,
            last: RetryCause::Status(503)
        })
    );
    assert_eq!(pause.pauses(), secs(&[1, 2, 4, 8, 16]));
    assert_eq!(transport.unused(), 0);

    let capped = RetryPolicy {
        max_attempts: 5,
        first_delay: Duration::from_secs(1),
        max_delay: Duration::from_secs(4),
        ..RetryPolicy::default()
    };
    let transport = FakeTransport::serving((0..5).map(|_| Err(TransportError::Connect)));
    let pause = RecordingPause::default();
    let client = Client::new(transport, pause.clone()).with_retry(capped);
    let result = client.fetch_day(&spy_sip_1hour(), day("2026-09-24")).await;
    assert_eq!(
        result,
        Err(FetchError::Exhausted {
            attempts: 5,
            last: RetryCause::Transport(TransportError::Connect)
        })
    );
    assert_eq!(pause.pauses(), secs(&[1, 2, 4, 4]));
}

#[tokio::test(flavor = "current_thread")]
async fn client_errors_are_not_retried() {
    for code in [400, 401, 403, 404, 422] {
        let transport = FakeTransport::serving([status(code)]);
        let pause = RecordingPause::default();
        let client = Client::new(transport.clone(), pause.clone());
        let result = client.fetch_day(&spy_sip_1hour(), day("2026-09-24")).await;
        assert_eq!(result, Err(FetchError::Status { status: code }));
        assert_eq!(transport.requested().len(), 1, "{code}");
        assert!(pause.pauses().is_empty(), "{code}");
    }
    let transport = FakeTransport::serving([Err(TransportError::RefusedPath)]);
    let client = Client::new(transport.clone(), RecordingPause::default());
    let result = client.fetch_day(&spy_sip_1hour(), day("2026-09-24")).await;
    assert_eq!(
        result,
        Err(FetchError::Transport(TransportError::RefusedPath))
    );
    assert_eq!(transport.requested().len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn a_malformed_page_names_its_page_number() {
    let transport = FakeTransport::serving([
        ok(&page(&["2026-09-24T08:00:00Z"], Some("A"))),
        ok(br#"{"message":"oops"}"#),
    ]);
    let client = Client::new(transport, RecordingPause::default());
    let result = client.fetch_day(&spy_sip_1hour(), day("2026-09-24")).await;
    assert!(matches!(result, Err(FetchError::Wire { page: 2, .. })));
}
