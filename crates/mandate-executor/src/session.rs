//! The session an exit goes in, derived from the committed US-equities calendar at the step's
//! clock, never from a label a caller supplies (trading-domain spec §4.3, §5.2, §9.4).
//!
//! Pre-market and after-hours take exits only, as limit orders with `extended_hours`; the
//! overnight session is disabled for v1 (DEC-30); crypto trades continuously.

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_time::{ExchangeCalendar, Session, SessionSpan, UtcNanos, new_york_date_and_hour};

use crate::gate::{SESSION_CLOSED, SESSION_UNKNOWN};
use crate::ports::Ports;
use crate::types::{Purpose, RiskClock};

/// §4.3's closing auction window: the last 10 minutes of the regular session, the spec's default.
const CLOSE_WINDOW_S: i64 = 600;

/// Where an instrument's order may go at a moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Venue {
    /// The regular session, or crypto's continuous one: an order goes as it is.
    Regular,
    /// The regular session's closing auction window (§4.3).
    Closing,
    /// Pre-market or after-hours: an exit goes as a limit with `extended_hours = true`.
    Extended,
    /// The overnight session or a closed market, where v1 trades nothing: an order goes as a
    /// regular-session limit, which the broker queues to its next eligible session (§5.2).
    Closed,
}

/// The US-equities session span `instrument` is in at `at`, or `None` for crypto, for an instant
/// the calendar does not cover, and for a closed market. An instant outside the calendar is no
/// session v1 names, so it changes nothing: an order goes as a regular-session limit, which the
/// broker queues to its next eligible session (§5.2), and nothing is held (DEC-260 (13)).
fn span(ports: &Ports<'_>, instrument: &InstrumentId, at: RiskClock) -> Option<SessionSpan> {
    if ports.instruments.asset_class(instrument) != Some(AssetClass::UsEquity) {
        return None;
    }
    let calendar = ExchangeCalendar::us_equities().ok()?;
    let now = UtcNanos::from_parts(at.secs(), 0).ok()?;
    let (today, _) = new_york_date_and_hour(now).ok()?;
    [Some(today), today.next().ok()]
        .into_iter()
        .flatten()
        .filter_map(|date| calendar.sessions(date).ok())
        .flatten()
        .find(|span| span.contains(now))
}

/// Whether `at` falls inside the calendar's validity range, which is what tells a closed market
/// from an instant the calendar cannot answer for.
fn covered(at: RiskClock) -> bool {
    let calendar = ExchangeCalendar::us_equities().ok();
    UtcNanos::from_parts(at.secs(), 0)
        .ok()
        .and_then(|now| new_york_date_and_hour(now).ok())
        .zip(calendar)
        .is_some_and(|((date, _), calendar)| calendar.is_trading_day(date).is_ok())
}

/// The venue for `instrument` at `at`.
pub(crate) fn venue(ports: &Ports<'_>, instrument: &InstrumentId, at: RiskClock) -> Venue {
    let equity = ports.instruments.asset_class(instrument) == Some(AssetClass::UsEquity);
    match span(ports, instrument, at) {
        Some(found) => match found.session() {
            Session::PreMarket | Session::AfterHours => Venue::Extended,
            Session::Overnight => Venue::Closed,
            Session::Regular if found.end().secs().saturating_sub(at.secs()) <= CLOSE_WINDOW_S => {
                Venue::Closing
            }
            Session::Regular => Venue::Regular,
        },
        None if equity && covered(at) => Venue::Closed,
        None => Venue::Regular,
    }
}

/// Why an equity exit at `at` waits for the next session (DEC-260 (13), the coordinator's ruling
/// D1): `session_closed` while no v1 session is open and the calendar names the next pre-market
/// open, where the hold ends; `session_unknown` when the calendar cannot name one, so the exit
/// stays held, never sent overnight or queued blind. `None` while a session is open.
pub(crate) fn closed_hold(
    ports: &Ports<'_>,
    instrument: &InstrumentId,
    at: RiskClock,
) -> Option<&'static str> {
    if venue(ports, instrument, at) != Venue::Closed {
        return None;
    }
    Some(if next_open(at).is_some() {
        SESSION_CLOSED
    } else {
        SESSION_UNKNOWN
    })
}

/// The start of the first pre-market session after `at`, looking a fortnight ahead, the longest
/// a calendar closes for; `None` when the calendar ends first.
fn next_open(at: RiskClock) -> Option<i64> {
    let calendar = ExchangeCalendar::us_equities().ok()?;
    let now = UtcNanos::from_parts(at.secs(), 0).ok()?;
    let (mut date, _) = new_york_date_and_hour(now).ok()?;
    for _ in 0..14 {
        let found =
            calendar.sessions(date).ok()?.into_iter().find(|span| {
                span.session() == Session::PreMarket && span.start().secs() > at.secs()
            });
        if let Some(open) = found {
            return Some(open.start().secs());
        }
        date = date.next().ok()?;
    }
    None
}

/// Whether an order of `purpose` goes as an extended-hours limit: an exit in pre-market or
/// after-hours that may trade there (§4.3) — a risk exit, a protective order or a flatten. An
/// opening never does (§3.1), and a discretionary or an unconfirmed owner exit waits for the
/// regular session (§5.5, §9.6), as the broker queues a regular-session limit.
pub(crate) fn extended_hours(
    ports: &Ports<'_>,
    instrument: &InstrumentId,
    at: RiskClock,
    purpose: Purpose,
) -> bool {
    purpose.exempt_from_pacing() && venue(ports, instrument, at) == Venue::Extended
}

/// The second from which a resting stop can trigger at `at` (§5.4: stops do not trigger in
/// extended hours): the regular session's open for a US equity in it, the beginning of time for
/// crypto or an instant the calendar does not cover, and `None` outside the regular session.
pub(crate) fn stops_trigger_since(
    ports: &Ports<'_>,
    instrument: &InstrumentId,
    at: RiskClock,
) -> Option<i64> {
    match venue(ports, instrument, at) {
        Venue::Regular | Venue::Closing => {
            Some(span(ports, instrument, at).map_or(i64::MIN, |found| found.start().secs()))
        }
        Venue::Extended | Venue::Closed => None,
    }
}

/// Whether a print observed at `then` is in the same session as `now` (§8.2: a last trade counts
/// only in-session). Crypto, and instants the calendar does not cover, are one session.
pub(crate) fn same_session(
    ports: &Ports<'_>,
    instrument: &InstrumentId,
    then: RiskClock,
    now: RiskClock,
) -> bool {
    span(ports, instrument, then) == span(ports, instrument, now)
}

#[cfg(test)]
mod venue_tests {
    use mandate_accounting::{AssetClass, InstrumentId};
    use mandate_num::ShareIncrement;

    use super::{Venue, extended_hours, same_session, stops_trigger_since, venue};
    use crate::error::ExecutorError;
    use crate::ports::{InstrumentSnapshot, Ports};
    use crate::reconcile::tests::{Everything, Ids, aapl, executor_config, fees};
    use crate::types::{ExitTier, Purpose, RiskClock};

    struct Coin;

    impl InstrumentSnapshot for Coin {
        fn asset_class(&self, _instrument: &InstrumentId) -> Option<AssetClass> {
            Some(AssetClass::Crypto)
        }

        fn increment(&self, _instrument: &InstrumentId) -> Option<ShareIncrement> {
            Some(ShareIncrement::Fractional)
        }

        fn exit_tier(&self, _instrument: &InstrumentId) -> Option<ExitTier> {
            None
        }
    }

    /// 2026-09-22, a Tuesday, in ET: each boundary of §4.3's sessions and the second either side.
    const PRE_MARKET: i64 = 1_790_064_000;
    const OPEN: i64 = 1_790_083_800;
    const CLOSE_WINDOW: i64 = 1_790_106_600;
    const CLOSE: i64 = 1_790_107_200;
    const AFTER_HOURS_CLOSE: i64 = 1_790_121_600;
    const SATURDAY: i64 = 1_790_438_400;

    fn at(secs: i64) -> RiskClock {
        RiskClock::from_secs(secs)
    }

    #[test]
    fn each_session_boundary_is_the_calendars() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let aapl = aapl()?;
        for (secs, expected) in [
            (PRE_MARKET - 1, Venue::Closed),
            (PRE_MARKET, Venue::Extended),
            (OPEN - 1, Venue::Extended),
            (OPEN, Venue::Regular),
            (CLOSE_WINDOW - 1, Venue::Regular),
            (CLOSE_WINDOW, Venue::Closing),
            (CLOSE - 1, Venue::Closing),
            (CLOSE, Venue::Extended),
            (AFTER_HOURS_CLOSE - 1, Venue::Extended),
            (AFTER_HOURS_CLOSE, Venue::Closed),
            (SATURDAY, Venue::Closed),
            (0, Venue::Regular),
        ] {
            assert_eq!(venue(&ports, &aapl, at(secs)), expected, "at {secs}");
        }
        assert_eq!(stops_trigger_since(&ports, &aapl, at(OPEN + 5)), Some(OPEN));
        assert_eq!(
            stops_trigger_since(&ports, &aapl, at(CLOSE - 1)),
            Some(OPEN)
        );
        assert_eq!(stops_trigger_since(&ports, &aapl, at(OPEN - 1)), None);
        assert_eq!(stops_trigger_since(&ports, &aapl, at(SATURDAY)), None);
        assert_eq!(stops_trigger_since(&ports, &aapl, at(0)), Some(i64::MIN));
        assert!(same_session(&ports, &aapl, at(OPEN), at(CLOSE - 1)));
        assert!(!same_session(&ports, &aapl, at(CLOSE - 1), at(CLOSE)));
        assert!(same_session(&ports, &aapl, at(0), at(1)));
        for (purpose, expected) in [
            (Purpose::RiskExit, true),
            (Purpose::Protective, true),
            (Purpose::Flatten, true),
            (Purpose::OwnerExit, false),
            (Purpose::DiscretionaryExit, false),
            (Purpose::Open, false),
            (Purpose::Increase, false),
        ] {
            assert_eq!(
                extended_hours(&ports, &aapl, at(CLOSE), purpose),
                expected,
                "{purpose:?} in after-hours"
            );
            assert!(
                !extended_hours(&ports, &aapl, at(OPEN), purpose),
                "{purpose:?}"
            );
        }
        Ok(())
    }

    /// Crypto trades continuously (§4.3): every instant is its regular session, and a stop can
    /// trigger at any of them.
    #[test]
    fn crypto_is_always_in_session() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Coin,
            config: &config,
            fees: &fees,
        };
        let coin = InstrumentId::new("BTC/USD")?;
        for secs in [PRE_MARKET - 1, CLOSE_WINDOW, CLOSE, SATURDAY] {
            assert_eq!(venue(&ports, &coin, at(secs)), Venue::Regular, "at {secs}");
            assert_eq!(stops_trigger_since(&ports, &coin, at(secs)), Some(i64::MIN));
            assert!(!extended_hours(&ports, &coin, at(secs), Purpose::RiskExit));
        }
        assert!(same_session(&ports, &coin, at(CLOSE - 1), at(SATURDAY)));
        Ok(())
    }
}
