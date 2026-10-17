//! The rehearsal's owner clock is the `rh-sim` build's alone (DEC-962 item 2): the `mandate`
//! binary this test is built with names `MANDATE_RH_SIM_CLOCK` and reads it exactly when it was
//! built with `rh-sim`, which only the rehearsal's build turns on; the founder's CLI is built with
//! no feature. Each case is the other's seeded bug: the default build runs the first two, and
//! `cargo test -p mandate-cli --features rh-sim --test sim_clock` the second two.

use std::fs;
use std::process::{Command, Output};

/// The clock's variable, as `src/sim_clock.rs` names it; written out here so this test does not
/// link the module it looks for.
const SIM_CLOCK_VAR: &str = "MANDATE_RH_SIM_CLOCK";

/// What the `rh-sim` CLI says of a clock it cannot read.
const UNREADABLE: &str = "the rehearsal's owner clock is not an RFC 3339 instant";

fn cli_holds_the_clock_variable() -> bool {
    let binary = fs::read(env!("CARGO_BIN_EXE_mandate")).expect("the CLI binary reads");
    binary
        .windows(SIM_CLOCK_VAR.len())
        .any(|window| window == SIM_CLOCK_VAR.as_bytes())
}

/// `workspace open` against a journal that cannot be reached, with an unreadable clock: the
/// command reads its clock before it opens the journal.
fn workspace_open_with_an_unreadable_clock() -> Output {
    let store = std::env::temp_dir().join(format!("mandate-cli-sim-clock-{}", std::process::id()));
    Command::new(env!("CARGO_BIN_EXE_mandate"))
        .args(["workspace", "open", "--workspace", "ws_sim", "--journal"])
        .arg("not-a-dsn")
        .arg("--store")
        .arg(&store)
        .env_clear()
        .env(SIM_CLOCK_VAR, "next monday")
        .output()
        .expect("the CLI starts")
}

#[cfg(not(feature = "rh-sim"))]
#[test]
fn a_cli_built_without_rh_sim_holds_no_clock_override() {
    assert!(
        !cli_holds_the_clock_variable(),
        "the founder's CLI must not read its clock from a variable"
    );
}

#[cfg(not(feature = "rh-sim"))]
#[test]
fn a_cli_built_without_rh_sim_ignores_the_clock_variable() {
    let output = workspace_open_with_an_unreadable_clock();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "the journal cannot be reached");
    assert!(
        !stderr.contains(UNREADABLE),
        "the variable must mean nothing to a CLI built without rh-sim: {stderr}"
    );
}

#[cfg(feature = "rh-sim")]
#[test]
fn the_rehearsal_cli_reads_its_clock_from_the_variable() {
    assert!(
        cli_holds_the_clock_variable(),
        "the rh-sim CLI names the clock's variable, so the absence check above can fail"
    );
}

#[cfg(feature = "rh-sim")]
#[test]
fn the_rehearsal_cli_refuses_an_unreadable_clock_before_the_journal() {
    let output = workspace_open_with_an_unreadable_clock();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains(UNREADABLE), "{stderr}");
}
