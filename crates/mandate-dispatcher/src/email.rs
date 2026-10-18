//! The email channel adapter (notifications spec §4.4, §5.2; E8-11 slice S3b, DEC-700 item 4,
//! DEC-820 item 4, DEC-2508, DEC-2510).
//!
//! **One recipient, bound at construction.** [`EmailProvider`] holds the founder's opaque
//! [`Recipient`] and a [`MailTransport`] already bound to the founder's [`FounderAddress`]. A
//! send to any other recipient, on any other channel, or of the brief is answered
//! `permanent { recipient_not_permitted }` before the transport is called, so nothing is formed or
//! submitted for it (spec §4.4). That reason is not retried, marks no address `unreachable`, and
//! raises no `channel_lost` (§5.2).
//!
//! **The transport takes no recipient.** [`MailTransport::submit`] is given the rendered
//! [`FounderMail`] and the send's [`IdempotencyKey`], never an address: it writes the mail to the
//! address it was built with (rung 1). [`RecordedTransport`] is the recorded-fixture transport,
//! which keeps each submitted message's wire form and sends nothing. The founder-only SMTP
//! transport is a later slice; no other implementor may exist while the general footer is
//! unresolved (§4.4, rung 2).
//!
//! **A transport's failure is a closed code.** [`MailFailure`] holds no provider text, which may
//! echo the message (§5.2), and [`EmailProvider`] maps each code to its outcome (DEC-2510).
//!
//! Every entry point returns a `Result`, so a stub reports `Unimplemented` (DEC-77).

use mandate_notify::email::{FounderAddress, FounderMail};
use mandate_notify::{
    AddressHandle, IdempotencyKey, Notification, NotifyError, Origin, Outcome, Provider, Recipient,
};

/// Why a transport did not take a message: a closed code, holding nothing the provider said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailFailure {
    /// No answer within the send's deadline.
    Timeout,
    /// The provider asked the sender to slow down.
    RateLimited,
    /// The provider or the connection failed in a way a later attempt may not.
    Unavailable,
    /// The provider refused the sender's credentials.
    AuthFailed,
    /// The provider refused the recipient's address.
    AddressRejected,
    /// The message is larger than the provider takes.
    TooLarge,
}

/// The provider's id for a message it took, as journaled on `NoticeAttempted`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submitted {
    pub message_id: String,
}

/// A transport bound at construction to the one address it sends to. It takes no recipient.
pub trait MailTransport {
    /// Submits `mail` to the transport's bound address, keyed by `key`'s digest: the provider's
    /// answer is `Ok(Ok(..))` when it took the message and `Ok(Err(..))` with its closed code when it
    /// did not.
    ///
    /// # Errors
    /// Only when the message could not be formed; a provider's every answer is the inner result.
    fn submit(
        &mut self,
        mail: &FounderMail,
        key: &IdempotencyKey,
    ) -> Result<Result<Submitted, MailFailure>, NotifyError>;
}

/// The recorded-fixture transport: it keeps each submitted message's wire form, addressed to the
/// address it was bound to, and sends nothing. Each submission answers from the script in order,
/// and once the script is spent it takes the message as `recorded-<n>`, the n-th taken from 1.
pub struct RecordedTransport {
    address: FounderAddress,
    script: Vec<MailFailure>,
    captured: Vec<Vec<u8>>,
}

impl RecordedTransport {
    /// A fixture transport bound to `address` that takes every message.
    ///
    /// # Errors
    /// None once implemented. The `Result` is the stub API's shape.
    pub fn bound_to(address: FounderAddress) -> Result<Self, NotifyError> {
        let _ = address;
        Err(NotifyError::Unimplemented { story: "E8-11" })
    }

    /// A fixture transport bound to `address` that answers `script`'s failures first, in order.
    ///
    /// # Errors
    /// None once implemented. The `Result` is the stub API's shape.
    pub fn failing(address: FounderAddress, script: Vec<MailFailure>) -> Result<Self, NotifyError> {
        let _ = (address, script);
        Err(NotifyError::Unimplemented { story: "E8-11" })
    }

    /// The wire form of every message taken, in order.
    ///
    /// # Errors
    /// None once implemented. The `Result` is the stub API's shape.
    pub fn captured(&self) -> Result<&[Vec<u8>], NotifyError> {
        let _ = (&self.address, &self.script, &self.captured);
        Err(NotifyError::Unimplemented { story: "E8-11" })
    }
}

impl MailTransport for RecordedTransport {
    fn submit(
        &mut self,
        mail: &FounderMail,
        key: &IdempotencyKey,
    ) -> Result<Result<Submitted, MailFailure>, NotifyError> {
        let _ = (mail, key);
        Err(NotifyError::Unimplemented { story: "E8-11" })
    }
}

/// The email channel's adapter: the founder's recipient and a transport bound to the founder's
/// address. A mail's links use the origin each send is given.
pub struct EmailProvider<T: MailTransport> {
    founder: Recipient,
    transport: T,
}

impl<T: MailTransport> EmailProvider<T> {
    /// The adapter that mails `founder` alone, through `transport`.
    ///
    /// # Errors
    /// None once implemented. The `Result` is the stub API's shape.
    pub fn new(founder: Recipient, transport: T) -> Result<Self, NotifyError> {
        let _ = (founder, transport);
        Err(NotifyError::Unimplemented { story: "E8-11" })
    }

    /// The transport, for a test to read what it captured.
    ///
    /// # Errors
    /// None once implemented. The `Result` is the stub API's shape.
    pub fn transport(&self) -> Result<&T, NotifyError> {
        let _ = (&self.founder, &self.transport);
        Err(NotifyError::Unimplemented { story: "E8-11" })
    }
}

impl<T: MailTransport> Provider for EmailProvider<T> {
    fn send(
        &mut self,
        origin: &Origin,
        notification: &Notification,
        address: &AddressHandle,
        key: &IdempotencyKey,
    ) -> Result<Outcome, NotifyError> {
        let _ = (origin, notification, address, key);
        Err(NotifyError::Unimplemented { story: "E8-11" })
    }
}
