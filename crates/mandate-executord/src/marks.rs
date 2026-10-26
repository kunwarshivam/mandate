//! The executor's risk marks (DEC-2380 item 3): a `MarkUpdated` at `schema_version` 2 on the
//! account stream for an instrument the deployment trades, from the quote its feed handed, so the
//! risk fold and the gate context (journal spec §9.30, DEC-2305) read a mark with its verdict. A
//! mark records what the quote was, sane or not; the fold, not this module, decides what a mark
//! that is not sane, or one outside the regular session, does.

use mandate_accounting::AssetClass;
use mandate_executor::{EventDraft, EventId, MarketObservation, RiskClock};
use mandate_time::ExchangeCalendar;

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
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn mark_draft(
    _observation: &MarketObservation,
    _class: AssetClass,
    _calendar: &ExchangeCalendar,
    _feed: &str,
    _event_id: EventId,
    _risk_clock: RiskClock,
) -> Option<EventDraft> {
    todo!()
}
