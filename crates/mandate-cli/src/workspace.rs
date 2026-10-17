//! `mandate workspace open` (first paper trade brief, D1c; DEC-527 item 7): commits the workspace
//! control stream's `StreamOpened`, once, as the journal vectors' `control_services` opener, in
//! `paper` or, with `--environment live`, in `live` (DEC-952). The stream's first event fixes its
//! environment, so a live workspace is its own workspace, and every other control command appends
//! to the stream this opens in its environment.
//!
//! `mandate workspace found` (lane ID1, DEC-991) writes the workspace's founding grant once: the
//! `MemberActivated` with reason `founding` that makes its owner a member with the owner bundle
//! (identity spec §3.2, §4.3, ID-13; journal spec §9.12), through the one membership writer,
//! `mandate_identity::found_workspace`. It is idempotent, so a Kubernetes Job may run it again:
//! a re-run for the same member finds the grant, commits nothing, and succeeds.

use std::io::Write;

use clap::{Args, Subcommand};

use mandate_journal::Environment;
use mandate_time::UtcNanos;

use crate::control::{
    ControlError, ControlJournal, Now, Owner, Submitted, control_stream, open_control_stream,
};
use crate::postgres::{JournalArgs, PgControlJournal, read_stream};

#[derive(Debug, Subcommand)]
pub enum WorkspaceCommand {
    /// Open the workspace control stream, in paper or live. Refused if the stream holds any event.
    Open(OpenArgs),
    /// Write the workspace's founding grant: its owner's membership with the owner bundle, once.
    /// A re-run for the same member commits nothing and succeeds; any other membership refuses it.
    Found(FoundArgs),
}

/// There is no `--user` and no role: the system writes the grant, and its roles are the owner
/// bundle, never chosen.
#[derive(Debug, Args)]
pub struct FoundArgs {
    /// The workspace whose control stream to write the founding grant on.
    #[arg(long, value_name = "ID")]
    pub workspace: String,
    /// The founder's user principal: an opaque ULID (identity spec §3.1), never a name, an
    /// address, or an identity-provider subject.
    #[arg(long, value_name = "ULID")]
    pub member: String,
    /// How the founder signs in: `passkey`, `oidc`, or `email_link` (identity spec §6.1).
    #[arg(long, value_name = "METHOD")]
    pub method: String,
    #[command(flatten)]
    pub target: JournalArgs,
}

/// What `found` asks for, as given.
#[derive(Debug, Clone, Copy)]
pub struct Founding<'a> {
    pub workspace: &'a str,
    pub member: &'a str,
    pub method: &'a str,
}

/// What `found` did: the `seq` of the founding grant, and whether this run committed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Founded {
    pub seq: u64,
    pub granted: bool,
}

/// There is no `--user`: the event's actor is the `control_services` opener, not the owner.
#[derive(Debug, Args)]
pub struct OpenArgs {
    /// The workspace whose control stream to open.
    #[arg(long, value_name = "ID")]
    pub workspace: String,
    /// `paper`, the default, or `live`: the workspace's environment, for life (DEC-952).
    #[arg(long, value_name = "ENV", default_value = "paper")]
    pub environment: String,
    #[command(flatten)]
    pub target: JournalArgs,
}

/// Commits `StreamOpened` on `ctl:{workspace}` and prints the event id and `seq` it committed.
///
/// # Errors
/// A refusal whose message carries its code: `environment_invalid` for any environment but
/// `paper` or `live`, then `owner_workspace_invalid`, both before the journal is
/// opened or the store created, then `workspace_already_open` for a stream that holds any event,
/// before anything is appended, or the journal's error; no message names the DSN, and a refusal
/// prints nothing.
pub fn open(args: &OpenArgs, now: Now, report: &mut impl Write) -> anyhow::Result<Submitted> {
    let refused = |reason| ControlError::Refused { reason };
    let environment = match args.environment.as_str() {
        "paper" => Environment::Paper,
        "live" => Environment::Live,
        _ => return Err(refused("environment_invalid").into()),
    };
    let owner = Owner {
        workspace: args.workspace.clone(),
        user: String::new(),
        environment,
    };
    let stream = control_stream(&owner).map_err(|_| refused("owner_workspace_invalid"))?;
    if !read_stream(&args.target.journal, &stream)?.is_empty() {
        return Err(refused("workspace_already_open").into());
    }
    let mut journal = PgControlJournal::open(&args.target)?;
    let submitted = open_control_stream(&mut journal, &owner, now)?;
    let Submitted { event_id, seq } = &submitted;
    writeln!(
        report,
        "opened {} as event {event_id} at seq {seq}",
        stream.as_str()
    )?;
    Ok(submitted)
}

/// Writes `args`' founding grant on the workspace's Postgres journal, reading the clock with
/// `clock` inside each attempt, and prints what it did.
///
/// # Errors
/// As [`found_in`], and the journal's error opening it; no message names the DSN.
pub fn found(
    args: &FoundArgs,
    clock: &mut dyn FnMut() -> UtcNanos,
    report: &mut impl Write,
) -> anyhow::Result<Founded> {
    let _ = (args, clock, report);
    Err(ControlError::Unimplemented { story: "ID1" }.into())
}

/// Writes `founding`'s grant on `journal`, reading the clock with `clock` inside each attempt, and
/// prints what it did.
///
/// # Errors
/// [`ControlError::Refused`] with `method_invalid`, `member_invalid`, or
/// `owner_workspace_invalid` before the journal is read; then `workspace_not_open`,
/// `workspace_not_new`, or `members_exist`, committing nothing; [`ControlError::Journal`] when the
/// journal fails or does not settle.
pub fn found_in(
    journal: &mut dyn ControlJournal,
    founding: Founding<'_>,
    clock: &mut dyn FnMut() -> UtcNanos,
    report: &mut dyn Write,
) -> Result<Founded, ControlError> {
    let _ = (journal, founding, clock, report);
    Err(ControlError::Unimplemented { story: "ID1" })
}
