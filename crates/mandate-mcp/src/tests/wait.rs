//! A read waits a bounded time for the ordinary bucket's next token (DEC-959 item 2, the R0
//! rehearsal's R11): the rehearsal's own sequence of calls fits the conservative budget, a read
//! that would wait past the bound is refused at once and sends nothing, and an opening or an exit
//! never waits. The transport runs on a [`StepClock`] that a wait moves by exactly its span, so
//! the clock is the record of every wait.

use std::time::Duration;

use serde_json::json;

use super::server::{StepClock, reply, serve, with_type};
use crate::{BucketConfig, BudgetConfig, CallClass, McpError, Monotonic, TransportConfig};

const OK: &str = r#""result":{}"#;

fn config(capacity: u32, refill_every: Duration, read_wait: Duration) -> TransportConfig {
    TransportConfig {
        budget: BudgetConfig {
            ordinary: BucketConfig {
                capacity,
                refill_every,
            },
            reserved: BucketConfig {
                capacity: 1,
                refill_every: Duration::from_secs(3600),
            },
        },
        read_wait,
        ..TransportConfig::CONSERVATIVE
    }
}

/// The tools a live start reads, in the order the rehearsal's run reads them once the preflight
/// reuses the lapse's snapshot: the lapse's three scoped reads, each behind its `get_accounts`
/// check, then the preflight's tradability, behind its own, and the quote.
const REHEARSAL_READS: [&str; 9] = [
    "get_accounts",
    "get_portfolio",
    "get_accounts",
    "get_equity_positions",
    "get_accounts",
    "get_equity_orders",
    "get_accounts",
    "get_equity_tradability",
    "get_equity_quotes",
];

/// R11: on the conservative budget, the connect's `initialize`, `notifications/initialized` and
/// `tools/list`, then the nine reads, all go through. The ordinary bucket's ten tokens are spent
/// by the tenth call, so the last two reads each wait one refill period, never longer than the
/// bound, and nothing is refused.
#[tokio::test]
async fn the_rehearsals_read_sequence_fits_the_budget() {
    let mut answers = vec![reply(OK), with_type(202, "application/json", ""), reply(OK)];
    answers.extend(REHEARSAL_READS.iter().map(|_| reply(OK)));
    let server = serve(answers).await;
    let clock = StepClock::default();
    let transport = server.transport_with_clock(TransportConfig::CONSERVATIVE, &clock);
    let ordinary = CallClass::Ordinary;
    transport
        .request(ordinary, "initialize", &json!({}))
        .await
        .unwrap();
    transport
        .notify(ordinary, "notifications/initialized", &json!({}))
        .await
        .unwrap();
    transport
        .request(ordinary, "tools/list", &json!({}))
        .await
        .unwrap();
    let mut waited = Vec::new();
    for tool in REHEARSAL_READS {
        let before = clock.elapsed();
        let params = json!({"name": tool, "arguments": {}});
        let answered = transport
            .request(CallClass::Read, "tools/call", &params)
            .await;
        assert!(answered.is_ok(), "{tool}: {answered:?}");
        waited.push(clock.elapsed() - before);
    }
    let second = Duration::from_secs(1);
    let mut expected = vec![Duration::ZERO; 7];
    expected.extend([second, second]);
    assert_eq!(waited, expected);
    assert_eq!(server.seen().len(), 12);
}

/// A read on an empty bucket whose next token is due past the bound is refused at once: the
/// clock does not move and nothing is sent. One due exactly at the bound is waited for and sent.
#[tokio::test]
async fn an_exhausted_budget_waits_then_refuses_within_the_bound() {
    let server = serve(vec![reply(OK), reply(OK)]).await;
    let clock = StepClock::default();
    let bound = Duration::from_secs(2);
    let transport = server.transport_with_clock(config(1, Duration::from_secs(3), bound), &clock);
    let read = |name: &'static str| {
        let transport = &transport;
        async move {
            transport
                .request(CallClass::Read, "tools/call", &json!({"name": name}))
                .await
        }
    };
    read("first").await.unwrap();
    let refused = read("refused").await.unwrap_err();
    assert!(matches!(refused, McpError::Throttled), "{refused:?}");
    assert_eq!(clock.elapsed(), Duration::ZERO);
    assert_eq!(server.seen().len(), 1);
    clock.set(Duration::from_secs(1));
    read("at the bound").await.unwrap();
    assert_eq!(clock.elapsed(), Duration::from_secs(3));
    let past = read("past the bound").await.unwrap_err();
    assert!(matches!(past, McpError::Throttled), "{past:?}");
    assert_eq!(clock.elapsed(), Duration::from_secs(3));
    let seen = server.seen();
    assert_eq!(seen.len(), 2);
    assert!(seen[1].contains("at the bound"), "{}", seen[1]);
}

/// A token due one nanosecond past the bound is not waited for.
#[tokio::test]
async fn a_token_due_just_past_the_bound_is_refused() {
    let server = serve(vec![reply(OK)]).await;
    let clock = StepClock::default();
    let bound = Duration::from_secs(2);
    let period = bound + Duration::from_nanos(1);
    let transport = server.transport_with_clock(config(1, period, bound), &clock);
    transport
        .request(CallClass::Read, "tools/call", &json!({}))
        .await
        .unwrap();
    let refused = transport
        .request(CallClass::Read, "tools/call", &json!({}))
        .await
        .unwrap_err();
    assert!(matches!(refused, McpError::Throttled), "{refused:?}");
    assert_eq!(clock.elapsed(), Duration::ZERO);
}

/// Only a read waits: an opening on the empty ordinary bucket and an exit on two empty buckets
/// are refused at once, with the clock unmoved; an exit takes the reserved token without
/// waiting; and a read then waits its one period. A zero bound never waits.
#[tokio::test]
async fn an_opening_or_an_exit_never_waits() {
    let server = serve(vec![reply(OK), reply(OK), reply(OK)]).await;
    let clock = StepClock::default();
    let second = Duration::from_secs(1);
    let transport = server.transport_with_clock(config(1, second, second), &clock);
    let call = |class| {
        let transport = &transport;
        async move { transport.request(class, "tools/call", &json!({})).await }
    };
    call(CallClass::Read).await.unwrap();
    let opening = call(CallClass::Ordinary).await.unwrap_err();
    assert!(matches!(opening, McpError::Throttled), "{opening:?}");
    assert_eq!(clock.elapsed(), Duration::ZERO);
    call(CallClass::RiskReducing).await.unwrap();
    let exit = call(CallClass::RiskReducing).await.unwrap_err();
    assert!(matches!(exit, McpError::Throttled), "{exit:?}");
    assert_eq!(clock.elapsed(), Duration::ZERO);
    call(CallClass::Read).await.unwrap();
    assert_eq!(clock.elapsed(), second);
    assert_eq!(server.seen().len(), 3);

    let server = serve(vec![reply(OK)]).await;
    let clock = StepClock::default();
    let transport = server.transport_with_clock(config(1, second, Duration::ZERO), &clock);
    let call = |class| {
        let transport = &transport;
        async move { transport.request(class, "tools/call", &json!({})).await }
    };
    call(CallClass::Read).await.unwrap();
    let never = call(CallClass::Read).await.unwrap_err();
    assert!(matches!(never, McpError::Throttled), "{never:?}");
    assert_eq!(clock.elapsed(), Duration::ZERO);
}
