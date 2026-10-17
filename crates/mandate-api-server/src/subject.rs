//! The subject link (§3.11 item 7, DEC-978 item 7): which user principal an identity-provider
//! subject signs in as, read from a root-owned file of digests at each sign-in and refresh.
//!
//! Each line is `sha256:<64 lowercase hex> <principal ULID>`. The digest is SHA-256 over the
//! canonical JSON of `{"issuer", "subject"}`, so no raw subject is stored and a subject of another
//! issuer never maps. A file that is not a regular file, is writable by group or others, is owned
//! by neither the process's user nor root, holds a malformed line, or names one digest twice is
//! refused whole: no line of it is read.

use std::path::Path;

use mandate_identity::PrincipalId;

/// Why the subject link refused; each is the one 401 to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkRefusal {
    /// The file is absent, unreadable, or not a regular file.
    Unreadable,
    /// The file is writable by group or others, or owned by another user.
    Exposed,
    /// A line is not `sha256:<hex> <ULID>`.
    Malformed,
    /// Two lines name the same digest.
    Duplicate,
    /// No line names this subject.
    NotLinked,
}

/// `sha256:<hex>` of the canonical `{"issuer": issuer, "subject": subject}`.
pub fn digest(issuer: &str, subject: &str) -> String {
    let _ = (issuer, subject);
    String::new()
}

/// The principal `issuer`'s `subject` signs in as, read from the file at `path` now.
///
/// # Errors
/// [`LinkRefusal`], in the order its variants are listed.
pub fn principal(path: &Path, issuer: &str, subject: &str) -> Result<PrincipalId, LinkRefusal> {
    principal_owned(path, issuer, subject, [0, 0])
}

/// [`principal`] with the file's allowed owners given: the process's user and root.
///
/// # Errors
/// [`LinkRefusal`], in the order its variants are listed.
pub fn principal_owned(
    path: &Path,
    issuer: &str,
    subject: &str,
    owners: [u32; 2],
) -> Result<PrincipalId, LinkRefusal> {
    let _ = (path, issuer, subject, owners);
    Err(LinkRefusal::Unreadable)
}
