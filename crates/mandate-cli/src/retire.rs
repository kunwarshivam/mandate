//! The owner's Stop of an agent no process ran (journal spec v0.66 §9.1, §9.21, rule 246;
//! [DEC-2039](../../../docs/project/decisions/DEC-2039.md); E21-33 slice 5b).
//!
//! An owner's Stop reaches an agent whose runtime ran as an `OwnerCommandIssued`, which the
//! runtime judges. An agent whose runtime never ran has no process to judge it: a version-1
//! deployment, a version-2 one rule 201 refuses, or one the manager has not yet started. With the
//! right code, [`stop`] first claims that agent's stream: it appends the stream's `StreamOpened` as
//! the owner at head 0, under the epoch the head holds, so it fences no runtime and commits only on
//! a stream nothing opened. A stream its own claim opened, an earlier Stop that did not finish, is
//! resumed. On a claimed stream nothing ran, so the agent's net dollar loss is 0, and [`stop`]
//! writes its `AgentStopped`: reason `owner_stop`, `loss_added` 0, and the connection its deployment
//! names (version 2) or its stored document names (version 1), at the control head it read; a
//! record appended in between is decided again, and the claim stands. In every other case, a
//! stream a runtime opened or took first, an agent never deployed, a version-1 deployment whose
//! document cannot be read for its connection, or a wrong code, the Stop goes as
//! [`crate::agent::command`] sends it, as before: it is never refused for this (`AGENTS.md`
//! rule 13).

use mandate_journal::{AppendOutcome, ArtifactSource, StoredEvent};
use mandate_time::new_york_date_and_hour;

use crate::agent::{self, Command};
use crate::control::{
    Confirmation, ControlError, ControlJournal, Decision, Ids, Now, Owner, Repeat, Shape,
    Submitted, agent_stream, claim_draft, commit_choice_at, control_stream, decide, envelope,
    same_environment, text,
};
use crate::version::{Row, rows, stored};

const DEPLOYED: &str = "AgentDeployed";
const STOPPED: &str = "AgentStopped";
const OPENED: &str = "StreamOpened";

/// How many times [`stop`] reads the streams and decides again when the control stream moves
/// before it gives up.
const DECISIONS: u32 = 3;

/// What one reading of the streams decides.
enum Retirement {
    /// The `AgentStopped` was written.
    Retired(Submitted),
    /// The agent may have run: its runtime opened its stream, or took the stream before the claim.
    /// The Stop goes as an `OwnerCommandIssued`, which its runtime judges.
    Ran,
    /// No deployment names the agent. The Stop goes as an `OwnerCommandIssued`, as before.
    NeverDeployed,
    /// A version-1 deployment whose document cannot be read for its connection. The Stop goes as
    /// an `OwnerCommandIssued` rather than guess one.
    DocumentUnread,
    /// The control stream moved past the head the decision read.
    Moved,
}

/// The owner's Stop of `agent`, with the code `typed`: the `AgentStopped` of an agent no process
/// ran, or the `OwnerCommandIssued` of any other.
///
/// Nothing in the retirement refuses the Stop (DEC-2039 item 6, `AGENTS.md` rule 13): a claim the
/// journal refuses or cannot take, a read or append that fails, or a control stream that keeps
/// moving under the retirement sends it as [`crate::agent::command`] does, as before. After a
/// failure the control stream is read again first: an `AgentStopped` whose append did commit is
/// returned, and no command is sent beside it.
///
/// # Errors
/// [`crate::agent::command`]'s alone: a wrong or missing code refused as `step_up_missing` with
/// nothing committed, or a journal that cannot take the `OwnerCommandIssued` either.
#[allow(
    clippy::too_many_arguments,
    reason = "the journal, store, ids and owner every gesture takes, and the Stop's four inputs"
)]
pub fn stop(
    journal: &mut dyn ControlJournal,
    store: &dyn ArtifactSource,
    ids: &mut dyn Ids,
    owner: &Owner,
    agent: &str,
    release: bool,
    typed: Option<&str>,
    now: Now,
) -> Result<Submitted, ControlError> {
    let command = Command::Stop { release };
    let confirmed = agent::code(journal, owner, agent, &command)?
        .is_some_and(|code| typed == Some(code.as_str()));
    if confirmed {
        for _ in 0..DECISIONS {
            if let Some(submitted) = retired(journal, owner, agent) {
                return Ok(submitted);
            }
            match retire(journal, store, owner, agent, now) {
                Ok(Retirement::Retired(submitted)) => return Ok(submitted),
                Ok(Retirement::Ran | Retirement::NeverDeployed | Retirement::DocumentUnread) => {
                    break;
                }
                Err(_) => match retired(journal, owner, agent) {
                    Some(submitted) => return Ok(submitted),
                    None => break,
                },
                Ok(Retirement::Moved) => {}
            }
        }
        if let Some(submitted) = retired(journal, owner, agent) {
            return Ok(submitted);
        }
    }
    agent::command(journal, ids, owner, agent, &command, typed, now)
}

/// The `AgentStopped` that retires `agent`, read afresh: before each decision, so a Stop of an
/// agent already retired writes nothing more, and after a retirement that failed or the last
/// decision, since its append may have committed although no answer settled it, or its answer
/// reported a moved head, and a Stop has one effect. A stop is final
/// (§9.21), so any `AgentStopped` naming the agent retires it. `None` when none is stored, or the
/// control stream cannot be read.
fn retired(journal: &dyn ControlJournal, owner: &Owner, agent: &str) -> Option<Submitted> {
    let rows = rows(journal, owner).ok()?;
    latest(&rows, STOPPED, agent).map(Row::submitted)
}

/// The latest `event_type` row naming `agent`.
fn latest<'a>(rows: &'a [Row], event_type: &str, agent: &str) -> Option<&'a Row> {
    rows.iter()
        .rev()
        .find(|r| r.event_type == event_type && r.member("agent_id") == Some(agent))
}

/// Whether `first`, the first record of an agent's stream, is a claim: a `StreamOpened` no `agent`
/// actor wrote, told by the actor's kind alone (journal spec §9.1).
fn is_claim(first: &StoredEvent) -> Result<bool, ControlError> {
    let body = envelope(first)?;
    let kind = body
        .get("actor")
        .and_then(|actor| actor.get("kind"))
        .and_then(|kind| kind.as_str());
    Ok(first.event_type == OPENED && kind != Some("agent"))
}

/// Whether the agent's stream is claimed, by this Stop or an earlier one. `false` when a runtime
/// opened or took it first, or the journal stored no claim: a conflicting event under the claim's
/// id, or a claim its checks refuse, is no claim, and the Stop goes by command.
fn claim(
    journal: &mut dyn ControlJournal,
    owner: &Owner,
    agent: &str,
    now: Now,
) -> Result<bool, ControlError> {
    let stream = agent_stream(owner, agent)?;
    if let Some(first) = journal.rows(&stream)?.first() {
        return is_claim(first);
    }
    let epoch = journal.head(&stream)?.writer_epoch;
    let (stream, _, bytes) = claim_draft(owner, agent, now)?;
    match journal.append(&stream, 0, epoch, now.at, &[bytes.as_slice()])? {
        AppendOutcome::Committed(_) | AppendOutcome::AlreadyCommitted(_) => Ok(true),
        AppendOutcome::Fenced { .. } => Ok(false),
        AppendOutcome::HeadMismatch { .. }
        | AppendOutcome::Ambiguous
        | AppendOutcome::Unavailable => match journal.rows(&stream)?.first() {
            Some(first) => is_claim(first),
            None => Ok(false),
        },
        AppendOutcome::IdempotencyConflict { .. } | AppendOutcome::Invalid { .. } => Ok(false),
    }
}

/// One reading of the control stream, the claim, and one decision.
fn retire(
    journal: &mut dyn ControlJournal,
    store: &dyn ArtifactSource,
    owner: &Owner,
    agent: &str,
    now: Now,
) -> Result<Retirement, ControlError> {
    let control = control_stream(owner)?;
    same_environment(journal, &control, owner)?;
    let rows = rows(journal, owner)?;
    let Some(deployed) = latest(&rows, DEPLOYED, agent) else {
        return Ok(Retirement::NeverDeployed);
    };
    let version = deployed
        .member("mandate_version")
        .ok_or_else(|| ControlError::Journal("a deployment names no mandate version".into()))?;
    let connection = match deployed.member("connection_id") {
        Some(connection) => connection.to_owned(),
        None => match stored(store, version) {
            Ok((_, mandate)) => mandate.connection_id.as_str().to_owned(),
            Err(_) => return Ok(Retirement::DocumentUnread),
        },
    };
    if !claim(journal, owner, agent, now)? {
        return Ok(Retirement::Ran);
    }
    let (retired_on, _) = new_york_date_and_hour(now.at)
        .map_err(|e| ControlError::Journal(format!("the retirement date: {e:?}")))?;
    let key = vec![
        ("agent_id", text(agent)),
        ("connection_id", text(&connection)),
        ("loss_added", text("0")),
        ("reason", text("owner_stop")),
        ("retired_on", text(&retired_on.to_string())),
    ];
    let Decision::Fresh(decided) = decide(
        journal,
        owner,
        STOPPED,
        key,
        Confirmation::Fixed(false),
        Repeat::AlwaysCommits,
    )?
    else {
        return Ok(Retirement::Moved);
    };
    let shape = Shape {
        schema_version: 1,
        artifact_refs: Vec::new(),
        config_refs: vec![("mandate_version", text(version))],
    };
    let head = rows.last().map_or(0, |r| r.seq);
    Ok(
        match commit_choice_at(journal, owner, decided, shape, now, head)? {
            Some(submitted) => Retirement::Retired(submitted),
            None => Retirement::Moved,
        },
    )
}
