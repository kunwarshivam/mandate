//! The `mandate` command line. `download` fetches Alpaca historical bars and trades into Parquet
//! datasets (backlog E2-1).

use clap::{Parser, Subcommand};

pub mod download;

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
}
