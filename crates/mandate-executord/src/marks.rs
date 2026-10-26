//! The executor's risk marks (DEC-2380 item 3): a `MarkUpdated` at `schema_version` 2 on the
//! account stream for an instrument the deployment trades, from the quote its feed handed, so the
//! risk fold and the gate context (journal spec §9.30, DEC-2305) read a mark with its verdict. A
//! mark records what the quote was, sane or not; the fold, not this module, decides what a mark
//! that is not sane, or one outside the regular session, does.

use mandate_accounting::AssetClass;
use mandate_canon::{Key, Object, Value};
use mandate_executor::{EventDraft, EventId, MarketObservation, RiskClock};
use mandate_num::Price;
use mandate_time::{ExchangeCalendar, UtcNanos};

/// The record this module writes, at its version 2 (journal spec §9.30).
const MARK: &str = "MarkUpdated";

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
