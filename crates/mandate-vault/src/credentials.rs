//! Credentials as files only their owner can read (DEC-2022): where `mandate vault provision`
//! leaves each identity's AppRole credentials, and how it reads the operator's token. Neither is
//! ever an argument, an environment variable, or printed.

use std::fs::{self, DirBuilder, OpenOptions, Permissions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

use secrecy::{ExposeSecret, SecretString};

use crate::SourceError;
use crate::openbao::RoleCredentials;
use crate::policy::role_name;

/// The suffix of the file that holds an identity's role id, after its role name.
pub const ROLE_ID_SUFFIX: &str = ".role-id";
/// The suffix of the file that holds an identity's secret id, after its role name.
pub const SECRET_ID_SUFFIX: &str = ".secret-id";

/// The permission bits of group and others, none of which a credential's file or directory may
/// carry.
const OPEN_TO_OTHERS: u32 = 0o077;

/// Writes each identity's credentials into `dir`, `<role>.role-id` and `<role>.secret-id`, each a
/// plain file of mode 0600 holding the value alone, replacing any earlier plain file of that name.
/// `dir` is made with mode 0700 when it cannot be read, which fails unless it is absent; an
/// existing one must be a directory, not a symbolic link, of mode 0700 exactly. Every output name
/// is checked before any is written, and one that is anything but a plain file, a planted link
/// included, refuses the whole write. Returns the names written, sorted.
pub fn write_credentials(
    dir: &Path,
    credentials: &[RoleCredentials],
) -> Result<Vec<String>, SourceError> {
    match fs::symlink_metadata(dir) {
        Ok(meta) if meta.is_dir() && meta.permissions().mode() & 0o777 == 0o700 => {}
        Ok(_) => return Err(SourceError::CredentialFile),
        Err(_) => {
            DirBuilder::new()
                .mode(0o700)
                .create(dir)
                .and_then(|()| fs::set_permissions(dir, Permissions::from_mode(0o700)))
                .map_err(|_| SourceError::CredentialFile)?;
        }
    }
    let mut files = Vec::new();
    for credential in credentials {
        let role = role_name(&credential.identity);
        for (suffix, value) in [
            (ROLE_ID_SUFFIX, credential.role_id.as_str()),
            (SECRET_ID_SUFFIX, credential.secret_id.expose_secret()),
        ] {
            let name = format!("{role}{suffix}");
            if fs::symlink_metadata(dir.join(&name)).is_ok_and(|meta| !meta.is_file()) {
                return Err(SourceError::CredentialFile);
            }
            files.push((name, value));
        }
    }
    for (name, value) in &files {
        write_owner_only(&dir.join(name), value)?;
    }
    let mut written: Vec<String> = files.into_iter().map(|(name, _)| name).collect();
    written.sort();
    Ok(written)
}

/// Writes `value` to `path`, a plain file or absent, as a plain file of mode 0600, whatever mode
/// an earlier file had.
fn write_owner_only(path: &Path, value: &str) -> Result<(), SourceError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| SourceError::CredentialFile)?;
    file.set_permissions(Permissions::from_mode(0o600))
        .and_then(|()| file.write_all(value.as_bytes()))
        .and_then(|()| file.sync_all())
        .map_err(|_| SourceError::CredentialFile)
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
    let token = text.strip_suffix('\n').unwrap_or(&text);
    if token.is_empty() || token.contains(['\n', '\r']) {
        return Err(SourceError::CredentialFile);
    }
    Ok(SecretString::from(token.to_owned()))
}
