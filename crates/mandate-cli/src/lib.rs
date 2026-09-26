//! The `mandate` command line. `download` fetches Alpaca historical bars and trades into Parquet
//! datasets (backlog E2-1); `inspect` reports what a stored dataset covers and whether to trust
//! it (E2-2).

use clap::{Parser, Subcommand};

pub mod download;
pub mod inspect;

/// Mandate research tools.
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
}
