//! `mandate step-up confirm` ([DEC-2104](../../../docs/project/decisions/DEC-2104.md) item 2,
//! [DEC-2106](../../../docs/project/decisions/DEC-2106.md); journal spec §9.29): the owner confirms,
//! on the host, the step-up challenge the workspace API issued for a paper grant made in the web
//! app. The command reads the approval from the journal, never from the browser, and appends one
//! `StepUpConfirmed` to the workspace control stream, written by the principal. It grants nothing:
//! the browser still submits the grant, and only the API's `cli_confirm` verifier reads the record.
//!
//! Every check is local and refuses with a code, appending nothing, in DEC-2106 item 2's order. A
//! re-run of a command that did commit finds it (DEC-2106 item 3, DEC-290).
//!
//! `mandate step-up confirm-connect` ([DEC-2125](../../../docs/project/decisions/DEC-2125.md) item
//! 3; journal spec v0.82 §9.29) confirms, the same way, the challenge the API issued for a paper
//! OAuth connect's start. The command computes the connect's action object itself, from the
//! workspace and the current disclosure document and version, never from the browser; it stores the
//! object and appends one `StepUpConfirmed` of kind `connection` naming its digest.

use std::io::Write;

use clap::{Args, Subcommand};
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{Digest, Value, is_ulid};
use mandate_connections::manager::{DISCLOSURE_DOCUMENT, DISCLOSURE_VERSION, oauth_connect_object};
use mandate_identity::StepUpActionKind;
use mandate_journal::{ArtifactStore, Environment, StreamId};

use crate::approvals::{self, Shown};
use crate::control::{
    Confirmation, ControlError, ControlJournal, Decision, Now, Owner, Repeat, Shape, Submitted,
    commit_choice_at, control_stream, decide, envelope, same_environment, seconds, text,
};
use crate::inbox::show_lines;
use crate::postgres::{JournalArgs, PgControlJournal};

/// The record the command appends (journal spec §9.29).
pub const EVENT_TYPE: &str = "StepUpConfirmed";

#[derive(Debug, Subcommand)]
pub enum StepUpCommand {
    /// Confirm, in paper, the step-up challenge the web app shows for an approval's grant, with
    /// the code it shows.
    Confirm(ConfirmArgs),
    /// Confirm, in paper, the step-up challenge the web app shows for an OAuth connect's start,
    /// with the code it shows (DEC-2125).
    ConfirmConnect(ConfirmConnectArgs),
}

#[derive(Debug, Args)]
pub struct ConfirmConnectArgs {
    /// The workspace the connect is for, whose control stream the confirmation is appended to.
    #[arg(long, value_name = "ID")]
    pub workspace: String,
    /// The confirming principal's ULID, as the workspace API writes its `actor.id`.
    #[arg(long, value_name = "ULID")]
    pub principal: String,
    /// The challenge the web app shows.
    #[arg(long, value_name = "ULID")]
    pub challenge: String,
    /// The code the web app shows, `XXXX-XXXX`.
    #[arg(long)]
    pub code: String,
    #[command(flatten)]
    pub target: JournalArgs,
}

/// One connect confirmation the owner asks for: the flags' values, as typed. It names no action
/// digest and no disclosure version: the command computes both (DEC-2125 item 3).
#[derive(Debug, Clone, Copy)]
pub struct ConnectConfirming<'a> {
    pub workspace: &'a str,
    pub principal: &'a str,
    pub challenge: &'a str,
    pub code: &'a str,
}

impl<'a> From<&'a ConfirmConnectArgs> for ConnectConfirming<'a> {
    fn from(args: &'a ConfirmConnectArgs) -> Self {
        Self {
            workspace: &args.workspace,
            principal: &args.principal,
            challenge: &args.challenge,
            code: &args.code,
        }
    }
}

/// A connect confirmation whose flags passed [`check_connect`], before any journal is opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedConnect {
    owner: Owner,
    challenge: String,
    code: String,
}

/// What [`confirm_connect`] found: the connect's action digest, and the confirmation it appended
/// or, on a re-run, the one it had already appended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectConfirmed {
    pub action_digest: String,
    pub submitted: Submitted,
    pub earlier: bool,
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

/// How many characters the code has: 40 bits of Crockford base 32 (DEC-2104 item 1).
const CODE_LEN: usize = 8;

/// Crockford's base-32 digits, upper case (journal spec §9.29's `confirm_code`).
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// How many times [`confirm`] reads the rows again after the control stream moved past the head it
/// checked them at, before it gives up.
const ATTEMPTS: u32 = 8;

fn refused(reason: &'static str) -> ControlError {
    ControlError::Refused { reason }
}

/// The code as `StepUpConfirmed` carries it (DEC-2104 item 2, DEC-2106 item 2.4).
///
/// # Errors
/// [`ControlError::Refused`] with `code_invalid`.
pub fn normalize(code: &str) -> Result<String, ControlError> {
    let bare = match code.split_once('-') {
        None => code.to_owned(),
        Some((head, tail)) if head.len() == 4 => format!("{head}{tail}"),
        Some(_) => return Err(refused("code_invalid")),
    };
    let normal: String = bare
        .chars()
        .map(|c| match c.to_ascii_uppercase() {
            'I' | 'L' => '1',
            'O' => '0',
            c => c,
        })
        .collect();
    if normal.len() == CODE_LEN && normal.bytes().all(|b| CROCKFORD.contains(&b)) {
        Ok(normal)
    } else {
        Err(refused("code_invalid"))
    }
}

/// DEC-2106 items 2.1 to 2.4, which read no journal. The owner is the principal, in `paper`.
///
/// # Errors
/// [`ControlError::Refused`] with `owner_workspace_invalid`, `principal_invalid`,
/// `challenge_invalid`, or `code_invalid`, in that order.
pub fn check(confirming: &Confirming<'_>) -> Result<Checked, ControlError> {
    let owner = Owner {
        workspace: confirming.workspace.to_owned(),
        user: confirming.principal.to_owned(),
        environment: Environment::Paper,
    };
    if control_stream(&owner).is_err() {
        return Err(refused("owner_workspace_invalid"));
    }
    if !is_ulid(confirming.principal) {
        return Err(refused("principal_invalid"));
    }
    if !is_ulid(confirming.challenge) {
        return Err(refused("challenge_invalid"));
    }
    let code = normalize(confirming.code)?;
    Ok(Checked {
        owner,
        agent: confirming.agent.to_owned(),
        approval: confirming.approval.to_owned(),
        challenge: confirming.challenge.to_owned(),
        code,
    })
}

/// DEC-2106 items 2.1 to 2.4 for a connect: the owner is the principal, in `paper`.
///
/// # Errors
/// [`ControlError::Refused`] with `owner_workspace_invalid`, `principal_invalid`,
/// `challenge_invalid`, or `code_invalid`, in that order.
pub fn check_connect(confirming: &ConnectConfirming<'_>) -> Result<CheckedConnect, ControlError> {
    let checked = check(&Confirming {
        workspace: confirming.workspace,
        principal: confirming.principal,
        agent: "",
        approval: "",
        challenge: confirming.challenge,
        code: confirming.code,
    })?;
    Ok(CheckedConnect {
        owner: checked.owner,
        challenge: checked.challenge,
        code: checked.code,
    })
}

/// The connect's action object for `checked`'s workspace at the current disclosure version, stored
/// in `store`, then one `StepUpConfirmed` of kind `connection` naming its digest appended at `now`,
/// or the one a run that committed appended (DEC-2125 item 3, DEC-2106 item 3).
///
/// # Errors
/// [`ControlError::Refused`] with `workspace_environment` for a workspace that is not paper,
/// `workspace_not_open` for one whose control stream was never opened, or `challenge_confirmed`
/// for a challenge already confirmed otherwise, having stored and appended nothing;
/// [`ControlError::Journal`] when the journal or the store cannot be read or written.
pub fn confirm_connect(
    journal: &mut dyn ControlJournal,
    store: &mut dyn ArtifactStore,
    checked: &CheckedConnect,
    now: Now,
) -> Result<ConnectConfirmed, ControlError> {
    let stream = control_stream(&checked.owner)?;
    let object = oauth_connect_object(
        &checked.owner.workspace,
        DISCLOSURE_DOCUMENT,
        DISCLOSURE_VERSION,
    )
    .map_err(|e| ControlError::Journal(format!("the connect's action object: {e}")))?;
    let action_digest = format!("sha256:{}", Digest::of(&object));
    let ours = Ours {
        challenge: &checked.challenge,
        action_digest: &action_digest,
        principal: &checked.owner.user,
        code: &checked.code,
    };
    for _ in 0..ATTEMPTS {
        let head = journal.head(&stream)?.seq;
        same_environment(journal, &stream, &checked.owner)?;
        opened(journal, &stream)?;
        if let Some(submitted) = earlier(journal, &stream, &ours)? {
            return Ok(ConnectConfirmed {
                action_digest,
                submitted,
                earlier: true,
            });
        }
        store
            .put_artifact(&object)
            .map_err(|e| ControlError::Journal(format!("the artifact store: {}", e.code())))?;
        let key = vec![
            ("action_digest", text(&action_digest)),
            ("action_kind", text(StepUpActionKind::Connection.code())),
            ("challenge", text(&checked.challenge)),
            ("code", text(&checked.code)),
            ("confirmed_at", seconds(now.at.secs())?),
            ("principal", text(&checked.owner.user)),
        ];
        let owner = &checked.owner;
        let decided = match decide(
            journal,
            owner,
            EVENT_TYPE,
            key,
            Confirmation::Fixed(false),
            Repeat::AlwaysCommits,
        )? {
            Decision::Fresh(decided) => decided,
            Decision::Committed(submitted) => {
                return Ok(ConnectConfirmed {
                    action_digest,
                    submitted,
                    earlier: true,
                });
            }
        };
        let shape = Shape {
            schema_version: 1,
            artifact_refs: vec![action_digest.clone()],
            config_refs: Vec::new(),
        };
        if let Some(submitted) = commit_choice_at(journal, owner, decided, shape, now, head)? {
            return Ok(ConnectConfirmed {
                action_digest,
                submitted,
                earlier: false,
            });
        }
    }
    Err(ControlError::Journal(format!(
        "the control stream moved on each of {ATTEMPTS} reads; nothing was appended"
    )))
}

/// Runs [`confirm_connect`] and prints `confirmed <challenge> as event <id> at seq <n> for
/// <digest>`, or `already confirmed …` on a re-run.
///
/// # Errors
/// As [`confirm_connect`]. A refusal prints nothing.
pub fn run_confirm_connect(
    checked: &CheckedConnect,
    journal: &mut dyn ControlJournal,
    store: &mut dyn ArtifactStore,
    now: Now,
    report: &mut dyn Write,
) -> anyhow::Result<ConnectConfirmed> {
    let confirmed = confirm_connect(journal, store, checked, now)?;
    let word = if confirmed.earlier {
        "already confirmed"
    } else {
        "confirmed"
    };
    writeln!(
        report,
        "{word} {} as event {} at seq {} for {}",
        checked.challenge,
        confirmed.submitted.event_id,
        confirmed.submitted.seq,
        confirmed.action_digest
    )?;
    Ok(confirmed)
}

/// The binary's `confirm-connect`: [`check_connect`] first, then the journal and the store
/// `--journal` and `--store` name, opened only once it passes, then [`run_confirm_connect`].
///
/// # Errors
/// As [`check_connect`] and [`run_confirm_connect`], or the store's failure to open.
pub fn run_connect_command(
    args: &ConfirmConnectArgs,
    now: Now,
    report: &mut dyn Write,
) -> anyhow::Result<ConnectConfirmed> {
    let checked = check_connect(&args.into())?;
    let mut journal = PgControlJournal::open(&args.target)?;
    let mut store = FsArtifactStore::open(&args.target.store)
        .map_err(|e| ControlError::Journal(format!("the artifact store: {}", e.code())))?;
    run_confirm_connect(&checked, &mut journal, &mut store, now, report)
}

/// DEC-2106 items 2.5 to 2.9 against `journal`, then one `StepUpConfirmed` appended at `now`, at the
/// head the rows were read at (item 4), or the one a run that committed appended (item 3).
///
/// # Errors
/// [`ControlError::Refused`] with DEC-2106 item 2's code, having appended nothing;
/// [`ControlError::Journal`] when the journal cannot be read or refuses the record.
pub fn confirm(
    journal: &mut dyn ControlJournal,
    checked: &Checked,
    now: Now,
) -> Result<Confirmed, ControlError> {
    let stream = control_stream(&checked.owner)?;
    for _ in 0..ATTEMPTS {
        let head = journal.head(&stream)?.seq;
        same_environment(journal, &stream, &checked.owner)?;
        let shown = approvals::show(journal, &checked.owner, &checked.agent, &checked.approval)?;
        if shown.live {
            return Err(refused("paper_only"));
        }
        if now.secs >= shown.deadline_s {
            return Err(refused("late"));
        }
        let ours = Ours {
            challenge: &checked.challenge,
            action_digest: &shown.content_hash,
            principal: &checked.owner.user,
            code: &checked.code,
        };
        if let Some(submitted) = earlier(journal, &stream, &ours)? {
            return Ok(Confirmed {
                shown,
                submitted,
                earlier: true,
            });
        }
        let key = vec![
            ("action_digest", text(&shown.content_hash)),
            ("action_kind", text(StepUpActionKind::Approve.code())),
            ("challenge", text(&checked.challenge)),
            ("code", text(&checked.code)),
            ("confirmed_at", seconds(now.at.secs())?),
            ("principal", text(&checked.owner.user)),
        ];
        let confirmation = Confirmation::Fixed(false);
        let owner = &checked.owner;
        let decided = match decide(
            journal,
            owner,
            EVENT_TYPE,
            key,
            confirmation,
            Repeat::AlwaysCommits,
        )? {
            Decision::Fresh(decided) => decided,
            Decision::Committed(submitted) => {
                return Ok(Confirmed {
                    shown,
                    submitted,
                    earlier: true,
                });
            }
        };
        let shape = Shape {
            schema_version: 1,
            artifact_refs: vec![shown.content_hash.clone()],
            config_refs: Vec::new(),
        };
        if let Some(submitted) = commit_choice_at(journal, owner, decided, shape, now, head)? {
            return Ok(Confirmed {
                shown,
                submitted,
                earlier: false,
            });
        }
    }
    Err(ControlError::Journal(format!(
        "the control stream moved on each of {ATTEMPTS} reads; nothing was appended"
    )))
}

/// A workspace whose control stream holds no `StreamOpened` first is `workspace_not_open`, as
/// `mandate workspace found` reads it: the journal refuses any other first record, so the connect's
/// object is never stored for an append that cannot commit.
fn opened(journal: &dyn ControlJournal, stream: &StreamId) -> Result<(), ControlError> {
    match journal.rows(stream)?.first() {
        Some(first) if first.event_type == "StreamOpened" => Ok(()),
        _ => Err(refused("workspace_not_open")),
    }
}

/// The members a confirmation of this command's carries, so a re-run can tell its own record from
/// another's.
struct Ours<'a> {
    challenge: &'a str,
    action_digest: &'a str,
    principal: &'a str,
    code: &'a str,
}

/// Rule 243 and DEC-2106 item 3: the confirmation of `ours`' challenge already on `stream`, if it
/// is this command's, or `challenge_confirmed` for any other.
fn earlier(
    journal: &dyn ControlJournal,
    stream: &StreamId,
    ours: &Ours<'_>,
) -> Result<Option<Submitted>, ControlError> {
    for row in journal.rows(stream)? {
        if row.event_type != EVENT_TYPE {
            continue;
        }
        let event = envelope(&row)?;
        let member = |name: &str| {
            event
                .get("payload")
                .and_then(|p| p.get(name))
                .and_then(Value::as_str)
        };
        if member("challenge") != Some(ours.challenge) {
            continue;
        }
        let same = member("action_digest") == Some(ours.action_digest)
            && member("principal") == Some(ours.principal)
            && member("code") == Some(ours.code);
        if !same {
            return Err(refused("challenge_confirmed"));
        }
        return Ok(Some(Submitted {
            event_id: row.event_id,
            seq: row.seq,
        }));
    }
    Ok(None)
}

/// What the command prints (DEC-2106 item 5): the approval as `approvals show` prints it, then
/// `confirmed` or `already confirmed <challenge> as event <id> at seq <n> for <content_hash>`.
///
/// # Errors
/// As [`crate::inbox::show_lines`].
pub fn confirmed_lines(
    checked: &Checked,
    confirmed: &Confirmed,
) -> Result<Vec<String>, ControlError> {
    let mut lines = show_lines(&confirmed.shown)?;
    let word = if confirmed.earlier {
        "already confirmed"
    } else {
        "confirmed"
    };
    lines.push(format!(
        "{word} {} as event {} at seq {} for {}",
        checked.challenge,
        confirmed.submitted.event_id,
        confirmed.submitted.seq,
        confirmed.shown.content_hash
    ));
    Ok(lines)
}

/// Runs [`confirm`] against `journal` and prints [`confirmed_lines`]. The binary runs [`check`]
/// first and opens the journal `--journal` names only once it passes.
///
/// # Errors
/// As [`confirm`]. A refusal prints nothing.
pub fn run_confirm(
    checked: &Checked,
    journal: &mut dyn ControlJournal,
    now: Now,
    report: &mut dyn Write,
) -> anyhow::Result<Confirmed> {
    let confirmed = confirm(journal, checked, now)?;
    for line in confirmed_lines(checked, &confirmed)? {
        writeln!(report, "{line}")?;
    }
    Ok(confirmed)
}
