//! `mandate agent deploy` (first paper trade brief, D2b; DEC-530 item 9). It deploys an agent with
//! the workspace's latest confirmed mandate version: it stores the deployment record, with fresh
//! `cli_confirm` evidence, and commits exactly one `AgentDeployed` (journal spec §9.2, DEC-155
//! item 5). In paper, or in live on the owner's recorded Robinhood connection (DEC-925). The
//! deployment is version 2, naming the document's connection, exactly when journal spec v0.54
//! §9.24 rule 201 holds on the control stream, and version 1 otherwise (DEC-1171); an agent id is
//! deployed once (DEC-1173 item 1). Rule 200 holds by construction: the `connection_id` it names
//! is the one parsed from the stored document it deploys. A version whose universe may hold a
//! second instrument is refused `second_instrument`, after the checks on the agent and before the
//! V-rules: the executor's risk fold reads one instrument (DEC-2809 item 1, DEC-2804 item 8).

use mandate_journal::ArtifactStore;

use crate::control::{
    ControlError, ControlJournal, Ids, Now, Owner, Submitted, agent_stream, code_of, object,
    seconds, text,
};
use crate::version::{
    CONFIRMED, CREATED, Row, admitted, check_instruments, check_rules, commit_at, latest,
    paper_or_live, refused, rows, stored,
};

const DEPLOYED: &str = "AgentDeployed";
const STOPPED: &str = "AgentStopped";
const ESTABLISHED: &str = "ConnectionEstablished";
const REVOKED: &str = "ConnectionRevoked";

/// The agent's active deployment: its latest `AgentDeployed`, when no `AgentStopped` for it is
/// later.
pub(crate) fn active<'a>(rows: &'a [Row], agent: &str) -> Option<&'a Row> {
    last_of(rows, agent).filter(|latest| latest.event_type == DEPLOYED)
}

/// The agent's latest `AgentDeployed` or `AgentStopped`.
fn last_of<'a>(rows: &'a [Row], agent: &str) -> Option<&'a Row> {
    rows.iter().rev().find(|r| {
        matches!(r.event_type.as_str(), DEPLOYED | STOPPED) && r.member("agent_id") == Some(agent)
    })
}

/// Journal spec v0.54 §9.24 rule 201 for a deployment on `connection`: a version-2
/// `ConnectionEstablished` of it is on the control stream, and its latest establishment or
/// revocation is an establishment.
fn established_v2(rows: &[Row], connection: &str) -> bool {
    let of = |r: &&Row| r.member("connection_id") == Some(connection);
    let bound = rows
        .iter()
        .filter(of)
        .any(|r| r.event_type == ESTABLISHED && r.schema_version == 2);
    let latest = rows
        .iter()
        .rev()
        .filter(of)
        .find(|r| matches!(r.event_type.as_str(), ESTABLISHED | REVOKED));
    bound && latest.is_some_and(|r| r.event_type == ESTABLISHED)
}

/// How many times [`deploy`] reads the rows and decides again when the stream moves under a
/// version-2 deployment before it gives up.
const DECISIONS: u32 = 3;

/// Deploys `agent` with `version` when `code` is the one shown for both (DEC-530 item 4): stores
/// the record, then commits one `AgentDeployed` with `config_refs.mandate_version`. A re-run while
/// the agent's active deployment is `version` returns it and commits nothing. A version-2
/// deployment is appended at the head its rows were read at, so rule 201 still holds at the append:
/// when another writer moves the stream first (a revocation, say), the rows are read and the
/// deployment decided again.
///
/// # Errors
/// [`ControlError::Refused`] with a DEC-530 item 9 code, having written nothing;
/// [`ControlError::Journal`] for a store or journal that fails, or a stream that keeps moving.
#[allow(
    clippy::too_many_arguments,
    reason = "the journal, store, ids and owner every gesture takes, and the gesture's four inputs"
)]
pub fn deploy(
    journal: &mut dyn ControlJournal,
    store: &mut dyn ArtifactStore,
    ids: &mut dyn Ids,
    owner: &Owner,
    agent: &str,
    version: &str,
    code: &str,
    now: Now,
) -> Result<Submitted, ControlError> {
    for _ in 0..DECISIONS {
        let gesture = (agent, version, code);
        if let Some(submitted) = decide_and_commit(journal, store, ids, owner, gesture, now)? {
            return Ok(submitted);
        }
    }
    Err(ControlError::Journal(
        "the control stream kept moving under the deployment; nothing was committed".into(),
    ))
}

/// One reading of the rows and one decision: `None` when a version-2 deployment's head moved.
fn decide_and_commit(
    journal: &mut dyn ControlJournal,
    store: &mut dyn ArtifactStore,
    ids: &mut dyn Ids,
    owner: &Owner,
    (agent, version, code): (&str, &str, &str),
    now: Now,
) -> Result<Option<Submitted>, ControlError> {
    paper_or_live(owner)?;
    agent_stream(owner, agent).map_err(|_| refused("agent_invalid"))?;
    let rows = rows(journal, owner)?;
    if latest(&rows, CREATED, version).is_none() {
        return Err(refused("version_unknown"));
    }
    let (document, mandate) = stored(store, version)?;
    admitted(journal, owner, &document)?;
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
    if code != code_of(&gesture) {
        return Err(refused("code_mismatch"));
    }
    match last_of(&rows, agent) {
        Some(deployed) if deployed.event_type == DEPLOYED => {
            if deployed.member("mandate_version") == Some(version) {
                return Ok(Some(deployed.submitted()));
            }
            return Err(refused("agent_active"));
        }
        Some(_) => return Err(refused("agent_stopped")),
        None => {}
    }
    if mandate.universe.max_instruments > 1 {
        return Err(refused("second_instrument"));
    }
    check_rules(&rows, store, (&mandate, agent), None, now)?;
    check_instruments(&rows, store, &document)?;
    let step_up = object(vec![
        ("assertion_id", text(&ids.assertion_id())),
        ("authenticated_at", seconds(now.secs)?),
        ("method", text("cli_confirm")),
    ])?;
    let record = object(vec![
        ("agent_id", text(agent)),
        ("kind", text("deployment_record")),
        ("mandate_version", text(version)),
        ("step_up", step_up),
        ("user", text(&owner.user)),
    ])?;
    let connection = mandate.connection_id.as_str();
    let mut members = vec![("agent_id", text(agent))];
    let (schema_version, pinned) = if established_v2(&rows, connection) {
        members.push(("connection_id", text(connection)));
        (2, rows.last().map(|r| r.seq))
    } else {
        (1, None)
    };
    let event = ((DEPLOYED, members, true), schema_version, pinned);
    let bound = vec![("mandate_version", text(version))];
    commit_at(journal, store, owner, event, (version, &record, bound), now)
}
