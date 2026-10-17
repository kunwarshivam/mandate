//! `mandate-stub-issuer --bind <ip>:<port> [--audience <aud>]` (DEC-2100). Refusals are written to
//! standard error as their code alone, and the process exits 2.

use std::process::ExitCode;

use mandate_stub_issuer::start::Refused;

fn main() -> ExitCode {
    eprintln!("{}", Refused::Unimplemented { story: "E21-52" }.code());
    ExitCode::from(2)
}
