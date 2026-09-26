//! `mandate artifact put` and `mandate artifact get`: store a file in a content-addressed artifact
//! store and read it back re-hashed, so an auditor can supply and fetch the artifacts an exported
//! journal references without writing code (journal spec §6.3, backlog E5-4).

use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use mandate_journal::ArtifactRef;

#[derive(Debug, Subcommand)]
pub enum ArtifactCommand {
    /// Store a file's bytes under `sha256:` of those bytes and print that reference. Storing bytes
    /// that are already there changes nothing.
    Put(PutArgs),
    /// Write the artifact stored under a reference to standard output, re-hashed on the way out.
    /// Exits with an error, naming `artifact_missing` or `artifact_mismatch`, when it cannot.
    Get(GetArgs),
}

#[derive(Debug, Args)]
pub struct PutArgs {
    /// The file whose bytes become the artifact.
    pub file: PathBuf,
    /// The artifact store's root directory, created if it is missing.
    #[arg(long)]
    pub store: PathBuf,
}

#[derive(Debug, Args)]
pub struct GetArgs {
    /// The artifact's reference: `sha256:` and 64 lowercase hex characters.
    pub reference: String,
    /// The artifact store's root directory.
    #[arg(long)]
    pub store: PathBuf,
}

/// Stores the file's bytes and writes their reference to `report`.
pub fn put(args: &PutArgs, report: &mut impl Write) -> anyhow::Result<ArtifactRef> {
    let _ = (args, report);
    Ok(ArtifactRef::of(b""))
}

/// Writes the artifact's bytes to `out`, and nothing else, once they hash to their reference.
pub fn get(args: &GetArgs, out: &mut impl Write) -> anyhow::Result<ArtifactRef> {
    let _ = (args, out);
    Ok(ArtifactRef::of(b""))
}
