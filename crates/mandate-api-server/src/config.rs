//! The checks the process makes before it serves anything (§3.11 items 1 and 2, DEC-978 items 1
//! and 2, DEC-822 item 4). Each refusal names the check, and at most a variable's name, never a
//! value.

use std::ffi::OsStr;
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;

/// The session signing key's file name under `$CREDENTIALS_DIRECTORY` (`deploy/systemd`).
pub const SESSION_KEY_FILE: &str = "session-signing-key";

/// The shortest session signing key accepted, in bytes: HMAC-SHA256's block of security.
pub const MIN_SESSION_KEY_BYTES: usize = 32;

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
    /// The check is not implemented yet (DEC-77).
    Unimplemented,
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
            Self::Unimplemented => write!(f, "{} has not been implemented yet", crate::STORY),
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
    let _ = (bind, pod_address);
    Err(StartRefusal::Unimplemented)
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
    let _ = vars.into_iter().count();
    Err(StartRefusal::Unimplemented)
}

/// Refuses a database URL that carries a password, in its user information or as a `password`
/// parameter; the deployment's URL authenticates over the local socket by peer authentication.
///
/// # Errors
/// [`StartRefusal::PasswordInDatabaseUrl`].
pub fn check_database_url(url: &str) -> Result<(), StartRefusal> {
    let _ = url;
    Err(StartRefusal::Unimplemented)
}

/// Reads [`SESSION_KEY_FILE`] from `credentials_dir`: at least [`MIN_SESSION_KEY_BYTES`], and
/// readable by its owner only.
///
/// # Errors
/// [`StartRefusal::SessionKeyMissing`], [`StartRefusal::SessionKeyExposed`], or
/// [`StartRefusal::SessionKeyTooShort`], in that order.
pub fn read_session_key(credentials_dir: &Path) -> Result<SessionKey, StartRefusal> {
    let _ = credentials_dir;
    Err(StartRefusal::Unimplemented)
}
