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
use std::time::Duration;

use clap::{Args, Subcommand};
use mandate_canon::{Digest, to_canonical};
use mandate_time::UtcNanos;

use crate::approvals::{self, Ended, Listed, Outcome, Shown, State};
use crate::control::{ControlError, ControlJournal, Ids, Now, Owner, Submitted};
use crate::postgres::{JournalArgs, PgControlJournal};
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
/// None: every approval has a line. The `Result` is the stub API's shape.
pub fn list_lines(listed: &[Listed]) -> Result<Vec<String>, ControlError> {
    if listed.is_empty() {
        return Ok(vec!["no approvals".to_owned()]);
    }
    Ok(listed
        .iter()
        .map(|l| {
            format!(
                "{} {} {} deadline {} remaining {}",
                l.approval,
                l.agent,
                state_word(l.state),
                l.deadline_s,
                l.remaining_s
            )
        })
        .collect())
}

fn state_word(state: State) -> &'static str {
    match state {
        State::Pending => "pending",
        State::Acted => "acted",
        State::Skipped(Ended::ByOwner) => "skipped_by_owner",
        State::Skipped(Ended::OnRevalidation) => "skipped_on_revalidation",
        State::Skipped(Ended::TimedOut) => "timed_out",
        State::Skipped(Ended::Canceled) => "canceled",
    }
}

/// `show`'s lines: `approval <approval> deadline <s>`, the content object's canonical JSON exactly,
/// `content_hash <hash>`, and `code <code>`.
///
/// # Errors
/// [`ControlError::Journal`] for content whose canonical bytes are not UTF-8, which canonical JSON
/// never is.
pub fn show_lines(shown: &Shown) -> Result<Vec<String>, ControlError> {
    let canonical = String::from_utf8(to_canonical(&shown.content))
        .map_err(|_| ControlError::Journal("the request's content is not UTF-8".to_owned()))?;
    Ok(vec![
        format!("approval {} deadline {}", shown.approval, shown.deadline_s),
        canonical,
        format!("content_hash {}", shown.content_hash),
        format!("code {}", shown.code),
    ])
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
    Ok(vec![
        format!(
            "granted {approval} as event {} at seq {} for {content_hash}",
            submitted.event_id, submitted.seq
        ),
        approvals::message(outcome, now)?,
    ])
}

/// `skip`'s line: `skipped <approval> as event <id> at seq <n>`.
///
/// # Errors
/// As [`list_lines`].
pub fn skipped_line(approval: &str, submitted: &Submitted) -> Result<String, ControlError> {
    Ok(format!(
        "skipped {approval} as event {} at seq {}",
        submitted.event_id, submitted.seq
    ))
}

/// The `count`th step-up assertion id a command run by `owner` at `at` mints, which the binary's
/// `Ids` hands out: derived from the three, with no new dependency (DEC-533's K1a reading). Each is
/// a fresh id the workspace has not seen (mandate spec §6.1), since no two commands run at one
/// nanosecond on one owner's clock.
///
/// `cli-` and the first 32 hex digits of the SHA-256 of the workspace, the user, the instant and the
/// count, one per line: user ids hold no newline (DEC-527 item 3), so no two inputs share a seed.
///
/// # Errors
/// None. The `Result` is the stub API's shape.
pub fn assertion_id(owner: &Owner, at: UtcNanos, count: u32) -> Result<String, ControlError> {
    let seed = format!("{}\n{}\n{at}\n{count}", owner.workspace, owner.user);
    let hex = Digest::of(seed.as_bytes()).to_hex();
    Ok(format!("cli-{}", &hex[..32]))
}

/// The binary's [`Ids`]: [`assertion_id`] for the command's owner and instant, counted from 1.
pub(crate) struct InstantIds<'a> {
    owner: &'a Owner,
    at: UtcNanos,
    count: u32,
}

impl<'a> InstantIds<'a> {
    pub(crate) fn new(owner: &'a Owner, now: Now) -> Self {
        Self {
            owner,
            at: now.at,
            count: 0,
        }
    }
}

impl Ids for InstantIds<'_> {
    fn assertion_id(&mut self) -> String {
        self.count = self.count.saturating_add(1);
        assertion_id(self.owner, self.at, self.count).unwrap_or_default()
    }
}

fn print(report: &mut impl Write, lines: &[String]) -> anyhow::Result<()> {
    for line in lines {
        writeln!(report, "{line}")?;
    }
    Ok(())
}

/// Runs `approvals list` and prints [`list_lines`].
///
/// # Errors
/// A refusal whose message carries its code: `owner_workspace_invalid` or `owner_user_invalid`
/// before the journal is opened or the store created, then the journal's error; no message names
/// the DSN, and a refusal prints nothing.
pub fn run_list(args: &ListArgs, now: Now, report: &mut impl Write) -> anyhow::Result<Vec<Listed>> {
    let owner = args.owner.owner()?;
    let journal = PgControlJournal::open(&args.target)?;
    let agents: Vec<&str> = args.agents.iter().map(String::as_str).collect();
    let listed = approvals::list(&journal, &owner, &agents, now)?;
    print(report, &list_lines(&listed)?)?;
    Ok(listed)
}

/// Runs `approvals show` and prints [`show_lines`].
///
/// # Errors
/// As [`run_list`], then [`crate::approvals::show`]'s refusals.
pub fn run_show(args: &ShowArgs, report: &mut impl Write) -> anyhow::Result<Shown> {
    let owner = args.owner.owner()?;
    let journal = PgControlJournal::open(&args.target)?;
    let shown = approvals::show(&journal, &owner, &args.agent, &args.approval)?;
    print(report, &show_lines(&shown)?)?;
    Ok(shown)
}

/// Runs `approvals approve` with ids from [`assertion_id`], waits up to `--wait-s` for the runtime's record,
/// and prints [`granted_lines`].
///
/// # Errors
/// As [`run_list`], then [`crate::approvals::approve`]'s refusals.
pub fn run_approve(
    args: &ApproveArgs,
    now: Now,
    report: &mut impl Write,
) -> anyhow::Result<Submitted> {
    let owner = args.owner.owner()?;
    let mut journal = PgControlJournal::open(&args.target)?;
    let (agent, approval) = (args.agent.as_str(), args.approval.as_str());
    let shown = approvals::show(&journal, &owner, agent, approval)?;
    let mut ids = InstantIds::new(&owner, now);
    let submitted = approvals::approve(
        &mut journal,
        &mut ids,
        &owner,
        agent,
        approval,
        &args.code,
        now,
    )?;
    let mut outcome = approvals::outcome(&journal, &owner, agent, &submitted)?;
    for _ in 0..args.wait_s {
        if outcome != Outcome::NotRecorded {
            break;
        }
        journal.wait(Duration::from_secs(1));
        outcome = approvals::outcome(&journal, &owner, agent, &submitted)?;
    }
    let lines = granted_lines(approval, &shown.content_hash, &submitted, &outcome, now)?;
    print(report, &lines)?;
    Ok(submitted)
}

/// Runs `approvals skip` and prints [`skipped_line`].
///
/// # Errors
/// As [`run_list`], then [`crate::approvals::skip`]'s refusals.
pub fn run_skip(args: &SkipArgs, now: Now, report: &mut impl Write) -> anyhow::Result<Submitted> {
    let owner = args.owner.owner()?;
    let mut journal = PgControlJournal::open(&args.target)?;
    let mut ids = InstantIds::new(&owner, now);
    let (agent, approval) = (args.agent.as_str(), args.approval.as_str());
    let submitted = approvals::skip(&mut journal, &mut ids, &owner, agent, approval, now)?;
    print(report, &[skipped_line(approval, &submitted)?])?;
    Ok(submitted)
}
