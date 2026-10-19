//! E2-5 slice 2 (DEC-1174; data-plane spec v0.3 §3.2, §3.3, DP-5, DP-7): the stream's connection
//! states and reconnect over recorded frames, with no network and no clock. Each expectation is
//! the spec's table, written out here, and the property's oracle is its own record of what it fed.

use std::collections::BTreeSet;
use std::time::Duration;

use mandate_accounting::InstrumentId;
use mandate_alpaca::stream::{Channels, read_frame, subscribe_frame};
use mandate_alpaca::stream_link::{
    FIRST_BACKOFF, Link, LinkAction, LinkInput, LinkState, MAX_BACKOFF, backoff,
};
use mandate_time::UtcNanos;
use proptest::prelude::*;

const DOWN_ALERT_S: i64 = 30;

fn id(symbol: &str) -> InstrumentId {
    InstrumentId::new(symbol).unwrap()
}

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(1_790_000_000 + secs, 0).unwrap()
}

fn channels() -> Channels {
    let both: BTreeSet<InstrumentId> = [id("AAPL"), id("SPY")].into_iter().collect();
    Channels {
        quotes: both.clone(),
        statuses: both,
        trades: [id("AAPL")].into_iter().collect(),
        ..Channels::default()
    }
}

fn fresh() -> Link {
    Link::new(channels(), Duration::from_secs(30))
}

fn frame(messages: &[String]) -> LinkInput {
    LinkInput::Frame(read_frame(&format!("[{}]", messages.join(","))).unwrap())
}

fn connected() -> String {
    r#"{"T":"success","msg":"connected"}"#.to_owned()
}

fn authenticated() -> String {
    r#"{"T":"success","msg":"authenticated"}"#.to_owned()
}

fn subscription() -> String {
    r#"{"T":"subscription","trades":["AAPL"],"quotes":["AAPL","SPY"],"statuses":["AAPL","SPY"]}"#
        .to_owned()
}

fn quote(symbol: &str) -> String {
    format!(
        r#"{{"T":"q","S":"{symbol}","bx":"U","bp":87.66,"bs":1,"ax":"Q","ap":87.68,"as":4,"t":"2026-10-19T15:51:45.335689322Z","c":["R"],"z":"C"}}"#
    )
}

fn status(symbol: &str) -> String {
    format!(
        r#"{{"T":"s","S":"{symbol}","sc":"T","sm":"Trading","rc":"","rm":"","t":"2026-10-19T15:51:45Z","z":"C"}}"#
    )
}

fn error(code: u16) -> String {
    format!(r#"{{"T":"error","code":{code},"msg":"x"}}"#)
}

/// Connects, authenticates and subscribes at `secs`, returning the actions of the subscription.
fn subscribe(link: &mut Link, secs: i64) -> Vec<LinkAction> {
    assert_eq!(
        link.on(at(secs), frame(&[connected()]), 0),
        [LinkAction::Authenticate]
    );
    assert_eq!(
        link.on(at(secs), frame(&[authenticated()]), 0),
        [LinkAction::Subscribe(subscribe_frame(&channels()).unwrap())]
    );
    link.on(at(secs), frame(&[subscription()]), 0)
}

fn reconnect_after(actions: &[LinkAction]) -> Option<Duration> {
    actions.iter().find_map(|a| match a {
        LinkAction::Reconnect { after } => Some(*after),
        _ => None,
    })
}

#[test]
fn a_connection_authenticates_subscribes_and_resyncs_each_instrument_alone() {
    let mut link = fresh();
    assert_eq!(link.state(), LinkState::Connecting);
    assert_eq!(subscribe(&mut link, 0), []);
    assert_eq!(link.state(), LinkState::Resyncing);
    assert!(!link.opens(&id("AAPL")), "no quote or status yet");
    link.on(at(1), frame(&[quote("AAPL")]), 0);
    assert!(!link.opens(&id("AAPL")), "a quote alone is not a resync");
    link.on(at(2), frame(&[status("AAPL")]), 0);
    assert!(link.opens(&id("AAPL")), "AAPL resynced on its own");
    assert!(!link.opens(&id("SPY")), "SPY still waits");
    assert_eq!(link.state(), LinkState::Resyncing);
    link.on(at(3), frame(&[status("SPY"), quote("SPY")]), 0);
    assert_eq!(link.state(), LinkState::Live);
    assert!(link.opens(&id("SPY")));
    assert!(!link.opens(&id("QQQ")), "never subscribed");
}

#[test]
fn what_arrives_before_the_subscription_does_not_resync() {
    let mut link = fresh();
    link.on(at(0), frame(&[connected()]), 0);
    link.on(at(0), frame(&[authenticated()]), 0);
    link.on(
        at(1),
        frame(&[quote("AAPL"), status("AAPL"), subscription()]),
        0,
    );
    assert!(
        !link.opens(&id("AAPL")),
        "both arrived before the subscription"
    );
    link.on(at(2), frame(&[quote("AAPL"), status("AAPL")]), 0);
    assert!(link.opens(&id("AAPL")));
}

#[test]
fn an_instrument_with_no_status_channel_never_opens() {
    let mut link = Link::new(
        Channels {
            quotes: [id("AAPL")].into_iter().collect(),
            ..Channels::default()
        },
        Duration::from_secs(30),
    );
    link.on(at(0), frame(&[connected()]), 0);
    link.on(at(0), frame(&[authenticated()]), 0);
    link.on(
        at(0),
        frame(&[r#"{"T":"subscription","quotes":["AAPL"]}"#.to_owned()]),
        0,
    );
    link.on(at(1), frame(&[quote("AAPL"), quote("AAPL")]), 0);
    assert!(!link.opens(&id("AAPL")), "DEC-1174 item 2");
    assert_ne!(link.state(), LinkState::Live);
}

#[test]
fn a_loss_blocks_every_instrument_and_the_reconnect_resyncs_from_scratch_with_its_gap() {
    let mut link = fresh();
    subscribe(&mut link, 0);
    link.on(
        at(1),
        frame(&[quote("AAPL"), status("AAPL"), quote("SPY"), status("SPY")]),
        0,
    );
    assert_eq!(link.state(), LinkState::Live);
    let lost = link.on(at(5), LinkInput::Lost, 0);
    assert_eq!(link.state(), LinkState::Down);
    assert!(!link.opens(&id("AAPL")) && !link.opens(&id("SPY")));
    assert_eq!(reconnect_after(&lost), Some(backoff(0, 0)));
    assert_eq!(link.on(at(6), LinkInput::Reconnecting, 0), []);
    assert_eq!(link.state(), LinkState::Connecting);
    let resubscribed = subscribe(&mut link, 9);
    assert_eq!(
        resubscribed,
        [LinkAction::Gap {
            from: at(1),
            to: at(9)
        }]
    );
    assert_eq!(link.state(), LinkState::Resyncing);
    assert!(
        !link.opens(&id("AAPL")),
        "the old quote and status do not count"
    );
    link.on(at(10), frame(&[quote("AAPL"), status("AAPL")]), 0);
    assert!(link.opens(&id("AAPL")));
}

#[test]
fn a_rejected_credential_or_plan_pauses_and_never_reconnects() {
    for code in [402, 409] {
        let mut link = fresh();
        link.on(at(0), frame(&[connected()]), 0);
        let actions = link.on(at(0), frame(&[error(code)]), 0);
        assert_eq!(actions, [LinkAction::Alert("stream_credential_rejected")]);
        assert_eq!(link.state(), LinkState::Paused);
        assert_eq!(link.on(at(1), LinkInput::Lost, 0), [], "{code}");
        assert_eq!(link.on(at(100), LinkInput::Tick, 0), [], "{code}");
        assert_eq!(link.state(), LinkState::Paused);
        assert!(!link.opens(&id("AAPL")));
    }
}

#[test]
fn a_symbol_limit_is_reported_and_any_other_error_goes_down() {
    let mut link = fresh();
    subscribe(&mut link, 0);
    assert_eq!(
        link.on(at(1), frame(&[error(405)]), 0),
        [LinkAction::Alert("stream_symbol_limit")]
    );
    assert_eq!(link.state(), LinkState::Resyncing);
    for code in [406, 407, 500] {
        let mut link = fresh();
        subscribe(&mut link, 0);
        let actions = link.on(at(1), frame(&[error(code)]), 0);
        assert_eq!(link.state(), LinkState::Down, "{code}");
        assert!(reconnect_after(&actions).is_some(), "{code}");
    }
}

#[test]
fn the_backoff_doubles_with_jitter_and_is_capped_at_thirty_seconds() {
    assert_eq!(FIRST_BACKOFF, Duration::from_secs(1));
    assert_eq!(MAX_BACKOFF, Duration::from_secs(30));
    assert_eq!(backoff(0, 0), Duration::from_millis(500));
    assert_eq!(backoff(0, 999), Duration::from_micros(999_500));
    assert_eq!(backoff(1, 0), Duration::from_secs(1));
    assert_eq!(backoff(2, 500), Duration::from_secs(3));
    assert_eq!(backoff(4, 1000), Duration::from_secs(16));
    assert_eq!(backoff(5, 999), Duration::from_micros(29_985_000));
    assert_eq!(backoff(5, 0), Duration::from_secs(15));
    assert_eq!(backoff(40, 999), Duration::from_micros(29_985_000));
    assert_eq!(backoff(u32::MAX, 1000), MAX_BACKOFF);
}

#[test]
fn reconnects_grow_until_the_connection_is_live_again() {
    let mut link = fresh();
    let mut delays = Vec::new();
    for n in 0..3 {
        link.on(at(n), LinkInput::Reconnecting, 0);
        delays.push(reconnect_after(&link.on(at(n), LinkInput::Lost, 0)).unwrap());
    }
    assert_eq!(delays, [backoff(0, 0), backoff(1, 0), backoff(2, 0)]);
    link.on(at(10), LinkInput::Reconnecting, 0);
    subscribe(&mut link, 10);
    link.on(
        at(11),
        frame(&[quote("AAPL"), status("AAPL"), quote("SPY"), status("SPY")]),
        0,
    );
    assert_eq!(link.state(), LinkState::Live);
    let lost = link.on(at(12), LinkInput::Lost, 0);
    assert_eq!(
        reconnect_after(&lost),
        Some(backoff(0, 0)),
        "live resets it"
    );
}

#[test]
fn a_connection_down_past_its_threshold_alerts_once() {
    let mut link = fresh();
    subscribe(&mut link, 0);
    link.on(at(1), LinkInput::Lost, 0);
    assert_eq!(link.on(at(1 + DOWN_ALERT_S), LinkInput::Tick, 0), []);
    assert_eq!(
        link.on(at(2 + DOWN_ALERT_S), LinkInput::Tick, 0),
        [LinkAction::Alert("stream_down")]
    );
    assert_eq!(link.on(at(3 + DOWN_ALERT_S), LinkInput::Tick, 0), []);
    link.on(at(4 + DOWN_ALERT_S), LinkInput::Reconnecting, 0);
    subscribe(&mut link, 5 + DOWN_ALERT_S);
    link.on(
        at(5 + DOWN_ALERT_S),
        frame(&[quote("AAPL"), status("AAPL"), quote("SPY"), status("SPY")]),
        0,
    );
    assert_eq!(link.state(), LinkState::Live);
    link.on(at(6 + DOWN_ALERT_S), LinkInput::Lost, 0);
    assert_eq!(
        link.on(at(6 + 2 * DOWN_ALERT_S), LinkInput::Tick, 0),
        [],
        "the clock restarts at the new loss, not the first outage's"
    );
    assert_eq!(
        link.on(at(7 + 2 * DOWN_ALERT_S), LinkInput::Tick, 0),
        [LinkAction::Alert("stream_down")],
        "live again, so a new loss alerts again"
    );
}

#[test]
fn a_flapping_link_that_never_goes_live_alerts_once() {
    let mut link = fresh();
    subscribe(&mut link, 0);
    for n in 0..10 {
        let lost = 1 + 10 * n;
        link.on(at(lost), LinkInput::Lost, 0);
        link.on(at(lost + 2), LinkInput::Reconnecting, 0);
        subscribe(&mut link, lost + 3);
        assert_eq!(link.state(), LinkState::Resyncing);
        link.on(at(lost + 4), frame(&[quote("AAPL")]), 0);
    }
    let alerts: usize = (100..200)
        .map(|secs| {
            link.on(at(secs), LinkInput::Tick, 0)
                .iter()
                .filter(|a| **a == LinkAction::Alert("stream_down"))
                .count()
        })
        .sum();
    assert_eq!(
        alerts, 1,
        "a subscription that never resyncs ends no outage"
    );
}

#[test]
fn a_frame_while_down_or_a_handshake_out_of_turn_asks_for_nothing() {
    let mut link = fresh();
    subscribe(&mut link, 0);
    link.on(
        at(1),
        frame(&[quote("AAPL"), status("AAPL"), quote("SPY"), status("SPY")]),
        0,
    );
    assert_eq!(link.on(at(2), frame(&[connected()]), 0), [], "live");
    assert_eq!(link.on(at(2), frame(&[authenticated()]), 0), [], "live");
    link.on(at(3), LinkInput::Lost, 0);
    assert_eq!(
        link.on(at(4), frame(&[error(500), quote("AAPL")]), 0),
        [],
        "already down: no second reconnect"
    );
    assert_eq!(link.on(at(4), LinkInput::Lost, 0), []);
    assert_eq!(link.state(), LinkState::Down);
}

#[test]
fn a_gap_runs_from_the_last_message_before_the_first_loss() {
    let mut link = fresh();
    link.on(at(0), frame(&[connected()]), 0);
    link.on(at(1), LinkInput::Lost, 0);
    link.on(at(2), LinkInput::Reconnecting, 0);
    assert_eq!(
        subscribe(&mut link, 3),
        [],
        "no gap before any subscription"
    );
    link.on(at(4), frame(&[quote("AAPL")]), 0);
    link.on(at(5), LinkInput::Lost, 0);
    link.on(at(6), LinkInput::Reconnecting, 0);
    link.on(at(7), frame(&[connected()]), 0);
    link.on(at(8), LinkInput::Lost, 0);
    link.on(at(9), LinkInput::Reconnecting, 0);
    assert_eq!(
        subscribe(&mut link, 10),
        [LinkAction::Gap {
            from: at(4),
            to: at(10)
        }]
    );
    link.on(at(11), frame(&[quote("SPY")]), 0);
    link.on(at(12), LinkInput::Reconnecting, 0);
    assert_eq!(
        subscribe(&mut link, 13),
        [LinkAction::Gap {
            from: at(11),
            to: at(13)
        }],
        "a restart the link did not ask for is a gap too"
    );
}

#[test]
fn a_rejected_channel_narrows_the_subscription_and_never_reconnects() {
    let mut link = fresh();
    subscribe(&mut link, 0);
    let actions = link.on(at(1), frame(&[error(410)]), 0);
    assert_eq!(
        actions,
        [LinkAction::Subscribe(
            subscribe_frame(&channels().narrowed()).unwrap()
        )]
    );
    assert_eq!(
        link.state(),
        LinkState::Connecting,
        "awaits the narrowed echo"
    );
    link.on(
        at(2),
        frame(&[r#"{"T":"subscription","trades":["AAPL"],"quotes":["AAPL","SPY"]}"#.to_owned()]),
        0,
    );
    assert_eq!(link.state(), LinkState::Resyncing);
    link.on(at(3), frame(&[quote("AAPL"), quote("AAPL")]), 0);
    assert!(
        !link.opens(&id("AAPL")),
        "no status channel now, so no opening under the live rule"
    );
}

#[test]
fn a_status_or_band_that_cannot_be_read_resyncs_the_connection() {
    for unreadable in [
        status("AAPL").replace(r#""sc":"T""#, r#""sc":7"#),
        r#"{"T":"l","S":"AAPL","u":"x"}"#.to_owned(),
    ] {
        let mut link = fresh();
        subscribe(&mut link, 0);
        link.on(
            at(1),
            frame(&[quote("AAPL"), status("AAPL"), quote("SPY"), status("SPY")]),
            0,
        );
        assert_eq!(link.state(), LinkState::Live);
        let actions = link.on(at(2), frame(std::slice::from_ref(&unreadable)), 0);
        assert_eq!(link.state(), LinkState::Down, "{unreadable}");
        assert!(reconnect_after(&actions).is_some());
    }
    let mut link = fresh();
    subscribe(&mut link, 0);
    link.on(at(1), frame(&[quote("AAPL").replace("87.66", "8e1")]), 0);
    assert_eq!(
        link.state(),
        LinkState::Resyncing,
        "a quote it cannot read is only a missed quote"
    );
}

#[test]
fn a_sustained_outage_alerts_once_however_many_reconnects_fail() {
    let mut link = fresh();
    subscribe(&mut link, 0);
    link.on(at(1), LinkInput::Lost, 0);
    link.on(at(20), LinkInput::Reconnecting, 0);
    assert_eq!(link.state(), LinkState::Connecting);
    assert!(reconnect_after(&link.on(at(25), LinkInput::Lost, 0)).is_some());
    assert_eq!(
        link.on(at(2 + DOWN_ALERT_S), LinkInput::Tick, 0),
        [LinkAction::Alert("stream_down")],
        "the outage runs from the first loss, not the failed reconnect"
    );
    link.on(at(40), LinkInput::Reconnecting, 0);
    link.on(at(41), frame(&[connected()]), 0);
    link.on(at(45), LinkInput::Lost, 0);
    assert_eq!(
        link.on(at(90), LinkInput::Tick, 0),
        [],
        "never a second alert"
    );
    link.on(at(91), LinkInput::Reconnecting, 0);
    subscribe(&mut link, 92);
    assert_eq!(
        link.on(at(200), LinkInput::Tick, 0),
        [],
        "alerted once already: never a second alert"
    );
}

#[test]
fn a_second_rejected_channel_pauses_rather_than_narrowing_again() {
    let mut link = fresh();
    subscribe(&mut link, 0);
    link.on(at(1), frame(&[error(410)]), 0);
    link.on(
        at(2),
        frame(&[r#"{"T":"subscription","trades":["AAPL"],"quotes":["AAPL","SPY"]}"#.to_owned()]),
        0,
    );
    assert_eq!(link.state(), LinkState::Resyncing);
    assert_eq!(
        link.on(at(3), frame(&[error(410)]), 0),
        [LinkAction::Alert("stream_subscription_invalid")],
        "nothing left to narrow: no resubscribe loop"
    );
    assert_eq!(link.state(), LinkState::Paused);
    assert_eq!(link.on(at(4), LinkInput::Lost, 0), []);
    let mut link = fresh();
    subscribe(&mut link, 0);
    link.on(at(1), frame(&[error(410)]), 0);
    assert_eq!(
        link.on(at(1), frame(&[error(410)]), 0),
        [LinkAction::Alert("stream_subscription_invalid")],
        "before the narrowed echo too"
    );
    assert_eq!(link.state(), LinkState::Paused);
}

#[test]
fn an_error_that_cannot_be_read_takes_the_link_down() {
    let unreadable = r#"{"T":"error","code":"x","msg":"x"}"#.to_owned();
    for subscribed in [false, true] {
        let mut link = fresh();
        if subscribed {
            subscribe(&mut link, 0);
        } else {
            link.on(at(0), frame(&[connected()]), 0);
        }
        let actions = link.on(at(1), frame(std::slice::from_ref(&unreadable)), 0);
        assert_eq!(link.state(), LinkState::Down, "subscribed: {subscribed}");
        assert!(reconnect_after(&actions).is_some());
        assert_eq!(
            link.on(at(2), frame(std::slice::from_ref(&unreadable)), 0),
            [],
            "already down"
        );
    }
}

#[test]
fn a_pause_or_narrow_from_a_lost_socket_changes_nothing() {
    for code in [402, 409, 410] {
        let mut link = fresh();
        subscribe(&mut link, 0);
        link.on(at(1), LinkInput::Lost, 0);
        assert_eq!(link.on(at(2), frame(&[error(code)]), 0), [], "{code}");
        assert_eq!(link.state(), LinkState::Down, "{code}");
    }
}

#[test]
fn a_subscription_with_nothing_left_pauses_instead_of_waiting() {
    let statuses_only = Channels {
        statuses: [id("AAPL")].into_iter().collect(),
        ..Channels::default()
    };
    let mut link = Link::new(statuses_only, Duration::from_secs(30));
    link.on(at(0), frame(&[connected()]), 0);
    link.on(at(0), frame(&[authenticated()]), 0);
    assert_eq!(
        link.on(at(1), frame(&[error(410)]), 0),
        [LinkAction::Alert("stream_subscription_invalid")]
    );
    assert_eq!(link.state(), LinkState::Paused);
    let mut link = Link::new(Channels::default(), Duration::from_secs(30));
    link.on(at(0), frame(&[connected()]), 0);
    assert_eq!(
        link.on(at(0), frame(&[authenticated(), subscription()]), 0),
        [LinkAction::Alert("stream_subscription_invalid")]
    );
    assert_eq!(
        link.state(),
        LinkState::Paused,
        "nothing after the pause in its frame is read"
    );
    let mut link = fresh();
    link.on(at(0), frame(&[connected()]), 0);
    assert_eq!(
        link.on(at(0), frame(&[authenticated(), subscription()]), 0),
        [LinkAction::Subscribe(subscribe_frame(&channels()).unwrap())]
    );
    assert_eq!(
        link.state(),
        LinkState::Resyncing,
        "a subscription in the same frame is read"
    );
}

#[test]
fn a_quote_missing_a_side_does_not_resync_its_instrument() {
    for one_sided in [
        quote("AAPL")
            .replace(r#""bp":87.66"#, r#""bp":0"#)
            .replace(r#""ap":87.68"#, r#""ap":0"#),
        quote("AAPL").replace(r#""bp":87.66"#, r#""bp":0"#),
        quote("AAPL").replace(r#""ap":87.68"#, r#""ap":0"#),
    ] {
        let mut link = fresh();
        subscribe(&mut link, 0);
        link.on(at(1), frame(&[one_sided.clone(), status("AAPL")]), 0);
        assert!(!link.opens(&id("AAPL")), "{one_sided}");
        link.on(at(2), frame(&[quote("AAPL")]), 0);
        assert!(link.opens(&id("AAPL")), "a two-sided quote resyncs it");
    }
}

#[test]
fn a_pause_opens_the_gap_at_the_last_message() {
    let mut link = fresh();
    link.on(at(0), frame(&[connected()]), 0);
    link.on(at(0), frame(&[error(402)]), 0);
    assert_eq!(link.open_gap(), None, "no subscription, no gap");
    let mut link = fresh();
    subscribe(&mut link, 0);
    assert_eq!(link.open_gap(), None);
    link.on(at(4), frame(&[quote("AAPL")]), 0);
    link.on(at(5), frame(&[error(402)]), 0);
    assert_eq!(link.state(), LinkState::Paused);
    assert_eq!(
        link.open_gap(),
        Some(at(5)),
        "from the last message received, the rejection itself"
    );
    let mut link = fresh();
    subscribe(&mut link, 0);
    link.on(at(4), frame(&[quote("AAPL")]), 0);
    link.on(at(6), LinkInput::Lost, 0);
    assert_eq!(link.open_gap(), Some(at(4)));
    link.on(at(7), LinkInput::Reconnecting, 0);
    subscribe(&mut link, 8);
    assert_eq!(link.open_gap(), None, "closed by the resubscription");
}

/// One step of the property.
#[derive(Debug, Clone)]
enum Step {
    Subscribe,
    Quote(usize),
    Status(usize),
    Lost,
    Error(u16),
    Tick,
}

const SYMBOLS: [&str; 2] = ["AAPL", "SPY"];

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        2 => Just(Step::Subscribe),
        4 => (0..2_usize).prop_map(Step::Quote),
        4 => (0..2_usize).prop_map(Step::Status),
        1 => Just(Step::Lost),
        1 => prop_oneof![Just(402_u16), Just(405), Just(406), Just(500)].prop_map(Step::Error),
        1 => Just(Step::Tick),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    /// Spec §3.2, DP-5, DP-7: an instrument opens only while the connection is subscribed and up
    /// and it has had a quote and a status since the latest subscription; a pause is final; and
    /// every reconnect's delay is within the cap. The oracle is the steps' own record.
    #[test]
    fn an_instrument_opens_only_after_its_own_resync(steps in prop::collection::vec(step(), 1..40)) {
        let mut link = fresh();
        let mut subscribed = false;
        let mut paused = false;
        let mut down = false;
        let mut seen = [(false, false); 2];
        for (n, step) in steps.iter().enumerate() {
            let now = at(i64::try_from(n).unwrap());
            let actions = match step {
                Step::Subscribe if !paused => {
                    link.on(now, LinkInput::Reconnecting, 0);
                    let actions = subscribe(&mut link, i64::try_from(n).unwrap());
                    subscribed = true;
                    down = false;
                    seen = [(false, false); 2];
                    actions
                }
                Step::Subscribe => Vec::new(),
                Step::Quote(i) => {
                    if subscribed { seen[*i].0 = true; }
                    link.on(now, frame(&[quote(SYMBOLS[*i])]), 0)
                }
                Step::Status(i) => {
                    if subscribed { seen[*i].1 = true; }
                    link.on(now, frame(&[status(SYMBOLS[*i])]), 0)
                }
                Step::Lost => {
                    subscribed = false;
                    down = true;
                    link.on(now, LinkInput::Lost, 0)
                }
                Step::Error(code) => {
                    match code {
                        402 if !down => { paused = true; subscribed = false; }
                        402 | 405 => {}
                        _ => { subscribed = false; down = true; }
                    }
                    link.on(now, frame(&[error(*code)]), 0)
                }
                Step::Tick => link.on(now, LinkInput::Tick, 0),
            };
            if let Some(after) = reconnect_after(&actions) {
                prop_assert!(after <= MAX_BACKOFF, "{after:?}");
                prop_assert!(!paused, "a paused link reconnected");
            }
            for (i, symbol) in SYMBOLS.iter().enumerate() {
                let expected = subscribed && !paused && seen[i].0 && seen[i].1;
                prop_assert_eq!(link.opens(&id(symbol)), expected, "{:?} {}", step, symbol);
            }
            if paused {
                prop_assert_eq!(link.state(), LinkState::Paused);
            }
        }
    }
}
