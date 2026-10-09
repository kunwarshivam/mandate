//! The provider interface every channel adapter implements (notifications spec §5.2, DEC-710 item
//! 8, DEC-712), and the fixture provider tests send through.

use crate::{AddressHandle, IdempotencyKey, Notification, NotifyError, Origin};

/// Why a send did not go through: spec §5.2's closed enum, `address_missing` (an active address
/// whose vault entry is gone) included (DEC-712). The dispatcher's own `not_pending` and
/// `retry_window_ended` (§5.3) are not a provider's to return. No provider error text is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Reason {
    Timeout,
    RateLimited,
    ProviderError,
    AddressRejected,
    AuthFailed,
    TooLarge,
    RecipientNotPermitted,
    AddressMissing,
    Bounced,
    Complained,
    Unsubscribed,
}

impl Reason {
    #[rustfmt::skip]
    pub const ALL: [Self; 11] = [
        Self::Timeout, Self::RateLimited, Self::ProviderError, Self::AddressRejected,
        Self::AuthFailed, Self::TooLarge, Self::RecipientNotPermitted, Self::AddressMissing,
        Self::Bounced, Self::Complained, Self::Unsubscribed,
    ];

    /// The reason as `NoticeAttempted` journals it, as spec §5.2 writes it (`address_missing`).
    ///
    /// # Errors
    /// Never once implemented: every reason has a key.
    pub fn key(self) -> Result<&'static str, NotifyError> {
        let _ = self;
        Err(NotifyError::Unimplemented { story: "E8-9" })
    }
}

/// What a send returns (spec §5.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Accepted { provider_message_id: String },
    Retryable { reason: Reason },
    Permanent { reason: Reason },
}

/// One channel's adapter. It takes no string from its caller: it renders the closed notice under
/// the fixed origin itself, dereferences the handle through the vault, and gives the provider the
/// key's digest (spec §5.2, DEC-710 items 5, 6).
pub trait Provider {
    /// Sends `notification` to `address`.
    ///
    /// # Errors
    /// Only when the send could not be formed; a provider's every answer is an [`Outcome`].
    fn send(
        &mut self,
        origin: &Origin,
        notification: &Notification,
        address: &AddressHandle,
        key: &IdempotencyKey,
    ) -> Result<Outcome, NotifyError>;
}

/// One accepted send as the fixture records it: the handle, the key's digest, and the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    pub address: AddressHandle,
    pub key: String,
    pub message: String,
}

/// A provider that sends nothing: it holds a fixture vault of addresses, accepts a send to a held
/// handle as `fixture-<n>` (the n-th accepted send, from 1) and records it, and answers
/// `permanent { address_missing }` for any other handle, recording nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureProvider {
    vault: Vec<(AddressHandle, String)>,
    sent: Vec<Sent>,
}

impl FixtureProvider {
    /// A fixture provider whose vault holds `vault`'s addresses, which it never records.
    ///
    /// # Errors
    /// Never once implemented.
    pub fn holding(vault: Vec<(AddressHandle, String)>) -> Result<Self, NotifyError> {
        let _ = vault;
        Err(NotifyError::Unimplemented { story: "E8-9" })
    }

    /// Every accepted send, in order.
    ///
    /// # Errors
    /// Never once implemented.
    pub fn sent(&self) -> Result<&[Sent], NotifyError> {
        let _ = self;
        Err(NotifyError::Unimplemented { story: "E8-9" })
    }
}

impl Provider for FixtureProvider {
    fn send(
        &mut self,
        origin: &Origin,
        notification: &Notification,
        address: &AddressHandle,
        key: &IdempotencyKey,
    ) -> Result<Outcome, NotifyError> {
        let _ = (self, origin, notification, address, key);
        Err(NotifyError::Unimplemented { story: "E8-9" })
    }
}
