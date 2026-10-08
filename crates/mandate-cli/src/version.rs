//! `mandate version create` and `mandate version confirm` (first paper trade brief, D2a; DEC-530).
//! Each stores the mandate document and its mandate spec §10 record in the artifact store, then
//! commits exactly one control-stream event naming them (journal spec §9.2, DEC-155 item 5). Both
//! are paper only, and a confirmation takes a `cli_confirm` code bound to the version shown. What
//! `version confirm` prints before the code is typed comes with the commands, D2c.

use mandate_journal::ArtifactStore;

use crate::control::{ControlError, ControlJournal, Ids, Now, Owner, Submitted};

/// Stores `document`, a mandate, in canonical form under its version (mandate spec §9.1), and its
/// record, then commits one `MandateVersionCreated`, every envelope path `user_entered` (DEC-530).
///
/// # Errors
/// [`ControlError::Refused`] with a DEC-530 code, having written nothing; [`ControlError::Journal`].
pub fn create(
    journal: &mut dyn ControlJournal,
    store: &mut dyn ArtifactStore,
    owner: &Owner,
    document: &[u8],
    now: Now,
) -> Result<Submitted, ControlError> {
    let _ = (journal, store, owner);
    let _ = (document, now);
    Err(ControlError::Unimplemented { story: "E10-16" })
}

/// Confirms `version` when `code` is the one shown for it (DEC-530 item 4): stores the record with
/// fresh `cli_confirm` evidence, then commits one `MandateConfirmed` naming every envelope path.
///
/// # Errors
/// As [`create`].
pub fn confirm(
    journal: &mut dyn ControlJournal,
    store: &mut dyn ArtifactStore,
    ids: &mut dyn Ids,
    owner: &Owner,
    version: &str,
    code: &str,
    now: Now,
) -> Result<Submitted, ControlError> {
    let _ = (journal, store, ids, owner);
    let _ = (version, code, now);
    Err(ControlError::Unimplemented { story: "E10-16" })
}
