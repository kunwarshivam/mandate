//! Credentials as files only their owner can read (DEC-2022): where `mandate vault provision`
//! leaves each identity's AppRole credentials, and how it reads the operator's token. Neither is
//! ever an argument, an environment variable, or printed.

use std::fs::{self, DirBuilder, File, OpenOptions, Permissions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::process;

use mandate_connections::record::ConnectionId;
use secrecy::zeroize::Zeroize;
use secrecy::{ExposeSecret, SecretString};

use crate::SourceError;
use crate::openbao::{RoleCredentials, secret_id_identities};
use crate::policy::{Identity, role_name};

/// The suffix of the file that holds an identity's role id, after its role name.
pub const ROLE_ID_SUFFIX: &str = ".role-id";
/// The suffix of the file that holds an identity's secret id, after its role name.
pub const SECRET_ID_SUFFIX: &str = ".secret-id";

/// The permission bits of group and others, none of which a credential's file or directory may
/// carry.
const OPEN_TO_OTHERS: u32 = 0o077;

/// The permission bits that let a user other than the owner add, remove, or rename a directory's
/// entries.
const WRITABLE_BY_OTHERS: u32 = 0o022;
/// The sticky bit: in a directory that carries it, only an entry's owner may remove or rename it.
const STICKY: u32 = 0o1000;
/// How many temporary names a write tries, each one it cannot make passed over, before it refuses.
const TEMP_ATTEMPTS: u32 = 8;

/// Writes each identity's credentials into `dir`, `<role>.role-id` and `<role>.secret-id`, each a
/// plain file of mode 0600 holding the value alone, replacing any earlier plain file of that name.
/// `dir` is made with mode 0700 when it cannot be read, which fails unless it is absent; an
/// existing one must be a directory, not a symbolic link, of mode 0700 exactly. The directory
/// holding `dir` must not let anyone else rename `dir` away: no group or other write bit unless it
/// is sticky. Every output name is checked before any is written, and one that is anything but a
/// plain file, a planted link included, refuses the whole write. Returns the names written,
/// sorted.
pub fn write_credentials(
    dir: &Path,
    credentials: &[RoleCredentials],
) -> Result<Vec<String>, SourceError> {
    let identities: Vec<Identity> = credentials
        .iter()
        .map(|credential| credential.identity.clone())
        .collect();
    if !check_for(dir, &identities)? {
        DirBuilder::new()
            .mode(0o700)
            .create(dir)
            .and_then(|()| fs::set_permissions(dir, Permissions::from_mode(0o700)))
            .map_err(|_| SourceError::CredentialFile)?;
    }
    let mut files = Vec::new();
    for credential in credentials {
        let role = role_name(&credential.identity);
        for (suffix, value) in [
            (ROLE_ID_SUFFIX, credential.role_id.as_str()),
            (SECRET_ID_SUFFIX, credential.secret_id.expose_secret()),
        ] {
            files.push((format!("{role}{suffix}"), value));
        }
    }
    for (name, value) in &files {
        write_owner_only(dir, name, value)?;
    }
    let mut written: Vec<String> = files.into_iter().map(|(name, _)| name).collect();
    written.sort();
    Ok(written)
}

/// Checks, before any vault is reached, that [`write_credentials`] would accept `dir` for the
/// credentials [`crate::openbao::provision`] issues for `connections`: the directory holding
/// `dir`, `dir` itself when it exists, and each output name in it, by the same rules. An absent
/// `dir` passes and is not made. A refusal here leaves no secret id issued and unwritten.
pub fn check_credential_dir(dir: &Path, connections: &[ConnectionId]) -> Result<(), SourceError> {
    check_for(dir, &secret_id_identities(connections, false)).map(|_| ())
}

/// Checks `dir` for `identities`' files by [`write_credentials`]'s rules and answers whether `dir`
/// exists: its parent lets no one else rename it away, an existing `dir` is a directory of mode
/// 0700, and each output name in it is a plain file or absent.
fn check_for(dir: &Path, identities: &[Identity]) -> Result<bool, SourceError> {
    let parent = dir
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent_mode = fs::metadata(parent)
        .map_err(|_| SourceError::CredentialFile)?
        .permissions()
        .mode();
    if parent_mode & WRITABLE_BY_OTHERS != 0 && parent_mode & STICKY == 0 {
        return Err(SourceError::CredentialFile);
    }
    match fs::symlink_metadata(dir) {
        Ok(meta) if meta.is_dir() && meta.permissions().mode() & 0o777 == 0o700 => {}
        Ok(_) => return Err(SourceError::CredentialFile),
        Err(_) => return Ok(false),
    }
    for identity in identities {
        let role = role_name(identity);
        for suffix in [ROLE_ID_SUFFIX, SECRET_ID_SUFFIX] {
            let name = dir.join(format!("{role}{suffix}"));
            if fs::symlink_metadata(name).is_ok_and(|meta| !meta.is_file()) {
                return Err(SourceError::CredentialFile);
            }
        }
    }
    Ok(true)
}

/// Writes `value` to `dir/name` as a plain file of mode 0600 that replaces whatever is there.
///
/// A plain file named `.<name>.<anything>.tmp`, which only a crashed earlier write leaves, is
/// removed first. The value then goes to `.<name>.<pid>.<attempt>.tmp`, made new so that no
/// existing file, link, or hard link is opened, and the next attempt is tried while that name
/// cannot be made. It is synced, renamed over `dir/name`, and `dir` is synced, so a crash leaves
/// the earlier file or the new one, never an empty one. The temporary file is removed when a later
/// step fails.
fn write_owner_only(dir: &Path, name: &str, value: &str) -> Result<(), SourceError> {
    let stale = format!(".{name}.");
    for entry in fs::read_dir(dir).map_err(|_| SourceError::CredentialFile)? {
        let entry = entry.map_err(|_| SourceError::CredentialFile)?;
        let entry_name = entry.file_name();
        let left_by_a_crash = entry_name.to_str().is_some_and(|entry_name| {
            entry_name.starts_with(&stale) && entry_name.ends_with(".tmp")
        });
        if left_by_a_crash && entry.file_type().is_ok_and(|kind| kind.is_file()) {
            fs::remove_file(entry.path()).map_err(|_| SourceError::CredentialFile)?;
        }
    }
    let mut opened = None;
    for attempt in 0..TEMP_ATTEMPTS {
        let temp = dir.join(format!(".{name}.{}.{attempt}.tmp", process::id()));
        if let Ok(file) = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
        {
            opened = Some((temp, file));
            break;
        }
    }
    let (temp, mut file) = opened.ok_or(SourceError::CredentialFile)?;
    let placed = file
        .set_permissions(Permissions::from_mode(0o600))
        .and_then(|()| file.write_all(value.as_bytes()))
        .and_then(|()| file.sync_all())
        .and_then(|()| fs::rename(&temp, dir.join(name)))
        .and_then(|()| File::open(dir).and_then(|held| held.sync_all()));
    if placed.is_err() {
        drop(fs::remove_file(&temp));
    }
    placed.map_err(|_| SourceError::CredentialFile)
}

/// Reads an operator's token from `path`: a plain file, not a symbolic link, with no group or
/// other permission bit, holding the token and at most one trailing newline. Anything else is
/// [`SourceError::CredentialFile`].
pub fn read_token_file(path: &Path) -> Result<SecretString, SourceError> {
    let meta = fs::symlink_metadata(path).map_err(|_| SourceError::CredentialFile)?;
    if !meta.is_file() || meta.permissions().mode() & OPEN_TO_OTHERS != 0 {
        return Err(SourceError::CredentialFile);
    }
    let text = fs::read_to_string(path).map_err(|_| SourceError::CredentialFile)?;
    token_from(&text)
}

/// The token `text` holds: the token and at most one trailing newline, with no other line break
/// or carriage return. Anything else is [`SourceError::CredentialFile`].
fn token_from(text: &str) -> Result<SecretString, SourceError> {
    let token = text.strip_suffix('\n').unwrap_or(text);
    if token.is_empty() || token.contains(['\n', '\r']) {
        return Err(SourceError::CredentialFile);
    }
    Ok(SecretString::from(token.to_owned()))
}

/// The most bytes [`read_token`] reads: an operator's token is far shorter.
pub const TOKEN_MAX: u64 = 4096;

/// Reads an operator's token from `input`, a pipe its caller owns, by the rules of
/// [`read_token_file`]'s text: the token and at most one trailing newline. Input longer than
/// [`TOKEN_MAX`] bytes, or not UTF-8, is [`SourceError::CredentialFile`] too; whatever was read
/// of a refused input is zeroized before it is dropped.
pub fn read_token(input: &mut impl Read) -> Result<SecretString, SourceError> {
    let mut bytes = Vec::new();
    let read = input
        .take(TOKEN_MAX.saturating_add(1))
        .read_to_end(&mut bytes);
    if read.is_err() || u64::try_from(bytes.len()).map_or(true, |read| read > TOKEN_MAX) {
        bytes.zeroize();
        return Err(SourceError::CredentialFile);
    }
    match String::from_utf8(bytes) {
        Ok(text) => token_from(SecretString::from(text).expose_secret()),
        Err(invalid) => {
            invalid.into_bytes().zeroize();
            Err(SourceError::CredentialFile)
        }
    }
}
