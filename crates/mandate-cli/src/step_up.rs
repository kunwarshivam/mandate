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
use mandate_canon::{Value, is_ulid};
use mandate_journal::{Environment, StreamId};

use crate::approvals::{self, Shown};
use crate::control::{
    Confirmation, ControlError, ControlJournal, Decision, Now, Owner, Repeat, Shape, Submitted,
    commit_choice_at, control_stream, decide, envelope, same_environment, seconds, text,
};
use crate::inbox::show_lines;
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
        if let Some(submitted) = earlier(journal, &stream, checked, &shown)? {
            return Ok(Confirmed {
                shown,
                submitted,
                earlier: true,
            });
        }
        let key = vec![
            ("action_digest", text(&shown.content_hash)),
            ("action_kind", text("approve")),
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

/// Rule 243 and DEC-2106 item 3: the confirmation of `checked`'s challenge already on `stream`, if
/// it is this command's, or `challenge_confirmed` for any other.
fn earlier(
    journal: &dyn ControlJournal,
    stream: &StreamId,
    checked: &Checked,
    shown: &Shown,
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
        if member("challenge") != Some(checked.challenge.as_str()) {
            continue;
        }
        let ours = member("action_digest") == Some(shown.content_hash.as_str())
            && member("principal") == Some(checked.owner.user.as_str())
            && member("code") == Some(checked.code.as_str());
        if !ours {
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
