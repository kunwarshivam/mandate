//! The closes object the runtime's observation names (DEC-2385 item 5): the closes of the stored
//! series as the trust check admits them at the clock, in `mandate_bars::closes_object`'s bytes,
//! put in the artifact store. The runtime mounts the store read-only, so it can only confirm the
//! object is there before it journals `ObservationRecorded` with its digest; reading the closes
//! back through the same trust check is also this process's check that what it stored is trusted.

use std::path::Path;

use mandate_artifacts_fs::FsArtifactStore;
use mandate_journal::{ArtifactRef, ArtifactStore};
use mandate_time::UtcNanos;

use crate::series::SeriesError;

/// Puts the closes object of the trusted closes in `dataset` for `symbol` at `now`, for the
/// instrument `asset_id`, in the artifact store at `artifacts`, and returns its reference. A put of
/// bytes already stored is the same reference.
///
/// # Errors
/// [`SeriesError::Untrusted`] when the trust check refuses the dataset at `now`, and
/// [`SeriesError::Artifacts`] when the store cannot be opened or written.
pub fn put_closes(
    artifacts: &Path,
    dataset: &Path,
    symbol: &str,
    asset_id: &str,
    now: UtcNanos,
) -> Result<ArtifactRef, SeriesError> {
    let closes = mandate_bars::trusted_daily_closes(dataset, symbol, now)
        .map_err(|_| SeriesError::Untrusted)?;
    let bytes = mandate_bars::closes_object(&closes, asset_id).map_err(|_| SeriesError::Decimal)?;
    let mut store = FsArtifactStore::open(artifacts).map_err(|_| SeriesError::Artifacts)?;
    store
        .put_artifact(&bytes)
        .map_err(|_| SeriesError::Artifacts)
}
