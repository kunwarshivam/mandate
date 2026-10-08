//! The notice id, the payload, and the link (notifications spec §4.2, §4.3; NT-1, NT-4).

use mandate_canon::{Key, Object, Value};

use crate::{NotifyError, TextKey};

/// The lowercase hex digits, in value order.
const DIGITS: &[u8; 16] = b"0123456789abcdef";

/// A cryptographically secure random source. The dispatcher passes the operating system's; tests
/// pass their own. This crate never draws randomness any other way.
pub trait SecureRandom {
    /// Fills `bytes` from the source.
    ///
    /// # Errors
    /// [`NotifyError::EntropyUnavailable`] when the source cannot.
    fn fill(&mut self, bytes: &mut [u8; 16]) -> Result<(), NotifyError>;
}

/// A random 128-bit notice id, the only id that leaves the workspace (spec §1.3, §4.2). It is
/// minted, never derived: it has no constructor from any event id, and text parses only as exactly
/// 32 lowercase hex digits, so a ULID, whose leading 48 bits are its creation time, can never be
/// sent as one (NT-4, DEC-702 item 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NoticeId([u8; 16]);

impl NoticeId {
    /// Mints a fresh id from `random`, taking nothing from the notice's cause.
    ///
    /// # Errors
    /// [`NotifyError::EntropyUnavailable`] when the source fails.
    pub fn mint(random: &mut dyn SecureRandom) -> Result<Self, NotifyError> {
        let mut bits = [0u8; 16];
        random.fill(&mut bits)?;
        Ok(Self(bits))
    }

    /// The id's 32 lowercase hex digits, as a link, the payload, and the dispatcher's
    /// `NoticeIssued` and `NoticeAttempted` carry it.
    ///
    /// # Errors
    /// Never once implemented.
    pub fn hex(&self) -> Result<String, NotifyError> {
        Ok(self
            .0
            .iter()
            .flat_map(|byte| [byte >> 4, byte & 0x0f])
            .filter_map(|nibble| DIGITS.get(usize::from(nibble)).copied())
            .map(char::from)
            .collect())
    }

    /// Reads back a notice id from its 32 lowercase hex digits, as a link or the dispatcher's own
    /// `NoticeIssued` carries it (DEC-702 item 2). A ULID, 26 characters of Crockford base32, never
    /// parses.
    ///
    /// # Errors
    /// [`NotifyError::NotANoticeId`] for anything but exactly 32 lowercase hex digits.
    pub fn parse(hex: &str) -> Result<Self, NotifyError> {
        let (pairs, rest) = hex.as_bytes().as_chunks::<2>();
        if pairs.len() != 16 || !rest.is_empty() {
            return Err(NotifyError::NotANoticeId);
        }
        let mut bits = [0u8; 16];
        for (byte, [hi, lo]) in bits.iter_mut().zip(pairs) {
            *byte = nibble(*hi)?
                .checked_mul(16)
                .and_then(|high| high.checked_add(nibble(*lo).ok()?))
                .ok_or(NotifyError::NotANoticeId)?;
        }
        Ok(Self(bits))
    }
}

/// The whole of a notice as it leaves the workspace: an id and a text key, and no field for
/// anything else (`AGENTS.md` rule 6 at rung 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Notification {
    pub notice: NoticeId,
    pub text: TextKey,
}

/// The payload: `{"notice": "<32 lowercase hex>", "text": "<text key>"}` and nothing else.
///
/// # Errors
/// Never once implemented: both keys are fixed and valid.
pub fn payload(notification: &Notification) -> Result<Value, NotifyError> {
    let key = |k| Key::new(k).map_err(|_| NotifyError::Unrepresentable { what: "key" });
    let mut members = Object::new();
    members.insert(key("notice")?, Value::Str(notification.notice.hex()?));
    members.insert(
        key("text")?,
        Value::Str(notification.text.key()?.to_owned()),
    );
    Ok(Value::Object(members))
}

/// The workspace app's fixed origin, `https://<lowercase host>[:<port>]`, from the deployment's
/// configuration (spec §4.3, DEC-710 item 3). It holds no path, query, fragment, or credentials,
/// so a link built on it can carry nothing but the notice id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin(String);

impl Origin {
    /// # Errors
    /// [`NotifyError::InvalidOrigin`] for anything but `https://<lowercase host>[:<port>]`.
    pub fn parse(origin: &str) -> Result<Self, NotifyError> {
        let authority = origin
            .strip_prefix("https://")
            .ok_or(NotifyError::InvalidOrigin)?;
        let (host, port) = match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        };
        let host_ok = host.split('.').all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        });
        if host_ok && port.is_none_or(is_port) {
            Ok(Self(origin.to_owned()))
        } else {
            Err(NotifyError::InvalidOrigin)
        }
    }
}

/// One lowercase hex digit's value.
fn nibble(digit: u8) -> Result<u8, NotifyError> {
    DIGITS
        .iter()
        .position(|d| *d == digit)
        .and_then(|value| u8::try_from(value).ok())
        .ok_or(NotifyError::NotANoticeId)
}

/// A TCP port from 1 to 65535, in decimal digits with no leading zero.
fn is_port(port: &str) -> bool {
    !port.starts_with('0')
        && port.bytes().all(|b| b.is_ascii_digit())
        && port.parse::<u16>().is_ok()
}

/// The link every channel renders: `<origin>/n/<notice id>` and nothing else (NT-4).
///
/// # Errors
/// Never once implemented.
pub fn link(origin: &Origin, notice: &NoticeId) -> Result<String, NotifyError> {
    Ok(format!("{}/n/{}", origin.0, notice.hex()?))
}
