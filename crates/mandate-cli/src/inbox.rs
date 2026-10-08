//! `mandate approvals list`, `show`, `approve` and `skip` (M7's CLI control surface; the first live
//! trade brief's K1a; DEC-533): [`crate::approvals`] over P0's `--journal` and `--store`, run as
//! D1b's owner, always in paper. `approve`'s `cli_confirm` step-up is paper only (mandate spec §6.1,
//! DEC-155 item 4); a live grant is DEC-529 item 3's, not this module's.
//!
//! What each prints is its output contract: opaque ids, states and seconds in `list`; the request's
//! content object as its exact canonical JSON, its hash and the code in `show`, so the grant shows
//! exactly the order that would be sent; and the event an answer committed, with the outcome line
//! of [`crate::approvals::message`]. A refusal prints nothing, and no message names the DSN.

use std::io::Write;

use clap::{Args, Subcommand};
use mandate_time::UtcNanos;

use crate::approvals::{Listed, Outcome, Shown};
use crate::control::{ControlError, Ids, Now, Owner, Submitted};
use crate::postgres::JournalArgs;
use crate::register::OwnerArgs;

#[derive(Debug, Subcommand)]
pub enum ApprovalsCommand {
    /// List the approvals of one or more agents: pending by deadline, then resolved.
    List(ListArgs),
    /// Show one pending approval's request and the code that approves it.
    Show(ShowArgs),
    /// Grant one pending approval with the code `show` printed (`cli_confirm`, paper only).
    Approve(ApproveArgs),
    /// Skip one pending approval. Doing nothing skips it too, at its deadline.
    Skip(SkipArgs),
}

#[derive(Debug, Args)]
pub struct ListArgs {
    /// An agent whose approvals to list; repeat it for more.
    #[arg(long = "agent", value_name = "ID", required = true)]
    pub agents: Vec<String>,
    #[command(flatten)]
    pub owner: OwnerArgs,
    #[command(flatten)]
    pub target: JournalArgs,
}

#[derive(Debug, Args)]
pub struct ShowArgs {
    pub approval: String,
    #[arg(long, value_name = "ID")]
    pub agent: String,
    #[command(flatten)]
    pub owner: OwnerArgs,
    #[command(flatten)]
    pub target: JournalArgs,
}

#[derive(Debug, Args)]
pub struct ApproveArgs {
    pub approval: String,
    #[arg(long, value_name = "ID")]
    pub agent: String,
    /// The confirmation code `show` printed for the request's content hash.
    #[arg(long)]
    pub code: String,
    /// Seconds to wait for the runtime's record of the answer, 0 to 30.
    #[arg(long = "wait-s", default_value_t = 30, value_parser = clap::value_parser!(u64).range(0..=30))]
    pub wait_s: u64,
    #[command(flatten)]
    pub owner: OwnerArgs,
    #[command(flatten)]
    pub target: JournalArgs,
}

#[derive(Debug, Args)]
pub struct SkipArgs {
    pub approval: String,
    #[arg(long, value_name = "ID")]
    pub agent: String,
    #[command(flatten)]
    pub owner: OwnerArgs,
    #[command(flatten)]
    pub target: JournalArgs,
}

/// `list`'s lines: one per approval, `<approval> <agent> <state> deadline <s> remaining <s>`, in
/// [`crate::approvals::list`]'s order, or the one line `no approvals`. Never the request's content.
///
/// # Errors
/// None once implemented; the stub's [`ControlError::Unimplemented`].
pub fn list_lines(listed: &[Listed]) -> Result<Vec<String>, ControlError> {
    let _ = listed;
    Err(ControlError::Unimplemented { story: "E8-3" })
}

/// `show`'s lines: `approval <approval> deadline <s>`, the content object's canonical JSON exactly,
/// `content_hash <hash>`, and `code <code>`.
///
/// # Errors
/// As [`list_lines`].
pub fn show_lines(shown: &Shown) -> Result<Vec<String>, ControlError> {
    let _ = shown;
    Err(ControlError::Unimplemented { story: "E8-3" })
}

/// `approve`'s lines: `granted <approval> as event <id> at seq <n> for <content_hash>`, then
/// [`crate::approvals::message`] for `outcome` at `now`.
///
/// # Errors
/// As [`list_lines`].
pub fn granted_lines(
    approval: &str,
    content_hash: &str,
    submitted: &Submitted,
    outcome: &Outcome,
    now: Now,
) -> Result<Vec<String>, ControlError> {
    let _ = (approval, content_hash, submitted, outcome, now);
    Err(ControlError::Unimplemented { story: "E8-3" })
}

/// `skip`'s line: `skipped <approval> as event <id> at seq <n>`.
///
/// # Errors
/// As [`list_lines`].
pub fn skipped_line(approval: &str, submitted: &Submitted) -> Result<String, ControlError> {
    let _ = (approval, submitted);
    Err(ControlError::Unimplemented { story: "E8-3" })
}

/// The step-up assertion ids the binary mints: derived from the owner and the instant the command
/// ran, and a count within the command, with no new dependency (DEC-533's K1a reading). Each is a
/// fresh id the workspace has not seen (mandate spec §6.1), since no two commands run at one
/// nanosecond on one owner's clock.
#[derive(Debug)]
pub struct InstantIds {
    _private: (),
}

impl InstantIds {
    /// # Errors
    /// The stub's [`ControlError::Unimplemented`].
    pub fn new(owner: &Owner, at: UtcNanos) -> Result<Self, ControlError> {
        let _ = (owner, at);
        Err(ControlError::Unimplemented { story: "E8-3" })
    }
}

impl Ids for InstantIds {
    fn assertion_id(&mut self) -> String {
        String::new()
    }
}

/// Runs `approvals list` and prints [`list_lines`].
///
/// # Errors
/// A refusal whose message carries its code: `owner_workspace_invalid` or `owner_user_invalid`
/// before the journal is opened or the store created, then the journal's error; no message names
/// the DSN, and a refusal prints nothing.
pub fn run_list(args: &ListArgs, now: Now, report: &mut impl Write) -> anyhow::Result<Vec<Listed>> {
    let _ = (args, now, report);
    Err(ControlError::Unimplemented { story: "E8-3" }.into())
}

/// Runs `approvals show` and prints [`show_lines`].
///
/// # Errors
/// As [`run_list`], then [`crate::approvals::show`]'s refusals.
pub fn run_show(args: &ShowArgs, report: &mut impl Write) -> anyhow::Result<Shown> {
    let _ = (args, report);
    Err(ControlError::Unimplemented { story: "E8-3" }.into())
}

/// Runs `approvals approve` with [`InstantIds`], waits up to `--wait-s` for the runtime's record,
/// and prints [`granted_lines`].
///
/// # Errors
/// As [`run_list`], then [`crate::approvals::approve`]'s refusals.
pub fn run_approve(
    args: &ApproveArgs,
    now: Now,
    report: &mut impl Write,
) -> anyhow::Result<Submitted> {
    let _ = (args, now, report);
    Err(ControlError::Unimplemented { story: "E8-3" }.into())
}

/// Runs `approvals skip` and prints [`skipped_line`].
///
/// # Errors
/// As [`run_list`], then [`crate::approvals::skip`]'s refusals.
pub fn run_skip(args: &SkipArgs, now: Now, report: &mut impl Write) -> anyhow::Result<Submitted> {
    let _ = (args, now, report);
    Err(ControlError::Unimplemented { story: "E8-3" }.into())
}
