//! The session the gate derives from `now` and the committed NYSE calendar (DEC-129 item 9), and
//! check 3's session and auction-window rules (trading-domain spec §4.3, §9.4). A caller never
//! labels the session: a check the caller can defeat is not an independent gate.

use mandate_time::{
    Date, ExchangeCalendar, Session as CalendarSession, SessionSpan, TimeError, UtcNanos,
    new_york_date_and_hour,
};

use crate::gate::Stop;
use crate::{
    AssetClass, GateConfig, GateError, GateInput, ProposedKind, Purpose, ReasonCode, Session,
    SessionAt, Verdict,
};

/// §4.3's opening auction: 09:28 to 09:30 ET, the last two minutes of pre-market.
const OPENING_AUCTION_S: i64 = 120;

/// The session at `now`. Crypto trades continuously. A US-equity instant outside every session of
/// the calendar (a weekend, a holiday, the night after a Friday) is reported as `Overnight`, the
/// session in which v1 takes no opening, rather than guessed. A date outside the calendar is
/// `ConfigOutOfRange`.
pub(crate) fn derive(
    now: UtcNanos,
    config: &GateConfig,
    asset_class: AssetClass,
) -> Result<SessionAt, GateError> {
    let closed = |session| SessionAt {
        session,
        start: now,
        end: now,
        opening_auction: false,
        close_window: false,
    };
    if asset_class == AssetClass::Crypto {
        return Ok(closed(Session::Continuous));
    }
    let calendar = ExchangeCalendar::us_equities().map_err(|_| GateError::ConfigOutOfRange)?;
    let (today, _) = new_york_date_and_hour(now).map_err(outside)?;
    let span = match find(&calendar, today, now)? {
        None => find(&calendar, today.next()?, now)?,
        found => found,
    };
    let Some(span) = span else {
        return Ok(closed(Session::Overnight));
    };
    let session = match span.session() {
        CalendarSession::Overnight => Session::Overnight,
        CalendarSession::PreMarket => Session::PreMarket,
        CalendarSession::Regular => Session::Regular,
        CalendarSession::AfterHours => Session::AfterHours,
    };
    let close_window_s = i64::from(config.close_window_minutes)
        .checked_mul(60)
        .ok_or(GateError::ConfigOutOfRange)?;
    Ok(SessionAt {
        session,
        start: span.start(),
        end: span.end(),
        opening_auction: session == Session::PreMarket
            && in_last(now, span.end(), OPENING_AUCTION_S)?,
        close_window: session == Session::Regular && in_last(now, span.end(), close_window_s)?,
    })
}

fn find(
    calendar: &ExchangeCalendar,
    date: Date,
    now: UtcNanos,
) -> Result<Option<SessionSpan>, GateError> {
    Ok(calendar
        .sessions(date)
        .map_err(outside)?
        .into_iter()
        .find(|s| s.contains(now)))
}

/// Whether `now` is in the last `secs` seconds before `end`.
fn in_last(now: UtcNanos, end: UtcNanos, secs: i64) -> Result<bool, GateError> {
    let from = end
        .secs()
        .checked_sub(secs)
        .ok_or(GateError::ConfigOutOfRange)?;
    Ok(now >= UtcNanos::from_parts(from, end.nanos())?)
}

fn outside(e: TimeError) -> GateError {
    match e {
        TimeError::OutsideCalendar => GateError::ConfigOutOfRange,
        other => GateError::Time(other),
    }
}

/// Check 3's session and auction-window rules, before its halt (§9.1: "session, auction window,
/// and halt").
///
/// - A US-equity opening outside the regular session is denied, `extended_hours_opening_not_
///   allowed` when it asks for extended hours and `session_not_allowed` otherwise. An opening that
///   asks for extended hours inside the regular session is denied the same way, because the broker
///   would keep it working into after-hours, where v1 opens nothing (§4.3, DEC-37).
/// - A market opening in the opening auction or the closing ten minutes is `auction_window`
///   (§4.3: "no opening orders in either"). A market-order exit there is not denied: it is
///   re-priced as a marketable limit after the checks (DEC-159, amending DEC-129 item 18).
/// - Outside the regular session a US-equity discretionary exit is deferred, and an owner exit is
///   deferred until the owner confirms the displayed bid (§5.5, §9.6). A defer is never a denial.
///
/// Every other exit passes: a session rule never denies a risk exit or a protective order
/// (`AGENTS.md` rule 13, MI-1).
pub(crate) fn rules(input: &GateInput<'_>, purpose: Purpose, at: &SessionAt) -> Option<Stop> {
    let equity = input.instrument.asset_class == AssetClass::UsEquity;
    let off_hours = equity && at.session != Session::Regular;
    let p = input.proposed;
    if equity && matches!(purpose, Purpose::Open | Purpose::Increase) {
        if p.extended_hours {
            return Some((Verdict::Deny, ReasonCode::ExtendedHoursOpeningNotAllowed));
        }
        if off_hours {
            return Some((Verdict::Deny, ReasonCode::SessionNotAllowed));
        }
    }
    let opening = matches!(purpose, Purpose::Open | Purpose::Increase);
    if opening && p.kind == ProposedKind::Market && (at.opening_auction || at.close_window) {
        return Some((Verdict::Deny, ReasonCode::AuctionWindow));
    }
    match purpose {
        Purpose::DiscretionaryExit if off_hours => Some((
            Verdict::Defer,
            ReasonCode::DiscretionaryExitRegularSessionOnly,
        )),
        Purpose::OwnerExit if off_hours && p.owner_confirmed_bid.is_none() => {
            Some((Verdict::Defer, ReasonCode::OwnerConfirmationRequired))
        }
        _ => None,
    }
}
