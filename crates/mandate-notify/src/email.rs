//! The email channel's message (notifications spec §4.4; DEC-438 items 22 to 24, DEC-700 item 4,
//! DEC-820 items 1 and 4; E8-11, DEC-2508).
//!
//! **Only the founder's own mail exists.** While the general footer is unresolved, the one mail
//! this crate renders is [`FounderMail`], whose footer is exactly [`FOUNDER_FOOTER`]. It holds no
//! recipient: the address is a [`FounderAddress`], bound by the transport at construction, and
//! [`FounderMail::wire`] writes it as the message's one `To`, with no `Cc` and no `Bcc` (rung 1).
//! A [`FounderAddress`] parses exactly one plain address, so a list, a display name, or a header
//! smuggled through a line break cannot become a second recipient.
//!
//! **The brief is not mailed yet.** `info` mail needs `List-Unsubscribe` pointing at the
//! workspace's one-click endpoint (spec §5.7), which does not exist, and its unsubscribe wording is
//! counsel's (DEC-79). So [`founder_mail`] refuses [`TextKey::BriefReady`] with
//! [`NotifyError::MailNotPermitted`] (DEC-2508), and no mail carries `List-Unsubscribe`.
//!
//! **Nothing personal but the address.** The subject is the notice's fixed text and the body is
//! the fixed text, [`OPEN_SENTENCE`], the link and the footer, with no greeting (NT-1). The HTML
//! part fetches nothing: no image, stylesheet, or script (spec §4.4). An address has no `Debug` or
//! `Display`, so it reaches no log (NT-2).
//!
//! Every entry point returns a `Result`, so a stub reports [`NotifyError::Unimplemented`]
//! (DEC-77).

use crate::{Notification, NotifyError, Origin, TextKey};

/// The sentence every mail body carries between the text and the link (spec §4.4).
pub const OPEN_SENTENCE: &str = "Open Owlhead to see it.";

/// `[[EMAIL-FOOTER]]` for mail addressed only to the founder's own address (DEC-820 item 4).
pub const FOUNDER_FOOTER: &str = "Sent by your Mandate workspace to its owner.";

/// The fixed no-reply sender on the notification domain (DEC-820 item 1, DEC-2508). Nothing reads
/// what is sent to it (NT-3).
pub const SENDER: &str = "no-reply@notify.owlhead.ai";

/// The one address the founder-only transport sends to, as the deployment's configuration names it
/// and the vault holds it. No `Debug` or `Display` (NT-2).
#[derive(Clone, PartialEq, Eq)]
pub struct FounderAddress(String);

impl FounderAddress {
    /// Reads exactly one plain address, `local@domain`, in printable ASCII: no space, no list
    /// separator, no display name, no comment, no quote, and no line break.
    ///
    /// # Errors
    /// [`NotifyError::NotAnAddress`] for anything else; the error names no part of the text.
    pub fn parse(text: &str) -> Result<Self, NotifyError> {
        let _ = text;
        Err(NotifyError::Unimplemented { story: "E8-11" })
    }
}

/// One rendered mail to the founder, holding no recipient (spec §4.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FounderMail {
    /// The notice's fixed text.
    pub subject: String,
    /// The plain-text part: the text, [`OPEN_SENTENCE`], the link, and [`FOUNDER_FOOTER`].
    pub text: String,
    /// The HTML part: the same four, fetching nothing.
    pub html: String,
}

/// The mail for `notification` under `origin`.
///
/// # Errors
/// [`NotifyError::MailNotPermitted`] for [`TextKey::BriefReady`] (DEC-2508).
pub fn founder_mail(
    origin: &Origin,
    notification: &Notification,
) -> Result<FounderMail, NotifyError> {
    let _ = (origin, notification, TextKey::BriefReady);
    Err(NotifyError::Unimplemented { story: "E8-11" })
}

impl FounderMail {
    /// The message as it is submitted: `From` [`SENDER`], `To` `to` and nobody else, `Subject`,
    /// `MIME-Version`, and a `multipart/alternative` body of the text and HTML parts, with CRLF
    /// line ends.
    ///
    /// # Errors
    /// None once implemented. The `Result` is the stub API's shape.
    pub fn wire(&self, to: &FounderAddress) -> Result<Vec<u8>, NotifyError> {
        let _ = to;
        Err(NotifyError::Unimplemented { story: "E8-11" })
    }
}
