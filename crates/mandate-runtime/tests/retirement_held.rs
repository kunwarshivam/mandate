//! Journal spec v0.79 §9.34 (DEC-2352 item 5, E8-17): the runtime's fold reads its own
//! `RetirementHeld` on the agent stream. A restarted runtime folds its stream first so it never
//! writes a period twice, so the fold keeps the latest period, its cause, its `held_since`, and the
//! tick it was written at; it steps nothing else, no mode, hold, or decision; and a record that
//! does not read as rule 265 admits it is refused, never folded as a guess.

mod common;

use common::{AGENT_STREAM, copied, deployment, event, int, object, text};
use mandate_canon::Value;
use mandate_runtime::{EventId, RiskClock, RuntimeError, RuntimeState, fold};

const CAUSE: &str = "01J8Z5RH0A000000000000000S";
const SINCE: i64 = 1_789_311_600;
const DAY: i64 = 86_400;

fn stamp(secs: i64) -> Value {
    text(
        &mandate_time::UtcNanos::from_parts(secs, 0)
            .unwrap()
            .to_string(),
    )
}

fn held(seq: u64, period: u64, written_at: i64) -> mandate_runtime::FoldedEvent {
    copied(
        AGENT_STREAM,
        seq,
        "RetirementHeld",
        object(&[
            ("held_since", stamp(SINCE)),
            ("period", int(period)),
            ("waiting_on", Value::Array(vec![text("position")])),
            ("risk_clock", stamp(written_at)),
        ]),
        &EventId(CAUSE.to_owned()),
    )
}

fn opened() -> RuntimeState {
    let mut state = RuntimeState::new(deployment());
    fold(
        &mut state,
        &event(
            AGENT_STREAM,
            1,
            "StreamOpened",
            object(&[("stream_type", text("agent"))]),
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    state
}

#[test]
fn the_fold_keeps_the_latest_period_of_a_held_retirement() {
    let mut state = opened();
    assert!(state.retirement_held().is_none());
    let mode = state.effective_mode();
    fold(&mut state, &held(2, 1, SINCE + DAY)).unwrap_or_else(|e| panic!("{e}"));
    let first = state.retirement_held().cloned().unwrap();
    assert_eq!(first.cause, EventId(CAUSE.to_owned()));
    assert_eq!(first.period, 1);
    assert_eq!(first.held_since, RiskClock::from_secs(SINCE));
    assert_eq!(first.written_at, RiskClock::from_secs(SINCE + DAY));
    fold(&mut state, &held(3, 2, SINCE + 2 * DAY)).unwrap_or_else(|e| panic!("{e}"));
    let second = state.retirement_held().cloned().unwrap();
    assert_eq!(
        (second.period, second.written_at),
        (2, RiskClock::from_secs(SINCE + 2 * DAY))
    );
    assert_eq!(
        second.held_since, first.held_since,
        "one sequence, one held_since"
    );
    assert_eq!(state.effective_mode(), mode, "it steps no mode or hold");
}

/// A way to break a held record.
type Broken = Box<dyn Fn() -> mandate_runtime::FoldedEvent>;

#[test]
fn a_held_record_that_does_not_read_is_refused_and_folds_nothing() {
    let cases: [(&str, Broken); 4] = [
        (
            "no cause",
            Box::new(|| mandate_runtime::FoldedEvent {
                causation_id: None,
                ..held(2, 1, SINCE + DAY)
            }),
        ),
        ("period 0", Box::new(|| held(2, 0, SINCE + DAY))),
        (
            "no risk clock",
            Box::new(|| {
                let mut e = held(2, 1, SINCE + DAY);
                if let Value::Object(p) = &mut e.payload {
                    p.remove("risk_clock");
                }
                e
            }),
        ),
        (
            "a held_since off the second",
            Box::new(|| {
                let mut e = held(2, 1, SINCE + DAY);
                if let Value::Object(p) = &mut e.payload {
                    p.insert(
                        mandate_canon::Key::new("held_since").unwrap(),
                        text("2026-09-21T15:00:00.500000000Z"),
                    );
                }
                e
            }),
        ),
    ];
    for (case, record) in cases {
        let mut state = opened();
        let refused = fold(&mut state, &record());
        assert!(
            matches!(refused, Err(ref e) if !matches!(e, RuntimeError::NotInterpreted { .. })),
            "{case}: {refused:?}"
        );
        assert!(state.retirement_held().is_none(), "{case}: nothing folded");
    }
}
