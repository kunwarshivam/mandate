//! The notice id, the payload, and the link (notifications spec §4.2, §4.3; NT-1, NT-4).

use mandate_canon::{Key, Object, Value};

use crate::{NotifyError, TextKey};

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
    /// Never: every id has its 32 digits.
    pub fn hex(&self) -> Result<String, NotifyError> {
        Ok(format!("{:032x}", u128::from_be_bytes(self.0)))
    }

    /// Reads back a notice id from its 32 lowercase hex digits, as a link or the dispatcher's own
    /// `NoticeIssued` carries it (DEC-702 item 2). A ULID, 26 characters of Crockford base32, never
    /// parses.
    ///
    /// # Errors
    /// [`NotifyError::NotANoticeId`] for anything but exactly 32 lowercase hex digits.
    pub fn parse(hex: &str) -> Result<Self, NotifyError> {
        if hex.len() != 32 || !hex.bytes().all(is_lowercase_hex_digit) {
            return Err(NotifyError::NotANoticeId);
        }
        u128::from_str_radix(hex, 16)
            .map(|bits| Self(bits.to_be_bytes()))
            .map_err(|_| NotifyError::NotANoticeId)
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
/// Never: both keys are fixed and valid.
pub fn payload(notification: &Notification) -> Result<Value, NotifyError> {
    let mut members = Object::new();
    members.insert(
        payload_key("notice")?,
        Value::Str(notification.notice.hex()?),
    );
    members.insert(
        payload_key("text")?,
        Value::Str(notification.text.key()?.to_owned()),
    );
    Ok(Value::Object(members))
}

fn payload_key(key: &str) -> Result<Key, NotifyError> {
    Key::new(key).map_err(|_| NotifyError::Unrepresentable {
        what: "payload key",
    })
}

fn is_lowercase_hex_digit(digit: u8) -> bool {
    matches!(digit, b'0'..=b'9' | b'a'..=b'f')
}

/// The workspace app's fixed origin, `https://<lowercase host>[:<port>]`, from the deployment's
/// configuration (spec §4.3, DEC-710 item 3). It holds no path, query, fragment, or credentials,
/// so a link built on it can carry nothing but the notice id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin(String);

impl Origin {
    /// Reads the origin as DEC-710 item 3 writes it: the host is one or more dot-separated labels,
    /// each non-empty, of `[a-z0-9-]`, neither starting nor ending with `-`; the port, if any, is
    /// 1 to 65535 in decimal digits alone, without a leading zero.
    ///
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
        if host.split('.').all(is_host_label) && port.is_none_or(is_port) {
            Ok(Self(origin.to_owned()))
        } else {
            Err(NotifyError::InvalidOrigin)
        }
    }
}

fn is_host_label(label: &str) -> bool {
    !label.is_empty()
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn is_port(port: &str) -> bool {
    !port.starts_with('0')
        && port.bytes().all(|b| b.is_ascii_digit())
        && port.parse::<u16>().is_ok()
}

/// The link every channel renders: `<origin>/n/<notice id>` and nothing else (NT-4).
///
/// # Errors
/// Never: the origin was checked when parsed, and every id has its 32 digits.
pub fn link(origin: &Origin, notice: &NoticeId) -> Result<String, NotifyError> {
    Ok(format!("{}/n/{}", origin.0, notice.hex()?))
}
