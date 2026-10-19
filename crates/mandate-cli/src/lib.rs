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
//! The `mandate` command line. `download` fetches Alpaca historical bars and trades into Parquet
//! datasets (backlog E2-1); `inspect` reports what a stored dataset covers and whether to trust
//! it (E2-2); `journal verify` runs journal spec §11 over an exported journal and its artifact
//! store, and `artifact put` and `get` supply and fetch those artifacts (E5-4); `journal
//! verify-cold` runs the cold store's per-range checks over a directory of segments and
//! manifests (E5-8). `db migrate` applies the journal's roles, schema and migrations as the
//! deploy step before any process starts (E21-50). `backtest record` journals a run
//! `mandate-backtest-runner` stored, re-checking every artifact it names (E4-5, DEC-1144).
//! `agent pause` and `agent kill` reach an agent, a connection or the workspace with nothing but
//! the journal (E21-34). `approvals` and `agent` are M7's owner control: the inbox, the owner's
//! answers, and the owner's commands, each committed to the workspace control stream (E8-1 to
//! E8-3). Their `clap` commands land once `mandate-journal` registers the control stream's schemas,
//! without which a real journal refuses every event they commit (DEC-257 item 17, DEC-279 item
//! 10). `vault import` stores the
//! platform's OAuth client secret in the demo host's local vault, from stdin (E10-13 V1). `step-up
//! confirm` records, in paper, the owner's confirmation of a web grant's step-up challenge (E9-4,
//! DEC-2104).

use clap::{Parser, Subcommand};

pub mod agent;
pub mod approvals;
pub mod artifact;
pub mod backtest;
pub mod brake;
pub mod config;
pub mod connection;
pub mod control;
pub mod db;
pub mod deploy;
pub mod download;
pub mod evaluation;
pub mod gestures;
pub mod identity;
pub mod inbox;
pub mod inspect;
pub mod journal;
pub mod owner;
pub mod postgres;
pub mod register;
pub mod retire;
#[cfg(any(test, feature = "rh-sim"))]
pub mod sim_clock;
pub mod step_up;
pub mod vault;
pub mod version;
pub mod workspace;

/// Mandate research and audit tools.
#[derive(Debug, Parser)]
#[command(name = "mandate", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Download Alpaca historical bars or trades into Parquet, one file per symbol per UTC day.
    /// Re-running changes nothing that is already stored.
    Download(download::DownloadArgs),
    /// Report each dataset's coverage, statistics, gaps between bars, duplicates, and partitions
    /// that cannot be trusted. Exits with an error when any partition has a problem.
    Inspect(inspect::InspectArgs),
    /// Check an exported journal: the hash chain, the artifacts its events reference, and an
    /// anchor when one is given.
    #[command(subcommand)]
    Journal(journal::JournalCommand),
    /// Put bytes into a content-addressed artifact store, or fetch them back re-hashed.
    #[command(subcommand)]
    Artifact(artifact::ArtifactCommand),
    /// The approval inbox: list, show, approve or skip an agent's approvals, in paper.
    #[command(subcommand)]
    Approvals(inbox::ApprovalsCommand),
    /// Store a configuration object and register it on the workspace control stream, in paper.
    #[command(subcommand)]
    Config(register::ConfigCommand),
    /// Store a model's content object and register it on the workspace control stream, in paper.
    #[command(subcommand)]
    Model(register::ModelCommand),
    /// Open the workspace control stream, once, in paper, or write its founding grant.
    #[command(subcommand)]
    Workspace(workspace::WorkspaceCommand),
    /// Link an identity provider's subject to a user principal in the API server's subject-link
    /// file.
    #[command(subcommand)]
    Identity(identity::IdentityCommand),
    /// Record a checked broker connection on the workspace control stream.
    #[command(subcommand)]
    Connection(connection::ConnectionCommand),
    /// Create a mandate version, or show its confirmation code and confirm it, in paper.
    #[command(subcommand)]
    Version(gestures::VersionCommand),
    /// Deploy an agent, or pause it or commit a kill switch at agent, connection or workspace scope.
    #[command(subcommand)]
    Agent(brake::AgentCommand),
    /// Import a platform secret into the demo host's local vault, from stdin only.
    #[command(subcommand)]
    Vault(vault::VaultCommand),
    /// Apply the journal's roles, schema and migrations: the deploy step before any process starts.
    #[command(subcommand)]
    Db(db::DbCommand),
    /// Journal a backtest the backtest runner stored, as the `backtest_runner` system.
    #[command(subcommand)]
    Backtest(backtest::BacktestCommand),
    /// Run and journal the forward-paper evaluation of an internal paper workspace, as the
    /// `evaluator` system.
    #[command(subcommand)]
    Evaluation(evaluation::EvaluationCommand),
    /// Confirm, in paper, the step-up challenge the web app shows for an approval's grant
    /// (DEC-2104).
    #[command(subcommand)]
    StepUp(step_up::StepUpCommand),
}
