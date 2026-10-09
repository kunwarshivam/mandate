//! The one snapshot the contexts are assembled from: what the broker answered to the preflight's
//! GETs, and the liquidity figures `mandate-liquidity` computes from typed stored daily bars and
//! broker minute bars (DEC-470 item 4, DEC-471). Nothing here computes a figure; it reads, parses,
//! maps, and names the refusal.

use std::path::Path;
use std::time::Duration;

use mandate_accounting::InstrumentId;
use mandate_alpaca::{
    AccountRules, AssetSnapshot, DataClient, DataTransport, LatestQuote, MinuteBars, Pause,
    RetryPolicy, TradingClient, TradingTransport, alpaca_account_rules,
};
use mandate_executor::{BrokerAccount, BrokerOrder, BrokerOutcome, BrokerPosition, BrokerRequest};
use mandate_liquidity::{
    DailyBar, DailyLiquidity, LiquidityError, MinuteVolume, TRAILING_WINDOW_S, daily_liquidity,
    trailing_volume,
};
use mandate_num::{Price, Qty, Usd};
use mandate_time::UtcNanos;

use super::absent;
use super::artifacts::Artifacts;
use crate::adapters::{trusted_daily_bars, untrusted};
use crate::error::Cause;

/// What the broker answered to the preflight's six GETs, exactly as `mandate-alpaca` read it, beside
/// the account rules `mandate-alpaca` declares for it.
#[derive(Debug, Clone)]
pub struct BrokerFacts {
    pub account: BrokerAccount,
    /// The account type and day-trading regime the connector declares for its broker (trading
    /// spec §7.2), which the gate's account snapshot takes rather than any shell value (X-9,
    /// DEC-840).
    pub account_rules: AccountRules,
    pub positions: Vec<BrokerPosition>,
    pub open_orders: Vec<BrokerOrder>,
    pub asset: AssetSnapshot,
    pub quote: LatestQuote,
    /// The complete IEX minute bars of the trailing window before the preflight's clock.
    pub minute_bars: MinuteBars,
}

/// The liquidity figures the gate reads, each computed by `mandate-liquidity`'s stated rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiquidityFacts {
    /// The close of the last completed session.
    pub prior_close: Price,
    /// The lower median of the last 20 sessions' `volume × close`.
    pub median_dollar_volume_20d: Usd,
    /// The mean of the last 20 sessions' volumes, truncated to whole shares.
    pub adv_20d: Qty,
    /// The volume of the complete one-minute bars inside the five minutes before the run clock.
    pub trailing_5m_volume: Qty,
}

/// The one snapshot the contexts are assembled from.
#[derive(Debug, Clone)]
pub struct PaperFacts {
    pub broker: BrokerFacts,
    pub liquidity: LiquidityFacts,
}

/// The broker reads the run needs, as GETs only: `GetAccount`, `ListPositions`, and
/// `ListOpenOrders` through [`TradingClient::call_one`], the asset record through
/// [`TradingClient::asset`], the latest IEX quote through [`DataClient::latest_quote`], whose age
/// bound is the rule-set artifact's, and the trailing window's IEX minute bars through
/// [`DataClient::recent_minute_bars`]. The account rules are the connector's declaration,
/// [`alpaca_account_rules`], which no broker call answers. Nothing here can submit, cancel, or
/// close.
///
/// # Errors
/// [`Cause::Absent`] when a read is not answered with the fact it asked for.
pub fn preflight<T, D, P>(
    artifacts: &Artifacts,
    trading: T,
    data: D,
    pause: P,
) -> Result<BrokerFacts, Cause>
where
    T: TradingTransport,
    D: DataTransport,
    P: Pause + Clone,
{
    let trailing_window = Duration::from_secs(
        u64::try_from(TRAILING_WINDOW_S).map_err(|_| absent("the trailing volume window"))?,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| absent("a runtime for the paper preflight"))?;
    let trading = TradingClient::new(trading, pause.clone(), RetryPolicy::default());
    let data = DataClient::new(data, pause);
    let symbol = &artifacts.instrument.symbol;
    runtime.block_on(async {
        let BrokerOutcome::Account(account) = trading
            .call_one(&BrokerRequest::GetAccount)
            .await
            .map_err(|_| absent("the broker's account"))?
        else {
            return Err(absent("the broker's account"));
        };
        let BrokerOutcome::Positions(positions) = trading
            .call_one(&BrokerRequest::ListPositions)
            .await
            .map_err(|_| absent("the broker's positions"))?
        else {
            return Err(absent("the broker's positions"));
        };
        let BrokerOutcome::OpenOrders(open_orders) = trading
            .call_one(&BrokerRequest::ListOpenOrders)
            .await
            .map_err(|_| absent("the broker's open orders"))?
        else {
            return Err(absent("the broker's open orders"));
        };
        let asset = trading
            .asset(symbol)
            .await
            .map_err(|_| absent("the broker's asset record"))?;
        let quote = data
            .latest_quote(symbol, artifacts.quote_max_age)
            .await
            .map_err(|_| absent("a current IEX quote"))?;
        let minute_bars = data
            .recent_minute_bars(symbol, trailing_window)
            .await
            .map_err(|_| absent("the trailing window's IEX minute bars"))?;
        Ok(BrokerFacts {
            account,
            account_rules: alpaca_account_rules(),
            positions,
            open_orders,
            asset,
            quote,
            minute_bars,
        })
    })
}

/// The liquidity figures at `now`, from two inputs `mandate-shell` parses and maps:
///
/// - `daily` must pass the same trust check the signal's bars do, ending on the last completed
///   session; its stored close and volume fields are parsed into [`DailyBar`] values, and
///   [`daily_liquidity`] reads the last 20.
/// - `minute_bars` must be `symbol`'s, as the preflight read them; [`trailing_volume`] sums those
///   that start inside the five minutes before `now` and refuses one that ends after it.
///
/// `symbol` is the run's instrument, the one its artifacts bind, so no instrument is pinned here.
///
/// IEX volumes understate consolidated volume, so every figure errs toward the tighter limit.
///
/// # Errors
/// [`Cause::Untrusted`] or the dataset's own error when the daily bars cannot be trusted at `now`,
/// and [`Cause::Absent`] naming the minute-bar fact that does not hold.
pub fn liquidity_facts(
    symbol: &InstrumentId,
    daily: &Path,
    minute_bars: &MinuteBars,
    now: UtcNanos,
) -> Result<LiquidityFacts, Cause> {
    if minute_bars.instrument != *symbol {
        return Err(absent("the instrument's minute bars"));
    }
    let daily: Vec<DailyBar> = trusted_daily_bars(daily, symbol.as_str(), now)?
        .iter()
        .map(|bar| -> Result<DailyBar, LiquidityError> {
            Ok(DailyBar {
                close: Price::parse(bar.close.as_str())?,
                volume: Qty::parse(bar.volume.as_str())?,
            })
        })
        .collect::<Result<_, _>>()
        .map_err(refusal)?;
    let DailyLiquidity {
        prior_close,
        median_dollar_volume_20d,
        adv_20d,
    } = daily_liquidity(&daily).map_err(refusal)?;
    let minutes: Vec<MinuteVolume> = minute_bars
        .bars
        .iter()
        .map(|bar| MinuteVolume {
            start: bar.start,
            volume: bar.volume,
        })
        .collect();
    Ok(LiquidityFacts {
        prior_close,
        median_dollar_volume_20d,
        adv_20d,
        trailing_5m_volume: trailing_volume(&minutes, now).map_err(refusal)?,
    })
}

fn refusal(error: LiquidityError) -> Cause {
    match error {
        LiquidityError::FewerSessions => untrusted("fewer than 20 sessions are stored"),
        LiquidityError::AheadOfClock => absent("minute bars complete at the run clock"),
        LiquidityError::Unordered => absent("minute bars in strictly increasing order"),
        LiquidityError::EmptyWindow => absent("a complete minute bar in the trailing five minutes"),
        LiquidityError::Num(error) => Cause::Num(error),
        LiquidityError::Time(_) => absent("a run clock inside the calendar"),
    }
}

#[cfg(test)]
mod tests;
