//! The broker's name for one of our orders: derived from journaled facts, never assigned.

use crate::error::ExecutorError;
use crate::types::EventId;

/// The `event_id` of the agent stream's `IntentProposed`, which journal spec §2 makes the intent's
/// identity. Nothing in this crate mints one, so two runtimes cannot mint the same one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IntentId(pub EventId);

/// The prefix every id this platform derives carries, which is also how a reconciliation tells an
/// order of ours from external activity (trading-domain spec §7.1).
pub const PREFIX: &str = "md-";

/// The broker's name for one of our orders, and our only idempotency key on the broker side.
///
/// There is no `ClientOrderId::new(String)`. The three derivations below and [`ClientOrderId::parse`]
/// are the only ways to obtain one, and each derivation's input is an `event_id`, which the journal
/// guarantees unique — so an id reused across restarts is unrepresentable rather than merely
/// forbidden (E7-2, task brief interpretation 6).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClientOrderId(String);

impl ClientOrderId {
    /// `md-<26 chars of the intent id>` for the order submitted for an intent.
    ///
    /// A pure function of the intent id alone, so every process, every writer epoch, and every
    /// restart derives the same value (E7-2). It is deliberately **not** a function of the attempt
    /// number: trading-domain spec §5.7 requires a resubmission after a confirmed absence to carry
    /// the same id, and an id that depended on the attempt could not.
    pub fn for_intent(intent: &IntentId) -> Result<Self, ExecutorError> {
        let _ = intent;
        Err(ExecutorError::Unimplemented { story: "E7-2" })
    }

    /// `md-<26>-r<26>` for the order that replaces one the broker replaced, derived from the
    /// `OrderStateChanged` event that recorded the replacement, never from a counter
    /// (trading-domain spec §5.7's `replaced` row).
    pub fn for_replacement(origin: &EventId) -> Result<Self, ExecutorError> {
        let _ = origin;
        Err(ExecutorError::Unimplemented { story: "E7-2" })
    }

    /// `md-<26>-p<26>` for a protective order submitted on its own — the OCO after a partial fill,
    /// a re-placement before expiry, a crypto stop-limit — derived from the `ProtectionChanged`
    /// draft that records it (trading-domain spec §5.4).
    pub fn for_protection(origin: &EventId) -> Result<Self, ExecutorError> {
        let _ = origin;
        Err(ExecutorError::Unimplemented { story: "E7-4" })
    }

    /// Reads an id back from the broker, validating the grammar. An id that does not parse is not
    /// ours, which is what makes an order carrying one external activity rather than a mystery
    /// (trading-domain spec §7.1, §11).
    pub fn parse(raw: &str) -> Result<Self, ExecutorError> {
        let _ = raw;
        Err(ExecutorError::Unimplemented { story: "E7-2" })
    }

    /// The wire form, which is what the submission sets Alpaca's `client_order_id` to.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A test-only constructor, so the crate's own unit tests can hold an id while every public
/// constructor is a stub. Outside `cfg(test)` there is still no way to build one from a string.
#[cfg(test)]
impl ClientOrderId {
    pub(crate) fn seeded_for_tests(raw: &str) -> Self {
        Self(raw.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::ClientOrderId;

    #[test]
    fn the_wire_form_is_the_id_itself() {
        assert_eq!(
            ClientOrderId::seeded_for_tests("md-01JABC").as_str(),
            "md-01JABC"
        );
    }
}
