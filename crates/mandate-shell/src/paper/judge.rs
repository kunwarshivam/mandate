//! Whether the snapshot holds at the run clock: an empty, active account; the reviewed asset record;
//! a current, uncrossed IEX quote; and a clock inside the regular session, before its close window.
//! Only instants are compared and moved here; no money or quantity is computed.

use mandate_accounting::AssetClass;
use mandate_alpaca::Feed;
use mandate_num::Usd;
use mandate_risk::GateConfig;
use mandate_time::{Date, ExchangeCalendar, Session, UtcNanos, new_york_date_and_hour};
use std::time::Duration;

use super::absent;
use super::artifacts::Artifacts;
use super::facts::BrokerFacts;
use crate::error::Cause;

const MINUTE_S: i64 = 60;

/// The New York trading date, once every fact holds at `now`.
pub(super) fn judge(
    artifacts: &Artifacts,
    broker: &BrokerFacts,
    now: UtcNanos,
    config: &GateConfig,
) -> Result<Date, Cause> {
    if !broker.positions.is_empty() || !broker.open_orders.is_empty() {
        return Err(absent("a paper account with no position and no open order"));
    }
    let account = &broker.account;
    if account.status != "ACTIVE"
        || account.trading_blocked
        || account.account_blocked
        || account.trade_suspended_by_user
    {
        return Err(absent("an active, unblocked paper account"));
    }
    if account.accrued_fees != Usd::ZERO {
        return Err(absent("a paper account without accrued fees"));
    }
    let asset = broker
        .asset
        .current(now, artifacts.quote_max_age)
        .map_err(|_| absent("a current asset record"))?;
    if asset.asset_id != artifacts.instrument.asset_id.as_str()
        || asset.instrument != artifacts.instrument.symbol
        || asset.class != AssetClass::UsEquity
        || asset.exchange != artifacts.instrument.broker_exchange
    {
        return Err(absent("the bound instrument's asset record"));
    }
    if artifacts.instrument.etp_classified_at > now {
        return Err(absent("an ETP classification dated before the run"));
    }
    let quote = &broker.quote;
    if quote.instrument != artifacts.instrument.symbol
        || quote.feed != Feed::Iex
        || quote.bid > quote.ask
    {
        return Err(absent("a current, uncrossed IEX quote"));
    }
    current_quote(quote.at, artifacts.quote_max_age, now)?;
    regular_session_outside_close_window(now, config)
}

/// Rechecks the time-sensitive facts at the binding gate's clock immediately before submission.
pub(super) fn judge_submission_time(
    quote_at: UtcNanos,
    quote_max_age: Duration,
    now: UtcNanos,
    config: &GateConfig,
) -> Result<(), Cause> {
    current_quote(quote_at, quote_max_age, now)?;
    regular_session_outside_close_window(now, config).map(|_| ())
}

fn current_quote(quote_at: UtcNanos, max_age: Duration, now: UtcNanos) -> Result<(), Cause> {
    let max_age_s = i64::try_from(max_age.as_secs()).map_err(|_| clock())?;
    let oldest = shifted(now, max_age_s.checked_neg().ok_or_else(clock)?)?;
    if quote_at > now || quote_at < oldest {
        return Err(absent("a current, uncrossed IEX quote"));
    }
    Ok(())
}

/// The New York date of the regular session `now` is in, unless `now` is in its last
/// `close_window_minutes`, as the gate derives both (trading-domain spec §4.3, §9.6).
fn regular_session_outside_close_window(now: UtcNanos, config: &GateConfig) -> Result<Date, Cause> {
    let calendar = ExchangeCalendar::us_equities().map_err(|_| clock())?;
    let (today, _) = new_york_date_and_hour(now).map_err(|_| clock())?;
    let session = calendar
        .sessions(today)
        .map_err(|_| clock())?
        .into_iter()
        .find(|span| span.session() == Session::Regular && span.contains(now))
        .ok_or_else(|| absent("a run clock inside the regular session"))?;
    let close_window_s = i64::from(config.close_window_minutes)
        .checked_mul(MINUTE_S)
        .ok_or_else(clock)?;
    let close_window = shifted(
        session.end(),
        close_window_s.checked_neg().ok_or_else(clock)?,
    )?;
    if now >= close_window {
        return Err(absent("a run clock before the close window"));
    }
    Ok(today)
}

/// `at` moved by `secs` whole seconds.
fn shifted(at: UtcNanos, secs: i64) -> Result<UtcNanos, Cause> {
    let moved = at.secs().checked_add(secs).ok_or_else(clock)?;
    UtcNanos::from_parts(moved, at.nanos()).map_err(|_| clock())
}

fn clock() -> Cause {
    absent("a run clock inside the calendar")
}
