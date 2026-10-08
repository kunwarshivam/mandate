//! `mandate config register` and `mandate model register` (first paper trade brief, D1; DEC-526).
//! Each stores the configuration object it names in the artifact store, then commits exactly one
//! `ConfigSnapshotRegistered` naming it on the workspace control stream (journal spec §9.2,
//! DEC-155 item 5). Both are paper only.

use mandate_journal::ArtifactStore;

use crate::control::{ControlError, ControlJournal, Now, Owner, Submitted};

/// The kinds `config register` registers: §9.2's version-1 kinds the paper run reads, and
/// version 2's `policy_set` and `model_registry` (DEC-484 item 4). `mandate_version` and
/// `model_version` are other commands'.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ConfigKind {
    FeeConfig,
    TradingCalendar,
    InstrumentSnapshot,
    RuleSet,
    PolicySet,
    ModelRegistry,
}

/// Stores `object`, a JSON object, in canonical form and commits one `ConfigSnapshotRegistered`
/// of `kind` naming its hash: schema version 2 for `policy_set` and `model_registry`, 1 for the
/// rest. An instrument snapshot must be exactly DEC-523's object, and a version-2 object's own
/// `kind` must be `kind`. A re-run of a committed registration commits nothing (DEC-290).
///
/// # Errors
/// [`ControlError::Refused`] with `paper_only` for an owner not in paper,
/// `config_object_invalid` for bytes that are not a JSON object, `instrument_snapshot_invalid`
/// for a snapshot outside DEC-523, or `config_kind_mismatch`, each before anything is stored or
/// committed; [`ControlError::Journal`] as `commit` reports.
pub fn register(
    journal: &mut dyn ControlJournal,
    store: &mut dyn ArtifactStore,
    owner: &Owner,
    kind: ConfigKind,
    object: &[u8],
    now: Now,
) -> Result<Submitted, ControlError> {
    let _ = (journal, store, owner);
    let _ = (kind, object, now);
    Err(ControlError::Unimplemented { story: "E10-16" })
}

/// Stores the content object `mandate_modelhost::content` computes for `model_id` at
/// `model_version`, and commits one `ConfigSnapshotRegistered` of kind `model_version` with its
/// hash, its sorted parameter names, and `admits_instruments: false` (DEC-504 item 3). No caller
/// supplies a hash: the host computes it from the code it runs.
///
/// # Errors
/// [`ControlError::Refused`] with `paper_only`, or `model_unknown` for a model the host has
/// no content for, before anything is stored or committed; [`ControlError::Journal`] as `commit`
/// reports.
pub fn register_model(
    journal: &mut dyn ControlJournal,
    store: &mut dyn ArtifactStore,
    owner: &Owner,
    model_id: &str,
    model_version: &str,
    now: Now,
) -> Result<Submitted, ControlError> {
    let _ = (journal, store, owner);
    let _ = (model_id, model_version, now);
    Err(ControlError::Unimplemented { story: "E10-16" })
}
