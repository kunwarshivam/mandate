//! The one plaintext a push may carry: `{"notice":"<32 lowercase hex>","text":"<key>"}` (spec
//! §4.2, DEC-438 item 1).

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

/// The serialized payload. Its constructor is private to this crate: nothing outside can put
/// domain text in it. Slice S1's `mandate_notify::Notification` becomes its only public source,
/// a minted notice id and a closed text key, when that crate merges.
#[derive(Clone, PartialEq, Eq)]
pub struct PushPlaintext(Vec<u8>);

impl std::fmt::Debug for PushPlaintext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PushPlaintext(..)")
    }
}

impl PushPlaintext {
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "S1's Notification is the caller")
    )]
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

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}
