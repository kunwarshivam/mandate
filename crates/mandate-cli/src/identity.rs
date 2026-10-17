//! `mandate identity link` (lane ID1, DEC-992): writes one line of the workspace API server's
//! subject-link file ([workspace API spec](../../../docs/specs/workspace-api.md) §3.11 item 7,
//! DEC-978 item 7), which maps the digest of an identity provider's `{issuer, subject}` to the
//! user principal it signs in as.
//!
//! The subject is read from standard input and never written anywhere: not to the file, the
//! output, an error, or an argument list a process listing shows (identity spec §3.1, §3.3). Only
//! its digest, `sha256:` and the SHA-256 of the canonical `{"issuer","subject"}` object, reaches
//! the file. The file is replaced whole, by a rename, so the server never reads a partial one; it
//! is written owner-writable and readable by all (`0644`), never group- or world-writable, and a
//! file the server would refuse is never extended.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use mandate_canon::{Digest, Key, Object, Value, to_canonical};
use mandate_identity::PrincipalId;

use crate::control::ControlError;

#[derive(Debug, Subcommand)]
pub enum IdentityCommand {
    /// Link an identity provider's subject, read from standard input, to a user principal in the
    /// API server's subject-link file. A link already there commits nothing and succeeds.
    Link(LinkArgs),
}

/// There is no `--subject`: the subject comes from standard input, so no process listing or shell
/// history shows it.
#[derive(Debug, Args)]
pub struct LinkArgs {
    /// The identity provider's issuer, exactly as its tokens carry it in `iss`.
    #[arg(long, value_name = "URL")]
    pub issuer: String,
    /// The user principal the subject signs in as: an opaque ULID (identity spec §3.1).
    #[arg(long, value_name = "ULID")]
    pub member: String,
    /// The subject-link file the API server reads (`MANDATE_API_SUBJECT_LINKS`).
    #[arg(long, value_name = "PATH")]
    pub links: PathBuf,
}

/// What a link did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linked {
    /// The subject's digest, as the file holds it.
    pub digest: String,
    /// Whether this run wrote it; `false` when the same link was already there.
    pub written: bool,
}

/// The longest subject read from standard input, in bytes. A provider's subject is an opaque id,
/// far shorter.
const MAX_SUBJECT: u64 = 1024;

/// The mode the file is written with: owner read and write, everyone else read.
const FILE_MODE: u32 = 0o644;

/// The mode bits that make a file group- or world-writable, which the server refuses.
const OTHERS_WRITE: u32 = 0o022;

fn refused(reason: &'static str) -> ControlError {
    ControlError::Refused { reason }
}

/// The digest the file names a subject by: `sha256:` and the SHA-256, in lowercase hex, of the
/// canonical object `{"issuer": issuer, "subject": subject}` (journal spec §4's canonical form).
///
/// # Errors
/// `issuer_invalid` for an issuer that is empty or holds whitespace or a control character, and
/// `subject_invalid` for a subject that is empty or holds a control character.
pub fn subject_digest(issuer: &str, subject: &str) -> Result<String, ControlError> {
    if issuer.is_empty() || issuer.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(refused("issuer_invalid"));
    }
    if subject.is_empty() || subject.chars().any(char::is_control) {
        return Err(refused("subject_invalid"));
    }
    let mut object = Object::new();
    for (name, value) in [("issuer", issuer), ("subject", subject)] {
        let key = Key::new(name).map_err(|_| ControlError::Journal(format!("key {name}")))?;
        object.insert(key, Value::Str(value.to_owned()));
    }
    let digest = Digest::of(&to_canonical(&Value::Object(object)));
    Ok(format!("sha256:{}", digest.to_hex()))
}

/// The links a file's text holds, by digest.
///
/// # Errors
/// `links_malformed` for any line that is not a digest, one space, and a ULID, for a digest
/// listed twice, or for text that does not end its last line.
pub fn parse_links(text: &str) -> Result<BTreeMap<String, PrincipalId>, ControlError> {
    let mut links = BTreeMap::new();
    if !text.is_empty() && !text.ends_with('\n') {
        return Err(refused("links_malformed"));
    }
    for line in text.lines() {
        let (digest, member) = line
            .split_once(' ')
            .ok_or_else(|| refused("links_malformed"))?;
        let member = PrincipalId::from_ulid_text(member).map_err(|_| refused("links_malformed"))?;
        if !is_digest(digest) || links.insert(digest.to_owned(), member).is_some() {
            return Err(refused("links_malformed"));
        }
    }
    Ok(links)
}

/// `sha256:` and 64 lowercase hex digits.
fn is_digest(text: &str) -> bool {
    text.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// The file's text for `links`: one line a link, ascending by digest.
pub fn render_links(links: &BTreeMap<String, PrincipalId>) -> Result<String, ControlError> {
    let mut text = String::new();
    for (digest, member) in links {
        let ulid = member
            .to_ulid_text()
            .map_err(|_| refused("member_invalid"))?;
        text.push_str(digest);
        text.push(' ');
        text.push_str(&ulid);
        text.push('\n');
    }
    Ok(text)
}

/// `links` with `digest` linked to `member`: `None` when that link is already there.
///
/// # Errors
/// `subject_linked_elsewhere` when the digest is linked to another principal: a subject signs in
/// as one principal, and re-pointing it is not this command's.
pub fn with_link(
    mut links: BTreeMap<String, PrincipalId>,
    digest: &str,
    member: PrincipalId,
) -> Result<Option<BTreeMap<String, PrincipalId>>, ControlError> {
    match links.get(digest) {
        Some(linked) if *linked == member => Ok(None),
        Some(_) => Err(refused("subject_linked_elsewhere")),
        None => {
            links.insert(digest.to_owned(), member);
            Ok(Some(links))
        }
    }
}

/// Links the subject read from `subject` (standard input) as `args` say, and prints the digest and
/// the principal, never the subject.
///
/// # Errors
/// As [`subject_digest`] and [`link_file`], and `member_invalid` for a member that is not a ULID,
/// checked before standard input is read.
pub fn link(
    args: &LinkArgs,
    subject: &mut dyn Read,
    report: &mut dyn Write,
) -> Result<Linked, ControlError> {
    let member =
        PrincipalId::from_ulid_text(&args.member).map_err(|_| refused("member_invalid"))?;
    let mut read = String::new();
    subject
        .take(MAX_SUBJECT.saturating_add(1))
        .read_to_string(&mut read)
        .map_err(|_| refused("subject_invalid"))?;
    if u64::try_from(read.len()).map_or(true, |n| n > MAX_SUBJECT) {
        return Err(refused("subject_invalid"));
    }
    let line = read
        .strip_suffix('\n')
        .map(|s| s.strip_suffix('\r').unwrap_or(s))
        .unwrap_or(&read);
    let digest = subject_digest(&args.issuer, line)?;
    let written = link_file(&args.links, &digest, member)?;
    let verb = if written { "linked" } else { "already linked" };
    writeln!(
        report,
        "{verb} {digest} to member {} in {}",
        args.member,
        args.links.display()
    )
    .map_err(|e| ControlError::Journal(format!("the report: {e}")))?;
    Ok(Linked { digest, written })
}

/// Adds `digest`'s link to `member` to the file at `path`, creating it if absent: returns whether
/// it wrote. The new text is written to `<path>.writing`, created exclusively, so a second writer
/// at once is refused rather than lost, then synced and renamed over `path`.
///
/// # Errors
/// `links_busy` while `<path>.writing` exists; `links_unsafe` for a file that is not a regular file,
/// is group- or world-writable, or is owned by neither this process's user nor root;
/// `links_malformed` as [`parse_links`]; `subject_linked_elsewhere` as [`with_link`];
/// [`ControlError::Journal`] when the file cannot be read or written. Nothing is changed on any of
/// them.
pub fn link_file(path: &Path, digest: &str, member: PrincipalId) -> Result<bool, ControlError> {
    let mut staging = path.as_os_str().to_owned();
    staging.push(".writing");
    let staging = PathBuf::from(staging);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(FILE_MODE)
        .open(&staging)
        .map_err(|e| match e.kind() {
            io::ErrorKind::AlreadyExists => refused("links_busy"),
            _ => io_error("create", &e),
        })?;
    let written = staged(path, &staging, file, digest, member);
    if !matches!(written, Ok(true)) {
        fs::remove_file(&staging).ok();
    }
    written
}

/// The rest of [`link_file`], with the staging file created and open.
fn staged(
    path: &Path,
    staging: &Path,
    mut file: File,
    digest: &str,
    member: PrincipalId,
) -> Result<bool, ControlError> {
    let own = file.metadata().map_err(|e| io_error("inspect", &e))?.uid();
    let existing = match fs::symlink_metadata(path) {
        Ok(meta) => {
            let trusted = meta.file_type().is_file()
                && (meta.uid() == own || meta.uid() == 0)
                && meta.mode() & OTHERS_WRITE == 0;
            if !trusted {
                return Err(refused("links_unsafe"));
            }
            fs::read_to_string(path).map_err(|e| io_error("read", &e))?
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(io_error("inspect", &e)),
    };
    let Some(links) = with_link(parse_links(&existing)?, digest, member)? else {
        return Ok(false);
    };
    file.set_permissions(fs::Permissions::from_mode(FILE_MODE))
        .map_err(|e| io_error("chmod", &e))?;
    file.write_all(render_links(&links)?.as_bytes())
        .map_err(|e| io_error("write", &e))?;
    file.sync_all().map_err(|e| io_error("sync", &e))?;
    fs::rename(staging, path).map_err(|e| io_error("rename", &e))?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        File::open(dir)
            .and_then(|d| d.sync_all())
            .map_err(|e| io_error("sync the directory", &e))?;
    }
    Ok(true)
}

fn io_error(what: &str, error: &io::Error) -> ControlError {
    ControlError::Journal(format!("the subject-link file: {what}: {}", error.kind()))
}
