//! The content-addressed store (§3.11 item 2): read by the agent reads (DEC-1020), and written by
//! the version routes only by adding an object under its own digest, never replacing one
//! ([DEC-2100](../../../docs/project/decisions/DEC-2100.md) item 8).

use std::sync::Mutex;

use mandate_journal::{ArtifactError, ArtifactRef, ArtifactSource, ArtifactStore};

/// A store the server reads and adds objects to, shared across requests.
pub trait ObjectStore: ArtifactSource + Send + Sync {
    /// Stores `bytes` under their own digest; bytes already stored change nothing.
    ///
    /// # Errors
    /// The store's own [`ArtifactError`], and [`ArtifactError::Unavailable`] for a store a
    /// panicked writer left unusable.
    fn put(&self, bytes: &[u8]) -> Result<ArtifactRef, ArtifactError>;
}

/// An [`ArtifactStore`] behind a lock, so one store serves every request.
#[derive(Debug)]
pub struct Objects<S>(Mutex<S>);

impl<S: ArtifactStore + Send> Objects<S> {
    pub fn new(store: S) -> Self {
        Self(Mutex::new(store))
    }
}

impl<S: ArtifactStore + Send> ArtifactSource for Objects<S> {
    fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        let store = self.0.lock().map_err(|_| ArtifactError::Unavailable)?;
        store.read_artifact(reference)
    }
}

impl<S: ArtifactStore + Send> ObjectStore for Objects<S> {
    fn put(&self, bytes: &[u8]) -> Result<ArtifactRef, ArtifactError> {
        let mut store = self.0.lock().map_err(|_| ArtifactError::Unavailable)?;
        store.put_artifact(bytes)
    }
}
