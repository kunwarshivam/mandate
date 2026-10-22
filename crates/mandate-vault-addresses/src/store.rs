//! The two handles and the values they take and return (DEC-2120 items 2 to 4). Every secret
//! member is a `secrecy` type, so a derived `Debug` prints it redacted.

use secrecy::{SecretBox, SecretString};

use crate::AddressError;

/// A workspace id, the name of its directory under `addresses/`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorkspaceId(String);

/// An address: the `added` event's own id (journal spec §9.15, rule 210), a ULID.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AddressRef(String);

/// The user principal whose address it is (journal spec §9.15's `member`), a ULID.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MemberId(String);

impl WorkspaceId {
    /// 1 to 64 characters of ASCII letters, digits, `_`, and `-`, so it names exactly one
    /// directory and never `.`, `..`, or a path.
    pub fn parse(_text: &str) -> Result<Self, AddressError> {
        Err(AddressError::Unimplemented { story: "E8-14" })
    }
}

impl AddressRef {
    /// A canonical ULID: 26 characters of upper-case Crockford base32, the first at most `7`.
    pub fn parse(_text: &str) -> Result<Self, AddressError> {
        Err(AddressError::Unimplemented { story: "E8-14" })
    }
}

impl MemberId {
    /// A canonical ULID, as [`AddressRef::parse`] reads one.
    pub fn parse(_text: &str) -> Result<Self, AddressError> {
        Err(AddressError::Unimplemented { story: "E8-14" })
    }
}

/// An entry's write count: `1` on its first write, one more on every write after.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u64);

/// A UTC instant in nanoseconds since the Unix epoch. The caller passes the time; the store reads
/// no clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnixNanos(pub u64);

/// The subscription's keys as unpadded base64url (workspace API §5.7).
#[derive(Debug)]
pub struct PushKeys {
    pub p256dh: SecretString,
    pub auth: SecretString,
}

/// What a `PUT` writes (DEC-2120 item 2). The store adds the endpoint hash, `version`, and
/// `written_at` itself.
#[derive(Debug)]
pub struct NewEntry {
    pub member: MemberId,
    pub endpoint: SecretString,
    pub keys: PushKeys,
    pub key_id: SecretString,
    pub request_digest: SecretBox<[u8; 32]>,
}

/// What the dispatcher reads for one send.
#[derive(Debug)]
pub struct SendEntry {
    pub endpoint: SecretString,
    pub keys: PushKeys,
    pub key_id: SecretString,
}

/// The answer to [`AddressWriter::put_if_absent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Put {
    /// No entry was under the `address_ref`; this one was written at `version` 1.
    Written(Version),
    /// The same request's entry was already there; nothing was written.
    AlreadyHeld(Version),
}

/// One of a member's entries that holds an endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub address_ref: AddressRef,
    pub version: Version,
}

/// An entry as the sweep sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub address_ref: AddressRef,
    pub version: Version,
    pub written_at: UnixNanos,
}

/// The answer to [`AddressWriter::compare_and_delete`] and [`AddressWriter::erase`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deletion {
    Deleted,
    /// A later write bumped the version; the entry stays.
    Stale,
    Absent,
}

/// The workspace API's handle (DEC-2120 item 7). Every operation names its workspace.
#[derive(Debug)]
pub struct AddressWriter {
    _sealed: (),
}

impl AddressWriter {
    /// Writes a new entry at `version` 1. The same member and request digest already there is
    /// [`Put::AlreadyHeld`]; another digest or member is [`AddressError::Conflict`].
    pub fn put_if_absent(
        &self,
        _workspace: &WorkspaceId,
        _address_ref: &AddressRef,
        _entry: &NewEntry,
        _now: UnixNanos,
    ) -> Result<Put, AddressError> {
        Err(AddressError::Unimplemented { story: "E8-14" })
    }

    /// Writes the same entry again, bumping `version` and `written_at`, and recreates it if a
    /// sweep deleted it; another digest or member is [`AddressError::Conflict`].
    pub fn put_again(
        &self,
        _workspace: &WorkspaceId,
        _address_ref: &AddressRef,
        _entry: &NewEntry,
        _now: UnixNanos,
    ) -> Result<Version, AddressError> {
        Err(AddressError::Unimplemented { story: "E8-14" })
    }

    /// The member's entries whose keyed endpoint hash matches `endpoint`, sorted by
    /// `address_ref`. Never another member's (DEC-2120 item 4).
    pub fn entries_holding(
        &self,
        _workspace: &WorkspaceId,
        _member: &MemberId,
        _endpoint: &SecretString,
    ) -> Result<Vec<Held>, AddressError> {
        Err(AddressError::Unimplemented { story: "E8-14" })
    }

    /// Deletes the entry only while its version is `version`.
    pub fn compare_and_delete(
        &self,
        _workspace: &WorkspaceId,
        _address_ref: &AddressRef,
        _version: Version,
    ) -> Result<Deletion, AddressError> {
        Err(AddressError::Unimplemented { story: "E8-14" })
    }

    /// Every entry whose file was last modified before `cutoff` and whose `written_at` is before
    /// `cutoff` or later than `now`, sorted by `address_ref`: the sweep's candidates, read from
    /// the headers and the files' modification times alone. It also unlinks every `.tmp-` file
    /// last modified before `cutoff`. It lists and judges nothing: the sweep erases a candidate
    /// only on DEC-2120 item 9's positive evidence, with [`Self::compare_and_delete`].
    pub fn inert_older_than(
        &self,
        _workspace: &WorkspaceId,
        _cutoff: UnixNanos,
        _now: UnixNanos,
    ) -> Result<Vec<Written>, AddressError> {
        Err(AddressError::Unimplemented { story: "E8-14" })
    }

    /// Deletes the entry whatever its version, for the redaction or erasure its event's
    /// `pii_refs` drive (journal spec §6.4): [`Deletion::Deleted`] or [`Deletion::Absent`].
    pub fn erase(
        &self,
        _workspace: &WorkspaceId,
        _address_ref: &AddressRef,
    ) -> Result<Deletion, AddressError> {
        Err(AddressError::Unimplemented { story: "E8-14" })
    }

    /// Removes the workspace's directory and everything in it, for workspace deletion.
    pub fn purge_workspace(&self, _workspace: &WorkspaceId) -> Result<(), AddressError> {
        Err(AddressError::Unimplemented { story: "E8-14" })
    }
}

/// The dispatcher's handle: one read for one send, and nothing else (DEC-2120 item 7). It has no
/// write, delete, erase, purge, or list method, and no conversion into an [`AddressWriter`].
#[derive(Debug)]
pub struct AddressReader {
    _sealed: (),
}

impl AddressReader {
    /// The entry under `address_ref`, or `None` when there is none or its header's member is not
    /// `recipient`: another member's entry reads exactly as a missing one. The member is compared
    /// before the body is opened, so a header edited to name the recipient fails the seal.
    pub fn entry(
        &self,
        _workspace: &WorkspaceId,
        _recipient: &MemberId,
        _address_ref: &AddressRef,
    ) -> Result<Option<SendEntry>, AddressError> {
        Err(AddressError::Unimplemented { story: "E8-14" })
    }
}
