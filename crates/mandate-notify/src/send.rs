//! What a send is keyed by and what it carries (notifications spec §4.3, §5.1, §5.2; NT-2,
//! DEC-710 items 4 to 6): the opaque recipient, the address handle, the digest a provider sees in
//! place of the idempotency key, and the rendered message.

use mandate_canon::{Digest, Key, Object, Value, to_canonical};

use crate::{NoticeId, Notification, NotifyError, Origin, link};

/// A recipient as journaled, logged, and keyed: an opaque user id of 1 to 64 characters of
/// `[a-z0-9_]`, starting with a letter, and never an address or a name (NT-2, DEC-710 item 4).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Recipient(String);

impl Recipient {
    /// Reads a recipient id as DEC-710 item 4 writes it.
    ///
    /// # Errors
    /// [`NotifyError::NotARecipient`] for anything but 1 to 64 characters of `[a-z0-9_]` starting
    /// with a letter.
    pub fn parse(id: &str) -> Result<Self, NotifyError> {
        let mut bytes = id.bytes();
        let starts_with_letter = bytes.next().is_some_and(|b| b.is_ascii_lowercase());
        if starts_with_letter && id.len() <= 64 && bytes.all(is_recipient_byte) {
            Ok(Self(id.to_owned()))
        } else {
            Err(NotifyError::NotARecipient)
        }
    }
}

/// A push channel, as the mandate's `notifications.channels` names it (spec §4.1). The pull
/// channels are delivered in their cause's own batch and never reach a provider, so none is here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PushChannel {
    Email,
    Phone,
    Slack,
    Sms,
    Telegram,
    WebPush,
}

impl PushChannel {
    pub const ALL: [Self; 6] = [
        Self::Email,
        Self::Phone,
        Self::Slack,
        Self::Sms,
        Self::Telegram,
        Self::WebPush,
    ];

    /// The channel as the mandate schema's enum writes it: `email`, `phone`, `slack`, `sms`,
    /// `telegram`, or `web_push`.
    ///
    /// # Errors
    /// Never: every channel has a key.
    pub fn key(self) -> Result<&'static str, NotifyError> {
        Ok(match self {
            Self::Email => "email",
            Self::Phone => "phone",
            Self::Slack => "slack",
            Self::Sms => "sms",
            Self::Telegram => "telegram",
            Self::WebPush => "web_push",
        })
    }
}

/// Where a send goes, as the dispatcher holds it: the opaque recipient and the channel. The
/// adapter dereferences it through the vault; the address itself is never in it (NT-2, DEC-710
/// item 4).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AddressHandle {
    pub recipient: Recipient,
    pub channel: PushChannel,
}

/// What a provider is given as a send's idempotency key: the lowercase hex SHA-256 of the
/// canonical `{"channel", "notice", "recipient"}` (DEC-710 item 5). It is as unique and as stable
/// across retries as spec §5.1's `(notice id, recipient, channel)`, and unlike it hands no provider
/// a recipient id that links notices over time (spec §12 item 4).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    /// The key of the send of `notice` to `address`.
    ///
    /// # Errors
    /// Never: every notice id, recipient, and channel is representable.
    pub fn of(notice: &NoticeId, address: &AddressHandle) -> Result<Self, NotifyError> {
        let mut triple = Object::new();
        triple.insert(
            triple_key("channel")?,
            Value::Str(address.channel.key()?.to_owned()),
        );
        triple.insert(triple_key("notice")?, Value::Str(notice.hex()?));
        triple.insert(
            triple_key("recipient")?,
            Value::Str(address.recipient.0.clone()),
        );
        let canonical = to_canonical(&Value::Object(triple));
        Ok(Self(Digest::of(&canonical).to_hex()))
    }

    /// The key's 64 lowercase hex digits, as a provider is given them.
    ///
    /// # Errors
    /// Never: the digits were written when the key was made.
    pub fn hex(&self) -> Result<&str, NotifyError> {
        Ok(&self.0)
    }
}

/// The rendered message every channel starts from: the notice's fixed text, a newline, and its
/// link (spec §4.3, DEC-710 item 6). A channel adapter adds only fixed copy around it, such as the
/// email's sentence and footer placeholder (spec §4.4).
///
/// # Errors
/// Never: every text key has its text, and the origin was checked when parsed.
pub fn rendered(origin: &Origin, notification: &Notification) -> Result<String, NotifyError> {
    Ok(format!(
        "{}\n{}",
        notification.text.text()?,
        link(origin, &notification.notice)?
    ))
}

fn is_recipient_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
}

fn triple_key(key: &str) -> Result<Key, NotifyError> {
    Key::new(key).map_err(|_| NotifyError::Unrepresentable {
        what: "idempotency key member",
    })
}
