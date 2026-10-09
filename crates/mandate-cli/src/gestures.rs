//! The `mandate version create`, `mandate version confirm` and `mandate agent deploy` commands
//! (first paper trade brief, D2c; DEC-530 item 1): [`crate::version`]'s and [`crate::deploy`]'s
//! functions over P0's `--journal` and `--store`, run as the owner DEC-527's `--workspace` and
//! `--user` name, always in paper. A gesture given no `--code` shows its code and the warnings it
//! acknowledges and commits nothing; given one, it commits.

use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use mandate_canon::{Digest, Value, to_canonical};
use mandate_journal::ArtifactSource;
use mandate_spec::Mandate;

use crate::control::{
    ControlError, ControlJournal, Now, Owner, Submitted, agent_stream, code_of, object, text,
};
use crate::deploy::{self, active};
use crate::inbox::InstantIds;
use crate::postgres::JournalArgs;
use crate::register::{OwnerArgs, open};
use crate::version::{
    self, CONFIRMED, CREATED, check_instruments, check_rules, envelope_paths, latest, paper_only,
    refused, rows, stored,
};

#[derive(Debug, Subcommand)]
pub enum VersionCommand {
    /// Store a mandate document and create its version on the workspace control stream.
    Create(CreateArgs),
    /// Show a version's confirmation code, or confirm the version with it.
    Confirm(ConfirmArgs),
}

#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    /// Show an agent's deployment code for a version, or deploy the agent with it.
    Deploy(DeployArgs),
}

#[derive(Debug, Args)]
pub struct CreateArgs {
    /// The mandate document, a JSON file the owner wrote.
    pub file: PathBuf,
    #[command(flatten)]
    pub owner: OwnerArgs,
    #[command(flatten)]
    pub target: JournalArgs,
}

#[derive(Debug, Args)]
pub struct ConfirmArgs {
    /// The version, `sha256:` and 64 hex digits, as `version create` printed it.
    pub version: String,
    /// The code `version confirm` showed for this version; without it, nothing is committed.
    #[arg(long, value_name = "CODE")]
    pub code: Option<String>,
    #[command(flatten)]
    pub owner: OwnerArgs,
    #[command(flatten)]
    pub target: JournalArgs,
}

#[derive(Debug, Args)]
pub struct DeployArgs {
    /// The agent's id.
    pub agent: String,
    /// The confirmed version to deploy it with.
    pub version: String,
    /// The code `agent deploy` showed for this agent and version; without it, nothing is
    /// committed.
    #[arg(long, value_name = "CODE")]
    pub code: Option<String>,
    #[command(flatten)]
    pub owner: OwnerArgs,
    #[command(flatten)]
    pub target: JournalArgs,
}

/// What a gesture shows before its code is typed: the code (DEC-530 item 4) and the warnings it
/// acknowledges (mandate spec §4.2), by code, sorted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub code: String,
    pub warnings: Vec<&'static str>,
}

/// What confirming `version` shows, after every check [`crate::version::confirm`] makes but the
/// code's. It writes nothing.
///
/// # Errors
/// [`crate::version::confirm`]'s refusals but `code_mismatch`; [`ControlError::Journal`].
pub fn confirmation(
    journal: &dyn ControlJournal,
    store: &dyn ArtifactSource,
    owner: &Owner,
    version: &str,
    now: Now,
) -> Result<Shown, ControlError> {
    paper_only(owner)?;
    let rows = rows(journal, owner)?;
    if latest(&rows, CREATED, version).is_none() {
        return Err(refused("version_unknown"));
    }
    let (document, mandate) = stored(store, version)?;
    let paths = envelope_paths(&document);
    let digest = Digest::of(&to_canonical(&document));
    let warnings = check_rules(&rows, store, (&mandate, ""), Some((digest, &paths)), now)?;
    check_instruments(&rows, store, &document)?;
    let gesture = object(vec![
        ("gesture", text("mandate_confirm")),
        ("mandate_version", text(version)),
    ])?;
    Ok(shown(&gesture, warnings))
}

/// What deploying `agent` with `version` shows, after every check [`crate::deploy::deploy`] makes
/// but the code's. It writes nothing.
///
/// # Errors
/// [`crate::deploy::deploy`]'s refusals but `code_mismatch`; [`ControlError::Journal`].
pub fn deployment(
    journal: &dyn ControlJournal,
    store: &dyn ArtifactSource,
    owner: &Owner,
    (agent, version): (&str, &str),
    now: Now,
) -> Result<Shown, ControlError> {
    paper_only(owner)?;
    agent_stream(owner, agent).map_err(|_| refused("agent_invalid"))?;
    let rows = rows(journal, owner)?;
    if latest(&rows, CREATED, version).is_none() {
        return Err(refused("version_unknown"));
    }
    let (document, mandate) = stored(store, version)?;
    let confirmed =
        latest(&rows, CONFIRMED, version).ok_or_else(|| refused("version_unconfirmed"))?;
    let last = rows.iter().rev().find(|r| r.event_type == CONFIRMED);
    if last.map(|r| r.seq) != Some(confirmed.seq) {
        return Err(refused("version_superseded"));
    }
    let gesture = object(vec![
        ("agent_id", text(agent)),
        ("gesture", text("agent_deploy")),
        ("mandate_version", text(version)),
    ])?;
    let mut warnings = Vec::new();
    match active(&rows, agent) {
        Some(deployed) if deployed.member("mandate_version") == Some(version) => {}
        Some(_) => return Err(refused("agent_active")),
        None => {
            warnings = check_rules(&rows, store, (&mandate, agent), None, now)?;
            check_instruments(&rows, store, &document)?;
        }
    }
    Ok(shown(&gesture, warnings))
}

/// The gesture's code and its warnings, in the code order [`check_rules`] gives them.
fn shown(gesture: &Value, warnings: Vec<&'static str>) -> Shown {
    Shown {
        code: code_of(gesture),
        warnings,
    }
}

/// Prints what a gesture shows: its code, then its warnings or `none`.
fn display(report: &mut impl Write, shown: &Shown) -> anyhow::Result<()> {
    writeln!(report, "code {}", shown.code)?;
    let warnings = if shown.warnings.is_empty() {
        "none".to_owned()
    } else {
        shown.warnings.join(", ")
    };
    writeln!(report, "warnings {warnings}")?;
    Ok(())
}

/// Prints what a gesture `done` committed or found.
fn committed(report: &mut impl Write, done: &str, at: &Submitted) -> anyhow::Result<()> {
    writeln!(report, "{done} as event {} at seq {}", at.event_id, at.seq)?;
    Ok(())
}

/// Runs `version create`, and prints the version and the event id and `seq` it committed or found.
///
/// # Errors
/// A refusal whose message carries its code: `owner_workspace_invalid`, `owner_user_invalid` or
/// `config_file_unreadable` before the journal is opened or the store created, then
/// [`crate::version::create`]'s; no message names the DSN.
pub fn create(args: &CreateArgs, now: Now, report: &mut impl Write) -> anyhow::Result<Submitted> {
    let owner = args.owner.owner()?;
    let document = std::fs::read(&args.file).map_err(|_| refused("config_file_unreadable"))?;
    let (mut journal, mut store) = open(&args.target)?;
    let submitted = version::create(&mut journal, &mut store, &owner, &document, now)?;
    let canonical =
        Mandate::parse(&mandate_canon::parse(&document).map_err(|_| refused("mandate_invalid"))?)
            .and_then(|m| m.canonical_bytes())
            .map_err(|_| refused("mandate_invalid"))?;
    let done = format!("created sha256:{}", Digest::of(&canonical).to_hex());
    committed(report, &done, &submitted)?;
    Ok(submitted)
}

/// Runs `version confirm`: without `--code`, prints [`confirmation`]'s code and warnings and
/// returns `None`; with it, confirms and prints the event id and `seq`.
///
/// # Errors
/// As [`create`], without the file, then [`confirmation`]'s or [`crate::version::confirm`]'s.
pub fn confirm(
    args: &ConfirmArgs,
    now: Now,
    report: &mut impl Write,
) -> anyhow::Result<Option<Submitted>> {
    let owner = args.owner.owner()?;
    let (mut journal, mut store) = open(&args.target)?;
    let Some(code) = &args.code else {
        display(
            report,
            &confirmation(&journal, &store, &owner, &args.version, now)?,
        )?;
        return Ok(None);
    };
    let mut ids = InstantIds::new(&owner, now);
    let stream = (&mut journal, &mut store);
    let submitted = version::confirm(
        stream.0,
        stream.1,
        &mut ids,
        &owner,
        &args.version,
        code,
        now,
    )?;
    committed(report, "confirmed", &submitted)?;
    Ok(Some(submitted))
}

/// Runs `agent deploy`: without `--code`, prints [`deployment`]'s code and warnings and returns
/// `None`; with it, deploys and prints the event id and `seq`.
///
/// # Errors
/// As [`confirm`], with [`deployment`]'s or [`crate::deploy::deploy`]'s.
pub fn deploy(
    args: &DeployArgs,
    now: Now,
    report: &mut impl Write,
) -> anyhow::Result<Option<Submitted>> {
    let owner = args.owner.owner()?;
    let (mut journal, mut store) = open(&args.target)?;
    let named = (args.agent.as_str(), args.version.as_str());
    let Some(code) = &args.code else {
        display(report, &deployment(&journal, &store, &owner, named, now)?)?;
        return Ok(None);
    };
    let mut ids = InstantIds::new(&owner, now);
    let stream = (&mut journal, &mut store);
    let submitted = deploy::deploy(
        stream.0, stream.1, &mut ids, &owner, named.0, named.1, code, now,
    )?;
    committed(report, "deployed", &submitted)?;
    Ok(Some(submitted))
}
