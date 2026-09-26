#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! Filesystem backend for the content-addressed artifact store (journal spec §6.3, backlog E5-2,
//! DEC-107). The pure core (`mandate-journal`) defines references, the store traits, and the
//! checked read; this crate keeps the filesystem I/O out of it (ADR-0001 ES-02).
//!
//! Layout, part of the on-disk format: `<root>/sha256/<first two hex>/<64 hex>` holds the object
//! whose SHA-256 is that hex, read-only once written; `<root>/tmp/` holds writes in progress.
//!
//! A put writes a temporary file, flushes it to disk, and hard-links it into place, which fails
//! rather than replace an existing object. Readers therefore see a whole object or none, a crash
//! leaves at most a temporary file, and concurrent puts of the same bytes all succeed with one
//! object. Durability relies on POSIX directory `fsync`.

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use mandate_journal::{ArtifactError, ArtifactRef, ArtifactSource, ArtifactStore, get_artifact};

const OBJECTS_DIR: &str = "sha256";
const TEMP_DIR: &str = "tmp";

/// Distinguishes the temporary files of one process; the process ID distinguishes processes.
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
/// Temporary names tried per put before it is `Unavailable`.
const TEMP_NAME_ATTEMPTS: u32 = 64;

/// An artifact store rooted at a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsArtifactStore {
    root: PathBuf,
}

impl FsArtifactStore {
    /// Opens the store at `root`, creating it and its object and temporary directories if they are
    /// missing; `Unavailable` if they cannot be created.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, ArtifactError> {
        let root = root.into();
        fs::create_dir_all(root.join(OBJECTS_DIR)).map_err(unavailable)?;
        fs::create_dir_all(root.join(TEMP_DIR)).map_err(unavailable)?;
        Ok(Self { root })
    }

    /// Where the object for `reference` lives.
    pub fn object_path(&self, reference: &ArtifactRef) -> PathBuf {
        let hex = reference.digest().to_hex();
        let shard = hex.get(..2).unwrap_or_default();
        self.root.join(OBJECTS_DIR).join(shard).join(&hex)
    }

    /// Writes `bytes` to a new temporary file, flushed to disk and read-only, and returns its path.
    /// A name that is taken, such as one a crashed process with the same ID left behind, is
    /// skipped for the next one.
    fn write_temp(&self, reference: &ArtifactRef, bytes: &[u8]) -> Result<PathBuf, ArtifactError> {
        let (path, mut file) = (0..TEMP_NAME_ATTEMPTS)
            .find_map(|_| {
                let path = self.root.join(TEMP_DIR).join(format!(
                    "{}.{}.{}",
                    reference.digest().to_hex(),
                    std::process::id(),
                    NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
                ));
                let file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .ok()?;
                Some((path, file))
            })
            .ok_or(ArtifactError::Unavailable)?;
        file.write_all(bytes).map_err(unavailable)?;
        let mut permissions = file.metadata().map_err(unavailable)?.permissions();
        permissions.set_readonly(true);
        file.set_permissions(permissions).map_err(unavailable)?;
        file.sync_all().map_err(unavailable)?;
        Ok(path)
    }

    /// Links the temporary file into place. A link never replaces an existing name, so when it
    /// fails, for that or any other reason, the put succeeds only if a whole object that
    /// re-hashes is already there.
    fn publish(&self, reference: &ArtifactRef, temp: &Path) -> Result<(), ArtifactError> {
        let target = self.object_path(reference);
        let shard = target.parent().ok_or(ArtifactError::Unavailable)?;
        fs::create_dir_all(shard).map_err(unavailable)?;
        match fs::hard_link(temp, &target) {
            Ok(()) => File::open(shard)
                .and_then(|dir| dir.sync_all())
                .map_err(unavailable),
            Err(_) => match get_artifact(self, reference) {
                Ok(_) => Ok(()),
                Err(ArtifactError::Missing) => Err(ArtifactError::Unavailable),
                Err(e) => Err(e),
            },
        }
    }
}

impl ArtifactSource for FsArtifactStore {
    fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        fs::read(self.object_path(reference)).map_err(|e| match e.kind() {
            ErrorKind::NotFound => ArtifactError::Missing,
            _ => ArtifactError::Unavailable,
        })
    }
}

/// A temporary file that cannot be removed after the link is left behind: it is never read, and
/// the put's outcome is the link's.
impl ArtifactStore for FsArtifactStore {
    fn put_artifact(&mut self, bytes: &[u8]) -> Result<ArtifactRef, ArtifactError> {
        let reference = ArtifactRef::of(bytes);
        let temp = self.write_temp(&reference, bytes)?;
        let published = self.publish(&reference, &temp);
        fs::remove_file(&temp).ok();
        published.map(|()| reference)
    }
}

/// Every I/O failure other than a missing object is `Unavailable`: safe to retry.
fn unavailable(_: std::io::Error) -> ArtifactError {
    ArtifactError::Unavailable
}
