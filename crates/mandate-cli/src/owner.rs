//! `mandate agent status`, `resume`, `stop`, `exit`, `acknowledge` and `request-ack` (E8-3's `clap`
//! follow-up, DEC-279 item 10; DEC-2507, DEC-2512): [`crate::agent`]'s owner commands over P0's
//! `--journal`, run as D1b's owner.
//!
//! `acknowledge` and `request-ack` take a code as `resume` does: without `--code` they print it and
//! commit nothing. An acknowledgment naming `--request` is refused, committing nothing, under
//! journal spec §9.28's rules 239 to 241.
//!
//! `resume` and `stop` without `--code` print the code [`crate::agent::code`] gives and commit
//! nothing; with it they commit one `OwnerCommandIssued`, and a wrong code is refused with nothing
//! committed. A Stop with `--release` prints [`RELEASE_WARNING`] before its code every time.
//! `exit` commits at once and is never refused (`AGENTS.md` rule 13, DEC-158 option (c)): without
//! the right code it carries no step-up evidence, and `--show-code` prints the code instead. A
//! refusal prints nothing, and no message names the DSN.

use std::io::Write;

use clap::Args;
use mandate_journal::ArtifactSource;
use mandate_num::{Price, Qty};

use crate::agent::{
    self, Command, Confirmed, Holds, RELEASE_WARNING, Restriction, Status, Subject,
};
use crate::control::{ControlError, ControlJournal, Ids, Now, Owner, Submitted};
use crate::inbox::InstantIds;
use crate::postgres::{JournalArgs, PgControlJournal};
use crate::register::{OwnerArgs, open};
use crate::retire;
use mandate_journal::holds::{Holder, Reason};

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
pub struct HoldsArgs {
    /// The agent whose holds to list: its own and its account's. Without it, every hold on the
    /// account.
    pub agent: Option<String>,
    /// The broker account whose stream holds them.
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

#[derive(Debug, Args)]
pub struct AcknowledgeArgs {
    /// The agent's id.
    pub agent: String,
    /// The `event_id` of the account stream's record to acknowledge: a `RiskLimitTriggered`, or
    /// with `--subject` the record that subject names.
    pub event: String,
    /// The `event_id` of the `OwnerAcknowledgmentRequested` another user recorded, where an
    /// independent user must acknowledge.
    #[arg(long, value_name = "ID")]
    pub request: Option<String>,
    /// What `event` is: `limit`, `restriction`, or `reconciliation` (the executor's hold on a
    /// reconciliation mismatch). Given, the acknowledgment is written as version 2 with it.
    #[arg(long, value_enum)]
    pub subject: Option<Subject>,
    /// The code `agent acknowledge` showed; without it, nothing is committed.
    #[arg(long, value_name = "CODE")]
    pub code: Option<String>,
    #[command(flatten)]
    pub owner: OwnerArgs,
    #[command(flatten)]
    pub target: JournalArgs,
}

#[derive(Debug, Args)]
pub struct RequestAckArgs {
    /// The agent's id.
    pub agent: String,
    /// The `event_id` of the account stream's record to ask another user to lift: a
    /// `RiskLimitTriggered`, or with `--subject` the record that subject names.
    pub event: String,
    /// What `event` is, as `agent acknowledge --subject` takes it.
    #[arg(long, value_enum)]
    pub subject: Option<Subject>,
    /// The code `agent request-ack` showed; without it, nothing is committed.
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

impl AcknowledgeArgs {
    #[must_use]
    pub fn typed(&self) -> Typed<'_> {
        self.code
            .as_deref()
            .map_or(Typed::Show, |c| Typed::Commit(Some(c)))
    }

    #[must_use]
    pub fn command(&self) -> Command {
        Command::Acknowledge {
            event: self.event.clone(),
            request: self.request.clone(),
            subject: self.subject,
        }
    }
}

impl RequestAckArgs {
    #[must_use]
    pub fn typed(&self) -> Typed<'_> {
        self.code
            .as_deref()
            .map_or(Typed::Show, |c| Typed::Commit(Some(c)))
    }

    #[must_use]
    pub fn command(&self) -> Command {
        Command::RequestAcknowledgment {
            event: self.event.clone(),
            subject: self.subject,
        }
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
#[allow(
    clippy::too_many_arguments,
    reason = "the journal, store, ids and owner every gesture takes, and the command's four inputs"
)]
pub fn answer(
    journal: &mut dyn ControlJournal,
    store: &dyn ArtifactSource,
    ids: &mut dyn Ids,
    owner: &Owner,
    agent: &str,
    command: &Command,
    typed: Typed<'_>,
    now: Now,
) -> Result<Vec<String>, ControlError> {
    let normalized;
    let command = match command {
        Command::Exit {
            instrument,
            bid: Some(bid),
        } => {
            normalized = Command::Exit {
                instrument: instrument.clone(),
                bid: Some(confirmed_bid(bid)?),
            };
            &normalized
        }
        other => other,
    };
    let Typed::Commit(typed) = typed else {
        let mut lines = Vec::new();
        if matches!(command, Command::Stop { release: true }) {
            lines.push(format!("warning {RELEASE_WARNING}"));
        }
        if let Some(code) = agent::code(journal, owner, agent, command)? {
            lines.push(format!("code {code}"));
        }
        return Ok(lines);
    };
    let Submitted { event_id, seq } = match command {
        Command::Stop { release } => {
            retire::stop(journal, store, ids, owner, agent, *release, typed, now)?
        }
        other => agent::command(journal, ids, owner, agent, other, typed, now)?,
    };
    let named = match command {
        Command::Exit { instrument, .. } => format!("owner_exit {agent} {instrument}"),
        Command::Resume => format!("resume {agent}"),
        Command::Stop { .. } => format!("stop {agent}"),
        Command::Pause => format!("pause {agent}"),
        Command::Acknowledge { event, .. } => format!("acknowledge {agent} {event}"),
        Command::RequestAcknowledgment { event, .. } => format!("request-ack {agent} {event}"),
    };
    Ok(vec![format!(
        "{named} committed as event {event_id} at seq {seq}"
    )])
}

/// A bid confirmation the exit price ladder can use, in canonical text: a positive bid, a positive
/// bid size and a positive floor at or below the bid, each a fixed-point decimal written as
/// displayed (trading spec §5.5, DEC-2507 item 1).
fn confirmed_bid(bid: &Confirmed) -> Result<Confirmed, ControlError> {
    let invalid = || ControlError::Refused {
        reason: "bid_invalid",
    };
    let (price, size, floor) = (
        canonical_decimal(&bid.bid),
        canonical_decimal(&bid.bid_size),
        canonical_decimal(&bid.floor),
    );
    let read_price = |text: &str| Price::parse(text).map_err(|_| invalid());
    let read_size = Qty::parse(&size).map_err(|_| invalid())?;
    if read_size.is_zero() || read_price(&floor)? > read_price(&price)? {
        return Err(invalid());
    }
    Ok(Confirmed {
        bid: price,
        bid_size: size,
        floor,
    })
}

/// `text` without its leading zeros before the point and its trailing zeros after it, so a value
/// typed as displayed (`501.20`, `00.50`, `495.`) reads as `mandate-num`'s canonical text, which
/// refuses everything that is not a decimal.
fn canonical_decimal(text: &str) -> String {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    let whole = whole.trim_start_matches('0');
    let whole = if whole.is_empty() { "0" } else { whole };
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() {
        whole.to_owned()
    } else {
        format!("{whole}.{fraction}")
    }
}

/// `status`'s lines: `mode`, `startup_hold`, `pending_approvals`, each restriction or `restrictions
/// none`, then `controls pause, stop, kill` (PX-3's order).
///
/// # Errors
/// None: every status has its lines. The `Result` is the stub API's shape.
pub fn status_lines(status: &Status) -> Result<Vec<String>, ControlError> {
    let hold = if status.startup_hold { "yes" } else { "no" };
    let mut lines = vec![
        format!("mode {}", status.mode),
        format!("startup_hold {hold}"),
        format!("pending_approvals {}", status.pending_approvals),
    ];
    if status.restrictions.is_empty() {
        lines.push("restrictions none".to_owned());
    }
    lines.extend(status.restrictions.iter().map(|r| match r {
        Restriction::Agent { restriction, mode } => {
            format!("restriction agent {restriction} {mode}")
        }
        Restriction::Instrument {
            instrument,
            restriction,
        } => format!("restriction instrument {instrument} {restriction}"),
    }));
    lines.push(PX3_CONTROLS.to_owned());
    Ok(lines)
}

/// The controls `status` offers, the lightest first (PX-3, DEC-135).
const PX3_CONTROLS: &str = "controls pause, stop, kill";

/// Runs `command` for `agent` through [`answer`] over the journal `target` names, once `flags`
/// name a valid owner (DEC-2507 item 3).
fn run(
    flags: &OwnerArgs,
    target: &JournalArgs,
    agent: &str,
    command: &Command,
    typed: Typed<'_>,
    now: Now,
    report: &mut impl Write,
) -> anyhow::Result<()> {
    let owner = flags.owner()?;
    let (mut journal, store) = open(target)?;
    let mut ids = InstantIds::new(&owner, now);
    let lines = answer(
        &mut journal,
        &store,
        &mut ids,
        &owner,
        agent,
        command,
        typed,
        now,
    )?;
    for line in lines {
        writeln!(report, "{line}")?;
    }
    Ok(())
}

/// `mandate agent holds`: [`agent::holds`], printed by [`holds_lines`]. Commits nothing.
///
/// # Errors
/// The owner's refusals before the journal is opened, then the journal's and the fold's.
pub fn run_holds(args: &HoldsArgs, report: &mut impl Write) -> anyhow::Result<()> {
    let owner = args.owner.owner()?;
    let journal = PgControlJournal::open(&args.target)?;
    let found = agent::holds(&journal, &owner, &args.account, args.agent.as_deref())?;
    for line in holds_lines(&found) {
        writeln!(report, "{line}")?;
    }
    Ok(())
}

/// The first line `mandate agent holds` prints: whether the workspace's latest membership record
/// requires an independent user to acknowledge, marked advisory until E9-6's effective-policy fold
/// lands, since the executor's §9.28 check 4 takes the strictest overlay of every agent deployed on
/// the account, which this listing does not read (backlog E7-4b holds fold minors, item 1).
///
/// # Errors
/// None once implemented.
pub fn independence_line(independent: bool) -> Result<String, ControlError> {
    let _ = independent;
    Err(ControlError::Unimplemented { story: "E7-4b" })
}

/// The lines `mandate agent holds` prints: whether an independent user must acknowledge, then each
/// hold on one line (its `event`, subject, holder, what it holds, its mode, since when, and its
/// unspent request and who made it), and under it the command that moves it on, to run with the
/// usual owner and journal flags: the `agent acknowledge` that lifts it, the `agent request-ack`
/// that asks another user to where one must, the answer to a request that another user runs when
/// the request is the caller's own (rule 239), or why none lifts it yet.
#[must_use]
pub fn holds_lines(found: &Holds) -> Vec<String> {
    let mut lines = vec![format!(
        "independent approval required: {}",
        if found.independent { "yes" } else { "no" }
    )];
    if found.holds.is_empty() {
        lines.push("no holds".to_owned());
    }
    for hold in &found.holds {
        let raised = &hold.raised;
        let subject = raised.reason.subject();
        let holder = match &raised.holder {
            Holder::Agent(agent) => agent.as_str(),
            Holder::Account => "*",
        };
        let held = match &raised.reason {
            Reason::Limit(code) | Reason::Cause(code) => code.as_str(),
            Reason::Reconciliation { restriction, .. } => restriction.as_str(),
        };
        let request = hold.request.as_ref().map(|r| r.event_id.as_str());
        let requested_by = hold.request.as_ref().map(|r| r.requested_by.as_str());
        lines.push(format!(
            "{} subject={subject} holder={holder} held={held} mode={} since={} request={} requested_by={}",
            raised.event,
            raised.mode.unwrap_or("-"),
            raised.raised_at,
            request.unwrap_or("-"),
            requested_by.unwrap_or("-"),
        ));
        let named = format!("{} {} --subject {subject}", hold.agent_id, raised.event);
        lines.push(match (subject, request) {
            ("restriction", _) => {
                "  not yet acknowledgeable: the executor lifts no restriction until E7-2y"
                    .to_owned()
            }
            (_, Some(request)) if requested_by == Some(found.user.as_str()) => format!(
                "  another user answers your request: mandate agent acknowledge {named} --request {request}"
            ),
            (_, Some(request)) => {
                format!("  mandate agent acknowledge {named} --request {request}")
            }
            (_, None) if found.independent => format!("  mandate agent request-ack {named}"),
            (_, None) => format!("  mandate agent acknowledge {named}"),
        });
    }
    lines
}

/// Runs `agent status` and prints [`status_lines`].
///
/// # Errors
/// The owner's refusals before the journal is opened, then the journal's.
pub fn run_status(args: &StatusArgs, report: &mut impl Write) -> anyhow::Result<()> {
    let owner = args.owner.owner()?;
    let journal = PgControlJournal::open(&args.target)?;
    let status = agent::status(&journal, &owner, &args.agent, &args.account)?;
    for line in status_lines(&status)? {
        writeln!(report, "{line}")?;
    }
    Ok(())
}

/// Runs `agent resume` and prints [`answer`]'s lines.
///
/// # Errors
/// The owner's refusals before the journal is opened, then [`answer`]'s.
pub fn run_resume(args: &ResumeArgs, now: Now, report: &mut impl Write) -> anyhow::Result<()> {
    let typed = args.typed();
    run(
        &args.owner,
        &args.target,
        &args.agent,
        &Command::Resume,
        typed,
        now,
        report,
    )
}

/// Runs `agent stop` and prints [`answer`]'s lines.
///
/// # Errors
/// As [`run_resume`].
pub fn run_stop(args: &StopArgs, now: Now, report: &mut impl Write) -> anyhow::Result<()> {
    let (command, typed) = (args.command(), args.typed());
    run(
        &args.owner,
        &args.target,
        &args.agent,
        &command,
        typed,
        now,
        report,
    )
}

/// Runs `agent acknowledge` and prints [`answer`]'s lines.
///
/// # Errors
/// As [`run_resume`], and §9.28's refusals of an acknowledgment naming a request.
pub fn run_acknowledge(
    args: &AcknowledgeArgs,
    now: Now,
    report: &mut impl Write,
) -> anyhow::Result<()> {
    let (command, typed) = (args.command(), args.typed());
    run(
        &args.owner,
        &args.target,
        &args.agent,
        &command,
        typed,
        now,
        report,
    )
}

/// Runs `agent request-ack` and prints [`answer`]'s lines.
///
/// # Errors
/// As [`run_resume`].
pub fn run_request_ack(
    args: &RequestAckArgs,
    now: Now,
    report: &mut impl Write,
) -> anyhow::Result<()> {
    let (command, typed) = (args.command(), args.typed());
    run(
        &args.owner,
        &args.target,
        &args.agent,
        &command,
        typed,
        now,
        report,
    )
}

/// Runs `agent exit` and prints [`answer`]'s lines.
///
/// # Errors
/// As [`run_resume`]; never a refusal of the exit itself.
pub fn run_exit(args: &ExitArgs, now: Now, report: &mut impl Write) -> anyhow::Result<()> {
    let (command, typed) = (args.command(), args.typed());
    run(
        &args.owner,
        &args.target,
        &args.agent,
        &command,
        typed,
        now,
        report,
    )
}
