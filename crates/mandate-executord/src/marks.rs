//! The executor's risk marks (DEC-2380 item 3): a `MarkUpdated` at `schema_version` 2 on the
//! account stream for an instrument the deployment trades, from the quote its feed handed, so the
//! risk fold and the gate context (journal spec §9.30, DEC-2305) read a mark with its verdict. A
//! mark records what the quote was, sane or not; the fold, not this module, decides what a mark
//! that is not sane, or one outside the regular session, does.

use std::collections::BTreeSet;
use std::time::Duration;

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_canon::{Key, Object, Value};
use mandate_executor::{EventDraft, EventId, MarketObservation, RiskClock};
use mandate_num::Price;
use mandate_time::{ExchangeCalendar, UtcNanos};

/// The record this module writes, at its version 2 (journal spec §9.30).
const MARK: &str = "MarkUpdated";

/// The longest interval between two marks of an instrument: well inside the gate's minute of
/// freshness (`deployment::FRESH`). A deployment whose quotes go stale sooner is marked more often
/// ([`mark_every`]).
pub const MARK_EVERY: Duration = Duration::from_secs(15);

/// The shortest interval between two marks of an instrument.
pub const MARK_AT_MOST_EVERY: Duration = Duration::from_secs(1);

/// How often to mark, given each deployment's quote staleness limit, which is also how long its
/// risk fold lets a held instrument go without a sane mark before `stale_mark` (mandate spec §5.2;
/// the risk book opens with it, DEC-2804). Half the smallest limit, so a mark always lands before
/// the fold's age reaches it at the next tick (DEC-2844), within [`MARK_AT_MOST_EVERY`] and
/// [`MARK_EVERY`]. With no limit, [`MARK_EVERY`].
pub fn mark_every(limits: impl IntoIterator<Item = Duration>) -> Duration {
    limits
        .into_iter()
        .filter_map(|limit| limit.checked_div(2))
        .min()
        .map_or(MARK_EVERY, |half| {
            half.clamp(MARK_AT_MOST_EVERY, MARK_EVERY)
        })
}

/// The instruments the executor marks: those the deployments it serves may open, so the risk
/// fold and the gate context hold a fresh mark before an opening is proposed.
pub trait Marked {
    /// The instruments to mark now.
    fn instruments(&self) -> BTreeSet<InstrumentId>;

    /// How often to mark them ([`mark_every`]); [`MARK_EVERY`] by default.
    fn every(&self) -> Duration {
        MARK_EVERY
    }
}

/// No instrument to mark: a wiring that knows no deployment's instruments.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoMarks;

impl Marked for NoMarks {
    fn instruments(&self) -> BTreeSet<InstrumentId> {
        BTreeSet::new()
    }
}

/// The `MarkUpdated` version-2 draft of `observation` for an instrument of `class`, with event id
/// `event_id`, at the executor's latest tick `risk_clock`, from the feed named `feed`:
/// - `price` is the quote's bid, else its last trade, else its mark, under `source` `quote`;
/// - `bid` is the quote's bid, and `sane` its verdict, never sane without a positive bid (rule
///   244);
/// - `session` is `crypto` for a crypto instrument, else the US-equities session `calendar` places
///   `risk_clock` in.
///
/// `None`, and no mark, for a quote with no price at all, or an equity's mark while the market is
/// closed or at an instant outside the calendar, which no session names.
pub fn mark_draft(
    observation: &MarketObservation,
    class: AssetClass,
    calendar: &ExchangeCalendar,
    feed: &str,
    event_id: EventId,
    risk_clock: RiskClock,
) -> Option<EventDraft> {
    let at = UtcNanos::from_parts(risk_clock.secs(), 0).ok()?;
    let session = match class {
        AssetClass::Crypto => "crypto",
        AssetClass::UsEquity => calendar.session_at(at).ok()??.as_str(),
    };
    let price = observation
        .bid
        .or(observation.last_trade)
        .or(observation.mark)?;
    let bid = observation.bid;
    let sane = observation.sane && bid.is_some();
    let decimal = |price: Price| Value::Str(price.to_string());
    let mut payload = Object::new();
    for (name, value) in [
        (
            "instrument_id",
            Value::Str(observation.instrument.as_str().to_owned()),
        ),
        ("price", decimal(price)),
        ("source", Value::Str("quote".to_owned())),
        ("feed", Value::Str(feed.to_owned())),
        ("bid", bid.map_or(Value::Null, decimal)),
        ("sane", Value::Bool(sane)),
        ("session", Value::Str(session.to_owned())),
        ("risk_clock", Value::Str(at.to_string())),
    ] {
        payload.insert(Key::new(name).ok()?, value);
    }
    Some(EventDraft {
        event_id,
        event_type: MARK.to_owned(),
        schema_version: 2,
        config_refs: Object::new(),
        causation_id: None,
        payload: Value::Object(payload),
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{MARK_AT_MOST_EVERY, MARK_EVERY, mark_every};

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    /// DEC-2844: half the smallest staleness limit, never under a second nor over fifteen; the
    /// demo's ten seconds marks every five, so the fold's mark age never reaches ten at a tick.
    #[test]
    fn marks_come_at_half_the_smallest_staleness_limit() {
        assert_eq!(mark_every([secs(10)]), secs(5));
        assert_eq!(mark_every([secs(40), secs(10), secs(60)]), secs(5));
        assert_eq!(mark_every([secs(60)]), MARK_EVERY);
        assert_eq!(mark_every([secs(31)]), MARK_EVERY);
        assert_eq!(mark_every([secs(1)]), MARK_AT_MOST_EVERY);
        assert_eq!(mark_every([secs(3)]), Duration::from_millis(1_500));
        assert_eq!(mark_every(Vec::new()), MARK_EVERY);
    }
}
