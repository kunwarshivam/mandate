//! `mandate-stub-issuer --bind <ip>:<port> [--audience <aud>]` (DEC-2100). Refusals are written to
//! standard error as their code alone, and the process exits 2.

use std::process::ExitCode;

#[expect(
    clippy::todo,
    reason = "a process's entry point has no error to carry, so its stub is todo!(), the other form DEC-137 names"
)]
fn main() -> ExitCode {
    todo!()
}
