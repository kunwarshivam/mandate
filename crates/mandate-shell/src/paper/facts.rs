//! The one snapshot the contexts are assembled from: what the broker answered to the preflight's
//! GETs, and the liquidity figures `mandate-liquidity` computes from typed stored daily bars and
//! broker minute bars (DEC-470 item 4, DEC-471). Nothing here computes a figure; it reads, parses,
//! maps, and names the refusal.

use std::path::Path;
use std::time::Duration;

use mandate_accounting::InstrumentId;
use mandate_alpaca::{
    AccountRules, AssetSnapshot, DataClient, DataTransport, Feed, LatestQuote, MinuteBars, Pause,
    RetryPolicy, TradingClient, TradingTransport, alpaca_account_rules,
};
use mandate_executor::{BrokerAccount, BrokerOrder, BrokerOutcome, BrokerPosition, BrokerRequest};
use mandate_liquidity::{
    DailyLiquidity, LiquidityError, MinuteVolume, TRAILING_WINDOW_S, trailing_volume,
};
use mandate_num::{Price, Qty, Usd};
use mandate_time::{Date, UtcNanos};

use mandate_robinhood::PreflightFacts;

use super::absent;
use super::artifacts::Artifacts;
use crate::adapters::{trusted_daily_closes, trusted_daily_liquidity, untrusted};
use crate::error::Cause;
use crate::robinhood::{robinhood_account, robinhood_account_rules};

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

/// The market reads every run needs from Alpaca, whichever broker holds the account: the asset
/// record, the latest IEX quote, and the trailing window's IEX minute bars (DEC-529 item 12: the
/// bars and the independent quote stay Alpaca's on the live path too).
#[derive(Debug, Clone)]
pub struct MarketFacts {
    pub asset: AssetSnapshot,
    pub quote: LatestQuote,
    /// The complete IEX minute bars of the trailing window before the preflight's clock.
    pub minute_bars: MinuteBars,
}

/// The broker reads the run needs, as GETs only: `GetAccount`, `ListPositions`, and
/// `ListOpenOrders` through [`TradingClient::call_one`], then [`market_facts`]'s reads. The
/// account rules are the connector's declaration, [`alpaca_account_rules`], which no broker call
/// answers. Nothing here can submit, cancel, or close.
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
    let runtime = preflight_runtime()?;
    let trading = TradingClient::new(trading, pause.clone(), RetryPolicy::default());
    let data = DataClient::new(data, pause);
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
        let MarketFacts {
            asset,
            quote,
            minute_bars,
        } = market_reads(artifacts, &trading, &data).await?;
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

/// [`MarketFacts`] as GETs only: the asset record through [`TradingClient::asset`], the latest
/// IEX quote through [`DataClient::latest_quote`], whose age bound is the rule-set artifact's,
/// and the trailing window's IEX minute bars through [`DataClient::recent_minute_bars`]. No
/// account is read.
///
/// # Errors
/// [`Cause::Absent`] when a read is not answered with the fact it asked for.
pub fn market_facts<T, D, P>(
    artifacts: &Artifacts,
    trading: T,
    data: D,
    pause: P,
) -> Result<MarketFacts, Cause>
where
    T: TradingTransport,
    D: DataTransport,
    P: Pause + Clone,
{
    let runtime = preflight_runtime()?;
    let trading = TradingClient::new(trading, pause.clone(), RetryPolicy::default());
    let data = DataClient::new(data, pause);
    runtime.block_on(market_reads(artifacts, &trading, &data))
}

/// The fresh IEX quote an asked opening's wake reads before it hands the owner's answers to the
/// runtime (DEC-943 item 2): one GET through [`DataClient::latest_quote`], the preflight's own
/// read, so its age bound is the rule set's `iex_quote_max_age_s` and its skew tolerance at the
/// read DEC-936's 250 ms, and a quote of another symbol is the client's own refusal; then the
/// shell judge's own checks of a quote: the IEX feed, and a bid not above the ask. Nothing else
/// is read.
///
/// # Errors
/// [`Cause::Absent`] when the read fails, is stale or stamped past the tolerance, or the quote
/// is of another symbol, off the IEX feed, or crossed.
pub fn fresh_quote<D, P>(artifacts: &Artifacts, data: D, pause: P) -> Result<LatestQuote, Cause>
where
    D: DataTransport,
    P: Pause,
{
    let runtime = preflight_runtime()?;
    let data = DataClient::new(data, pause);
    let symbol = &artifacts.instrument.symbol;
    let quote = runtime
        .block_on(data.latest_quote(symbol, artifacts.quote_max_age))
        .map_err(|_| absent(FRESH_QUOTE))?;
    if quote.feed != Feed::Iex || quote.bid > quote.ask {
        return Err(absent(FRESH_QUOTE));
    }
    Ok(quote)
}

/// What a failed [`fresh_quote`] names.
const FRESH_QUOTE: &str = "a current, uncrossed IEX quote at the wake";

/// The facts of a Robinhood connection (DEC-933 item 2): the agentic account, its positions and
/// its working orders as `mandate-robinhood`'s preflight read them, the account read by
/// [`robinhood_account`] under [`robinhood_account_rules`], beside Alpaca's market reads. The
/// Robinhood quote must be of the instrument the IEX quote is; whether the two agree is the
/// runner's cross-check (DEC-529 item 12), and the gate's quote stays the IEX one, the quote
/// whose age the run can judge (DEC-902 item 7: Robinhood's carries no time).
///
/// # Errors
/// [`Cause::Absent`] when the Robinhood quote names another instrument than the IEX quote.
pub fn robinhood_facts(
    market: MarketFacts,
    robinhood: &PreflightFacts,
) -> Result<BrokerFacts, Cause> {
    if robinhood.quote.symbol != market.quote.instrument {
        return Err(absent("the instrument's Robinhood quote"));
    }
    let snapshot = &robinhood.account;
    Ok(BrokerFacts {
        account: robinhood_account(snapshot),
        account_rules: robinhood_account_rules(),
        positions: snapshot.positions.clone(),
        open_orders: snapshot.open_orders.clone(),
        asset: market.asset,
        quote: market.quote,
        minute_bars: market.minute_bars,
    })
}

fn preflight_runtime() -> Result<tokio::runtime::Runtime, Cause> {
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| absent("a runtime for the paper preflight"))
}

async fn market_reads<T, D, P>(
    artifacts: &Artifacts,
    trading: &TradingClient<T, P>,
    data: &DataClient<D, P>,
) -> Result<MarketFacts, Cause>
where
    T: TradingTransport,
    D: DataTransport,
    P: Pause,
{
    let trailing_window = Duration::from_secs(
        u64::try_from(TRAILING_WINDOW_S).map_err(|_| absent("the trailing volume window"))?,
    );
    let symbol = &artifacts.instrument.symbol;
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
    Ok(MarketFacts {
        asset,
        quote,
        minute_bars,
    })
}

/// The liquidity figures at `now`, from two inputs `mandate-shell` parses and maps:
///
/// - `daily` must pass the same trust check the model host's closes do, ending on the last completed
///   session; `trusted_daily_liquidity`, `mandate_bars`'s one reading the runtime and the executor
///   share (DEC-2274), parses its closes and volumes and reads the last 20.
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
    let DailyLiquidity {
        prior_close,
        median_dollar_volume_20d,
        adv_20d,
    } = trusted_daily_liquidity(daily, symbol.as_str(), now)?;
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

/// The pinned instrument's stored daily closes, each with its session, for the model host
/// (E7-19 slice 5, the first paper trade brief's E1a, DEC-846 item 2): `daily` through the same
/// trust check [`liquidity_facts`] reads it through, ending on the last completed session at
/// `now`, and exactly as stored, one a day the manifest lists, in its date order.
///
/// # Errors
/// [`Cause::Untrusted`] or the dataset's own error when the daily bars cannot be trusted at `now`.
pub fn daily_closes(
    symbol: &InstrumentId,
    daily: &Path,
    now: UtcNanos,
) -> Result<Vec<(Date, Price)>, Cause> {
    trusted_daily_closes(daily, symbol.as_str(), now)
}

pub(crate) fn refusal(error: LiquidityError) -> Cause {
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
