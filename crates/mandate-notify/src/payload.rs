//! The notice id, the payload, and the link (notifications spec §4.2, §4.3; NT-1, NT-4).

use mandate_canon::Value;

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
/// minted, never derived: it has no constructor from text or from any event id, so a ULID, whose
/// leading 48 bits are its creation time, can never be sent as one (NT-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NoticeId([u8; 16]);

impl NoticeId {
    /// Mints a fresh id from `random`, taking nothing from the notice's cause.
    ///
    /// # Errors
    /// [`NotifyError::EntropyUnavailable`] when the source fails.
    pub fn mint(random: &mut dyn SecureRandom) -> Result<Self, NotifyError> {
        let _ = random;
        Err(NotifyError::Unimplemented { story: "E8-9" })
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
    let _ = notification;
    Err(NotifyError::Unimplemented { story: "E8-9" })
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
        let _ = origin;
        Err(NotifyError::Unimplemented { story: "E8-9" })
    }
}

/// The link every channel renders: `<origin>/n/<notice id>` and nothing else (NT-4).
///
/// # Errors
/// Never once implemented.
pub fn link(origin: &Origin, notice: &NoticeId) -> Result<String, NotifyError> {
    let _ = (origin, notice);
    Err(NotifyError::Unimplemented { story: "E8-9" })
}
