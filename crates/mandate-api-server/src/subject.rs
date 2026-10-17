//! The subject link (§3.11 item 7, DEC-978 item 7): which user principal an identity-provider
//! subject signs in as, read from a root-owned file of digests at each sign-in and refresh.
//!
//! Each line is `sha256:<64 lowercase hex> <principal ULID>`. The digest is SHA-256 over the
//! canonical JSON of `{"issuer", "subject"}`, so no raw subject is stored and a subject of another
//! issuer never maps. A file that is not a regular file, is writable by group or others, is owned
//! by neither the process's user nor root, holds a malformed line, or names one digest twice is
//! refused whole: no line of it is read.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use mandate_canon::{Digest, Key, Object, Value, to_canonical};
use mandate_identity::PrincipalId;
use rustix::process::geteuid;

/// Root's user id, the one owner besides the process's own that may hold the file.
const ROOT: u32 = 0;

/// The write bits of a file's group and others; the file must have neither.
const GROUP_OR_OTHERS_WRITE: u32 = 0o022;

/// A SHA-256 digest's length in hex digits.
const DIGEST_HEX: usize = 64;

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
    let mut object = Object::new();
    for (key, value) in [("issuer", issuer), ("subject", subject)] {
        if let Ok(key) = Key::new(key) {
            object.insert(key, Value::Str(value.to_owned()));
        }
    }
    format!(
        "sha256:{}",
        Digest::of(&to_canonical(&Value::Object(object)))
    )
}

/// The one file the server reads subject links from (DEC-1023): fixed, never a setting.
pub const SUBJECT_LINKS: &str = "/etc/mandate/subject-links";

/// The principal `issuer`'s `subject` signs in as, read from the file at `path` now.
/// A path that does not itself name the file opened, a symlink included, is refused as exposed
/// (DEC-1023), so only the file at the path is ever read.
///
/// # Errors
/// [`LinkRefusal`], in the order its variants are listed.
pub fn principal(path: &Path, issuer: &str, subject: &str) -> Result<PrincipalId, LinkRefusal> {
    principal_owned(path, issuer, subject, [geteuid().as_raw(), ROOT])
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
    let metadata = fs::metadata(path).map_err(|_| LinkRefusal::Unreadable)?;
    if !metadata.is_file() {
        return Err(LinkRefusal::Unreadable);
    }
    let mut file = File::open(path).map_err(|_| LinkRefusal::Unreadable)?;
    let opened = file.metadata().map_err(|_| LinkRefusal::Unreadable)?;
    if !opened.is_file() {
        return Err(LinkRefusal::Unreadable);
    }
    let at_path = fs::symlink_metadata(path).map_err(|_| LinkRefusal::Unreadable)?;
    if (at_path.dev(), at_path.ino()) != (opened.dev(), opened.ino()) {
        return Err(LinkRefusal::Exposed);
    }
    if opened.permissions().mode() & GROUP_OR_OTHERS_WRITE != 0 || !owners.contains(&opened.uid()) {
        return Err(LinkRefusal::Exposed);
    }
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|_| LinkRefusal::Malformed)?;
    let wanted = digest(issuer, subject);
    let body = text.strip_suffix('\n').unwrap_or(&text);
    let mut seen = BTreeSet::new();
    let mut found = None;
    for line in body.split('\n').filter(|_| !text.is_empty()) {
        let (digest, ulid) = line.split_once(' ').ok_or(LinkRefusal::Malformed)?;
        let hex = digest
            .strip_prefix("sha256:")
            .ok_or(LinkRefusal::Malformed)?;
        let lowercase = hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if hex.len() != DIGEST_HEX || !lowercase {
            return Err(LinkRefusal::Malformed);
        }
        let principal = PrincipalId::from_ulid_text(ulid).map_err(|_| LinkRefusal::Malformed)?;
        if !seen.insert(digest) {
            return Err(LinkRefusal::Duplicate);
        }
        if digest == wanted {
            found = Some(principal);
        }
    }
    found.ok_or(LinkRefusal::NotLinked)
}
