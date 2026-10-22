//! `mandate-token-exchange`, with no arguments (connections spec §5.2 step 4, DEC-1175, DEC-2629):
//! the one process that sends an OAuth connect's live token request. It reads its settings from
//! the environment the deployment manager gives its pod; a refusal is written to standard error as
//! its code alone, and the process exits 2 before it reaches the vault or the network.

use std::io::Write as _;
use std::process::ExitCode;

use mandate_token_exchange::startup::{StartRefused, settings};

/// The exit code of every start refusal.
const REFUSED: u8 = 2;

/// The exit code while the exchange after the start checks is still to land (E10-13 slice 5b-3b).
const NOT_YET_EXCHANGING: u8 = 3;

/// What the process writes, alone, when its start checks pass and the exchange has not landed.
const NOT_YET_EXCHANGING_CODE: &str = "token_exchange_not_yet_exchanging";

fn refuse(refused: StartRefused) -> ExitCode {
    let _ = writeln!(std::io::stderr().lock(), "{}", refused.code());
    ExitCode::from(REFUSED)
}

fn main() -> ExitCode {
    match settings(|name| std::env::var(name).ok()) {
        Err(refused) => refuse(refused),
        Ok(_settings) => {
            let _ = writeln!(std::io::stderr().lock(), "{NOT_YET_EXCHANGING_CODE}");
            ExitCode::from(NOT_YET_EXCHANGING)
        }
    }
}
