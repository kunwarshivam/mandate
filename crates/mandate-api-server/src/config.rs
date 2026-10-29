//! The checks the process makes before it serves anything (§3.11 items 1 and 2, DEC-978 items 1
//! and 2, DEC-822 item 4). Each refusal names the check, and at most a variable's name, never a
//! value.

use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use mandate_connections::start::AuthorizePage;
use mandate_release::startup::Tier;
use mandate_time::UtcNanos;

use crate::methods::CoolOffWaiver;

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
    /// [`AUTHORIZE_URL`] is set where it may not be, or names a page a cell may not use
    /// (DEC-2633 item 3).
    AuthorizeUrl,
    /// [`INTERNAL_PAPER_WORKSPACE`] is on, and the hosted control stream is not shown to be paper
    /// (DEC-1143 item 2).
    InternalWorkspaceNotPaper,
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
            Self::AuthorizeUrl => write!(
                f,
                "{AUTHORIZE_URL} is set outside a dev or ci debug build, or names no cell page"
            ),
            Self::InternalWorkspaceNotPaper => write!(
                f,
                "{INTERNAL_PAPER_WORKSPACE} is on, and the control stream is not shown to be paper"
            ),
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
    let ip = bind.ip().to_canonical();
    let pod = pod_address.map(|pod| pod.to_canonical());
    let local = ip.is_loopback() || (!ip.is_unspecified() && Some(ip) == pod);
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

/// The setting a dev or CI cell names its fixture provider's authorization page in (DEC-2633
/// item 3).
pub const AUTHORIZE_URL: &str = "MANDATE_ALPACA_AUTHORIZE_URL";

/// The authorization page the connect start sends the browser to: Alpaca's when [`AUTHORIZE_URL`]
/// is unset. A build with debug assertions admits a set one only in the `dev` and `ci` tiers, and
/// only a page [`AuthorizePage::for_cell`] admits; a build without them refuses any set value,
/// whatever the tier, as the token exchange refuses its token URL (DEC-2322). A value that is not
/// UTF-8 is still a value set.
///
/// # Errors
/// [`StartRefusal::AuthorizeUrl`].
pub fn authorize_page(tier: Tier, value: Option<&OsStr>) -> Result<AuthorizePage, StartRefusal> {
    let Some(text) = value else {
        return Ok(AuthorizePage::alpaca());
    };
    #[cfg(debug_assertions)]
    let judged = matches!(tier, Tier::Dev | Tier::Ci)
        .then(|| text.to_str().and_then(AuthorizePage::for_cell))
        .flatten()
        .ok_or(StartRefusal::AuthorizeUrl);
    #[cfg(not(debug_assertions))]
    let judged = {
        let _unread_in_release = (tier, text);
        Err(StartRefusal::AuthorizeUrl)
    };
    judged
}

/// The setting a `dev` or `ci` test cell waives a first passkey's enrolment cool-off with
/// ([DEC-2146](../../../docs/project/decisions/DEC-2146.md) item 2): exactly `true`, which the chart
/// renders from `api.stepUpCoolOffWaiver` and refuses outside those tiers.
pub const COOL_OFF_WAIVER: &str = "MANDATE_API_STEP_UP_COOL_OFF_WAIVER";

/// The waiver's expiry ([DEC-2150](../../../docs/project/decisions/DEC-2150.md) item 2), exactly
/// `YYYY-MM-DDTHH:MM:SSZ` in UTC: required with the waiver in `staging`, honoured in `dev` and
/// `ci`, and refused without the waiver. The chart renders it from `api.stepUpCoolOffWaiverUntil`.
pub const COOL_OFF_WAIVER_UNTIL: &str = "MANDATE_API_STEP_UP_COOL_OFF_WAIVER_UNTIL";

/// The latest expiry a staging cell may start with (DEC-2150 item 3). It is compiled in, so a
/// values edit alone cannot extend the waiver: a later expiry needs a code change and a new
/// founder decision.
pub const STAGING_WAIVER_CEILING: &str = "2026-11-05T00:00:00Z";

/// The waiver [`COOL_OFF_WAIVER`]'s `value` and its expiry [`COOL_OFF_WAIVER_UNTIL`]'s `until` set
/// in `tier`: none when both are unset; the tier's own, with its expiry, when `value` is exactly
/// `true` in `dev` or `ci`, or in `staging` with an expiry no later than
/// [`STAGING_WAIVER_CEILING`] (DEC-2146 item 2, DEC-2150 items 2 and 3). Any other value, `true` in
/// `production`, a staging waiver without an expiry or past the ceiling, an expiry in another
/// form, and an expiry without the waiver each refuse the start.
///
/// # Errors
/// [`StartRefusal::Setting`] naming [`COOL_OFF_WAIVER`] or [`COOL_OFF_WAIVER_UNTIL`].
pub fn cool_off_waiver(
    tier: Tier,
    value: Option<&OsStr>,
    until: Option<&OsStr>,
) -> Result<Option<(CoolOffWaiver, Option<UtcNanos>)>, StartRefusal> {
    let expiry = StartRefusal::Setting {
        name: COOL_OFF_WAIVER_UNTIL,
    };
    let Some(value) = value else {
        return match until {
            None => Ok(None),
            Some(_) => Err(expiry),
        };
    };
    let refused = StartRefusal::Setting {
        name: COOL_OFF_WAIVER,
    };
    if value != OsStr::new("true") {
        return Err(refused);
    }
    let waiver = match tier {
        Tier::Dev => CoolOffWaiver::Dev,
        Tier::Ci => CoolOffWaiver::Ci,
        Tier::Staging => CoolOffWaiver::Staging,
        Tier::Production => return Err(refused),
    };
    let until = match until {
        None => None,
        Some(text) => Some(expiry_instant(text).ok_or_else(|| expiry.clone())?),
    };
    if waiver == CoolOffWaiver::Staging {
        let ceiling =
            UtcNanos::parse_rfc3339(STAGING_WAIVER_CEILING).map_err(|_| expiry.clone())?;
        if until.is_none_or(|until| until > ceiling) {
            return Err(expiry);
        }
    }
    Ok(Some((waiver, until)))
}

/// The expiry `text` names when it is exactly `YYYY-MM-DDTHH:MM:SSZ`, a real UTC instant; `None`
/// for any other form (DEC-2150 item 2). The template refuses a fraction or an offset at the
/// `Z`'s place, and `parse_rfc3339` refuses anything shorter, or anything after the `Z`.
fn expiry_instant(text: &OsStr) -> Option<UtcNanos> {
    let text = text.to_str()?;
    let shaped = text
        .bytes()
        .zip(b"dddd-dd-ddTdd:dd:ddZ")
        .all(|(byte, want)| match want {
            b'd' => byte.is_ascii_digit(),
            _ => byte == *want,
        });
    if !shaped {
        return None;
    }
    UtcNanos::parse_rfc3339(text).ok()
}

/// Refuses a key set fetched over anything but `https` outside dev and CI (E21-53, DEC-2100 item
/// 9), so a staging or production server never trusts a loopback issuer, such as the local stub,
/// even when its settings name one. In dev and CI an `http` key set is admitted here; the key-set
/// client still takes `http` only on loopback.
///
/// # Errors
/// [`StartRefusal::Setting`] naming `MANDATE_API_JWKS_URL`.
pub fn check_key_set_tier(url: &str, tier: Tier) -> Result<(), StartRefusal> {
    let https = url
        .get(..HTTPS.len())
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case(HTTPS));
    if https || matches!(tier, Tier::Dev | Tier::Ci) {
        Ok(())
    } else {
        Err(StartRefusal::Setting {
            name: "MANDATE_API_JWKS_URL",
        })
    }
}

/// The one scheme a key set is fetched with outside dev and CI.
const HTTPS: &str = "https://";

/// Refuses a database connection string that carries a password, in a URL's user information or
/// as a `password` keyword, in either libpq form (a URL or `key=value` pairs). The string is read
/// percent-decoded, ASCII case-insensitively, and without whitespace, as libpq would read it; a
/// string that only looks like a password, such as a database named `password=`, is refused too,
/// which fails safe. The deployment's string authenticates over the local socket by peer
/// authentication.
///
/// # Errors
/// [`StartRefusal::PasswordInDatabaseUrl`].
pub fn check_database_url(url: &str) -> Result<(), StartRefusal> {
    let read: String = percent_decoded(url)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    let keyword = read.contains("password=");
    let userinfo = read.split_once("://").is_some_and(|(_, rest)| {
        let authority = rest.split(['/', '?']).next().unwrap_or_default();
        authority
            .rsplit_once('@')
            .is_some_and(|(userinfo, _)| userinfo.contains(':'))
    });
    if keyword || userinfo {
        Err(StartRefusal::PasswordInDatabaseUrl)
    } else {
        Ok(())
    }
}

/// `text` with each `%XX` escape decoded, lossily; an escape that is not two hex digits stays.
fn percent_decoded(text: &str) -> String {
    let mut out = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&first, tail)) = rest.split_first() {
        let escaped = (first == b'%')
            .then(|| tail.get(..2))
            .flatten()
            .and_then(|digits| std::str::from_utf8(digits).ok())
            .filter(|digits| digits.bytes().all(|b| b.is_ascii_hexdigit()))
            .and_then(|digits| u8::from_str_radix(digits, 16).ok());
        match (escaped, tail.get(2..)) {
            (Some(byte), Some(after)) => {
                out.push(byte);
                rest = after;
            }
            _ => {
                out.push(first);
                rest = tail;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Reads [`SESSION_KEY_FILE`] from `credentials_dir`: a regular file (a FIFO or directory is refused
/// as missing before it is opened, and the opened file is checked again, so its permissions and
/// bytes come from the one open), readable by its owner only, and at least
/// [`MIN_SESSION_KEY_BYTES`].
///
/// # Errors
/// [`StartRefusal::SessionKeyMissing`], [`StartRefusal::SessionKeyExposed`], or
/// [`StartRefusal::SessionKeyTooShort`], in that order.
pub fn read_session_key(credentials_dir: &Path) -> Result<SessionKey, StartRefusal> {
    let path = credentials_dir.join(SESSION_KEY_FILE);
    let regular = fs::metadata(&path).is_ok_and(|metadata| metadata.is_file());
    if !regular {
        return Err(StartRefusal::SessionKeyMissing);
    }
    let mut file = File::open(&path).map_err(|_| StartRefusal::SessionKeyMissing)?;
    let metadata = file
        .metadata()
        .map_err(|_| StartRefusal::SessionKeyMissing)?;
    if !metadata.is_file() {
        return Err(StartRefusal::SessionKeyMissing);
    }
    if metadata.permissions().mode() & OTHERS_AND_GROUP != 0 {
        return Err(StartRefusal::SessionKeyExposed);
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|_| StartRefusal::SessionKeyMissing)?;
    if bytes.len() < MIN_SESSION_KEY_BYTES {
        return Err(StartRefusal::SessionKeyTooShort);
    }
    Ok(SessionKey(bytes))
}

/// The content-addressed store at `root`, which the agent reads take mandate documents from
/// (DEC-1020 item 1) and the version routes add objects to (DEC-2101 item 8), or none when `root`
/// is absent or empty. A root that is not already a store (its `sha256` directory) is refused
/// rather than created.
///
/// # Errors
/// [`StartRefusal::Setting`] naming `MANDATE_API_ARTIFACTS` when `root` cannot be opened as a
/// store.
pub fn artifact_store(
    root: Option<&std::ffi::OsStr>,
) -> Result<
    Option<std::sync::Arc<crate::objects::Objects<mandate_artifacts_fs::FsArtifactStore>>>,
    StartRefusal,
> {
    let Some(root) = root.filter(|root| !root.is_empty()) else {
        return Ok(None);
    };
    let refused = StartRefusal::Setting {
        name: "MANDATE_API_ARTIFACTS",
    };
    if !std::path::Path::new(root).join("sha256").is_dir() {
        return Err(refused);
    }
    let store =
        mandate_artifacts_fs::FsArtifactStore::open(root).map_err(|_| StartRefusal::Setting {
            name: "MANDATE_API_ARTIFACTS",
        })?;
    Ok(Some(std::sync::Arc::new(crate::objects::Objects::new(
        store,
    ))))
}

/// The setting that marks the hosted workspace one of the team's internal paper workspaces, the
/// only ones `GET /evaluations/{id}` answers in (DEC-103, DEC-1002 item 6, DEC-1143).
pub const INTERNAL_PAPER_WORKSPACE: &str = "MANDATE_API_INTERNAL_PAPER_WORKSPACE";

/// [`INTERNAL_PAPER_WORKSPACE`]'s value: off when absent or empty, on only for exactly `true`, off
/// for exactly `false` (DEC-1143 item 1).
///
/// # Errors
/// [`StartRefusal::Setting`] naming it for any other value.
pub fn internal_paper_workspace(value: Option<&OsStr>) -> Result<bool, StartRefusal> {
    match value.map(OsStr::to_str) {
        None | Some(Some("" | "false")) => Ok(false),
        Some(Some("true")) => Ok(true),
        Some(_) => Err(StartRefusal::Setting {
            name: INTERNAL_PAPER_WORKSPACE,
        }),
    }
}

/// The start check for an internal paper workspace (DEC-1143 item 2): the hosted control stream,
/// as read at start, has a row and every row is `paper`. A stream that could not be read is
/// refused too: the setting is never taken on trust.
///
/// # Errors
/// [`StartRefusal::InternalWorkspaceNotPaper`].
pub fn check_internal_paper(
    control: Result<&[mandate_journal::StoredEvent], crate::journal::JournalUnavailable>,
) -> Result<(), StartRefusal> {
    match control {
        Ok(rows) if crate::backtests::paper(rows) => Ok(()),
        _ => Err(StartRefusal::InternalWorkspaceNotPaper),
    }
}
