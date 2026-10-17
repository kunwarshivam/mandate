//! `mandate agent pause` and `mandate agent kill` at agent, connection and workspace scope: the
//! kill-switch surface in the cell (E21-34, OPS-4; infrastructure design §3.6; DEC-2000). Each
//! needs only the journal, under the control role, so it runs as a one-off Job or a `docker
//! compose run` with the API, the deployment manager, the exporter, the model gateway and the
//! control plane all down; the stream owners apply what it commits (`AGENTS.md` rule 13).
//!
//! A kill switch is one `OwnerCommandIssued` of its scope, through [`crate::agent::kill`]: never
//! refused, and committed without step-up evidence, which no stream owner needs to stop and
//! flatten (DEC-158 option (c)). The journal's `pause` is agent-scoped only (journal spec §9.2), so
//! a pause of a connection or a workspace is one agent-scoped pause for each agent deployed there
//! (DEC-2000 item 2).

use std::collections::BTreeSet;
use std::io::Write;

use anyhow::bail;
use clap::{Args, Subcommand};
use mandate_journal::ArtifactSource;

use crate::agent::{self, Scope};
use crate::control::{ControlError, ControlJournal, Ids, Now, Owner, Submitted};
use crate::gestures;
use crate::inbox::InstantIds;
use crate::postgres::JournalArgs;
use crate::register::{OwnerArgs, open};

/// `mandate agent`: the deployment gesture, and the brakes.
#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    #[command(flatten)]
    Gesture(gestures::AgentCommand),
    /// Pause one agent, every agent deployed on a connection, or every agent of the workspace.
    /// Needs no code.
    Pause(BrakeArgs),
    /// Commit a kill switch for one agent, a connection, or the whole workspace. Never refused;
    /// needs no code.
    Kill(BrakeArgs),
}

/// What a brake reaches: exactly one of `--agent`, `--connection` or `--workspace-wide`.
#[derive(Debug, Clone, Args)]
#[group(required = true, multiple = false)]
pub struct ReachArgs {
    /// One agent, by id.
    #[arg(long, value_name = "ID")]
    pub agent: Option<String>,
    /// One broker connection, by id.
    #[arg(long, value_name = "ID")]
    pub connection: Option<String>,
    /// The whole workspace `--workspace` names.
    #[arg(long)]
    pub workspace_wide: bool,
}

impl ReachArgs {
    /// The scope these flags name.
    #[must_use]
    pub fn scope(&self) -> Scope {
        todo_scope(self)
    }
}

fn todo_scope(_reach: &ReachArgs) -> Scope {
    Scope::Workspace
}

#[derive(Debug, Args)]
pub struct BrakeArgs {
    #[command(flatten)]
    pub reach: ReachArgs,
    #[command(flatten)]
    pub owner: OwnerArgs,
    #[command(flatten)]
    pub target: JournalArgs,
}

/// One agent's pause: what was committed or found, or why it was not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paused {
    pub agent: String,
    pub outcome: Result<Submitted, ControlError>,
}

/// The agents a pause of `scope` reaches: the one agent named; or each agent whose latest
/// `AgentDeployed` has no later `AgentStopped`, of the whole workspace, or of the connection whose
/// stored mandate names it or whose mandate cannot be read (DEC-2000 item 2). In id order.
///
/// # Errors
/// [`ControlError::Journal`] when the control stream cannot be read.
pub fn reached(
    _journal: &dyn ControlJournal,
    _store: &dyn ArtifactSource,
    _owner: &Owner,
    _scope: &Scope,
) -> Result<BTreeSet<String>, ControlError> {
    Err(ControlError::Unimplemented { story: "E21-34" })
}

/// Pauses every agent [`reached`] names, one agent-scoped `OwnerCommandIssued` each, going on past
/// an agent whose pause fails (DEC-2000 item 3).
///
/// # Errors
/// [`reached`]'s; each agent's own failure is in its [`Paused`].
pub fn pause(
    _journal: &mut dyn ControlJournal,
    _store: &dyn ArtifactSource,
    _ids: &mut dyn Ids,
    _owner: &Owner,
    _scope: &Scope,
    _now: Now,
) -> Result<Vec<Paused>, ControlError> {
    Err(ControlError::Unimplemented { story: "E21-34" })
}

/// Commits a kill switch of `scope`, with no step-up evidence (DEC-2000 item 4).
///
/// # Errors
/// [`ControlError::Journal`] when the append fails; never a refusal.
pub fn kill(
    _journal: &mut dyn ControlJournal,
    _ids: &mut dyn Ids,
    _owner: &Owner,
    _scope: &Scope,
    _now: Now,
) -> Result<Submitted, ControlError> {
    let _ = agent::kill;
    Err(ControlError::Unimplemented { story: "E21-34" })
}

/// The line a kill switch prints.
#[must_use]
pub fn killed_line(_owner: &Owner, _scope: &Scope, _submitted: &Submitted) -> String {
    String::new()
}

/// The lines a pause prints, one per agent, or one saying nothing was in scope.
#[must_use]
pub fn paused_lines(_paused: &[Paused]) -> Vec<String> {
    Vec::new()
}

/// Runs `agent pause` and prints [`paused_lines`].
///
/// # Errors
/// The owner's refusals before the journal is opened, then [`pause`]'s; and, once every agent
/// was tried, a failure naming how many pauses did not commit.
pub fn run_pause(args: &BrakeArgs, now: Now, report: &mut impl Write) -> anyhow::Result<()> {
    let owner = args.owner.owner()?;
    let (mut journal, store) = open(&args.target)?;
    let mut ids = InstantIds::new(&owner, now);
    let scope = args.reach.scope();
    let paused = pause(&mut journal, &store, &mut ids, &owner, &scope, now)?;
    for line in paused_lines(&paused) {
        writeln!(report, "{line}")?;
    }
    let failed = paused.iter().filter(|p| p.outcome.is_err()).count();
    if failed > 0 {
        bail!("{failed} pauses did not commit");
    }
    Ok(())
}

/// Runs `agent kill` and prints [`killed_line`].
///
/// # Errors
/// The owner's refusals before the journal is opened, then [`kill`]'s.
pub fn run_kill(args: &BrakeArgs, now: Now, report: &mut impl Write) -> anyhow::Result<Submitted> {
    let owner = args.owner.owner()?;
    let (mut journal, _store) = open(&args.target)?;
    let mut ids = InstantIds::new(&owner, now);
    let scope = args.reach.scope();
    let submitted = kill(&mut journal, &mut ids, &owner, &scope, now)?;
    writeln!(report, "{}", killed_line(&owner, &scope, &submitted))?;
    Ok(submitted)
}
