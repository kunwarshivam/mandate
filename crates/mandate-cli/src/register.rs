//! The `mandate config register` and `mandate model register` commands (first paper trade brief,
//! D1b; DEC-527): [`crate::config::register`] and [`crate::config::register_model`] over P0's `--journal` and
//! `--store`, run as the owner `--workspace` and `--user` name, always in paper.

use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::config::ConfigKind;
use crate::control::{ControlError, Now, Owner, Submitted};
use crate::postgres::JournalArgs;

/// Who runs the command (DEC-527 items 1 and 3). There is no environment: the owner is always in
/// paper.
#[derive(Debug, Clone, Args)]
pub struct OwnerArgs {
    /// The workspace whose control stream the command commits to.
    #[arg(long, value_name = "ID")]
    pub workspace: String,
    /// The owner's opaque user id: 1 to 64 lower-case letters, digits, `_` or `-`; never an email
    /// or a name.
    #[arg(long, value_name = "ID")]
    pub user: String,
}

impl OwnerArgs {
    /// The owner these flags name, in `paper`.
    ///
    /// # Errors
    /// [`ControlError::Refused`] with `owner_workspace_invalid` or `owner_user_invalid`.
    pub fn owner(&self) -> Result<Owner, ControlError> {
        let _ = self;
        Err(ControlError::Unimplemented { story: "E10-16" })
    }
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Store a configuration object and register it on the workspace control stream.
    Register(RegisterArgs),
}

#[derive(Debug, Args)]
pub struct RegisterArgs {
    /// The kind of object, as journal spec §9.2 names it.
    #[arg(long, value_enum)]
    pub kind: ConfigKind,
    /// The object, a JSON file.
    pub file: PathBuf,
    #[command(flatten)]
    pub owner: OwnerArgs,
    #[command(flatten)]
    pub target: JournalArgs,
}

#[derive(Debug, Subcommand)]
pub enum ModelCommand {
    /// Store the content object the model host computes and register it on the workspace control
    /// stream.
    Register(ModelArgs),
}

#[derive(Debug, Args)]
pub struct ModelArgs {
    pub model_id: String,
    pub model_version: String,
    #[command(flatten)]
    pub owner: OwnerArgs,
    #[command(flatten)]
    pub target: JournalArgs,
}

/// Runs `config register`, and prints the event id and `seq` it committed or found.
///
/// # Errors
/// A refusal whose message carries its code: `owner_workspace_invalid`, `owner_user_invalid` or
/// `config_file_unreadable` before the journal is opened or the store created, then
/// [`crate::config::register`]'s; no message names the DSN.
pub fn run(args: &RegisterArgs, now: Now, report: &mut impl Write) -> anyhow::Result<Submitted> {
    let _ = (args, now, report);
    Err(ControlError::Unimplemented { story: "E10-16" }.into())
}

/// Runs `model register`, and prints the event id and `seq` it committed or found.
///
/// # Errors
/// As [`run`], without the file, then [`crate::config::register_model`]'s.
pub fn run_model(args: &ModelArgs, now: Now, report: &mut impl Write) -> anyhow::Result<Submitted> {
    let _ = (args, now, report);
    Err(ControlError::Unimplemented { story: "E10-16" }.into())
}
