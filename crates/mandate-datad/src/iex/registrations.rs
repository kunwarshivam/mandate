//! The workspace's control stream as the IEX source keeps it between reads (DEC-2283 item 3): the
//! rows read so far, extended with only the rows after the last one at each read, so a read costs
//! the new rows rather than the whole stream, and each snapshot object read once, its bytes kept
//! and checked again against its reference at every use.

use std::cell::RefCell;
use std::collections::BTreeMap;

use mandate_journal::{
    ArtifactError, ArtifactRef, ArtifactSource, StoredEvent, TrustedStart, check_artifact,
};

use crate::iex::instruments::{Registered, registered};

/// The control stream's rows read so far, and the snapshot objects read for them.
#[derive(Default)]
pub struct Registrations {
    rows: Vec<StoredEvent>,
    objects: RefCell<BTreeMap<String, Vec<u8>>>,
}

impl Registrations {
    /// Where the next read starts: after the last row read, chained to its hash.
    pub fn next_start(&self) -> TrustedStart {
        self.rows
            .last()
            .map_or(TrustedStart::GENESIS, |last| TrustedStart {
                from_seq: last.seq.saturating_add(1),
                prev_hash: last.hash,
            })
    }

    /// Adds the rows a read from [`Self::next_start`] returned.
    pub fn extend(&mut self, rows: Vec<StoredEvent>) {
        self.rows.extend(rows);
    }

    /// The registered instruments, each snapshot object read from `artifacts` the first time it is
    /// needed and from memory after.
    pub fn registered(&self, artifacts: &dyn ArtifactSource) -> Registered {
        let cached = Cached {
            inner: artifacts,
            objects: &self.objects,
        };
        registered(&self.rows, &cached)
    }
}

/// `inner`, remembering each object it hands out once its bytes match their reference, so a
/// corrupt read is refused and read again at the next use rather than kept.
struct Cached<'a> {
    inner: &'a dyn ArtifactSource,
    objects: &'a RefCell<BTreeMap<String, Vec<u8>>>,
}

impl ArtifactSource for Cached<'_> {
    fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        let key = reference.to_string();
        if let Some(bytes) = self.objects.borrow().get(&key) {
            return Ok(bytes.clone());
        }
        let bytes = self.inner.read_artifact(reference)?;
        check_artifact(reference, &bytes)?;
        self.objects.borrow_mut().insert(key, bytes.clone());
        Ok(bytes)
    }
}
