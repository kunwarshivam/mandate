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
use mandate_canon::Value;
use mandate_journal::ArtifactSource;

use crate::agent::{self, Command, Scope};
use crate::control::{ControlError, ControlJournal, Ids, Now, Owner, Submitted, agent_stream};
use crate::deploy::active;
use crate::gestures;
use crate::inbox::InstantIds;
use crate::owner;
use crate::postgres::JournalArgs;
use crate::register::{OwnerArgs, open};
use crate::version::{Row, refused, rows, stored};

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
    /// Show one agent's mode, startup hold, pending approvals and restrictions. Commits nothing.
    Status(owner::StatusArgs),
    /// Show the code that resumes one agent from the owner's own pause, or resume it with it.
    Resume(owner::ResumeArgs),
    /// Show the code that stops one agent, or stop it with it; `--release` hands its positions to
    /// the owner, unprotected.
    Stop(owner::StopArgs),
    /// Exit one instrument of one agent at once; never refused. Without the right code it carries
    /// no step-up evidence.
    Exit(owner::ExitArgs),
    /// Show the code that acknowledges one triggered limit, or acknowledge it with it; with
    /// `--request`, as the independent user answering another user's request.
    Acknowledge(owner::AcknowledgeArgs),
    /// Show the code that asks another user to acknowledge one triggered limit, or record the
    /// request with it.
    RequestAck(owner::RequestAckArgs),
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
        match (&self.agent, &self.connection) {
            (Some(agent), _) => Scope::Agent(agent.clone()),
            (None, Some(connection)) => Scope::Connection(connection.clone()),
            (None, None) => Scope::Workspace,
        }
    }
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
    journal: &dyn ControlJournal,
    store: &dyn ArtifactSource,
    owner: &Owner,
    scope: &Scope,
) -> Result<BTreeSet<String>, ControlError> {
    let connection = match scope {
        Scope::Agent(agent) => return Ok(BTreeSet::from([agent.clone()])),
        Scope::Connection(connection) => Some(connection.as_str()),
        Scope::Workspace => None,
    };
    let rows = rows(journal, owner)?;
    let agents: BTreeSet<&str> = rows
        .iter()
        .filter(|r| r.event_type == "AgentDeployed")
        .filter_map(|r| r.member("agent_id"))
        .collect();
    let on_connection = |deployed: &Row| {
        let named = deployed
            .member("mandate_version")
            .and_then(|version| stored(store, version).ok())
            .and_then(|(document, _)| {
                document
                    .get("connection_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });
        connection.is_none_or(|wanted| named.is_none_or(|named| named == wanted))
    };
    Ok(agents
        .into_iter()
        .filter(|agent| active(&rows, agent).is_some_and(on_connection))
        .map(str::to_owned)
        .collect())
}

/// Pauses every agent [`reached`] names, one agent-scoped `OwnerCommandIssued` each, going on past
/// an agent whose pause fails (DEC-2000 item 3).
///
/// # Errors
/// [`reached`]'s; each agent's own failure is in its [`Paused`].
pub fn pause(
    journal: &mut dyn ControlJournal,
    store: &dyn ArtifactSource,
    ids: &mut dyn Ids,
    owner: &Owner,
    scope: &Scope,
    now: Now,
) -> Result<Vec<Paused>, ControlError> {
    let agents = reached(journal, store, owner, scope)?;
    Ok(agents
        .into_iter()
        .map(|agent| {
            let outcome = agent_stream(owner, &agent)
                .map_err(|_| refused("agent_invalid"))
                .and_then(|_| {
                    agent::command(journal, ids, owner, &agent, &Command::Pause, None, now)
                });
            Paused { agent, outcome }
        })
        .collect())
}

/// Commits a kill switch of `scope`, with no step-up evidence (DEC-2000 item 4).
///
/// # Errors
/// [`ControlError::Journal`] when the append fails; never a refusal.
pub fn kill(
    journal: &mut dyn ControlJournal,
    ids: &mut dyn Ids,
    owner: &Owner,
    scope: &Scope,
    now: Now,
) -> Result<Submitted, ControlError> {
    agent::kill(journal, ids, owner, scope, None, now)
}

/// The line a kill switch prints.
#[must_use]
pub fn killed_line(owner: &Owner, scope: &Scope, submitted: &Submitted) -> String {
    let (kind, subject) = match scope {
        Scope::Agent(agent) => ("agent", agent.as_str()),
        Scope::Connection(connection) => ("connection", connection.as_str()),
        Scope::Workspace => ("workspace", owner.workspace.as_str()),
    };
    format!(
        "kill_switch {kind} {subject} committed as event {} at seq {}",
        submitted.event_id, submitted.seq
    )
}

/// The lines a pause prints, one per agent, or one saying nothing was in scope.
#[must_use]
pub fn paused_lines(paused: &[Paused]) -> Vec<String> {
    if paused.is_empty() {
        return vec!["no deployed agent in scope; nothing committed".to_owned()];
    }
    paused
        .iter()
        .map(|p| match &p.outcome {
            Ok(at) => format!(
                "paused {} as event {} at seq {}",
                p.agent, at.event_id, at.seq
            ),
            Err(why) => format!("pause {} not committed: {why}", p.agent),
        })
        .collect()
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
    pauses_settled(&paused)
}

/// `agent pause`'s exit once every agent was tried (DEC-2000 item 3): success only when every
/// pause committed, so an operator never reads a partly committed brake as done.
///
/// # Errors
/// How many pauses did not commit, when any did not.
pub fn pauses_settled(paused: &[Paused]) -> anyhow::Result<()> {
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
