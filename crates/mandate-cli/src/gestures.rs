//! The `mandate version create`, `mandate version confirm` and `mandate agent deploy` commands
//! (first paper trade brief, D2c; DEC-530 item 1): [`crate::version`]'s and [`crate::deploy`]'s
//! functions over P0's `--journal` and `--store`, run as the owner DEC-527's `--workspace` and
//! `--user` name, always in paper. A gesture given no `--code` shows its code and the warnings it
//! acknowledges and commits nothing; given one, it commits.

use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use mandate_journal::ArtifactSource;

use crate::control::{ControlError, ControlJournal, Now, Owner, Submitted};
use crate::postgres::JournalArgs;
use crate::register::OwnerArgs;

#[derive(Debug, Subcommand)]
pub enum VersionCommand {
    /// Store a mandate document and create its version on the workspace control stream.
    Create(CreateArgs),
    /// Show a version's confirmation code, or confirm the version with it.
    Confirm(ConfirmArgs),
}

#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    /// Show an agent's deployment code for a version, or deploy the agent with it.
    Deploy(DeployArgs),
}

#[derive(Debug, Args)]
pub struct CreateArgs {
    /// The mandate document, a JSON file the owner wrote.
    pub file: PathBuf,
    #[command(flatten)]
    pub owner: OwnerArgs,
    #[command(flatten)]
    pub target: JournalArgs,
}

#[derive(Debug, Args)]
pub struct ConfirmArgs {
    /// The version, `sha256:` and 64 hex digits, as `version create` printed it.
    pub version: String,
    /// The code `version confirm` showed for this version; without it, nothing is committed.
    #[arg(long, value_name = "CODE")]
    pub code: Option<String>,
    #[command(flatten)]
    pub owner: OwnerArgs,
    #[command(flatten)]
    pub target: JournalArgs,
}

#[derive(Debug, Args)]
pub struct DeployArgs {
    /// The agent's id.
    pub agent: String,
    /// The confirmed version to deploy it with.
    pub version: String,
    /// The code `agent deploy` showed for this agent and version; without it, nothing is
    /// committed.
    #[arg(long, value_name = "CODE")]
    pub code: Option<String>,
    #[command(flatten)]
    pub owner: OwnerArgs,
    #[command(flatten)]
    pub target: JournalArgs,
}

/// What a gesture shows before its code is typed: the code (DEC-530 item 4) and the warnings it
/// acknowledges (mandate spec §4.2), by code, sorted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub code: String,
    pub warnings: Vec<&'static str>,
}

/// What confirming `version` shows, after every check [`crate::version::confirm`] makes but the
/// code's. It writes nothing.
///
/// # Errors
/// [`crate::version::confirm`]'s refusals but `code_mismatch`; [`ControlError::Journal`].
pub fn confirmation(
    journal: &dyn ControlJournal,
    store: &dyn ArtifactSource,
    owner: &Owner,
    version: &str,
    now: Now,
) -> Result<Shown, ControlError> {
    let _ = (journal, store, owner);
    let _ = (version, now);
    Err(ControlError::Unimplemented { story: "E10-16" })
}

/// What deploying `agent` with `version` shows, after every check [`crate::deploy::deploy`] makes
/// but the code's. It writes nothing.
///
/// # Errors
/// [`crate::deploy::deploy`]'s refusals but `code_mismatch`; [`ControlError::Journal`].
pub fn deployment(
    journal: &dyn ControlJournal,
    store: &dyn ArtifactSource,
    owner: &Owner,
    (agent, version): (&str, &str),
    now: Now,
) -> Result<Shown, ControlError> {
    let _ = (journal, store, owner);
    let _ = (agent, version, now);
    Err(ControlError::Unimplemented { story: "E10-16" })
}

/// Runs `version create`, and prints the version and the event id and `seq` it committed or found.
///
/// # Errors
/// A refusal whose message carries its code: `owner_workspace_invalid`, `owner_user_invalid` or
/// `config_file_unreadable` before the journal is opened or the store created, then
/// [`crate::version::create`]'s; no message names the DSN.
pub fn create(args: &CreateArgs, now: Now, report: &mut impl Write) -> anyhow::Result<Submitted> {
    let _ = (args, now, report);
    Err(ControlError::Unimplemented { story: "E10-16" }.into())
}

/// Runs `version confirm`: without `--code`, prints [`confirmation`]'s code and warnings and
/// returns `None`; with it, confirms and prints the event id and `seq`.
///
/// # Errors
/// As [`create`], without the file, then [`confirmation`]'s or [`crate::version::confirm`]'s.
pub fn confirm(
    args: &ConfirmArgs,
    now: Now,
    report: &mut impl Write,
) -> anyhow::Result<Option<Submitted>> {
    let _ = (args, now, report);
    Err(ControlError::Unimplemented { story: "E10-16" }.into())
}

/// Runs `agent deploy`: without `--code`, prints [`deployment`]'s code and warnings and returns
/// `None`; with it, deploys and prints the event id and `seq`.
///
/// # Errors
/// As [`confirm`], with [`deployment`]'s or [`crate::deploy::deploy`]'s.
pub fn deploy(
    args: &DeployArgs,
    now: Now,
    report: &mut impl Write,
) -> anyhow::Result<Option<Submitted>> {
    let _ = (args, now, report);
    Err(ControlError::Unimplemented { story: "E10-16" }.into())
}
