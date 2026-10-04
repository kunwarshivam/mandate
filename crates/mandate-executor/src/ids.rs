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

/// The prefix of the triggered-stop watchdog's own intent id, `w-<event>` (§2.3, DEC-160 (11)).
pub(crate) const WATCHDOG: &str = "w-";

/// The prefix of the kill switch's own sell id, `k-<record>-<ordinal>` (§2.3's pattern for an
/// order the executor originates, DEC-160 (11)): the `KillSwitchActivated` that planned it.
pub(crate) const KILL: &str = "k-";

/// The longest `client_order_id` Alpaca accepts.
const MAX_LEN: usize = 128;

/// The broker's name for one of our orders, and our only idempotency key on the broker side.
///
/// There is no `ClientOrderId::new(String)`. The three derivations below and [`ClientOrderId::parse`]
/// are the only ways to obtain one, and each derivation's input is an `event_id`, which the journal
/// guarantees unique — so an id reused across restarts is unrepresentable rather than merely
/// forbidden (E7-2, task brief interpretation 6).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClientOrderId(String);

impl ClientOrderId {
    /// `md-<intent id>` for the order submitted for an intent.
    ///
    /// A pure function of the intent id alone, so every process, every writer epoch, and every
    /// restart derives the same value (E7-2). It is deliberately **not** a function of the attempt
    /// number: trading-domain spec §5.7 requires a resubmission after a confirmed absence to carry
    /// the same id, and an id that depended on the attempt could not. An intent id is a ULID, so
    /// its body is alphanumeric; anything else is refused rather than escaped.
    ///
    /// The executor's own orders are intents too: the triggered-stop watchdog's exit is
    /// `w-<event>` from its journaled record (trading-domain spec §2.3, DEC-160 (11)), and the
    /// kill switch's own sell is `k-<record>-<ordinal>` from its `KillSwitchActivated`, so
    /// `md-w-<event>` and `md-k-<record>-<ordinal>`. An event id is a ULID in production, and the
    /// `md-w-` and `md-k-` prefixes keep both apart from every agent intent's id whatever its
    /// event id's hyphens (DEC-260 (10)); one that would read back as a protective order's is
    /// refused, as [`Self::for_replacement`] refuses.
    pub fn for_intent(intent: &IntentId) -> Result<Self, ExecutorError> {
        let raw = intent.0.0.as_str();
        let executor_originated = match (raw.strip_prefix(WATCHDOG), raw.strip_prefix(KILL)) {
            (Some(record), _) | (None, Some(record)) => {
                !record.starts_with('-')
                    && !record.ends_with('-')
                    && record
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            }
            (None, None) => raw.bytes().all(|b| b.is_ascii_alphanumeric()),
        };
        if raw.is_empty() || raw == WATCHDOG || raw == KILL || !executor_originated {
            return Err(malformed(raw));
        }
        let id = Self::parse(&format!("{PREFIX}{raw}"))?;
        if id.protected_entry().is_some() {
            return Err(malformed(id.as_str()));
        }
        Ok(id)
    }

    /// `md-r-<origin>` for the order that replaces one the broker replaced, derived from the
    /// `OrderStateChanged` event that recorded the replacement, never from a counter
    /// (trading-domain spec §5.7's `replaced` row). An intent-derived id has no hyphen after the
    /// prefix, so the derivations can never meet; and an origin that would make the id read back
    /// as a protective order's (§2.3: everything before a `-p` that is itself an id of ours, as
    /// `md-r-p1` would name `md-r`) is refused rather than escaped.
    pub fn for_replacement(origin: &EventId) -> Result<Self, ExecutorError> {
        let id = Self::parse(&format!("{PREFIX}r-{}", origin.0))?;
        if id.protected_entry().is_some() {
            return Err(malformed(id.as_str()));
        }
        Ok(id)
    }

    /// `{entry}-p{protection}` for a protective order the platform places — a bracket's or OCO's
    /// parent, the OCO after a partial fill, a re-placement before expiry, a crypto stop-limit —
    /// named for the entry it protects and derived from the `ProtectionChanged` that records it
    /// (trading-domain spec §2.3, DEC-160). A re-placement keeps the entry and takes its own
    /// event. The event's id must be alphanumeric, like an intent's, so the entry reads back as
    /// everything before the last `-p`; anything else is refused rather than escaped.
    pub fn for_protection(
        entry: &ClientOrderId,
        protection: &EventId,
    ) -> Result<Self, ExecutorError> {
        let raw = protection.0.as_str();
        if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(malformed(raw));
        }
        Self::parse(&format!("{}-p{raw}", entry.as_str()))
    }

    /// Rung `step` of the exit price ladder for the exit this id names: the id itself for the
    /// first rung, `…-l{step}` for every later one, each a new order (trading-domain spec §2.3,
    /// §5.6, DEC-160 (10)). The step comes from the journaled rung count, so a restart derives the
    /// same id.
    pub fn rung(&self, step: u32) -> Result<Self, ExecutorError> {
        match step {
            0 => Ok(self.clone()),
            step => Self::parse(&format!("{}-l{step}", self.0)),
        }
    }

    /// A protective parent's leg: `…-tp` for the take-profit, `…-sl` for the stop (§2.3).
    pub fn leg(&self, leg: Leg) -> Result<Self, ExecutorError> {
        let suffix = match leg {
            Leg::TakeProfit => "tp",
            Leg::Stop => "sl",
        };
        Self::parse(&format!("{}-{suffix}", self.0))
    }

    /// Whether this id names a kill switch's own sell, `md-k-<record>-<ordinal>` (§2.3): the
    /// record the fold reads to end the plan the sell was planned by.
    pub fn kill_sell(&self) -> bool {
        self.0
            .strip_prefix(PREFIX)
            .and_then(|body| body.strip_prefix(KILL))
            .is_some_and(|rest| {
                !rest.starts_with('-')
                    && !rest.ends_with('-')
                    && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
    }

    /// The entry a protective order's id names — everything before its last `-p` — when that
    /// reads back as an id of ours (§2.3); `None` for any other id.
    pub fn protected_entry(&self) -> Option<Self> {
        self.0
            .rsplit_once("-p")
            .and_then(|(entry, _)| Self::parse(entry).ok())
    }

    /// Reads an id back from the broker, validating the grammar: the platform prefix, then one or
    /// more ASCII letters, digits, or hyphens, within Alpaca's 128-byte bound. An id that does not
    /// parse is not ours, which is what makes an order carrying one external activity rather than
    /// a mystery (trading-domain spec §7.1, §11).
    pub fn parse(raw: &str) -> Result<Self, ExecutorError> {
        let body = raw.strip_prefix(PREFIX).ok_or_else(|| malformed(raw))?;
        let grammatical = !body.is_empty()
            && raw.len() <= MAX_LEN
            && body.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
        if grammatical {
            Ok(Self(raw.to_owned()))
        } else {
            Err(malformed(raw))
        }
    }

    /// The wire form, which is what the submission sets Alpaca's `client_order_id` to.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Which leg of a protective parent an id names (trading-domain spec §2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leg {
    TakeProfit,
    Stop,
}

fn malformed(raw: &str) -> ExecutorError {
    ExecutorError::MalformedClientOrderId {
        raw: raw.to_owned(),
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
    use super::{ClientOrderId, IntentId, Leg};
    use crate::error::ExecutorError;
    use crate::types::EventId;

    #[test]
    fn a_protective_order_is_named_for_its_entry_and_its_legs_for_it() -> Result<(), ExecutorError>
    {
        let entry = ClientOrderId::for_intent(&IntentId(EventId("01JENTRY".to_owned())))?;
        let parent = ClientOrderId::for_protection(&entry, &EventId("01JPROT".to_owned()))?;
        assert_eq!(parent.as_str(), "md-01JENTRY-p01JPROT");
        assert_eq!(
            parent.leg(Leg::TakeProfit)?.as_str(),
            "md-01JENTRY-p01JPROT-tp"
        );
        assert_eq!(parent.leg(Leg::Stop)?.as_str(), "md-01JENTRY-p01JPROT-sl");
        for id in [
            parent.clone(),
            parent.leg(Leg::TakeProfit)?,
            parent.leg(Leg::Stop)?,
        ] {
            assert_eq!(
                id.protected_entry(),
                Some(entry.clone()),
                "{id:?} names its entry"
            );
        }
        assert_eq!(entry.protected_entry(), None, "an entry names no entry");
        Ok(())
    }

    #[test]
    fn a_protection_event_that_is_not_alphanumeric_is_refused() -> Result<(), ExecutorError> {
        let entry = ClientOrderId::for_intent(&IntentId(EventId("01JENTRY".to_owned())))?;
        for raw in ["", "e-1-2-0", "clock:ws1"] {
            assert_eq!(
                ClientOrderId::for_protection(&entry, &EventId(raw.to_owned())),
                Err(ExecutorError::MalformedClientOrderId {
                    raw: raw.to_owned()
                }),
                "{raw:?}"
            );
        }
        Ok(())
    }

    /// #258 round 1, minor 1: the entry is everything before the **last** `-p`, and only when
    /// that reads back as an id of ours.
    #[test]
    fn the_entry_is_read_back_from_the_last_protection_suffix_and_must_be_ours()
    -> Result<(), ExecutorError> {
        for (raw, entry) in [
            ("md-a-p1-p2", Some("md-a-p1")),
            ("md-a-p1-p2-sl", Some("md-a-p1")),
            ("md-p1", None),
            ("md-a", None),
        ] {
            assert_eq!(
                ClientOrderId::parse(raw)?.protected_entry(),
                entry.map(ClientOrderId::parse).transpose()?,
                "{raw}"
            );
        }
        Ok(())
    }

    /// #258 round 1, minor 2: no replacement id reads back as a protective order's.
    /// §2.3, DEC-160 (11), DEC-260 (10): the watchdog's own intent `w-<record>` takes letters,
    /// digits and inner hyphens after `w-`; an agent's intent stays alphanumeric; nothing empty,
    /// nothing with an outer hyphen, and nothing that reads back as a protective order's.
    #[test]
    fn only_a_watchdog_intent_may_carry_hyphens() -> Result<(), ExecutorError> {
        let id = |raw: &str| ClientOrderId::for_intent(&IntentId(EventId(raw.to_owned())));
        assert_eq!(id("w-e2-h8-o0")?.as_str(), "md-w-e2-h8-o0");
        assert_eq!(id("w-01JABC")?.as_str(), "md-w-01JABC");
        assert_eq!(id("01JABC")?.as_str(), "md-01JABC");
        for refused in [
            "", "w-", "w--e1", "w-e1-", "w-e_1", "01J-ABC", "-w-e1", "w-e1-p2",
        ] {
            assert!(id(refused).is_err(), "{refused:?} is refused");
        }
        Ok(())
    }

    #[test]
    fn a_replacement_id_never_names_an_entry() -> Result<(), ExecutorError> {
        for raw in ["p1", "e1-p2", "x-pq"] {
            assert_eq!(
                ClientOrderId::for_replacement(&EventId(raw.to_owned())),
                Err(ExecutorError::MalformedClientOrderId {
                    raw: format!("md-r-{raw}")
                }),
                "{raw:?}"
            );
        }
        for raw in ["01JORIGIN", "e1-h5-o2"] {
            let id = ClientOrderId::for_replacement(&EventId(raw.to_owned()))?;
            assert_eq!(id.as_str(), format!("md-r-{raw}"));
            assert_eq!(id.protected_entry(), None, "{raw:?}");
        }
        Ok(())
    }

    #[test]
    fn the_wire_form_is_the_id_itself() {
        assert_eq!(
            ClientOrderId::seeded_for_tests("md-01JABC").as_str(),
            "md-01JABC"
        );
    }
}
