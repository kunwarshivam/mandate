//! `mandate step-up confirm` ([DEC-2104](../../../docs/project/decisions/DEC-2104.md) item 2,
//! [DEC-2106](../../../docs/project/decisions/DEC-2106.md); journal spec §9.29): the owner confirms,
//! on the host, the step-up challenge the workspace API issued for a paper grant made in the web
//! app. The command reads the approval from the journal, never from the browser, and appends one
//! `StepUpConfirmed` to the workspace control stream, written by the principal. It grants nothing:
//! the browser still submits the grant, and only the API's `cli_confirm` verifier reads the record.
//!
//! Every check is local and refuses with a code, appending nothing, in DEC-2106 item 2's order. A
//! re-run of a command that did commit finds it (DEC-2106 item 3, DEC-290).

use std::io::Write;

use clap::{Args, Subcommand};

use crate::approvals::Shown;
use crate::control::{ControlError, ControlJournal, Now, Owner, Submitted};
use crate::postgres::JournalArgs;

/// The record the command appends (journal spec §9.29).
pub const EVENT_TYPE: &str = "StepUpConfirmed";

#[derive(Debug, Subcommand)]
pub enum StepUpCommand {
    /// Confirm, in paper, the step-up challenge the web app shows for an approval's grant, with
    /// the code it shows.
    Confirm(ConfirmArgs),
}

#[derive(Debug, Args)]
pub struct ConfirmArgs {
    /// The workspace whose control stream the confirmation is appended to.
    #[arg(long, value_name = "ID")]
    pub workspace: String,
    /// The confirming principal's ULID, as the workspace API writes its `actor.id`.
    #[arg(long, value_name = "ULID")]
    pub principal: String,
    /// The agent whose approval the grant answers.
    #[arg(long, value_name = "ID")]
    pub agent: String,
    /// The approval the grant answers.
    #[arg(long, value_name = "ID")]
    pub approval: String,
    /// The challenge the web app shows.
    #[arg(long, value_name = "ULID")]
    pub challenge: String,
    /// The code the web app shows, `XXXX-XXXX`.
    #[arg(long)]
    pub code: String,
    #[command(flatten)]
    pub target: JournalArgs,
}

/// One confirmation the owner asks for: the flags' values, as typed.
#[derive(Debug, Clone, Copy)]
pub struct Confirming<'a> {
    pub workspace: &'a str,
    pub principal: &'a str,
    pub agent: &'a str,
    pub approval: &'a str,
    pub challenge: &'a str,
    pub code: &'a str,
}

impl<'a> From<&'a ConfirmArgs> for Confirming<'a> {
    fn from(args: &'a ConfirmArgs) -> Self {
        Self {
            workspace: &args.workspace,
            principal: &args.principal,
            agent: &args.agent,
            approval: &args.approval,
            challenge: &args.challenge,
            code: &args.code,
        }
    }
}

/// A confirmation whose flags passed DEC-2106 items 2.1 to 2.4: built only by [`check`], before
/// any journal is opened, so [`confirm`] never sees a flag it has not checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    owner: Owner,
    agent: String,
    approval: String,
    challenge: String,
    code: String,
}

/// What [`confirm`] found: the approval as the journal holds it, and the confirmation it appended
/// or, on a re-run, the one it had already appended (DEC-2106 item 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirmed {
    pub shown: Shown,
    pub submitted: Submitted,
    pub earlier: bool,
}

/// The code as `StepUpConfirmed` carries it (DEC-2104 item 2, DEC-2106 item 2.4).
///
/// # Errors
/// [`ControlError::Refused`] with `code_invalid`.
pub fn normalize(_code: &str) -> Result<String, ControlError> {
    Err(ControlError::Unimplemented { story: "E9-4" })
}

/// DEC-2106 items 2.1 to 2.4, which read no journal. The owner is the principal, in `paper`.
///
/// # Errors
/// [`ControlError::Refused`] with `owner_workspace_invalid`, `principal_invalid`,
/// `challenge_invalid`, or `code_invalid`, in that order.
pub fn check(_confirming: &Confirming<'_>) -> Result<Checked, ControlError> {
    Err(ControlError::Unimplemented { story: "E9-4" })
}

/// DEC-2106 items 2.5 to 2.9 against `journal`, then one `StepUpConfirmed` appended at `now`, at the
/// head the rows were read at (item 4), or the one a run that committed appended (item 3).
///
/// # Errors
/// [`ControlError::Refused`] with DEC-2106 item 2's code, having appended nothing;
/// [`ControlError::Journal`] when the journal cannot be read or refuses the record.
pub fn confirm(
    _journal: &mut dyn ControlJournal,
    _checked: &Checked,
    _now: Now,
) -> Result<Confirmed, ControlError> {
    Err(ControlError::Unimplemented { story: "E9-4" })
}

/// What the command prints (DEC-2106 item 5): the approval as `approvals show` prints it, then
/// `confirmed` or `already confirmed <challenge> as event <id> at seq <n> for <content_hash>`.
///
/// # Errors
/// As [`crate::inbox::show_lines`].
pub fn confirmed_lines(
    _checked: &Checked,
    _confirmed: &Confirmed,
) -> Result<Vec<String>, ControlError> {
    Err(ControlError::Unimplemented { story: "E9-4" })
}

/// Runs [`confirm`] against `journal` and prints [`confirmed_lines`]. The binary runs [`check`]
/// first and opens the journal `--journal` names only once it passes.
///
/// # Errors
/// As [`confirm`]. A refusal prints nothing.
pub fn run_confirm(
    _checked: &Checked,
    _journal: &mut dyn ControlJournal,
    _now: Now,
    _report: &mut dyn Write,
) -> anyhow::Result<Confirmed> {
    Err(ControlError::Unimplemented { story: "E9-4" }.into())
}
