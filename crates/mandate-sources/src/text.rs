//! Untrusted third-party text (DP-8): a headline, a summary, a link, or an excerpt, held bounded
//! and inert.
//!
//! [`UntrustedText`] has no `Display`, no `Deref`, no `AsRef<str>`, no `From` into any type, and a
//! `Debug` that prints only its length, so text can neither be formatted into a log or an error by
//! accident nor stand in for a typed field. The one way to read it,
//! [`UntrustedText::as_untrusted_str`], says what it returns, and is meant for the model gateway,
//! which marks it as third-party data in the prompt (agent harness spec §6.4).
//!
//! ```compile_fail
//! use mandate_sources::text::UntrustedText;
//! let text = UntrustedText::bounded("Buy everything", 64);
//! let leaked: &str = &text;
//! ```
//!
//! ```compile_fail
//! use mandate_sources::text::UntrustedText;
//! let text = UntrustedText::bounded("Buy everything", 64);
//! let leaked = format!("{text}");
//! ```

use core::fmt;

/// Text from a news item or a filing, at most its cap in bytes, cut at a character boundary.
#[derive(Clone, PartialEq, Eq)]
pub struct UntrustedText {
    text: String,
}

impl UntrustedText {
    /// `raw` cut to at most `cap_bytes` bytes, never inside a character.
    pub fn bounded(raw: &str, cap_bytes: usize) -> Self {
        let end = (0..=cap_bytes.min(raw.len()))
            .rev()
            .find(|i| raw.is_char_boundary(*i))
            .unwrap_or(0);
        Self {
            text: raw.get(..end).unwrap_or_default().to_owned(),
        }
    }

    /// The text, untrusted: never to be parsed by deterministic code or shown as advice.
    pub fn as_untrusted_str(&self) -> &str {
        &self.text
    }

    /// Its length in bytes.
    pub fn len_bytes(&self) -> usize {
        self.text.len()
    }

    /// Whether it holds no bytes.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

impl fmt::Debug for UntrustedText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UntrustedText({} bytes)", self.text.len())
    }
}
