//! The service's artifact writer (DEC-2279 items 1, 2, 4 and 5): it stores the research runtime's
//! artifacts in the research store, a store of their own that no other writer uses, create-only,
//! under the reference it computes from the bytes it received, for its own workspace only, and
//! never past a fixed cap.

use std::path::Path;

use mandate_artifacts_fs::FsArtifactStore;
use mandate_gateway::wire::{PutReply, PutRequest};
use mandate_journal::ArtifactError;

/// The most bytes the research store may hold before a put that would add more is refused
/// `store_full` (DEC-2279 item 4): a constant of the service, not a value. The chart's research
/// store is to be at least twice this (DEC-2279 Consequences).
pub const STORE_CAP: u64 = 512 * 1024 * 1024;

/// The research store, the one workspace whose puts it takes, and the bytes the store holds,
/// counted once at open and then grown only by the objects this writer newly links.
#[derive(Debug)]
#[expect(
    dead_code,
    reason = "the tests land before the implementation (DEC-77); the stubs read no field"
)]
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
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn capped(root: &Path, workspace: &str, cap: u64) -> Result<Self, ArtifactError> {
        let _ = (root, workspace, cap);
        todo!()
    }

    /// The bytes the store holds as this writer counts them.
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn stored(&self) -> u64 {
        todo!()
    }

    /// Stores `put`'s bytes and answers their reference, or refuses: `foreign_workspace` for
    /// another workspace's put, `store_full` when a new object would take the store past the cap,
    /// and `store_unavailable` when the store did not take it or the object already under the
    /// reference no longer re-hashes. Bytes already stored answer their reference however full the
    /// store is, and count nothing again. After a put the store did not take, the count is taken
    /// again from the store, so an object linked before the put failed is still counted.
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn put(&mut self, put: &PutRequest) -> PutReply {
        let _ = put;
        todo!()
    }
}
