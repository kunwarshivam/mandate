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
//! The paper adapter: one run of the production cycle on the founder's Alpaca **paper** account
//! (E7-19 slice 5, the [first paper trade brief](../../../docs/project/tasks/first-paper-trade.md)'s
//! E1a, [DEC-846](../../../docs/project/decisions/DEC-846.md)).
//!
//! The only crate that sees both the model host and the shell (DEC-503 item 2). A run: the host
//! refusal, the control stream, E19-11's phase 1 and the artifacts (DEC-505), before any
//! credential; the credentials, the GET-only preflight and phase 2 (V-002); the trusted closes and
//! the host, where `Flat` or `Undecided` ends it; the closes stored, and only then the observation
//! and output handed to [`mandate_shell::ProductionCycle::run_observed`], the one door (FT-1,
//! FT-6). No product value is a constant here (FT-2).

use std::path::PathBuf;

use mandate_alpaca::{DataTransport, Pause, TradingTransport};
use mandate_modelhost::{Refusal, Signal};
use mandate_shell::control::{ConfigRefusal, ControlRecord, DeploymentRefusal};
use mandate_shell::{Report, ShellError};

/// What the founder asked for: opaque ids (TI-8), and no mandate, configuration, model output or
/// host (FT-3, FT-4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub workspace: String,
    pub agent: String,
    pub account_ref: String,
    /// The journal's DSN; only a placing run appends its cycle there (DEC-157 item 6). `None` is a
    /// scratch in-memory journal.
    pub journal: Option<String>,
    pub store: PathBuf,
    pub bars: PathBuf,
    pub place_one_order: bool,
}

/// The process's effects, the only things the binary supplies (ADR-0001 ES-05, ES-19).
pub trait Ports {
    type Transport: TradingTransport + DataTransport + Clone + 'static;
    type Pause: Pause + Clone + 'static;

    /// The workspace control stream's records, read once per run, or [`PaperError::Control`].
    fn control_stream(&mut self, workspace: &str) -> Result<Vec<ControlRecord>, PaperError>;

    /// The credentials and transport, or [`PaperError::Credentials`]; once, after all else.
    fn connect(&mut self) -> Result<Self::Transport, PaperError>;

    /// The clock and the timer; its `now` is the run's one clock read and the binding gate's.
    fn pause(&self) -> Self::Pause;
}

/// How a run that refused nothing ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// `Flat` or `Undecided`: nothing stored, handed on or sent (DEC-157 item 4).
    NoOutput(Signal),
    /// The cycle ran: what it submitted, or the order it would have placed (boxed: it is large).
    Cycle(Box<Report>),
}

/// Why a run stopped. After any of them nothing further is sent.
#[derive(Debug, thiserror::Error)]
pub enum PaperError {
    /// The body of every stub in the tests PR (DEC-77).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    #[error("{0}")]
    Usage(String),
    #[error("the control stream could not be read")]
    Control,
    #[error("the paper credentials or client are unavailable")]
    Credentials,
    #[error("the artifact store could not be opened or written")]
    Store,
    #[error("no confirmed deployment: {0}")]
    Deployment(DeploymentRefusal),
    #[error("the registered configuration cannot be used: {0}")]
    Configuration(ConfigRefusal),
    #[error("the model host refused: {0}")]
    Model(Refusal),
    #[error(transparent)]
    Shell(ShellError),
}

/// Parses the arguments after the program name: `--workspace`, `--agent`, `--account-ref`,
/// `--journal`, `--store` and `--bars`, each with a value; the required `--confirm-paper`; and
/// `--place-one-order`, which needs `--journal`. Any other argument is unknown, so none names a
/// mandate, a configuration, a model output or a host.
///
/// # Errors
/// [`PaperError::Usage`] for a missing acknowledgement, flag or value, or an unknown argument.
pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Args, PaperError> {
    let _ = args.into_iter();
    Err(PaperError::Unimplemented { story: "E7-19" })
}

/// One run, in the crate documentation's order; `vars` is the process environment.
///
/// # Errors
/// Every [`PaperError`] is a stop after which nothing further is sent.
pub fn run<P: Ports>(
    args: &Args,
    vars: &[(String, String)],
    ports: &mut P,
) -> Result<Outcome, PaperError> {
    let _ = (args, vars, ports);
    Err(PaperError::Unimplemented { story: "E7-19" })
}
