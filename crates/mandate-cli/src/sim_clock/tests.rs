//! The rehearsal's owner clock (DEC-962 item 2): the variable's instant, read as the moment the
//! owner ran the command, or the system's when it is unset; anything else refuses the command.

use mandate_time::UtcNanos;

use super::{SIM_CLOCK_VAR, SimClockError, owner_now};
use crate::control::Now;

fn system() -> Now {
    let at = UtcNanos::parse_rfc3339("2026-10-17T12:00:00.5Z").expect("an instant");
    Now {
        at,
        secs: at.secs(),
    }
}

#[test]
fn the_variable_is_the_one_the_rehearsal_script_sets() {
    assert_eq!(SIM_CLOCK_VAR, "MANDATE_RH_SIM_CLOCK");
}

#[test]
fn unset_it_is_the_system_clock() {
    assert_eq!(owner_now(None, system()), Ok(system()));
}

#[test]
fn set_it_is_the_instant_and_its_second() {
    let at = UtcNanos::parse_rfc3339("2026-10-19T15:00:00.25Z").expect("an instant");
    for text in ["2026-10-19T15:00:00.25Z", "2026-10-19T11:00:00.25-04:00"] {
        assert_eq!(
            owner_now(Some(text), system()),
            Ok(Now {
                at,
                secs: 1_792_422_000
            }),
            "{text}"
        );
    }
    assert_eq!(at.secs(), 1_792_422_000);
}

#[test]
fn an_unreadable_value_refuses_and_never_falls_back_to_the_system_clock() {
    for text in ["", "next monday", "2026-10-19", "2026-10-19T15:00:00"] {
        assert_eq!(owner_now(Some(text), system()), Err(SimClockError), "{text:?}");
    }
    assert_eq!(
        SimClockError.to_string(),
        "the rehearsal's owner clock is not an RFC 3339 instant"
    );
}
