//! `mandate agent deploy` (first paper trade brief, D2b; DEC-530 item 9). It deploys an agent with
//! the workspace's latest confirmed mandate version: it stores the deployment record, with fresh
//! `cli_confirm` evidence, and commits exactly one `AgentDeployed` (journal spec §9.2, DEC-155
//! item 5). Paper only.

use mandate_journal::ArtifactStore;

use crate::control::{ControlError, ControlJournal, Ids, Now, Owner, Submitted};

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
    let _ = (journal, store, ids, owner);
    let _ = (agent, version, code, now);
    Err(ControlError::Unimplemented { story: "E10-16" })
}
