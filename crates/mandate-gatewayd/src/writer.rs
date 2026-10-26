//! The service's artifact writer (DEC-2279 items 1, 2, 4 and 5): it stores the research runtime's
//! artifacts in the research store, a store of their own that no other writer uses, create-only,
//! under the reference it computes from the bytes it received, for its own workspace only, and
//! never past a fixed cap.

use std::path::Path;

use mandate_artifacts_fs::FsArtifactStore;
use mandate_gateway::wire::{PutRefusal, PutReply, PutRequest};
use mandate_journal::{ArtifactError, ArtifactRef, get_artifact};

/// The most bytes the research store may hold before a put that would add more is refused
/// `store_full` (DEC-2279 item 4): a constant of the service, not a value. The chart's research
/// store is to be at least twice this (DEC-2279 Consequences).
pub const STORE_CAP: u64 = 512 * 1024 * 1024;

/// The research store, the one workspace whose puts it takes, and the bytes the store holds,
/// counted once at open and then grown only by the objects this writer newly links.
#[derive(Debug)]
pub struct ArtifactWriter {
    store: FsArtifactStore,
    workspace: String,
    stored: u64,
    cap: u64,
}

impl ArtifactWriter {
    /// The research store at `root`, already laid out, for `workspace`'s puts, under
    /// [`STORE_CAP`].
    ///
    /// # Errors
    /// `Unavailable` when `root` is not a laid-out store, or a file in it cannot be read.
    pub fn open(root: &Path, workspace: &str) -> Result<Self, ArtifactError> {
        Self::capped(root, workspace, STORE_CAP)
    }

    /// As [`ArtifactWriter::open`], under `cap` instead.
    ///
    /// # Errors
    /// As [`ArtifactWriter::open`].
    pub fn capped(root: &Path, workspace: &str, cap: u64) -> Result<Self, ArtifactError> {
        let store = FsArtifactStore::open_existing(root)?;
        let stored = store.stored_bytes()?;
        Ok(Self {
            store,
            workspace: workspace.to_owned(),
            stored,
            cap,
        })
    }

    /// The bytes the store holds as this writer counts them.
    pub fn stored(&self) -> u64 {
        self.stored
    }

    /// Stores `put`'s bytes and answers their reference, or refuses: `foreign_workspace` for
    /// another workspace's put, `store_full` when a new object would take the store past the cap,
    /// and `store_unavailable` when the store did not take it or the object already under the
    /// reference no longer re-hashes. Bytes already stored answer their reference however full the
    /// store is, and count nothing again. After a put the store did not take, the count is taken
    /// again from the store, so an object linked before the put failed is still counted.
    pub fn put(&mut self, put: &PutRequest) -> PutReply {
        if put.workspace_id != self.workspace {
            return Err(PutRefusal::ForeignWorkspace);
        }
        let reference = ArtifactRef::of(&put.bytes);
        let within = u64::try_from(put.bytes.len())
            .ok()
            .and_then(|length| self.stored.checked_add(length))
            .filter(|after| *after <= self.cap);
        let Some(after) = within else {
            return match get_artifact(&self.store, &reference) {
                Ok(_) => Ok(reference.digest()),
                Err(ArtifactError::Missing) => Err(PutRefusal::StoreFull),
                Err(_) => Err(PutRefusal::StoreUnavailable),
            };
        };
        match self.store.put_counted(&put.bytes) {
            Ok((_, linked)) => {
                if linked {
                    self.stored = after;
                }
                Ok(reference.digest())
            }
            Err(_) => {
                self.stored = self.store.stored_bytes().unwrap_or(after).max(self.stored);
                Err(PutRefusal::StoreUnavailable)
            }
        }
    }
}
