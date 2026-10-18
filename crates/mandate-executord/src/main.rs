#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! `mandate-executord`: one broker account's executor process (E21-30, DEC-2003, DEC-2007).
//!
//! - `run --probe <socket> [--workspace <id> --account <ref>] [--fixture]` runs the process over
//!   Postgres (`MANDATE_JOURNAL_DSN`) or, with `--fixture`, an in-memory journal. It passes once a
//!   second, drains on SIGTERM, and exits with its lifecycle's status. A pod the deployment manager
//!   starts names its workspace and account by variables instead ([`launch`]).
//! - `connect-check --workspace <id> --account <ref> --connection <id> --pii-ref <ref> [--fixture]`
//!   journals the connect check of a paper Alpaca connection and prints `checked <event id>` or
//!   `check_failed <event id>` ([`connect`]).
//! - `probe livez|readyz --probe <socket>` is its `exec` probe (DEC-994 item 5).
//!
//! The broker is the fixture paper broker in every mode: this binary compiles no host transport
//! and reads no credential, so it reaches no broker host (E21-30; `AGENTS.md` rules 7 and 8). It
//! runs the fixture wiring ([`fixture`]), which admits no opening. This is the one place the host
//! clock is read (ES-05).


use std::process::ExitCode;

/// The stub the tests-first commit runs against: every mode fails until the implementation
/// commit (DEC-2007 item 3).
fn main() -> ExitCode {
    ExitCode::FAILURE
}
