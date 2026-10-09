//! The one plaintext a push may carry: `{"notice":"<32 lowercase hex>","text":"<key>"}` (spec
//! §4.2, DEC-438 item 1).

use mandate_notify::{Notification, TextKey};

use crate::WebPushError;

/// The closed text set (spec §4.2). Adding a key is a spec change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushText {
    ApprovalNeeded,
    AttentionNeeded,
    AccountChanged,
    BriefReady,
}

impl PushText {
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::ApprovalNeeded => "approval_needed",
            Self::AttentionNeeded => "attention_needed",
            Self::AccountChanged => "account_changed",
            Self::BriefReady => "brief_ready",
        }
    }
}

/// The serialized payload. Its byte-level constructor is private to this crate: nothing outside
/// can put domain text, or an id that was not minted, in it. Its only public source is
/// [`PushPlaintext::of`], from the closed `mandate_notify::Notification` (DEC-790 item 7, DEC-713).
#[derive(Clone, PartialEq, Eq)]
pub struct PushPlaintext(Vec<u8>);

impl std::fmt::Debug for PushPlaintext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PushPlaintext(..)")
    }
}

impl PushPlaintext {
    pub(crate) fn new(notice: [u8; 16], text: PushText) -> Self {
        let mut out = String::from(r#"{"notice":""#);
        for byte in notice {
            out.push_str(&format!("{byte:02x}"));
        }
        out.push_str(r#"","text":""#);
        out.push_str(text.key());
        out.push_str(r#""}"#);
        Self(out.into_bytes())
    }

    /// The plaintext of `notification`, exactly `{"notice":"<its id's 32 lowercase hex
    /// digits>","text":"<its text key>"}` (spec §4.2, DEC-790 item 7). It is the only public way to
    /// build one: it takes the closed notice, whose id was minted from a secure random source and
    /// never derived from an event id (NT-4), and never a string or raw bytes, so no instrument,
    /// quantity, price, account, mandate content, address, name, kind, class, or link can reach a
    /// push (NT-1, DEC-713). Every result fits one padded record, under the relay's cap.
    ///
    /// # Errors
    /// [`WebPushError::InvalidNotice`] if the notice id cannot be written as its 32 digits, which
    /// no minted id fails.
    pub fn of(notification: &Notification) -> Result<Self, WebPushError> {
        let digits = notification
            .notice
            .hex()
            .map_err(|_| WebPushError::InvalidNotice)?;
        let bits = u128::from_str_radix(&digits, 16).map_err(|_| WebPushError::InvalidNotice)?;
        Ok(Self::new(bits.to_be_bytes(), push_text(notification.text)))
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// The push text of each closed text key, one for one (spec §4.2). The match names every key, so a
/// new key fails to compile until it has its push text.
fn push_text(text: TextKey) -> PushText {
    match text {
        TextKey::ApprovalNeeded => PushText::ApprovalNeeded,
        TextKey::AttentionNeeded => PushText::AttentionNeeded,
        TextKey::AccountChanged => PushText::AccountChanged,
        TextKey::BriefReady => PushText::BriefReady,
    }
}
