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
//! manifests (E5-8). `approvals` and
//! `agent` are M7's owner control: the inbox, the owner's answers, and the owner's commands, each
//! committed to the workspace control stream (E8-1 to E8-3). Their `clap` commands land once
//! `mandate-journal` registers the control stream's schemas, without which a real journal refuses
//! every event they commit (DEC-257 item 17, DEC-279 item 10).

use clap::{Parser, Subcommand};

pub mod agent;
pub mod approvals;
pub mod artifact;
pub mod config;
pub mod connection;
pub mod control;
pub mod deploy;
pub mod download;
pub mod gestures;
pub mod inbox;
pub mod inspect;
pub mod journal;
pub mod postgres;
pub mod register;
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
    /// Open the workspace control stream, once, in paper.
    #[command(subcommand)]
    Workspace(workspace::WorkspaceCommand),
    /// Record a checked broker connection on the workspace control stream.
    #[command(subcommand)]
    Connection(connection::ConnectionCommand),
    /// Create a mandate version, or show its confirmation code and confirm it, in paper.
    #[command(subcommand)]
    Version(gestures::VersionCommand),
    /// Show an agent's deployment code, or deploy it with a confirmed version, in paper.
    #[command(subcommand)]
    Agent(gestures::AgentCommand),
}
