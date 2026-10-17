//! The wake hint with no channel behind it (DEC-17, ES-20; DEC-1012 item 3): `Sleep` waits its
//! whole bound and never reports a hint, so a loop on it reads the journal once per bound.

#![allow(
    clippy::disallowed_methods,
    reason = "the test measures the wait it bounds; no core code reads the clock"
)]

use std::time::{Duration, Instant};

use mandate_daemon::Wake;
use mandate_daemon::wake::Sleep;

#[test]
fn sleep_waits_its_whole_bound_and_reports_no_hint() {
    let bound = Duration::from_millis(50);
    let began = Instant::now();
    assert!(!Sleep.wait(bound));
    assert!(began.elapsed() >= bound);
}
