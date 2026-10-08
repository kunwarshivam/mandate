//! `mandate agent deploy` (first paper trade brief, D2b; DEC-530 item 9). It deploys an agent with
//! the workspace's latest confirmed mandate version: it stores the deployment record, with fresh
//! `cli_confirm` evidence, and commits exactly one `AgentDeployed` (journal spec §9.2, DEC-155
//! item 5). Paper only.

use mandate_canon::Value;
use mandate_journal::{ArtifactSource, ArtifactStore};
use mandate_spec::Mandate;

use crate::control::{
    ControlError, ControlJournal, Ids, Now, Owner, Submitted, agent_stream, code_of, object,
    seconds, text,
};
use crate::version::{
    CONFIRMED, CREATED, Row, check_instruments, check_rules, commit, latest, paper_only, refused,
    rows, stored,
};

const DEPLOYED: &str = "AgentDeployed";

/// The agent's active deployment: its latest `AgentDeployed`, when no `AgentStopped` for it is
/// later.
fn active<'a>(rows: &'a [Row], agent: &str) -> Option<&'a Row> {
    let latest = rows.iter().rev().find(|r| {
        matches!(r.event_type.as_str(), DEPLOYED | "AgentStopped")
            && r.member("agent_id") == Some(agent)
    })?;
    (latest.event_type == DEPLOYED).then_some(latest)
}

/// The code that deploys `agent` with `version` (DEC-530 item 4).
///
/// # Errors
/// [`ControlError::Journal`] for an object the canonical form cannot hold.
pub(crate) fn deploy_code(agent: &str, version: &str) -> Result<String, ControlError> {
    let gesture = object(vec![
        ("agent_id", text(agent)),
        ("gesture", text("agent_deploy")),
        ("mandate_version", text(version)),
    ])?;
    Ok(code_of(&gesture))
}

/// What deploying reads, and the agent's active deployment of `version` when it is one.
pub(crate) struct Checked {
    rows: Vec<Row>,
    document: Value,
    mandate: Mandate,
    pub(crate) earlier: Option<Submitted>,
}

/// DEC-530 item 9's checks up to `agent_active`, the code's only when one is given.
///
/// # Errors
/// As [`deploy`].
pub(crate) fn deploy_checks(
    journal: &dyn ControlJournal,
    store: &dyn ArtifactSource,
    owner: &Owner,
    (agent, version): (&str, &str),
    code: Option<&str>,
) -> Result<Checked, ControlError> {
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
    if let Some(code) = code
        && code != deploy_code(agent, version)?
    {
        return Err(refused("code_mismatch"));
    }
    let earlier = match active(&rows, agent) {
        Some(deployed) if deployed.member("mandate_version") == Some(version) => {
            Some(deployed.submitted())
        }
        Some(_) => return Err(refused("agent_active")),
        None => None,
    };
    Ok(Checked {
        rows,
        document,
        mandate,
        earlier,
    })
}

/// The rest of item 9's checks: the V-rules for `agent` and the instruments; the warnings.
///
/// # Errors
/// As [`deploy`].
pub(crate) fn rule_checks(
    checked: &Checked,
    store: &dyn ArtifactSource,
    agent: &str,
    now: Now,
) -> Result<Vec<&'static str>, ControlError> {
    let (rows, mandate) = (&checked.rows, &checked.mandate);
    let warnings = check_rules(rows, store, (mandate, agent), None, now)?;
    check_instruments(rows, store, &checked.document)?;
    Ok(warnings)
}

/// Deploys `agent` with `version` when `code` is the one shown for both (DEC-530 item 4): stores
/// the record, then commits one `AgentDeployed` with `config_refs.mandate_version`. A re-run while
/// the agent's active deployment is `version` returns it and commits nothing.
///
/// # Errors
/// [`ControlError::Refused`] with a DEC-530 item 9 code, having written nothing;
/// [`ControlError::Journal`] for a store or journal that fails.
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
    let checked = deploy_checks(&*journal, &*store, owner, (agent, version), Some(code))?;
    if let Some(earlier) = checked.earlier {
        return Ok(earlier);
    }
    rule_checks(&checked, &*store, agent, now)?;
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
    let event = (DEPLOYED, vec![("agent_id", text(agent))], true);
    let bound = vec![("mandate_version", text(version))];
    commit(journal, store, owner, event, (version, &record, bound), now)
}
