//! The `mandate version create`, `mandate version confirm` and `mandate agent deploy` commands
//! (first paper trade brief, D2c; DEC-530 item 1): [`crate::version`]'s and [`crate::deploy`]'s
//! functions over P0's `--journal` and `--store`, run as the owner DEC-527's `--workspace` and
//! `--user` name, always in paper. A gesture given no `--code` shows its code and the warnings it
//! acknowledges and commits nothing; given one, it commits.

use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::Digest;
use mandate_journal::ArtifactSource;

use crate::control::{ControlError, ControlJournal, Ids, Now, Owner, Submitted};
use crate::postgres::{JournalArgs, PgControlJournal};
use crate::register::OwnerArgs;
use crate::{deploy, version};

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
    let (_, _, warnings) = version::confirm_checks(journal, store, owner, version, None, now)?;
    let code = version::confirm_code(version)?;
    Ok(Shown { code, warnings })
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
    let checked = deploy::deploy_checks(journal, store, owner, (agent, version), None)?;
    let warnings = deploy::rule_checks(&checked, store, agent, now)?;
    let code = deploy::deploy_code(agent, version)?;
    Ok(Shown { code, warnings })
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
    let created = version::version_of(&document)?;
    let Submitted { event_id, seq } = &submitted;
    writeln!(report, "created {created} as event {event_id} at seq {seq}")?;
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
    let version = args.version.as_str();
    let Some(code) = &args.code else {
        let shown = confirmation(&journal, &store, &owner, version, now)?;
        return shown_to(report, &shown);
    };
    let mut ids = Minted::new(&owner, "mandate_confirm", version, now);
    let (journal, store) = (&mut journal, &mut store);
    let submitted = version::confirm(journal, store, &mut ids, &owner, version, code, now)?;
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
    let (agent, version) = (args.agent.as_str(), args.version.as_str());
    let Some(code) = &args.code else {
        let shown = deployment(&journal, &store, &owner, (agent, version), now)?;
        return shown_to(report, &shown);
    };
    let mut ids = Minted::new(&owner, agent, version, now);
    let (journal, store) = (&mut journal, &mut store);
    let submitted = deploy::deploy(journal, store, &mut ids, &owner, agent, version, code, now)?;
    committed(report, "deployed", &submitted)?;
    Ok(Some(submitted))
}

fn refused(reason: &'static str) -> ControlError {
    ControlError::Refused { reason }
}

/// The journal and the store `target` names, each opened only once every local check has passed.
fn open(target: &JournalArgs) -> Result<(PgControlJournal, FsArtifactStore), ControlError> {
    let journal = PgControlJournal::open(target)?;
    let store = FsArtifactStore::open(&target.store)
        .map_err(|e| ControlError::Journal(format!("the artifact store: {}", e.code())))?;
    Ok((journal, store))
}

/// Prints what a gesture shows: `code …` and `warnings …`, `none` when there are none.
fn shown_to(report: &mut impl Write, shown: &Shown) -> anyhow::Result<Option<Submitted>> {
    let warnings = match shown.warnings.as_slice() {
        [] => "none".to_owned(),
        codes => codes.join(" "),
    };
    writeln!(report, "code {}", shown.code)?;
    writeln!(report, "warnings {warnings}")?;
    Ok(None)
}

/// Prints what a typed code committed: the event id and `seq`.
fn committed(report: &mut impl Write, done: &str, submitted: &Submitted) -> anyhow::Result<()> {
    let Submitted { event_id, seq } = submitted;
    writeln!(report, "{done} as event {event_id} at seq {seq}")?;
    Ok(())
}

/// A gesture's step-up assertion ids: `cli-` and the hex SHA-256 of the owner, what the gesture is
/// about, the moment it ran and a counter, so a gesture made again later mints a new one (mandate
/// spec §6.1: an assertion is never reused).
struct Minted {
    seed: String,
    count: u64,
}

impl Minted {
    fn new(owner: &Owner, subject: &str, version: &str, now: Now) -> Self {
        let (user, at) = (&owner.user, now.at);
        let seed = format!("{user}\n{subject}\n{version}\n{at}");
        Self { seed, count: 0 }
    }
}

impl Ids for Minted {
    fn assertion_id(&mut self) -> String {
        self.count = self.count.saturating_add(1);
        let digest = Digest::of_parts(&[self.seed.as_bytes(), &self.count.to_be_bytes()]);
        format!("cli-{}", digest.to_hex())
    }
}
