//! The checks the process makes before it serves anything (§3.11 items 1 and 2, DEC-978 items 1
//! and 2, DEC-822 item 4). Each refusal names the check, and at most a variable's name, never a
//! value.

use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// The session signing key's file name under `$CREDENTIALS_DIRECTORY` (`deploy/systemd`).
pub const SESSION_KEY_FILE: &str = "session-signing-key";

/// The shortest session signing key accepted, in bytes: HMAC-SHA256's block of security.
pub const MIN_SESSION_KEY_BYTES: usize = 32;

/// The permission bits of a file's group and others; a credential file must have none.
const OTHERS_AND_GROUP: u32 = 0o077;

/// Name fragments that mark a variable as secret-shaped, matched ASCII case-insensitively.
pub const SECRET_NAME_PARTS: [&str; 4] = ["SECRET", "PASSWORD", "TOKEN", "KEY"];

/// Why the process refused to start. `Display` names the check and nothing the check read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartRefusal {
    /// The bind address is neither loopback nor the pod's own address.
    BindNotLocal,
    /// A variable with a secret-shaped name is in the environment; carries only its name.
    SecretInEnvironment { name: String },
    /// The database URL carries a password.
    PasswordInDatabaseUrl,
    /// The session signing key file is absent or unreadable.
    SessionKeyMissing,
    /// The session signing key is shorter than [`MIN_SESSION_KEY_BYTES`].
    SessionKeyTooShort,
    /// The session signing key file is readable by its group or by others.
    SessionKeyExposed,
    /// A required setting is absent or malformed; carries only its name.
    Setting { name: &'static str },
}

impl fmt::Display for StartRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BindNotLocal => f.write_str("the bind address is not loopback or the pod's own"),
            Self::SecretInEnvironment { name } => {
                write!(f, "a secret-named variable is in the environment: {name}")
            }
            Self::PasswordInDatabaseUrl => f.write_str("the database URL carries a password"),
            Self::SessionKeyMissing => f.write_str("the session signing key file is missing"),
            Self::SessionKeyTooShort => f.write_str("the session signing key is too short"),
            Self::SessionKeyExposed => {
                f.write_str("the session signing key file is readable by group or others")
            }
            Self::Setting { name } => {
                write!(f, "a required setting is absent or malformed: {name}")
            }
        }
    }
}

impl std::error::Error for StartRefusal {}

/// The session signing key. Its `Debug` prints nothing of it (rule 7).
#[derive(Clone, PartialEq, Eq)]
pub struct SessionKey(Vec<u8>);

impl SessionKey {
    /// The key's bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SessionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionKey(..)")
    }
}

/// `bind` if it is a loopback address or equals `pod_address`, the pod's own address; the
/// unspecified addresses and every other address are refused (DEC-978 item 1).
///
/// # Errors
/// [`StartRefusal::BindNotLocal`].
pub fn check_bind(
    bind: SocketAddr,
    pod_address: Option<IpAddr>,
) -> Result<SocketAddr, StartRefusal> {
    let ip = bind.ip();
    let local = ip.is_loopback() || (!ip.is_unspecified() && Some(ip) == pod_address);
    if local {
        Ok(bind)
    } else {
        Err(StartRefusal::BindNotLocal)
    }
}

/// Refuses an environment holding any variable whose name contains one of
/// [`SECRET_NAME_PARTS`], ASCII case-insensitively (DEC-978 item 2).
///
/// # Errors
/// [`StartRefusal::SecretInEnvironment`] naming the first such variable in `vars`' order.
pub fn check_environment<I, K, V>(vars: I) -> Result<(), StartRefusal>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<OsStr>,
{
    for (name, _) in vars {
        let name = name.as_ref().to_string_lossy();
        let upper = name.to_ascii_uppercase();
        if SECRET_NAME_PARTS.iter().any(|part| upper.contains(part)) {
            return Err(StartRefusal::SecretInEnvironment {
                name: name.into_owned(),
            });
        }
    }
    Ok(())
}

/// Refuses a database URL that carries a password, in its user information or as a `password`
/// parameter; the deployment's URL authenticates over the local socket by peer authentication.
///
/// # Errors
/// [`StartRefusal::PasswordInDatabaseUrl`].
pub fn check_database_url(url: &str) -> Result<(), StartRefusal> {
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let (authority_and_path, query) = after_scheme.split_once('?').unwrap_or((after_scheme, ""));
    let authority = authority_and_path
        .split_once('/')
        .map_or(authority_and_path, |(authority, _)| authority);
    let userinfo_has_password = authority
        .rsplit_once('@')
        .is_some_and(|(userinfo, _)| userinfo.contains(':'));
    let query_has_password = query.split('&').any(|pair| {
        pair.split_once('=')
            .map_or(pair, |(key, _)| key)
            .eq_ignore_ascii_case("password")
    });
    if userinfo_has_password || query_has_password {
        Err(StartRefusal::PasswordInDatabaseUrl)
    } else {
        Ok(())
    }
}

/// Reads [`SESSION_KEY_FILE`] from `credentials_dir`: at least [`MIN_SESSION_KEY_BYTES`], and
/// readable by its owner only.
///
/// # Errors
/// [`StartRefusal::SessionKeyMissing`], [`StartRefusal::SessionKeyExposed`], or
/// [`StartRefusal::SessionKeyTooShort`], in that order.
pub fn read_session_key(credentials_dir: &Path) -> Result<SessionKey, StartRefusal> {
    let path = credentials_dir.join(SESSION_KEY_FILE);
    let metadata = fs::metadata(&path).map_err(|_| StartRefusal::SessionKeyMissing)?;
    if metadata.permissions().mode() & OTHERS_AND_GROUP != 0 {
        return Err(StartRefusal::SessionKeyExposed);
    }
    let bytes = fs::read(&path).map_err(|_| StartRefusal::SessionKeyMissing)?;
    if bytes.len() < MIN_SESSION_KEY_BYTES {
        return Err(StartRefusal::SessionKeyTooShort);
    }
    Ok(SessionKey(bytes))
}
