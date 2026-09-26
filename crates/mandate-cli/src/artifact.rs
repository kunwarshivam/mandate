//! `mandate artifact put` and `mandate artifact get`: store a file in a content-addressed artifact
//! store and read it back re-hashed, so an auditor can supply and fetch the artifacts an exported
//! journal references without writing code (journal spec §6.3, backlog E5-4).

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, anyhow, bail};
use clap::{Args, Subcommand};
use mandate_artifacts_fs::FsArtifactStore;
use mandate_journal::{ArtifactError, ArtifactRef, ArtifactStore, get_artifact};

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

/// Why a store operation failed, with the stable code of spec §11 check 6 first.
fn store_error(what: &str, error: ArtifactError) -> anyhow::Error {
    anyhow!("{}: {what}: {error}", error.code())
}

/// Stores the file's bytes and writes their reference to `report`.
pub fn put(args: &PutArgs, report: &mut impl Write) -> anyhow::Result<ArtifactRef> {
    let bytes = fs::read(&args.file).with_context(|| format!("reading {}", args.file.display()))?;
    let mut store = FsArtifactStore::open(&args.store).map_err(|e| {
        store_error(
            &format!("opening the artifact store {}", args.store.display()),
            e,
        )
    })?;
    let reference = store
        .put_artifact(&bytes)
        .map_err(|e| store_error(&format!("storing {}", args.file.display()), e))?;
    writeln!(report, "{reference}").context("writing the reference")?;
    Ok(reference)
}

/// Writes the artifact's bytes to `out`, and nothing else, once they hash to their reference.
pub fn get(args: &GetArgs, out: &mut impl Write) -> anyhow::Result<ArtifactRef> {
    let reference = ArtifactRef::parse(&args.reference).ok_or_else(|| {
        anyhow!(
            "`{}` is not an artifact reference: `sha256:` and 64 lowercase hex characters",
            args.reference
        )
    })?;
    if !args.store.is_dir() {
        bail!(
            "the artifact store {} is not a directory",
            args.store.display()
        );
    }
    let store = FsArtifactStore::open(&args.store).map_err(|e| {
        store_error(
            &format!("opening the artifact store {}", args.store.display()),
            e,
        )
    })?;
    let bytes =
        get_artifact(&store, &reference).map_err(|e| store_error(&reference.to_string(), e))?;
    out.write_all(&bytes).context("writing the artifact")?;
    Ok(reference)
}
