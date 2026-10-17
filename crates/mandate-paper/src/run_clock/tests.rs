//! The rehearsal's run clock (DEC-962 item 1) over a hand-moved base clock: it reads the given
//! instant when made and moves exactly as the base moves from then on, so every age, skew and
//! session rule sees the instant the rehearsal asked for and the run's own elapsed time. The
//! expected instants are written out, not computed by the clock's arithmetic.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use mandate_alpaca::Pause;
use mandate_time::UtcNanos;

use super::{SIM_CLOCK_VAR, SimClock, sim_clock};
use crate::PaperError;

/// A base clock a test moves by hand, held as nanoseconds since the epoch; its pause moves it by
/// the duration and is ready at once.
#[derive(Debug, Clone)]
struct Manual(Rc<Cell<i128>>);

const NANOS: i128 = 1_000_000_000;

impl Manual {
    fn at(text: &str) -> Self {
        let at = instant(text);
        Self(Rc::new(Cell::new(
            i128::from(at.secs()) * NANOS + i128::from(at.nanos()),
        )))
    }

    fn set(&self, text: &str) {
        self.0.set(Self::at(text).0.get());
    }

    fn advance(&self, by: Duration) {
        let by = i128::try_from(by.as_nanos()).expect("a short step");
        self.0.set(self.0.get() + by);
    }
}

impl Pause for Manual {
    fn now(&self) -> UtcNanos {
        let total = self.0.get();
        UtcNanos::from_parts(
            i64::try_from(total.div_euclid(NANOS)).expect("in range"),
            u32::try_from(total.rem_euclid(NANOS)).expect("in range"),
        )
        .expect("in range")
    }

    fn pause(&self, duration: Duration) -> impl Future<Output = ()> {
        self.advance(duration);
        std::future::ready(())
    }
}

fn instant(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).expect("a test instant")
}

fn vars(value: Option<&str>) -> Vec<(String, String)> {
    let mut vars = vec![(
        "MANDATE_RH_SIM_ENDPOINT".to_owned(),
        "http://127.0.0.1:1/mcp".to_owned(),
    )];
    if let Some(value) = value {
        vars.push((SIM_CLOCK_VAR.to_owned(), value.to_owned()));
    }
    vars
}

#[test]
fn the_variable_is_the_one_the_rehearsal_script_sets() {
    assert_eq!(SIM_CLOCK_VAR, "MANDATE_RH_SIM_CLOCK");
}

#[test]
fn a_later_instant_reads_as_given_and_moves_with_the_base() {
    let base = Manual::at("2026-10-17T12:00:00.25Z");
    let clock = SimClock::starting_at(instant("2026-10-19T15:00:00Z"), base.clone());
    assert_eq!(clock.now(), instant("2026-10-19T15:00:00Z"));
    base.advance(Duration::from_millis(1_500));
    assert_eq!(clock.now(), instant("2026-10-19T15:00:01.5Z"));
    base.advance(Duration::from_nanos(1));
    assert_eq!(clock.now(), instant("2026-10-19T15:00:01.500000001Z"));
}

#[test]
fn an_earlier_instant_reads_as_given_and_moves_with_the_base() {
    let base = Manual::at("2026-10-17T12:00:00.75Z");
    let clock = SimClock::starting_at(instant("2026-10-16T15:00:00.5Z"), base.clone());
    assert_eq!(clock.now(), instant("2026-10-16T15:00:00.5Z"));
    base.advance(Duration::from_millis(600));
    assert_eq!(clock.now(), instant("2026-10-16T15:00:01.1Z"));
}

#[test]
fn a_base_clock_that_steps_back_steps_the_run_clock_back_by_as_much() {
    let base = Manual::at("2026-10-17T12:00:01Z");
    let clock = SimClock::starting_at(instant("2026-10-19T15:00:00.2Z"), base.clone());
    base.set("2026-10-17T12:00:00.5Z");
    assert_eq!(clock.now(), instant("2026-10-19T14:59:59.7Z"));
}

#[test]
fn a_pause_waits_on_the_base_and_moves_the_run_clock_by_its_duration() {
    let base = Manual::at("2026-10-17T12:00:00Z");
    let clock = SimClock::starting_at(instant("2026-10-19T15:00:00Z"), base.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a runtime");
    runtime.block_on(clock.pause(Duration::from_secs(2)));
    assert_eq!(base.now(), instant("2026-10-17T12:00:02Z"));
    assert_eq!(clock.now(), instant("2026-10-19T15:00:02Z"));
}

#[test]
fn the_variable_sets_the_run_clock_in_any_rfc_3339_zone() {
    for text in [
        "2026-10-19T15:00:00Z",
        "2026-10-19T11:00:00-04:00",
        "2026-10-19T15:00:00.000000000Z",
    ] {
        let base = Manual::at("2026-10-17T12:00:00Z");
        let clock = sim_clock(&vars(Some(text)), base).expect("a readable clock");
        assert_eq!(clock.now(), instant("2026-10-19T15:00:00Z"), "{text}");
    }
}

#[test]
fn without_the_variable_the_run_clock_is_the_base() {
    let base = Manual::at("2026-10-17T12:00:00.125Z");
    let clock = sim_clock(&vars(None), base.clone()).expect("no variable is the base clock");
    assert_eq!(clock.now(), instant("2026-10-17T12:00:00.125Z"));
    base.advance(Duration::from_secs(3));
    assert_eq!(clock.now(), instant("2026-10-17T12:00:03.125Z"));
}

#[test]
fn a_variable_of_another_name_is_not_read() {
    let base = Manual::at("2026-10-17T12:00:00Z");
    let vars = vec![
        (
            format!("{SIM_CLOCK_VAR}_X"),
            "2026-10-19T15:00:00Z".to_owned(),
        ),
        (
            "MANDATE_RH_SIM".to_owned(),
            "2026-10-19T15:00:00Z".to_owned(),
        ),
    ];
    let clock = sim_clock(&vars, base).expect("no clock variable");
    assert_eq!(clock.now(), instant("2026-10-17T12:00:00Z"));
}

#[test]
fn an_unreadable_variable_refuses_the_run_and_never_falls_back_to_the_base() {
    for text in [
        "",
        "next monday",
        "2026-10-19",
        "2026-10-19 15:00:00Z",
        "2026-10-19T15:00:00",
        "2026-10-19T25:00:00Z",
        "2026-10-19t15:00:00Z",
    ] {
        let base = Manual::at("2026-10-17T12:00:00Z");
        let refused = sim_clock(&vars(Some(text)), base).map(|clock| clock.now());
        assert!(
            matches!(refused, Err(PaperError::SimClock)),
            "{text:?}: {refused:?}"
        );
    }
    assert_eq!(
        PaperError::SimClock.to_string(),
        "the rehearsal's run clock is not an RFC 3339 instant"
    );
}
