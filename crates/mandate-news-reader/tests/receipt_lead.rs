#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "tests: a broken fixture or a wrong answer should stop the test"
)]
//! The receipt-lead bound (DEC-2289, DEC-2281 item 3): a round whose receipt, the host clock's
//! reading, runs more than [`RECEIPT_LEAD_S`] ahead of the trusted time asks nothing and journals
//! nothing, so no `knowledge_at` is ever set past the trusted time by more than the lead. The round
//! is counted, the operator is alerted once an episode, and the reader pauses as after a failed
//! round. The bound is judged against the trusted time alone, never the vendor's item times. The
//! oracle reads the journal's own rows after every step.

mod common;

use std::time::Duration;

use mandate_news_reader::{RECEIPT_AHEAD, RECEIPT_LEAD_S, STORE_CLOCK_UNAVAILABLE};
use mandate_process::State;
use mandate_sources::ingest::FUTURE_SKEW_MS;
use mandate_time::UtcNanos;
use proptest::prelude::*;

use common::*;

fn ahead(trusted: UtcNanos, lead: i64) -> Clock {
    Clock {
        now: plus(trusted, lead),
        trusted,
    }
}

/// A ready reader following SPY, the vendor answering `pages` in turn.
fn ready(pages: &[Vec<u8>]) -> (Journal, Recorded, Reader) {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    spy(&journal, &shelf);
    let transport = Recorded::answering(pages);
    let reader = started(&journal, &shelf, &transport, t0());
    assert_eq!(reader.lifecycle().state(), State::Ready);
    (journal, transport, reader)
}

fn one_item(id: u64) -> Vec<u8> {
    page(
        &[item(
            id,
            "2026-10-26T14:00:00Z",
            "2026-10-26T14:00:00Z",
            &["SPY"],
        )],
        None,
    )
}

/// The latest `knowledge_at` the stream holds, if any.
fn last_known(journal: &Journal) -> Option<UtcNanos> {
    stored(journal)
        .iter()
        .map(|row| UtcNanos::parse(member(row, "knowledge_at").as_str().unwrap()).unwrap())
        .max()
}

#[test]
fn a_round_past_the_lead_asks_and_journals_nothing_and_is_counted_and_alerted() {
    let (journal, transport, mut reader) = ready(&[one_item(41000010)]);
    block_on(reader.step(ahead(t0(), RECEIPT_LEAD_S + 1)));
    assert!(transport.sent().is_empty(), "nothing asked");
    assert!(journal.news().is_empty(), "nothing journaled");
    assert_eq!(reader.counts().receipt_ahead, 1);
    assert_eq!(reader.take_alerts(), vec![RECEIPT_AHEAD]);
    assert_eq!(reader.take_alerts(), Vec::<&str>::new(), "taken once");
    assert_eq!(
        reader.not_ready(),
        None,
        "a held round is not a refusal of the reader"
    );
}

#[test]
fn at_the_lead_the_round_asks_and_journals_at_its_receipt() {
    let (journal, transport, mut reader) = ready(&[one_item(41000011)]);
    block_on(reader.step(ahead(t0(), RECEIPT_LEAD_S)));
    assert_eq!(transport.sent().len(), 1);
    assert_eq!(stored(&journal).len(), 1);
    assert_eq!(last_known(&journal), Some(plus(t0(), RECEIPT_LEAD_S)));
    assert_eq!(reader.counts().receipt_ahead, 0);
    assert!(reader.take_alerts().is_empty());
}

/// A host clock behind the trusted time holds nothing: it only stamps an earlier receipt.
#[test]
fn a_host_clock_behind_the_trusted_time_holds_nothing() {
    let (journal, transport, mut reader) = ready(&[one_item(41000012)]);
    block_on(reader.step(Clock {
        now: t0(),
        trusted: plus(t0(), 3_600),
    }));
    assert_eq!(transport.sent().len(), 1);
    assert_eq!(stored(&journal).len(), 1);
    assert_eq!(reader.counts().receipt_ahead, 0);
}

/// The pause after a held round is the failed round's cooldown, by the trusted time, so a clock
/// stuck ahead is judged again once each cooldown and never spins; the episode alerts once, and a
/// round within the bound ends it, so the next episode alerts again. The round that resumes asks
/// from the stream's last `knowledge_at`, which no held round moved.
#[test]
fn an_episode_alerts_once_pauses_by_the_cooldown_and_ends_within_the_bound() {
    let (journal, transport, mut reader) = ready(&[one_item(41000013), one_item(41000014)]);
    let jump = RECEIPT_LEAD_S + 600;
    block_on(reader.step(ahead(t0(), jump)));
    for second in 1..30 {
        block_on(reader.step(ahead(plus(t0(), second), jump)));
    }
    assert_eq!(reader.counts().receipt_ahead, 1, "paused for the cooldown");
    block_on(reader.step(ahead(plus(t0(), 30), jump)));
    assert_eq!(reader.counts().receipt_ahead, 2, "judged again after it");
    assert_eq!(
        reader.take_alerts(),
        vec![RECEIPT_AHEAD],
        "one alert an episode"
    );
    assert!(transport.sent().is_empty());

    let resumed = plus(t0(), 100);
    block_on(reader.step(Clock::agreed(resumed)));
    assert_eq!(
        transport.sent().len(),
        1,
        "the clock agreeing again, the round asks"
    );
    assert!(
        transport.sent()[0].contains("start=2026-10-24T"),
        "an empty stream still asks from 48 hours back: {:?}",
        transport.sent()
    );
    assert_eq!(last_known(&journal), Some(resumed));
    assert!(reader.take_alerts().is_empty(), "no alert within the bound");

    block_on(reader.step(ahead(plus(t0(), 200), jump)));
    assert_eq!(
        reader.take_alerts(),
        vec![RECEIPT_AHEAD],
        "a new episode alerts again"
    );
    assert_eq!(
        last_known(&journal),
        Some(resumed),
        "a held round moves nothing"
    );
}

/// The bound reads the trusted time, never the vendor's item times: an item published two days
/// before, on a first poll, is journaled.
#[test]
fn an_old_item_on_a_first_poll_is_journaled() {
    let old = page(
        &[item(
            41000015,
            "2026-10-24T16:00:00Z",
            "2026-10-24T16:00:00Z",
            &["SPY"],
        )],
        None,
    );
    let (journal, _, mut reader) = ready(&[old]);
    block_on(reader.step(Clock::agreed(t0())));
    assert_eq!(stored(&journal).len(), 1);
    assert_eq!(reader.counts().receipt_ahead, 0);
}

/// The oracle: the journal's latest `knowledge_at` against the step's trusted time, the lead, and
/// the sources' future skew a vendor time may run past its receipt; `Some` names the breach.
fn breach(journal: &Journal, clock: Clock) -> Option<String> {
    let last = last_known(journal)?;
    let skew = i64::from(FUTURE_SKEW_MS / 1_000);
    (last > plus(clock.trusted, RECEIPT_LEAD_S + skew)).then(|| {
        format!(
            "knowledge_at {last} past trusted {} + lead + skew",
            clock.trusted
        )
    })
}

/// The oracle fails on a journal holding an item known past the bound.
#[test]
fn the_oracle_catches_an_item_known_past_the_bound() {
    let (journal, _, mut reader) = ready(&[one_item(41000016)]);
    let at = plus(t0(), RECEIPT_LEAD_S);
    block_on(reader.step(ahead(t0(), RECEIPT_LEAD_S)));
    assert_eq!(breach(&journal, ahead(t0(), RECEIPT_LEAD_S)), None);
    assert!(
        breach(&journal, Clock::agreed(plus(at, -RECEIPT_LEAD_S - 2))).is_some(),
        "a trusted time two seconds earlier puts the stored item past the lead and the skew"
    );
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, failure_persistence: None, ..ProptestConfig::default() })]

    /// Over any run of steps, the trusted time moving forward by up to two minutes a step and the
    /// host clock anywhere from five minutes behind it to ten ahead, no stored `knowledge_at` ever
    /// runs past the step's trusted time by more than the lead; and a host clock that never runs
    /// past the lead holds no round and raises no alert.
    #[test]
    fn no_item_is_known_past_the_trusted_time_and_the_lead(
        steps in prop::collection::vec((0i64..120, -300i64..600), 1..40),
    ) {
        let pages: Vec<Vec<u8>> = (0..steps.len())
            .map(|n| page(&[item(41000100 + n as u64, "2026-10-26T10:00:00Z", "2026-10-26T10:00:00Z", &["SPY"])], None))
            .collect();
        let (journal, _, mut reader) = ready(&pages);
        let mut trusted = t0();
        let mut ever_ahead = false;
        for (forward, offset) in steps {
            trusted = plus(trusted, forward);
            let clock = Clock { now: plus(trusted, offset), trusted };
            ever_ahead |= offset > RECEIPT_LEAD_S;
            block_on(reader.step(clock));
            prop_assert_eq!(breach(&journal, clock), None);
        }
        if !ever_ahead {
            prop_assert_eq!(reader.counts().receipt_ahead, 0);
            prop_assert!(reader.take_alerts().is_empty());
        }
    }
}

/// The binary's trusted time: the host clock at start plus the monotonic clock's elapsed time,
/// carrying nanoseconds into seconds, and the start itself past the representable range.
#[test]
fn the_trusted_time_is_the_start_plus_the_elapsed_time() {
    let start = UtcNanos::from_parts(1_000, 700_000_000).unwrap();
    let cases = [
        (
            Duration::ZERO,
            UtcNanos::from_parts(1_000, 700_000_000).unwrap(),
        ),
        (
            Duration::new(5, 200_000_000),
            UtcNanos::from_parts(1_005, 900_000_000).unwrap(),
        ),
        (
            Duration::new(5, 300_000_000),
            UtcNanos::from_parts(1_006, 0).unwrap(),
        ),
        (
            Duration::new(5, 400_000_001),
            UtcNanos::from_parts(1_006, 100_000_001).unwrap(),
        ),
        (Duration::from_secs(u64::MAX), start),
    ];
    for (elapsed, expected) in cases {
        assert_eq!(
            Clock::trusted_after(start, elapsed),
            expected,
            "{elapsed:?}"
        );
    }
}

/// Every pause runs by the trusted time, not the host clock (DEC-2289 item 4): after a failed
/// request, with the host clock 20 s ahead of the trusted time and within the lead, the reader asks
/// nothing until the trusted time is 30 s on, and asks again then.
#[test]
fn a_failed_request_pauses_until_the_trusted_time_is_a_cooldown_on() {
    let (_, transport, mut reader) = ready(&[]);
    let at = |trusted: i64| Clock {
        now: plus(t0(), trusted + 20),
        trusted: plus(t0(), trusted),
    };
    block_on(reader.step(at(0)));
    assert_eq!(transport.sent().len(), 1, "the request the transport fails");
    assert_eq!(reader.counts().fetch_failed.values().sum::<u64>(), 1);
    block_on(reader.step(at(29)));
    assert_eq!(transport.sent().len(), 1, "paused, trusted 29 s on");
    block_on(reader.step(at(30)));
    assert_eq!(transport.sent().len(), 2, "asked again, trusted 30 s on");
}

/// The idle round's pause runs by the trusted time too: with nothing to ask, the reader waits until
/// the trusted time is 30 s on before it reads its streams for a round again.
#[test]
fn an_idle_round_pauses_until_the_trusted_time_is_a_cooldown_on() {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    let transport = Recorded::answering(&[one_item(41000017)]);
    let mut reader = started(&journal, &shelf, &transport, t0());
    let at = |trusted: i64| Clock {
        now: plus(t0(), trusted + 20),
        trusted: plus(t0(), trusted),
    };
    block_on(reader.step(at(0)));
    assert!(transport.sent().is_empty(), "no thesis: nothing to ask");
    spy(&journal, &shelf);
    block_on(reader.step(at(29)));
    assert!(transport.sent().is_empty(), "paused, trusted 29 s on");
    block_on(reader.step(at(30)));
    assert_eq!(transport.sent().len(), 1, "a round, trusted 30 s on");
}

/// DEC-2292: the bound also holds against the store's clock, a second host's. A host clock already
/// ahead when the reader starts passes the monotonic trusted time, which starts from it, but not the
/// store's: the round is held, counted and alerted, and a round within both asks.
#[test]
fn a_host_clock_ahead_at_start_is_held_by_the_store_s_clock() {
    let (journal, transport, mut reader) = ready(&[one_item(41000018)]);
    journal.store_clock.set(Some(Some(t0())));
    block_on(reader.step(Clock::agreed(plus(t0(), 3_600))));
    assert!(
        transport.sent().is_empty(),
        "an hour past the store's clock"
    );
    assert_eq!(reader.counts().receipt_ahead, 1);
    assert_eq!(reader.take_alerts(), vec![RECEIPT_AHEAD]);
    assert!(journal.news().is_empty());

    let (journal, transport, mut reader) = ready(&[one_item(41000019)]);
    journal.store_clock.set(Some(Some(t0())));
    block_on(reader.step(Clock::agreed(plus(t0(), RECEIPT_LEAD_S))));
    assert_eq!(
        transport.sent().len(),
        1,
        "within the lead of the store's clock"
    );
    assert_eq!(reader.counts().receipt_ahead, 0);
}

/// The bound is the earlier of the trusted time and the store's clock: a store clock ahead of the
/// trusted time lifts nothing.
#[test]
fn a_store_clock_ahead_of_the_trusted_time_lifts_nothing() {
    let (journal, transport, mut reader) = ready(&[one_item(41000020)]);
    journal.store_clock.set(Some(Some(plus(t0(), 3_600))));
    block_on(reader.step(ahead(t0(), RECEIPT_LEAD_S + 1)));
    assert!(transport.sent().is_empty());
    assert_eq!(reader.counts().receipt_ahead, 1);
}

/// A store whose clock cannot be read holds every round (`AGENTS.md` rule 3): counted, alerted
/// `store_clock_unavailable` once an episode, paused by the cooldown, and ended by a round the
/// store's clock bounds again.
#[test]
fn a_store_whose_clock_cannot_be_read_holds_the_round_and_alerts_once() {
    let (journal, transport, mut reader) = ready(&[one_item(41000021)]);
    journal.store_clock.set(Some(None));
    block_on(reader.step(Clock::agreed(t0())));
    assert!(transport.sent().is_empty());
    assert_eq!(reader.counts().store_clock_unavailable, 1);
    assert_eq!(reader.counts().receipt_ahead, 0);
    assert_eq!(reader.take_alerts(), vec![STORE_CLOCK_UNAVAILABLE]);
    block_on(reader.step(Clock::agreed(plus(t0(), 29))));
    assert_eq!(reader.counts().store_clock_unavailable, 1, "paused");
    block_on(reader.step(Clock::agreed(plus(t0(), 30))));
    assert_eq!(reader.counts().store_clock_unavailable, 2);
    assert!(reader.take_alerts().is_empty(), "one alert an episode");
    journal.store_clock.set(Some(Some(plus(t0(), 60))));
    block_on(reader.step(Clock::agreed(plus(t0(), 60))));
    assert_eq!(
        transport.sent().len(),
        1,
        "the store's clock answering again, the round asks"
    );
    journal.store_clock.set(Some(None));
    block_on(reader.step(Clock::agreed(plus(t0(), 200))));
    assert_eq!(
        reader.take_alerts(),
        vec![STORE_CLOCK_UNAVAILABLE],
        "a new episode alerts again"
    );
}

/// Switching from one hold to the other is a new episode: each alerts once.
#[test]
fn a_held_round_of_the_other_kind_alerts_its_own_code() {
    let (journal, _, mut reader) = ready(&[]);
    journal.store_clock.set(Some(None));
    block_on(reader.step(Clock::agreed(t0())));
    journal.store_clock.set(Some(Some(t0())));
    block_on(reader.step(ahead(plus(t0(), 30), RECEIPT_LEAD_S + 1)));
    assert_eq!(
        reader.take_alerts(),
        vec![STORE_CLOCK_UNAVAILABLE, RECEIPT_AHEAD]
    );
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, failure_persistence: None, ..ProptestConfig::default() })]

    /// With a store clock anywhere from ten minutes behind the trusted time to ten ahead, no stored
    /// `knowledge_at` runs past the earlier of the two by more than the lead and the skew.
    #[test]
    fn no_item_is_known_past_the_store_s_clock_and_the_lead(
        steps in prop::collection::vec((0i64..120, -300i64..600, -600i64..600), 1..30),
    ) {
        let pages: Vec<Vec<u8>> = (0..steps.len())
            .map(|n| page(&[item(41000300 + n as u64, "2026-10-26T10:00:00Z", "2026-10-26T10:00:00Z", &["SPY"])], None))
            .collect();
        let (journal, _, mut reader) = ready(&pages);
        let mut trusted = t0();
        for (forward, offset, store) in steps {
            trusted = plus(trusted, forward);
            let store_now = plus(trusted, store);
            journal.store_clock.set(Some(Some(store_now)));
            let clock = Clock { now: plus(trusted, offset), trusted };
            block_on(reader.step(clock));
            let earlier = Clock::agreed(trusted.min(store_now));
            prop_assert_eq!(breach(&journal, earlier), None);
        }
    }
}
