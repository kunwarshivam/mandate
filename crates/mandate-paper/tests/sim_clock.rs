//! The rehearsal's run clock is the `rh-sim` build's alone (DEC-962 item 1, DEC-922 item 1): the
//! runner binary this test is built with names `MANDATE_RH_SIM_CLOCK` and reads it exactly when it
//! was built with `rh-sim`. The `live` build without `rh-sim` is the first case, so the binary
//! that places a live order never reads its clock from a variable. Each case is the other's
//! seeded bug: the default build runs the first two, and `cargo test -p mandate-paper --features
//! live,rh-sim,mandate-mcp/rh-sim --test sim_clock` the second two.

use std::fs;
use std::process::{Command, Output};

/// The clock's variable, as `src/run_clock.rs` names it; written out here so this test does not
/// link the module it looks for.
const SIM_CLOCK_VAR: &str = "MANDATE_RH_SIM_CLOCK";

/// The one line the `rh-sim` runner prints for a clock it cannot read, before anything else.
const UNREADABLE: &str = "the rehearsal's run clock is not an RFC 3339 instant";

fn runner_holds_the_clock_variable() -> bool {
    let binary = fs::read(env!("CARGO_BIN_EXE_mandate-paper")).expect("the runner binary reads");
    binary
        .windows(SIM_CLOCK_VAR.len())
        .any(|window| window == SIM_CLOCK_VAR.as_bytes())
}

/// The runner with no argument and an unreadable clock, and no other variable a run reads.
fn runner_with_an_unreadable_clock() -> Output {
    Command::new(env!("CARGO_BIN_EXE_mandate-paper"))
        .env_clear()
        .env(SIM_CLOCK_VAR, "next monday")
        .output()
        .expect("the runner starts")
}

#[cfg(not(feature = "rh-sim"))]
#[test]
fn a_runner_built_without_rh_sim_holds_no_clock_override() {
    assert!(
        !runner_holds_the_clock_variable(),
        "a runner built without rh-sim must not read its clock from a variable"
    );
}

#[cfg(not(feature = "rh-sim"))]
#[test]
fn a_runner_built_without_rh_sim_ignores_the_clock_variable() {
    let output = runner_with_an_unreadable_clock();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        !stderr.contains(UNREADABLE),
        "the variable must mean nothing to a runner built without rh-sim: {stderr}"
    );
}

#[cfg(feature = "rh-sim")]
#[test]
fn the_rehearsal_runner_reads_its_clock_from_the_variable() {
    assert!(
        runner_holds_the_clock_variable(),
        "the rh-sim runner names the clock's variable, so the absence check above can fail"
    );
}

#[cfg(feature = "rh-sim")]
#[test]
fn the_rehearsal_runner_refuses_an_unreadable_clock_before_anything() {
    let output = runner_with_an_unreadable_clock();
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim_end(),
        UNREADABLE
    );
    assert!(output.stdout.is_empty());
}
