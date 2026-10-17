//! The research driver's trigger queue (ADR-0005 §4, DEC-2204): which run starts next, and with which
//! tools. At most one run is in flight per agent (HI-8); owner and explain runs go ahead of queued
//! runs and never interrupt the one in flight; event triggers are debounced per subject and cause
//! and rate-limited per agent; mode gates every start; and a run's tool classes follow from its
//! kind and the mode alone, fixed before anything untrusted is read.

use mandate_research_driver::{
    EventCause, Offered, QueueConfig, RunKind, Started, ToolClass, Trigger, TriggerQueue,
    tool_classes,
};
use mandate_runtime::{Mode, RiskClock};

const T0: i64 = 1_760_000_000;

fn at(offset: i64) -> RiskClock {
    RiskClock::from_secs(T0 + offset)
}

fn config() -> QueueConfig {
    QueueConfig {
        debounce_s: 300,
        monitoring_per_hour: 4,
        max_queued: 8,
    }
}

fn news(offset: i64, subject: &str) -> Trigger {
    Trigger::Event {
        at: at(offset),
        subject: subject.to_owned(),
        cause: EventCause::News,
    }
}

fn schedule(offset: i64) -> Trigger {
    Trigger::Schedule { at: at(offset) }
}

fn owner(offset: i64, message: &str) -> Trigger {
    Trigger::OwnerMessage {
        at: at(offset),
        message_id: message.to_owned(),
    }
}

fn ask(offset: i64, approval: &str) -> Trigger {
    Trigger::AskRaised {
        at: at(offset),
        approval_id: approval.to_owned(),
    }
}

fn kind_of(start: Option<Started>) -> Option<RunKind> {
    start.map(|s| s.kind)
}

#[test]
fn one_run_in_flight_per_agent() {
    let mut q = TriggerQueue::new(config());
    assert_eq!(q.offer(schedule(0)), Offered::Queued);
    assert_eq!(q.offer(news(1, "AAPL")), Offered::Queued);
    assert_eq!(
        kind_of(q.next(at(2), Mode::Normal)),
        Some(RunKind::Monitoring)
    );
    assert_eq!(q.next(at(3), Mode::Normal), None, "HI-8: one in flight");
    assert!(q.in_flight().is_some());
    q.ended();
    assert_eq!(
        kind_of(q.next(at(4), Mode::Normal)),
        Some(RunKind::Ideation)
    );
    q.ended();
    assert_eq!(q.next(at(5), Mode::Normal), None, "nothing left queued");
}

#[test]
fn owner_and_explain_runs_go_ahead_of_queued_runs_but_never_interrupt_the_one_in_flight() {
    let mut q = TriggerQueue::new(config());
    q.offer(schedule(0));
    assert_eq!(
        kind_of(q.next(at(1), Mode::Normal)),
        Some(RunKind::Ideation)
    );
    q.offer(news(2, "AAPL"));
    q.offer(owner(3, "msg_1"));
    q.offer(ask(4, "apr_1"));
    assert_eq!(
        q.next(at(5), Mode::Normal),
        None,
        "the ideation run is never interrupted"
    );
    q.ended();
    assert_eq!(kind_of(q.next(at(6), Mode::Normal)), Some(RunKind::Owner));
    q.ended();
    assert_eq!(kind_of(q.next(at(7), Mode::Normal)), Some(RunKind::Explain));
    q.ended();
    assert_eq!(
        kind_of(q.next(at(8), Mode::Normal)),
        Some(RunKind::Monitoring)
    );
}

#[test]
fn owner_runs_keep_their_order_and_their_message() {
    let mut q = TriggerQueue::new(config());
    q.offer(owner(0, "msg_1"));
    q.offer(ask(1, "apr_1"));
    q.offer(owner(2, "msg_2"));
    let first = q.next(at(3), Mode::Normal);
    assert_eq!(
        first.as_ref().map(|s| s.trigger.clone()),
        Some(owner(0, "msg_1"))
    );
    q.ended();
    assert_eq!(
        q.next(at(4), Mode::Normal).map(|s| s.trigger),
        Some(ask(1, "apr_1"))
    );
    q.ended();
    assert_eq!(
        q.next(at(5), Mode::Normal).map(|s| s.trigger),
        Some(owner(2, "msg_2"))
    );
}

#[test]
fn an_event_on_the_same_subject_and_cause_within_the_window_is_debounced() {
    let mut q = TriggerQueue::new(config());
    assert_eq!(q.offer(news(0, "AAPL")), Offered::Queued);
    assert_eq!(q.offer(news(299, "AAPL")), Offered::Debounced);
    assert_eq!(
        q.offer(news(10, "MSFT")),
        Offered::Queued,
        "another subject"
    );
    let filing = Trigger::Event {
        at: at(20),
        subject: "AAPL".to_owned(),
        cause: EventCause::Filing,
    };
    assert_eq!(q.offer(filing), Offered::Queued, "another cause");
    assert_eq!(
        q.offer(news(300, "AAPL")),
        Offered::Coalesced,
        "past the window, but a run for the subject is still queued"
    );
    assert_eq!(
        kind_of(q.next(at(301), Mode::Normal)),
        Some(RunKind::Monitoring)
    );
    q.ended();
}

#[test]
fn a_debounced_event_does_not_extend_the_window() {
    let mut q = TriggerQueue::new(config());
    q.offer(news(0, "AAPL"));
    q.next(at(1), Mode::Normal);
    q.ended();
    assert_eq!(q.offer(news(200, "AAPL")), Offered::Debounced);
    assert_eq!(
        q.offer(news(300, "AAPL")),
        Offered::Queued,
        "the window counts from the last accepted event, so a steady drip still gets through"
    );
}

#[test]
fn a_news_storm_cannot_buy_a_run_storm() {
    let mut q = TriggerQueue::new(config());
    let mut started = 0;
    for second in 0..3600 {
        let subject = format!("SYM{}", second % 50);
        q.offer(news(second, &subject));
        if q.next(at(second), Mode::Normal).is_some() {
            started += 1;
            q.ended();
        }
    }
    assert_eq!(
        started, 4,
        "monitoring_per_hour caps the starts in any rolling hour"
    );
    assert!(q.queued() <= config().max_queued, "the queue stays bounded");
    for second in 3600..3700 {
        if q.next(at(second), Mode::Normal).is_some() {
            q.ended();
            started += 1;
        }
    }
    assert!(started > 4, "a new hour admits again");
}

#[test]
fn the_rate_limit_counts_monitoring_starts_only() {
    let mut q = TriggerQueue::new(config());
    for n in 0..4 {
        q.offer(news(n * 400, &format!("SYM{n}")));
        assert!(q.next(at(n * 400 + 1), Mode::Normal).is_some());
        q.ended();
    }
    q.offer(news(1700, "SYM9"));
    assert_eq!(
        q.next(at(1701), Mode::Normal),
        None,
        "the fifth monitoring run in the hour waits"
    );
    q.offer(owner(1702, "msg_1"));
    assert_eq!(
        kind_of(q.next(at(1703), Mode::Normal)),
        Some(RunKind::Owner),
        "the owner is never rate-limited by monitoring"
    );
    q.ended();
    q.offer(schedule(1704));
    assert_eq!(
        kind_of(q.next(at(1705), Mode::Normal)),
        Some(RunKind::Ideation),
        "a waiting monitoring run does not block the schedule"
    );
}

#[test]
fn one_queued_ideation_run_however_many_ticks() {
    let mut q = TriggerQueue::new(config());
    assert_eq!(q.offer(schedule(0)), Offered::Queued);
    assert_eq!(q.offer(schedule(60)), Offered::Coalesced);
    assert_eq!(q.queued(), 1);
}

#[test]
fn the_queue_is_bounded_and_drops_the_newest_event_first() {
    let mut q = TriggerQueue::new(config());
    for n in 0..8 {
        assert_eq!(q.offer(news(n, &format!("SYM{n}"))), Offered::Queued);
    }
    assert_eq!(q.offer(news(9, "SYM9")), Offered::Full);
    assert_eq!(
        q.offer(owner(10, "msg_1")),
        Offered::Queued,
        "an owner message is never dropped for a full queue of events"
    );
}

#[test]
fn no_run_starts_while_paused_or_stopped_and_the_one_in_flight_is_cancelled() {
    for mode in [Mode::Paused, Mode::Stopped] {
        let mut q = TriggerQueue::new(config());
        q.offer(schedule(0));
        assert_eq!(q.next(at(1), mode), None, "{mode:?}: no run starts");
        assert_eq!(q.queued(), 0, "{mode:?}: what was queued before is dropped");
        q.offer(owner(2, "msg_1"));
        assert_eq!(q.next(at(3), mode), None, "{mode:?}: not even an owner run");

        let mut q = TriggerQueue::new(config());
        q.offer(schedule(0));
        assert!(q.next(at(1), Mode::Normal).is_some());
        assert!(
            q.mode_changed(mode),
            "{mode:?}: the run in flight is cancelled at its next step"
        );
        assert!(q.in_flight().is_some(), "until the worker acknowledges it");
        q.ended();
        assert_eq!(q.next(at(2), Mode::Normal), None, "nothing was kept");
    }
}

#[test]
fn a_mode_change_that_still_allows_runs_cancels_nothing() {
    let mut q = TriggerQueue::new(config());
    q.offer(schedule(0));
    q.next(at(1), Mode::Normal);
    assert!(!q.mode_changed(Mode::ExitsOnly));
    assert!(!q.mode_changed(Mode::Normal));
}

#[test]
fn the_tool_classes_follow_from_the_kind_and_the_mode_alone() {
    use ToolClass::{Proposing, Read, RiskReducing, Think};
    assert_eq!(
        tool_classes(RunKind::Ideation, Mode::Normal),
        &[Read, Think, RiskReducing, Proposing]
    );
    assert_eq!(
        tool_classes(RunKind::Monitoring, Mode::Normal),
        &[Read, Think, RiskReducing],
        "an event-triggered run never proposes a thesis or an entry"
    );
    assert_eq!(tool_classes(RunKind::Owner, Mode::Normal), &[Read, Think]);
    assert_eq!(tool_classes(RunKind::Explain, Mode::Normal), &[Read, Think]);
    for kind in [RunKind::Ideation, RunKind::Monitoring] {
        assert_eq!(
            tool_classes(kind, Mode::ExitsOnly),
            &[Read, Think, RiskReducing],
            "§6.7: in exits_only a run is review-only"
        );
    }
    for mode in [Mode::Paused, Mode::Stopped] {
        for kind in [
            RunKind::Ideation,
            RunKind::Monitoring,
            RunKind::Owner,
            RunKind::Explain,
        ] {
            assert!(tool_classes(kind, mode).is_empty(), "{kind:?} in {mode:?}");
        }
    }
}

#[test]
fn a_started_run_carries_its_tool_classes_from_the_queue() {
    let mut q = TriggerQueue::new(config());
    q.offer(news(0, "AAPL"));
    let started = q.next(at(1), Mode::ExitsOnly);
    assert_eq!(
        started.map(|s| s.tools),
        Some(tool_classes(RunKind::Monitoring, Mode::ExitsOnly).to_vec())
    );
}
