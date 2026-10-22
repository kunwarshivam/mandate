//! The re-seal tool's handle (DEC-2120 item 6). It runs as `owlhead_api`, the one user that can
//! write entries, and opens under the writer's checks. It counts entries by the key that sealed
//! them and re-seals those under the previous key under the current one, each under its
//! workspace's lock and only while the entry's version is still the one it read. It never erases:
//! an entry it cannot open is reported by opaque ids and counts as remaining until an operator
//! erases it with [`crate::AddressWriter::erase`].
//!
//! Neither the API's handle nor the dispatcher's can re-seal:
//!
//! ```no_run
//! use mandate_vault_addresses::maintenance::{AddressMaintenance, Resealed};
//! use mandate_vault_addresses::{AddressError, UnixNanos};
//!
//! fn reseal(store: &AddressMaintenance, now: UnixNanos) -> Result<Resealed, AddressError> {
//!     store.reseal(now)
//! }
//! ```
//!
//! ```compile_fail
//! use mandate_vault_addresses::maintenance::Resealed;
//! use mandate_vault_addresses::{AddressError, AddressReader, UnixNanos};
//!
//! fn reseal(store: &AddressReader, now: UnixNanos) -> Result<Resealed, AddressError> {
//!     store.reseal(now)
//! }
//! ```

use crate::startup::Inputs;
use crate::{AddressError, AddressRef, UnixNanos, WorkspaceId};

/// An entry the tool could not open: a file check, a header, or a seal that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unopenable {
    pub workspace: WorkspaceId,
    pub address_ref: AddressRef,
}

/// How many entries each key holds, read from the headers, and the entries whose header could not
/// be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyCounts {
    pub under_current: u64,
    pub under_previous: u64,
    pub unopenable: Vec<Unopenable>,
}

/// What one re-seal pass did. The previous key retires only when a later
/// [`AddressMaintenance::count_by_key_version`] reports none under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resealed {
    pub resealed: u64,
    pub unopenable: Vec<Unopenable>,
}

/// The re-seal tool's handle.
#[derive(Debug)]
pub struct AddressMaintenance {
    _sealed: (),
}

/// Opens the re-seal tool's handle under [`crate::startup::open_writer`]'s checks.
pub fn open_maintenance(_inputs: &Inputs<'_>) -> Result<AddressMaintenance, AddressError> {
    Err(AddressError::Unimplemented { story: "E8-14" })
}

impl AddressMaintenance {
    /// Counts every workspace's entries by the key their header names, without opening a body.
    pub fn count_by_key_version(&self) -> Result<KeyCounts, AddressError> {
        Err(AddressError::Unimplemented { story: "E8-14" })
    }

    /// Re-seals every entry under the previous key under the current one, its `endpoint_hash`
    /// recomputed under the current key's subkey, bumping its version as every write does.
    pub fn reseal(&self, _now: UnixNanos) -> Result<Resealed, AddressError> {
        Err(AddressError::Unimplemented { story: "E8-14" })
    }
}
