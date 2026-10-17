//! The rehearsal's market fixture is the `rh-sim` build's alone (DEC-961 item 1, DEC-922 item 1):
//! the runner binary this test is built with names the fixture's variable exactly when it was
//! built with `rh-sim`. The `live` build without `rh-sim` is the first case, so a binary that
//! could ever read market data from a fixture is never the one that places a live order. Each
//! case is the other's seeded bug: the default build runs the first, and `cargo test -p
//! mandate-paper --features live,rh-sim,mandate-mcp/rh-sim --test market_fixture` the second.

use std::fs;

/// The fixture's variable, as `src/sim_market.rs` names it; written out here so this test does
/// not link the module it looks for.
const MARKET_FIXTURE_VAR: &[u8] = b"MANDATE_RH_SIM_MARKET";

fn runner_holds_the_fixture_variable() -> bool {
    let binary = fs::read(env!("CARGO_BIN_EXE_mandate-paper")).expect("the runner binary reads");
    binary
        .windows(MARKET_FIXTURE_VAR.len())
        .any(|window| window == MARKET_FIXTURE_VAR)
}

#[cfg(not(feature = "rh-sim"))]
#[test]
fn a_runner_built_without_rh_sim_holds_no_market_fixture() {
    assert!(
        !runner_holds_the_fixture_variable(),
        "a runner built without rh-sim must not read market data from a fixture"
    );
}

#[cfg(feature = "rh-sim")]
#[test]
fn the_rehearsal_runner_reads_its_market_from_the_fixture() {
    assert!(
        runner_holds_the_fixture_variable(),
        "the rh-sim runner names the fixture's variable, so the absence check above can fail"
    );
}
