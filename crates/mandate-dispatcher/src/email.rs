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
//! Every entry point returns a `Result`, the shape its DEC-77 stub had; those that cannot fail
//! always answer `Ok`.

use mandate_notify::email::{FounderAddress, FounderMail, founder_mail};
use mandate_notify::{
    AddressHandle, IdempotencyKey, Notification, NotifyError, Origin, Outcome, Provider,
    PushChannel, Reason, Recipient,
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
///
/// It sends nothing, so it ignores the idempotency key: a retried send is captured again. The
/// founder-only SMTP transport must use the key, so a retry the provider already took is not
/// mailed twice (spec §5.1, §5.2).
pub struct RecordedTransport {
    address: FounderAddress,
    script: Vec<MailFailure>,
    captured: Vec<Vec<u8>>,
}

impl RecordedTransport {
    /// A fixture transport bound to `address` that takes every message.
    ///
    /// # Errors
    /// None: it always answers `Ok`. The `Result` is the shape its DEC-77 stub had.
    pub fn bound_to(address: FounderAddress) -> Result<Self, NotifyError> {
        Self::failing(address, Vec::new())
    }

    /// A fixture transport bound to `address` that answers `script`'s failures first, in order.
    ///
    /// # Errors
    /// None: it always answers `Ok`. The `Result` is the shape its DEC-77 stub had.
    pub fn failing(address: FounderAddress, script: Vec<MailFailure>) -> Result<Self, NotifyError> {
        let mut script = script;
        script.reverse();
        Ok(Self {
            address,
            script,
            captured: Vec::new(),
        })
    }

    /// The wire form of every message taken, in order.
    ///
    /// # Errors
    /// None: it always answers `Ok`. The `Result` is the shape its DEC-77 stub had.
    pub fn captured(&self) -> Result<&[Vec<u8>], NotifyError> {
        Ok(&self.captured)
    }
}

impl MailTransport for RecordedTransport {
    fn submit(
        &mut self,
        mail: &FounderMail,
        key: &IdempotencyKey,
    ) -> Result<Result<Submitted, MailFailure>, NotifyError> {
        let _ = key;
        if let Some(failure) = self.script.pop() {
            return Ok(Err(failure));
        }
        self.captured.push(mail.wire(&self.address)?);
        Ok(Ok(Submitted {
            message_id: format!("recorded-{}", self.captured.len()),
        }))
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
    /// None: it always answers `Ok`. The `Result` is the shape its DEC-77 stub had.
    pub fn new(founder: Recipient, transport: T) -> Result<Self, NotifyError> {
        Ok(Self { founder, transport })
    }

    /// The transport, for a test to read what it captured.
    ///
    /// # Errors
    /// None: it always answers `Ok`. The `Result` is the shape its DEC-77 stub had.
    pub fn transport(&self) -> Result<&T, NotifyError> {
        Ok(&self.transport)
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
        let refused = Outcome::Permanent {
            reason: Reason::RecipientNotPermitted,
        };
        if address.channel != PushChannel::Email || address.recipient != self.founder {
            return Ok(refused);
        }
        let mail = match founder_mail(origin, notification) {
            Ok(mail) => mail,
            Err(NotifyError::MailNotPermitted) => return Ok(refused),
            Err(e) => return Err(e),
        };
        Ok(match self.transport.submit(&mail, key)? {
            Ok(submitted) => Outcome::Accepted {
                provider_message_id: submitted.message_id,
            },
            Err(failure) => outcome_of(failure),
        })
    }
}

/// A transport failure's outcome (spec §5.2, DEC-2510 item 3): the failures a later attempt may
/// clear are retryable, and the provider's refusals of the sender, the address, or the size are
/// permanent.
fn outcome_of(failure: MailFailure) -> Outcome {
    match failure {
        MailFailure::Timeout => Outcome::Retryable {
            reason: Reason::Timeout,
        },
        MailFailure::RateLimited => Outcome::Retryable {
            reason: Reason::RateLimited,
        },
        MailFailure::Unavailable => Outcome::Retryable {
            reason: Reason::ProviderError,
        },
        MailFailure::AuthFailed => Outcome::Permanent {
            reason: Reason::AuthFailed,
        },
        MailFailure::AddressRejected => Outcome::Permanent {
            reason: Reason::AddressRejected,
        },
        MailFailure::TooLarge => Outcome::Permanent {
            reason: Reason::TooLarge,
        },
    }
}
