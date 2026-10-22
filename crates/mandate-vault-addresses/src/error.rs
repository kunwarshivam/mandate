//! Why an operation failed. Every variant carries opaque ids only: never an endpoint, a key, a
//! digest, a hash, or a path's contents (DEC-2120 item 5).

use std::io::ErrorKind;

use crate::store::AddressRef;

/// Why an entry could not be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryFault {
    /// The name is not a regular file, or the open file is not the inode its name's `lstat`
    /// gave, has more than one link, or is not owned by `owlhead_api`, group `owlhead_notify`,
    /// mode exactly `0640` (DEC-2120 item 2).
    File,
    /// The cleartext header does not parse.
    Header,
    /// The body did not open: the key, or the workspace, `address_ref`, or header bound in its
    /// associated data, differs (DEC-2120 item 3).
    Seal,
    /// The header names a key this process no longer holds: after a rotation's window, the
    /// dispatcher records `address_missing` (DEC-2120 item 6).
    RetiredKey,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum AddressError {
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    #[error("a vault key is set in the environment; keys arrive only as systemd credentials")]
    KeyInEnvironment,
    #[error(
        "the address key credential is missing, not a regular file, not 32 bytes, or readable by group or other"
    )]
    KeyCredential,
    #[error("the token key is among this process's credentials")]
    TokenKeyPresent,
    #[error("the pending key is among the dispatcher's credentials")]
    PendingKeyPresent,
    #[error("the {dir} directory has the wrong owner, group, or mode, or is a symlink")]
    Layout { dir: &'static str },
    #[error("the id is not a valid {kind}")]
    InvalidId { kind: &'static str },
    #[error("an address entry could not be opened")]
    Entry {
        address_ref: AddressRef,
        fault: EntryFault,
    },
    #[error("the address entry is held for another request or member")]
    Conflict { address_ref: AddressRef },
    #[error("the secure random source failed")]
    Random,
    #[error("the address store could not be read or written")]
    Io { kind: ErrorKind },
}
