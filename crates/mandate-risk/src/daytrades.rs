//! E6-6's second slice: §9.2's `legacy_pdt` day-trade ledger, folded account-wide from every
//! agent's fills (DEC-129 item 6, DEC-150 item 7, DEC-259). Check 8 reads the result as
//! [`AgentSnapshot::day_trades`](crate::AgentSnapshot::day_trades); nothing here decides an order.

use std::collections::{BTreeMap, BTreeSet};

use mandate_num::Qty;
use mandate_time::{
    Date, ExchangeCalendar, TimeError, UtcNanos, new_york_date_and_hour, new_york_midnight,
};

use crate::{
    AccountFill, AssetClass, AssetId, DayTrade, DayTradeFold, DayTradeInput, DayTradeLedger,
    GateError, Side,
};

/// §2.2's cutoff: an equity fill at or after 20:00 ET belongs to the next trading day, as
/// `mandate-time`'s `TradingCalendar::equity_trade_date` dates it.
const TRADE_DATE_CUTOFF_HOUR: u8 = 20;

/// §9.2's window is today plus this many prior trading days.
const PRIOR_TRADING_DAYS: usize = 4;

/// Half a day in seconds: twelve hours before New York midnight is always the previous calendar
/// date, whatever the daylight-saving offset.
const HALF_DAY_S: i64 = 43_200;

/// One US-equity instrument's state through today's fills.
struct Book {
    overnight: Qty,
    same_day: Qty,
    bought_since_last_day_trade: bool,
    sold_today: bool,
}

impl Book {
    const EMPTY: Self = Self {
        overnight: Qty::ZERO,
        same_day: Qty::ZERO,
        bought_since_last_day_trade: false,
        sold_today: false,
    };
}

pub(crate) fn fold(input: &DayTradeInput<'_>) -> Result<DayTradeFold, GateError> {
    let calendar = ExchangeCalendar::us_equities().map_err(|_| GateError::ConfigOutOfRange)?;
    let today = trading_day(&calendar, input.now)?;
    let window_start = window_start(&calendar, today)?;

    let mut books: BTreeMap<AssetId, Book> = BTreeMap::new();
    for (instrument, qty) in input.held_overnight {
        books
            .entry(instrument.clone())
            .or_insert(Book::EMPTY)
            .overnight = *qty;
    }
    let mut today_trades = Vec::new();
    let mut previous: Option<UtcNanos> = None;
    for fill in input.fills_today {
        if fill.at > input.now || previous.is_some_and(|p| fill.at < p) {
            return Err(GateError::DayTradeLedgerOutOfOrder);
        }
        previous = Some(fill.at);
        if fill.asset_class == AssetClass::Crypto {
            continue;
        }
        if trading_day(&calendar, fill.at)? != today {
            return Err(GateError::DayTradeLedgerOutOfOrder);
        }
        let book = books.entry(fill.instrument.clone()).or_insert(Book::EMPTY);
        if apply(book, fill)? {
            today_trades.push(DayTrade {
                date: today,
                instrument: fill.instrument.clone(),
            });
        }
    }

    let mut earlier_in_window = 0_usize;
    for trade in input.earlier {
        if trade.date >= today {
            return Err(GateError::DayTradeLedgerOutOfOrder);
        }
        if trade.date >= window_start {
            earlier_in_window = earlier_in_window
                .checked_add(1)
                .ok_or(GateError::ConfigOutOfRange)?;
        }
    }
    let window_count = earlier_in_window
        .checked_add(today_trades.len())
        .and_then(|n| u32::try_from(n).ok())
        .ok_or(GateError::ConfigOutOfRange)?;

    let sold_earlier_today: BTreeSet<AssetId> = books
        .iter()
        .filter(|(_, b)| b.sold_today)
        .map(|(i, _)| i.clone())
        .collect();
    let open_same_day_positions: BTreeSet<AssetId> = books
        .iter()
        .filter(|(_, b)| !b.same_day.is_zero())
        .map(|(i, _)| i.clone())
        .collect();
    Ok(DayTradeFold {
        ledger: DayTradeLedger {
            window_count,
            flagged_pattern_day_trader: input.flagged_pattern_day_trader,
            sold_earlier_today,
            open_same_day_positions,
        },
        today: today_trades,
    })
}

/// One equity fill against its instrument's book; `true` when it completes a day trade. A sell
/// takes shares held overnight first, and only a sell that closes shares bought today, after a
/// purchase not yet matched by a counted day trade, counts: so buy, buy, sell is one day trade
/// and buy, sell, buy, sell is two. A sell of more than the account holds would be a short sale,
/// which v1 never makes (`AGENTS.md` rule 12), so the fills are inconsistent and nothing is
/// guessed.
fn apply(book: &mut Book, fill: &AccountFill) -> Result<bool, GateError> {
    match fill.side {
        Side::Buy => {
            book.same_day = book.same_day.checked_add(fill.qty)?;
            book.bought_since_last_day_trade = true;
            Ok(false)
        }
        Side::Sell => {
            book.sold_today = true;
            let from_overnight = fill.qty.min(book.overnight);
            book.overnight = book.overnight.checked_sub(from_overnight)?;
            let from_today = fill.qty.checked_sub(from_overnight)?;
            if from_today.is_zero() {
                return Ok(false);
            }
            book.same_day = book
                .same_day
                .checked_sub(from_today)
                .map_err(|_| GateError::DayTradeLedgerInconsistent)?;
            let counts = book.bought_since_last_day_trade;
            book.bought_since_last_day_trade = false;
            Ok(counts)
        }
    }
}

/// §2.2's equity trade date of an instant: its New York date, moved to the next day from 20:00 ET,
/// then to the first trading day on or after that.
fn trading_day(calendar: &ExchangeCalendar, at: UtcNanos) -> Result<Date, GateError> {
    let (date, hour) = new_york_date_and_hour(at)?;
    let mut date = if hour >= TRADE_DATE_CUTOFF_HOUR {
        date.next()?
    } else {
        date
    };
    while !calendar.is_trading_day(date).map_err(outside)? {
        date = date.next()?;
    }
    Ok(date)
}

/// The first day of §9.2's window: the fourth trading day before `today`.
fn window_start(calendar: &ExchangeCalendar, today: Date) -> Result<Date, GateError> {
    let mut date = today;
    let mut found = 0_usize;
    while found < PRIOR_TRADING_DAYS {
        date = previous(date)?;
        if calendar.is_trading_day(date).map_err(outside)? {
            found = found.checked_add(1).ok_or(GateError::ConfigOutOfRange)?;
        }
    }
    Ok(date)
}

fn previous(date: Date) -> Result<Date, GateError> {
    let midnight = new_york_midnight(date)?;
    let secs = midnight
        .secs()
        .checked_sub(HALF_DAY_S)
        .ok_or(GateError::ConfigOutOfRange)?;
    let (date, _) = new_york_date_and_hour(UtcNanos::from_parts(secs, 0)?)?;
    Ok(date)
}

fn outside(e: TimeError) -> GateError {
    match e {
        TimeError::OutsideCalendar => GateError::ConfigOutOfRange,
        other => GateError::Time(other),
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn id(text: &str) -> Result<AssetId, GateError> {
        AssetId::new(text).map_err(|_| GateError::InstrumentUnknown)
    }

    fn at(text: &str) -> Result<UtcNanos, GateError> {
        Ok(UtcNanos::parse_rfc3339(text)?)
    }

    fn qty(text: &str) -> Result<Qty, GateError> {
        Ok(Qty::parse(text)?)
    }

    fn earlier(rows: &[(&str, &str)]) -> Result<Vec<DayTrade>, GateError> {
        rows.iter()
            .map(|(date, instrument)| {
                Ok(DayTrade {
                    date: Date::parse(date)?,
                    instrument: id(instrument)?,
                })
            })
            .collect()
    }

    fn fill(
        when: &str,
        instrument: &str,
        side: Side,
        amount: &str,
    ) -> Result<AccountFill, GateError> {
        let asset_class = if instrument == "BTCUSD" {
            AssetClass::Crypto
        } else {
            AssetClass::UsEquity
        };
        Ok(AccountFill {
            at: at(when)?,
            instrument: id(instrument)?,
            asset_class,
            side,
            qty: qty(amount)?,
        })
    }

    struct Scene {
        now: UtcNanos,
        earlier: Vec<DayTrade>,
        held_overnight: BTreeMap<AssetId, Qty>,
        fills_today: Vec<AccountFill>,
        flagged: bool,
    }

    impl Scene {
        fn at(now: &str) -> Result<Self, GateError> {
            Ok(Self {
                now: at(now)?,
                earlier: Vec::new(),
                held_overnight: BTreeMap::new(),
                fills_today: Vec::new(),
                flagged: false,
            })
        }

        fn fold(&self) -> Result<DayTradeFold, GateError> {
            fold(&DayTradeInput {
                now: self.now,
                earlier: &self.earlier,
                held_overnight: &self.held_overnight,
                fills_today: &self.fills_today,
                flagged_pattern_day_trader: self.flagged,
            })
        }
    }

    fn set(names: &[&str]) -> Result<BTreeSet<AssetId>, GateError> {
        names.iter().map(|n| id(n)).collect()
    }

    const TEN_AM: &str = "2026-09-21T10:00:00-04:00";

    /// RC-09: on Monday 2026-09-21 the window is 09-15 to 09-21, so the day trades of 09-15, 09-16
    /// and 09-18 count and one on Friday 09-11, the fifth trading day back, does not.
    #[test]
    fn the_window_is_today_and_four_prior_trading_days() -> Result<(), GateError> {
        let mut s = Scene::at(TEN_AM)?;
        s.earlier = earlier(&[
            ("2026-09-14", "ZZZ"),
            ("2026-09-15", "AAA"),
            ("2026-09-16", "BBB"),
            ("2026-09-18", "CCC"),
        ])?;
        assert_eq!(s.fold()?.ledger.window_count, 3);
        s.earlier = earlier(&[("2026-09-15", "AAA"), ("2026-09-15", "AAA")])?;
        assert_eq!(
            s.fold()?.ledger.window_count,
            2,
            "two day trades in one instrument on one day are two"
        );
        Ok(())
    }

    /// Labor Day, Monday 2026-09-07, is closed: on Thursday 09-10 the four prior trading days are
    /// 09-09, 09-08, 09-04 and 09-03, so 09-03 counts and 09-02 does not.
    #[test]
    fn a_holiday_is_not_one_of_the_four_prior_trading_days() -> Result<(), GateError> {
        let mut s = Scene::at("2026-09-10T10:00:00-04:00")?;
        s.earlier = earlier(&[("2026-09-03", "AAA")])?;
        assert_eq!(s.fold()?.ledger.window_count, 1);
        s.earlier = earlier(&[("2026-09-02", "AAA")])?;
        assert_eq!(s.fold()?.ledger.window_count, 0);
        Ok(())
    }

    /// RC-09B's account: a buy of 10 opens a same-day position without a day trade, and selling it
    /// later the same day is the third day trade in the window.
    #[test]
    fn rc_09b_open_then_close_is_one_more_day_trade() -> Result<(), GateError> {
        let mut s = Scene::at("2026-09-21T11:00:00-04:00")?;
        s.earlier = earlier(&[("2026-09-16", "BBB"), ("2026-09-18", "CCC")])?;
        s.fills_today = vec![fill("2026-09-21T10:01:00-04:00", "AAPL", Side::Buy, "10")?];
        let mid = s.fold()?;
        assert_eq!(mid.ledger.window_count, 2);
        assert_eq!(mid.ledger.open_same_day_positions, set(&["AAPL"])?);
        assert_eq!(mid.ledger.sold_earlier_today, set(&[])?);
        assert_eq!(mid.today, Vec::new());

        s.now = at("2026-09-21T14:00:05-04:00")?;
        s.fills_today
            .push(fill("2026-09-21T14:00:05-04:00", "AAPL", Side::Sell, "10")?);
        let end = s.fold()?;
        assert_eq!(end.ledger.window_count, 3);
        assert_eq!(end.ledger.open_same_day_positions, set(&[])?);
        assert_eq!(end.ledger.sold_earlier_today, set(&["AAPL"])?);
        assert_eq!(end.today, earlier(&[("2026-09-21", "AAPL")])?);
        Ok(())
    }

    /// Shares held overnight are sold first: with 10 held, buying 10 and selling 10 sells the old
    /// shares and leaves today's open, so no day trade yet; the next sell closes today's shares.
    #[test]
    fn shares_held_overnight_are_sold_first() -> Result<(), GateError> {
        let mut s = Scene::at("2026-09-21T15:00:00-04:00")?;
        s.held_overnight.insert(id("AAPL")?, qty("10")?);
        s.fills_today = vec![
            fill(TEN_AM, "AAPL", Side::Buy, "10")?,
            fill("2026-09-21T11:00:00-04:00", "AAPL", Side::Sell, "10")?,
        ];
        let first = s.fold()?;
        assert_eq!(first.ledger.window_count, 0);
        assert_eq!(first.ledger.open_same_day_positions, set(&["AAPL"])?);
        assert_eq!(first.ledger.sold_earlier_today, set(&["AAPL"])?);

        s.fills_today
            .push(fill("2026-09-21T12:00:00-04:00", "AAPL", Side::Sell, "1")?);
        assert_eq!(s.fold()?.ledger.window_count, 1);
        Ok(())
    }

    /// Each same-day open-then-close counts once: buy, buy, sell, sell is one day trade, and
    /// buy, sell, buy, sell is two.
    #[test]
    fn each_open_then_close_counts_once() -> Result<(), GateError> {
        let mut s = Scene::at("2026-09-21T15:00:00-04:00")?;
        s.fills_today = vec![
            fill(TEN_AM, "AAPL", Side::Buy, "5")?,
            fill("2026-09-21T10:01:00-04:00", "AAPL", Side::Buy, "5")?,
            fill("2026-09-21T10:02:00-04:00", "AAPL", Side::Sell, "4")?,
            fill("2026-09-21T10:03:00-04:00", "AAPL", Side::Sell, "6")?,
        ];
        assert_eq!(s.fold()?.ledger.window_count, 1);
        s.fills_today = vec![
            fill(TEN_AM, "MSFT", Side::Buy, "5")?,
            fill("2026-09-21T10:01:00-04:00", "MSFT", Side::Sell, "5")?,
            fill("2026-09-21T10:02:00-04:00", "MSFT", Side::Buy, "5")?,
            fill("2026-09-21T10:03:00-04:00", "MSFT", Side::Sell, "5")?,
        ];
        let fold = s.fold()?;
        assert_eq!(fold.ledger.window_count, 2);
        assert_eq!(
            fold.today,
            earlier(&[("2026-09-21", "MSFT"), ("2026-09-21", "MSFT")])?
        );
        Ok(())
    }

    /// Crypto never counts, in the count or in either set; a fractional equity day trade does.
    #[test]
    fn crypto_never_counts_and_a_fractional_day_trade_does() -> Result<(), GateError> {
        let mut s = Scene::at("2026-09-21T15:00:00-04:00")?;
        s.fills_today = vec![
            fill(TEN_AM, "BTCUSD", Side::Buy, "0.01")?,
            fill(
                "2026-09-21T10:01:00-04:00",
                "BTCUSD",
                Side::Sell,
                "0.009975",
            )?,
            fill("2026-09-21T10:02:00-04:00", "BTCUSD", Side::Sell, "5")?,
        ];
        let crypto = s.fold()?;
        assert_eq!(crypto.ledger, DayTradeLedger::default());
        assert_eq!(crypto.today, Vec::new());

        s.fills_today = vec![
            fill(TEN_AM, "AAPL", Side::Buy, "0.5")?,
            fill("2026-09-21T10:01:00-04:00", "AAPL", Side::Sell, "0.5")?,
        ];
        assert_eq!(s.fold()?.ledger.window_count, 1);
        Ok(())
    }

    /// The broker's flag is the ledger's.
    #[test]
    fn the_pattern_day_trader_flag_passes_through() -> Result<(), GateError> {
        let mut s = Scene::at(TEN_AM)?;
        s.flagged = true;
        assert!(s.fold()?.ledger.flagged_pattern_day_trader);
        s.flagged = false;
        assert!(!s.fold()?.ledger.flagged_pattern_day_trader);
        Ok(())
    }

    /// From 20:00 ET the trading day is the next one (§2.2): at 21:00 on Monday, today is Tuesday,
    /// Monday's day trades are earlier ones, and a fill at 20:30 is Tuesday's.
    #[test]
    fn the_day_turns_at_twenty_hundred() -> Result<(), GateError> {
        let mut s = Scene::at("2026-09-21T21:00:00-04:00")?;
        s.earlier = earlier(&[("2026-09-21", "AAPL"), ("2026-09-15", "AAA")])?;
        s.fills_today = vec![
            fill("2026-09-21T20:30:00-04:00", "AAPL", Side::Buy, "1")?,
            fill("2026-09-21T20:31:00-04:00", "AAPL", Side::Sell, "1")?,
        ];
        let fold = s.fold()?;
        assert_eq!(
            fold.ledger.window_count, 2,
            "09-15 is the fifth trading day before Tuesday 09-22, so 09-21 and the new one count"
        );
        assert_eq!(fold.today, earlier(&[("2026-09-22", "AAPL")])?);
        s.fills_today = vec![fill("2026-09-21T19:59:59-04:00", "AAPL", Side::Buy, "1")?];
        assert!(matches!(s.fold(), Err(GateError::DayTradeLedgerOutOfOrder)));
        Ok(())
    }

    /// Every malformed input is a refusal, never a smaller count.
    #[test]
    fn a_malformed_history_is_refused() -> Result<(), GateError> {
        let out_of_order = |s: &Scene| matches!(s.fold(), Err(GateError::DayTradeLedgerOutOfOrder));
        let mut s = Scene::at("2026-09-21T15:00:00-04:00")?;
        s.fills_today = vec![
            fill("2026-09-21T10:01:00-04:00", "AAPL", Side::Buy, "1")?,
            fill(TEN_AM, "AAPL", Side::Buy, "1")?,
        ];
        assert!(out_of_order(&s), "fills out of execution order");
        s.fills_today = vec![
            fill(TEN_AM, "AAPL", Side::Buy, "1")?,
            fill(TEN_AM, "AAPL", Side::Sell, "1")?,
        ];
        assert_eq!(
            s.fold()?.ledger.window_count,
            1,
            "two fills at one instant are in order"
        );
        s.fills_today = vec![
            fill("2026-09-21T10:01:00-04:00", "BTCUSD", Side::Buy, "1")?,
            fill(TEN_AM, "BTCUSD", Side::Sell, "1")?,
        ];
        assert!(out_of_order(&s), "crypto fills out of order too");
        s.fills_today = vec![fill("2026-09-21T15:00:01-04:00", "AAPL", Side::Buy, "1")?];
        assert!(out_of_order(&s), "a fill after now");
        s.fills_today = vec![fill("2026-09-21T15:00:00-04:00", "AAPL", Side::Buy, "1")?];
        assert!(s.fold().is_ok(), "a fill at now is today's");
        s.fills_today = vec![fill("2026-09-18T15:00:00-04:00", "AAPL", Side::Buy, "1")?];
        assert!(
            out_of_order(&s),
            "an equity fill from an earlier trading day"
        );
        s.fills_today = vec![fill("2026-09-18T15:00:00-04:00", "BTCUSD", Side::Buy, "1")?];
        assert!(s.fold().is_ok(), "crypto has no trading day to check");
        s.fills_today = Vec::new();
        s.earlier = earlier(&[("2026-09-21", "AAPL")])?;
        assert!(out_of_order(&s), "an earlier day trade dated today");
        s.earlier = earlier(&[("2026-09-20", "AAPL")])?;
        assert_eq!(
            s.fold()?.ledger.window_count,
            1,
            "the day before today is earlier"
        );

        s.earlier = Vec::new();
        s.held_overnight.insert(id("AAPL")?, qty("2")?);
        s.fills_today = vec![
            fill(TEN_AM, "AAPL", Side::Buy, "1")?,
            fill("2026-09-21T10:01:00-04:00", "AAPL", Side::Sell, "3")?,
        ];
        assert_eq!(s.fold()?.ledger.window_count, 1, "every held share sold");
        s.fills_today.push(fill(
            "2026-09-21T10:02:00-04:00",
            "AAPL",
            Side::Sell,
            "0.000000001",
        )?);
        assert!(matches!(
            s.fold(),
            Err(GateError::DayTradeLedgerInconsistent)
        ));
        Ok(())
    }

    /// A date outside the committed calendar is refused, never guessed.
    #[test]
    fn a_date_outside_the_calendar_is_refused() -> Result<(), GateError> {
        let s = Scene::at("1990-01-02T10:00:00-05:00")?;
        assert!(matches!(s.fold(), Err(GateError::ConfigOutOfRange)));
        Ok(())
    }

    /// One instrument's fills as (is_buy, units), each unit 0.5 shares.
    fn day_of_fills() -> impl Strategy<Value = (u64, Vec<(bool, u64)>)> {
        (
            0_u64..4,
            prop::collection::vec((any::<bool>(), 1_u64..4), 0..12),
        )
    }

    proptest! {
        /// Against an oracle in whole half-share units that never touches the crate's ledger: a
        /// sell's same-day part is what the running total sold exceeds the overnight holding by,
        /// less what earlier sells already took, and it counts when a buy came after the last
        /// counted one. Sells the account cannot cover are dropped from the generated day.
        #[test]
        fn the_count_matches_a_running_total_oracle((overnight, ops) in day_of_fills()) {
            let half = |units: u64| {
                let whole = units / 2;
                Qty::parse(&if units % 2 == 1 { format!("{whole}.5") } else { format!("{whole}") })
            };
            let mut s = Scene::at("2026-09-21T15:00:00-04:00")?;
            s.held_overnight.insert(id("AAPL")?, half(overnight)?);
            let (mut bought, mut sold, mut want, mut buy_since) = (0_u64, 0_u64, 0_u32, false);
            let mut sold_any = false;
            for (minute, (is_buy, units)) in (1_u32..).zip(ops) {
                let side = if is_buy {
                    bought = bought.saturating_add(units);
                    buy_since = true;
                    Side::Buy
                } else {
                    if sold.saturating_add(units) > overnight.saturating_add(bought) {
                        continue;
                    }
                    let beyond_before = sold.saturating_sub(overnight);
                    sold = sold.saturating_add(units);
                    sold_any = true;
                    if sold.saturating_sub(overnight) > beyond_before && buy_since {
                        want = want.saturating_add(1);
                        buy_since = false;
                    }
                    Side::Sell
                };
                let when = format!("2026-09-21T10:{minute:02}:00-04:00");
                s.fills_today.push(AccountFill {
                    at: at(&when)?,
                    instrument: id("AAPL")?,
                    asset_class: AssetClass::UsEquity,
                    side,
                    qty: half(units)?,
                });
            }
            let fold = s.fold()?;
            prop_assert_eq!(fold.ledger.window_count, want);
            prop_assert_eq!(u32::try_from(fold.today.len()).ok(), Some(want));
            let open = bought > sold.saturating_sub(overnight);
            prop_assert_eq!(fold.ledger.open_same_day_positions, if open { set(&["AAPL"])? } else { set(&[])? });
            prop_assert_eq!(fold.ledger.sold_earlier_today, if sold_any { set(&["AAPL"])? } else { set(&[])? });
        }
    }
}
