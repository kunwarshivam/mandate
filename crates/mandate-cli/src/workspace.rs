//! `mandate workspace open` (first paper trade brief, D1c; DEC-527 item 7): commits the workspace
//! control stream's `StreamOpened`, once, as the journal vectors' `control_services` opener, in
//! paper. Every other control command appends to the stream this opens.

use std::io::Write;

use clap::{Args, Subcommand};

use mandate_journal::Environment;

use crate::control::{ControlError, Now, Owner, Submitted, control_stream, open_control_stream};
use crate::postgres::{JournalArgs, PgControlJournal, read_stream};

#[derive(Debug, Subcommand)]
pub enum WorkspaceCommand {
    /// Open the workspace control stream, in paper. Refused if the stream holds any event.
    Open(OpenArgs),
}

/// There is no `--user`: the event's actor is the `control_services` opener, not the owner. There
/// is no environment: the stream is opened in paper.
#[derive(Debug, Args)]
pub struct OpenArgs {
    /// The workspace whose control stream to open.
    #[arg(long, value_name = "ID")]
    pub workspace: String,
    #[command(flatten)]
    pub target: JournalArgs,
}

/// Commits `StreamOpened` on `ctl:{workspace}` and prints the event id and `seq` it committed.
///
/// # Errors
/// A refusal whose message carries its code: `owner_workspace_invalid` before the journal is
/// opened or the store created, then `workspace_already_open` for a stream that holds any event,
/// before anything is appended, or the journal's error; no message names the DSN, and a refusal
/// prints nothing.
pub fn open(args: &OpenArgs, now: Now, report: &mut impl Write) -> anyhow::Result<Submitted> {
    let owner = Owner {
        workspace: args.workspace.clone(),
        user: String::new(),
        environment: Environment::Paper,
    };
    let refused = |reason| ControlError::Refused { reason };
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
