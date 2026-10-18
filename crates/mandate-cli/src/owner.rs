//! `mandate agent status`, `resume`, `stop` and `exit` (E8-3's `clap` follow-up, DEC-279 item 10;
//! DEC-2507): [`crate::agent`]'s owner commands over P0's `--journal`, run as D1b's owner.
//!
//! `resume` and `stop` without `--code` print the code [`crate::agent::code`] gives and commit
//! nothing; with it they commit one `OwnerCommandIssued`, and a wrong code is refused with nothing
//! committed. A Stop with `--release` prints [`RELEASE_WARNING`] before its code every time.
//! `exit` commits at once and is never refused (`AGENTS.md` rule 13, DEC-158 option (c)): without
//! the right code it carries no step-up evidence, and `--show-code` prints the code instead. A
//! refusal prints nothing, and no message names the DSN.

use std::io::Write;

use clap::Args;

use crate::agent::{Command, Confirmed, RELEASE_WARNING, Status};
use crate::control::{ControlError, ControlJournal, Ids, Now, Owner};
use crate::postgres::JournalArgs;
use crate::register::OwnerArgs;

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// The agent's id.
    pub agent: String,
    /// The broker account whose stream holds the agent's restrictions.
    #[arg(long, value_name = "ID")]
    pub account: String,
    #[command(flatten)]
    pub owner: OwnerArgs,
    #[command(flatten)]
    pub target: JournalArgs,
}

#[derive(Debug, Args)]
pub struct ResumeArgs {
    /// The agent's id.
    pub agent: String,
    /// The code `agent resume` showed; without it, nothing is committed.
    #[arg(long, value_name = "CODE")]
    pub code: Option<String>,
    #[command(flatten)]
    pub owner: OwnerArgs,
    #[command(flatten)]
    pub target: JournalArgs,
}

#[derive(Debug, Args)]
pub struct StopArgs {
    /// The agent's id.
    pub agent: String,
    /// Release the agent's positions to the owner, unprotected, instead of needing it flat
    /// (DEC-136).
    #[arg(long)]
    pub release: bool,
    /// The code `agent stop` showed; without it, nothing is committed.
    #[arg(long, value_name = "CODE")]
    pub code: Option<String>,
    #[command(flatten)]
    pub owner: OwnerArgs,
    #[command(flatten)]
    pub target: JournalArgs,
}

/// The displayed bid, bid size and floor the owner confirmed, all three or none (DEC-58, DEC-66).
#[derive(Debug, Clone, Args)]
pub struct BidArgs {
    /// The displayed bid the owner confirms.
    #[arg(long = "confirm-bid", value_name = "BID", requires_all = ["bid_size", "floor"])]
    pub bid: Option<String>,
    /// The displayed bid size the owner confirms.
    #[arg(long, value_name = "N", requires_all = ["bid", "floor"])]
    pub bid_size: Option<String>,
    /// The lowest price the owner accepts.
    #[arg(long, value_name = "PRICE", requires_all = ["bid", "bid_size"])]
    pub floor: Option<String>,
}

impl BidArgs {
    /// The confirmation these flags give, when they give one.
    #[must_use]
    pub fn confirmed(&self) -> Option<Confirmed> {
        match (&self.bid, &self.bid_size, &self.floor) {
            (Some(bid), Some(bid_size), Some(floor)) => Some(Confirmed {
                bid: bid.clone(),
                bid_size: bid_size.clone(),
                floor: floor.clone(),
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Args)]
pub struct ExitArgs {
    /// The agent's id.
    pub agent: String,
    /// The instrument to exit.
    pub instrument: String,
    #[command(flatten)]
    pub bid: BidArgs,
    /// The code `agent exit --show-code` showed; without the right one the exit still commits,
    /// with no step-up evidence.
    #[arg(long, value_name = "CODE", conflicts_with = "show_code")]
    pub code: Option<String>,
    /// Print the exit's code and commit nothing.
    #[arg(long)]
    pub show_code: bool,
    #[command(flatten)]
    pub owner: OwnerArgs,
    #[command(flatten)]
    pub target: JournalArgs,
}

/// What the owner asked of a command that takes a code (DEC-2507 item 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Typed<'a> {
    /// Print the code, and for a release the warning before it; commit nothing.
    Show,
    /// Commit, with the code typed, if any.
    Commit(Option<&'a str>),
}

impl ResumeArgs {
    #[must_use]
    pub fn typed(&self) -> Typed<'_> {
        self.code
            .as_deref()
            .map_or(Typed::Show, |c| Typed::Commit(Some(c)))
    }
}

impl StopArgs {
    #[must_use]
    pub fn typed(&self) -> Typed<'_> {
        self.code
            .as_deref()
            .map_or(Typed::Show, |c| Typed::Commit(Some(c)))
    }

    #[must_use]
    pub fn command(&self) -> Command {
        Command::Stop {
            release: self.release,
        }
    }
}

impl ExitArgs {
    #[must_use]
    pub fn typed(&self) -> Typed<'_> {
        if self.show_code {
            Typed::Show
        } else {
            Typed::Commit(self.code.as_deref())
        }
    }

    #[must_use]
    pub fn command(&self) -> Command {
        Command::Exit {
            instrument: self.instrument.clone(),
            bid: self.bid.confirmed(),
        }
    }
}

/// What `command` for `agent` prints: with [`Typed::Show`], the warning a release shows and then
/// `code <code>`, committing nothing; with [`Typed::Commit`], the one line naming the event it
/// committed or found (DEC-2507 item 2).
///
/// # Errors
/// [`crate::agent::command`]'s, a refused resume or Stop committing nothing; `bid_invalid`,
/// committing nothing, for an exit whose confirmed bid, bid size or floor is not a positive
/// fixed-point decimal or whose floor is above its bid. A well-formed confirmation is normalized to
/// its canonical text before the code binds it (DEC-2507 item 1).
pub fn answer(
    journal: &mut dyn ControlJournal,
    ids: &mut dyn Ids,
    owner: &Owner,
    agent: &str,
    command: &Command,
    typed: Typed<'_>,
    now: Now,
) -> Result<Vec<String>, ControlError> {
    let _ = (
        journal,
        ids,
        owner,
        agent,
        command,
        typed,
        now,
        RELEASE_WARNING,
    );
    Err(ControlError::Unimplemented { story: "E8-3" })
}

/// `status`'s lines: `mode`, `startup_hold`, `pending_approvals`, each restriction or `restrictions
/// none`, then `controls pause, stop, kill` (PX-3's order).
///
/// # Errors
/// None once implemented: every status has its lines. The `Result` is the stub API's shape.
pub fn status_lines(status: &Status) -> Result<Vec<String>, ControlError> {
    let _ = status;
    Err(ControlError::Unimplemented { story: "E8-3" })
}

/// Runs `agent status` and prints [`status_lines`].
///
/// # Errors
/// The owner's refusals before the journal is opened, then the journal's.
pub fn run_status(args: &StatusArgs, report: &mut impl Write) -> anyhow::Result<()> {
    let _ = (args, report);
    Err(ControlError::Unimplemented { story: "E8-3" }.into())
}

/// Runs `agent resume` and prints [`answer`]'s lines.
///
/// # Errors
/// The owner's refusals before the journal is opened, then [`answer`]'s.
pub fn run_resume(args: &ResumeArgs, now: Now, report: &mut impl Write) -> anyhow::Result<()> {
    let _ = (args, now, report);
    Err(ControlError::Unimplemented { story: "E8-3" }.into())
}

/// Runs `agent stop` and prints [`answer`]'s lines.
///
/// # Errors
/// As [`run_resume`].
pub fn run_stop(args: &StopArgs, now: Now, report: &mut impl Write) -> anyhow::Result<()> {
    let _ = (args, now, report);
    Err(ControlError::Unimplemented { story: "E8-3" }.into())
}

/// Runs `agent exit` and prints [`answer`]'s lines.
///
/// # Errors
/// As [`run_resume`]; never a refusal of the exit itself.
pub fn run_exit(args: &ExitArgs, now: Now, report: &mut impl Write) -> anyhow::Result<()> {
    let _ = (args, now, report);
    Err(ControlError::Unimplemented { story: "E8-3" }.into())
}
