//! The research driver's trigger queue (agent harness spec v0.2 §6.9, DEC-2204): which run's trigger
//! goes to the core next. At most one run is in flight per agent (HI-8); an owner or explain run
//! goes ahead of a queued scheduled run and never interrupts the one in flight; no run starts while
//! the agent is `paused` or `stopped`; and any mode change cancels the run in flight (§6.9's `mode`
//! end). The core, not the queue, decides from folded state which kind a scheduled run is and what
//! it may call. Event triggers wait for a mandate spec change (E19-14), so there are none here.

use mandate_research_driver::{Offered, QueueConfig, Trigger, TriggerQueue};
use mandate_runtime::{Mode, RiskClock};

const T0: i64 = 1_760_000_000;

fn at(offset: i64) -> RiskClock {
    RiskClock::from_secs(T0 + offset)
}

fn config() -> QueueConfig {
    QueueConfig { max_answers: 4 }
}

fn schedule(offset: i64) -> Trigger {
    Trigger::Schedule { at: at(offset) }
}

fn owner(offset: i64, cause: &str) -> Trigger {
    Trigger::OwnerQuestion {
        at: at(offset),
        cause: cause.to_owned(),
    }
}

fn approval(offset: i64, cause: &str) -> Trigger {
    Trigger::Approval {
        at: at(offset),
        cause: cause.to_owned(),
    }
}

#[test]
fn one_run_in_flight_per_agent() {
    let mut q = TriggerQueue::new(config());
    assert_eq!(q.offer(schedule(0)), Offered::Queued);
    assert_eq!(q.next(Mode::Normal), Some(schedule(0)));
    assert!(q.in_flight());
    assert_eq!(q.offer(owner(1, "q1")), Offered::Queued);
    assert_eq!(q.next(Mode::Normal), None, "HI-8: one in flight");
    q.ended();
    assert!(!q.in_flight());
    assert_eq!(q.next(Mode::Normal), Some(owner(1, "q1")));
    q.ended();
    assert_eq!(q.next(Mode::Normal), None, "nothing left queued");
}

#[test]
fn owner_and_explain_runs_go_ahead_of_a_queued_scheduled_run_in_their_order() {
    let mut q = TriggerQueue::new(config());
    q.offer(schedule(0));
    q.offer(owner(1, "q1"));
    q.offer(approval(2, "a1"));
    q.offer(owner(3, "q2"));
    let mut order = Vec::new();
    while let Some(trigger) = q.next(Mode::Normal) {
        order.push(trigger);
        q.ended();
    }
    assert_eq!(
        order,
        vec![
            owner(1, "q1"),
            approval(2, "a1"),
            owner(3, "q2"),
            schedule(0)
        ]
    );
}

#[test]
fn schedule_ticks_coalesce_into_one_queued_run() {
    let mut q = TriggerQueue::new(config());
    assert_eq!(q.offer(schedule(0)), Offered::Queued);
    assert_eq!(q.offer(schedule(60)), Offered::Coalesced);
    assert_eq!(q.queued(), 1);
    assert_eq!(
        q.next(Mode::Normal),
        Some(schedule(0)),
        "the first tick's instant is kept: the core judges due-ness from its own schedule"
    );
}

#[test]
fn owner_and_explain_runs_are_capped_and_a_schedule_is_never_crowded_out() {
    let mut q = TriggerQueue::new(config());
    for n in 0..4 {
        assert_eq!(q.offer(owner(n, &format!("q{n}"))), Offered::Queued);
    }
    assert_eq!(q.offer(approval(5, "a1")), Offered::Full);
    assert_eq!(
        q.offer(schedule(6)),
        Offered::Queued,
        "the scheduled run has its own slot"
    );
    assert_eq!(q.queued(), 5);
}

#[test]
fn the_same_owner_question_or_approval_is_queued_once() {
    let mut q = TriggerQueue::new(config());
    assert_eq!(q.offer(owner(0, "q1")), Offered::Queued);
    assert_eq!(q.offer(owner(1, "q1")), Offered::Coalesced);
    assert_eq!(q.offer(approval(2, "a1")), Offered::Queued);
    assert_eq!(q.offer(approval(3, "a1")), Offered::Coalesced);
    assert_eq!(
        q.offer(approval(4, "q1")),
        Offered::Queued,
        "an approval and an owner question are different runs"
    );
}

#[test]
fn no_run_starts_while_paused_or_stopped_and_what_was_queued_is_dropped() {
    for mode in [Mode::Paused, Mode::Stopped] {
        let mut q = TriggerQueue::new(config());
        q.offer(schedule(0));
        q.offer(owner(1, "q1"));
        assert_eq!(q.next(mode), None, "{mode:?}: no run starts");
        assert_eq!(
            q.queued(),
            0,
            "{mode:?}: nothing queued before it starts after"
        );
        assert_eq!(q.next(Mode::Normal), None);
    }
}

#[test]
fn any_mode_change_cancels_the_run_in_flight() {
    for (start, change) in [
        (Mode::Normal, Mode::ExitsOnly),
        (Mode::ExitsOnly, Mode::Normal),
        (Mode::Normal, Mode::Paused),
        (Mode::Normal, Mode::Stopped),
    ] {
        let mut q = TriggerQueue::new(config());
        q.offer(schedule(0));
        assert!(q.next(start).is_some());
        assert!(
            !q.mode_changed(start),
            "{start:?}: the same mode cancels nothing"
        );
        assert!(
            q.mode_changed(change),
            "{start:?} to {change:?}: §6.9 ends the run as `mode`"
        );
        assert!(q.in_flight(), "until its end is handled");
        q.ended();
    }
}

#[test]
fn a_pause_or_stop_also_drops_what_was_queued() {
    let mut q = TriggerQueue::new(config());
    q.offer(schedule(0));
    assert!(q.next(Mode::Normal).is_some());
    q.offer(owner(1, "q1"));
    assert!(q.mode_changed(Mode::Paused));
    assert_eq!(q.queued(), 0);
    let mut q = TriggerQueue::new(config());
    q.offer(schedule(0));
    assert!(q.next(Mode::Normal).is_some());
    q.offer(owner(1, "q1"));
    assert!(q.mode_changed(Mode::ExitsOnly));
    assert_eq!(
        q.queued(),
        1,
        "a mode that still allows runs keeps the queue"
    );
}

#[test]
fn a_mode_change_with_no_run_in_flight_cancels_nothing() {
    let mut q = TriggerQueue::new(config());
    assert!(!q.mode_changed(Mode::ExitsOnly));
    assert!(!q.mode_changed(Mode::Paused));
}
