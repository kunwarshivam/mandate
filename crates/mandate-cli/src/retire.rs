//! The owner's Stop of an agent no process ran (journal spec v0.66 §9.21, rule 246;
//! [DEC-2039](../../../docs/project/decisions/DEC-2039.md)).

use mandate_journal::ArtifactSource;

use crate::agent::{self, Command};
use crate::control::{ControlError, ControlJournal, Ids, Now, Owner, Submitted};

/// The owner's Stop of `agent`, with the code `typed`.
///
/// # Errors
/// [`crate::agent::command`]'s.
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
    let _ = store;
    agent::command(
        journal,
        ids,
        owner,
        agent,
        &Command::Stop { release },
        typed,
        now,
    )
}
