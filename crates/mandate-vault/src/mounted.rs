//! AppRole credentials and the vault's CA read from a mounted Kubernetes Secret (DEC-2630): the
//! kubelet writes each key as a link, `<name> -> ..data/<name>`, with `..data` a link to the
//! current timestamped directory, so a read follows links, but only to a file inside the mount.
//! The file it reaches must be a plain file with no group or other permission bit.

use std::path::Path;

use secrecy::SecretString;

use crate::SourceError;
use crate::policy::Identity;

/// Reads the file `name` of the mounted directory `dir` whole: `name` is one plain path
/// component, and the file it names, its links followed, is a plain file inside `dir` that no
/// group or other may read, write, or run. Anything else, a link that leaves `dir` by a relative
/// or an absolute target included, is [`SourceError::CredentialFile`]. The file is opened without
/// following a link at its last step, so a link swapped in after the check is refused too.
pub fn read_mounted(dir: &Path, name: &str) -> Result<SecretString, SourceError> {
    let _ = (dir, name);
    Err(SourceError::Unimplemented { story: "E21-32" })
}

/// Reads `identity`'s AppRole credentials from the mounted directory `dir`, by
/// [`read_mounted`]: `<role>.role-id` and `<role>.secret-id`, each one line, with at most one
/// trailing newline, which is dropped. An empty value, a second line, or a carriage return is
/// [`SourceError::CredentialFile`]. A token exchange is [`SourceError::Config`]: its credential is
/// a single-use grant, never mounted.
pub fn read_mounted_credentials(
    dir: &Path,
    identity: &Identity,
) -> Result<(String, SecretString), SourceError> {
    let _ = (dir, identity);
    Err(SourceError::Unimplemented { story: "E21-32" })
}
