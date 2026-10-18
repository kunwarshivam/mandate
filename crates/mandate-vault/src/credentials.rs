//! Credentials as files only their owner can read (DEC-2022): where `mandate vault provision`
//! leaves each identity's AppRole credentials, and how it reads the operator's token. Neither is
//! ever an argument, an environment variable, or printed.

use std::path::Path;

use secrecy::SecretString;

use crate::SourceError;
use crate::openbao::RoleCredentials;

/// The suffix of the file that holds an identity's role id, after its role name.
pub const ROLE_ID_SUFFIX: &str = ".role-id";
/// The suffix of the file that holds an identity's secret id, after its role name.
pub const SECRET_ID_SUFFIX: &str = ".secret-id";

/// Writes each identity's credentials into `dir`, `<role>.role-id` and `<role>.secret-id`, each a
/// plain file of mode 0600 holding the value alone, replacing any earlier file of that name. `dir`
/// is made with mode 0700 when absent; an existing one must be a directory, not a symbolic link,
/// open to its owner alone. Returns the names written, sorted.
pub fn write_credentials(
    _dir: &Path,
    _credentials: &[RoleCredentials],
) -> Result<Vec<String>, SourceError> {
    Err(SourceError::Unimplemented { story: "E21-32" })
}

/// Reads an operator's token from `path`: a plain file, not a symbolic link, with no group or
/// other permission bit, holding the token and at most one trailing newline. Anything else is
/// [`SourceError::CredentialFile`].
pub fn read_token_file(_path: &Path) -> Result<SecretString, SourceError> {
    Err(SourceError::Unimplemented { story: "E21-32" })
}
