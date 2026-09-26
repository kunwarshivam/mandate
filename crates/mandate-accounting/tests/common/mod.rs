//! Builders shared by the accounting tests.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use mandate_accounting::{
    Account, AssetClass, Config, CryptoFees, EquityFees, Execution, FeeFamily, Input, InstrumentId,
    Liquidity, Side, TafCapBasis,
};
use mandate_num::{Bps, FeeCap, FeePerShare, FeeRate, Price, Qty, Usd};
use mandate_time::{Date, TradingCalendar, UtcNanos};

pub fn d(s: &str) -> Date {
    Date::parse(s).unwrap()
}

pub fn at(s: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(s).unwrap()
}

pub fn usd(s: &str) -> Usd {
    Usd::parse(s).unwrap()
}

pub fn fee_cap(s: &str) -> FeeCap {
    FeeCap::parse(s).unwrap()
}

pub fn id(s: &str) -> InstrumentId {
    InstrumentId::new(s).unwrap()
}

/// The `us_2026` calendar of the trading-domain reference cases.
pub fn us_2026() -> TradingCalendar {
    TradingCalendar::new(
        d("2026-09-01"),
        d("2026-12-31"),
        [d("2026-11-26"), d("2026-12-25")],
        [d("2026-10-12"), d("2026-11-11")],
    )
    .unwrap()
}

/// The `test_default` fee configuration of the reference cases.
pub fn test_default() -> Config {
    Config {
        equities: EquityFees {
            sec_rate: FeeRate::parse("0.00003").unwrap(),
            taf_per_share: FeePerShare::parse("0.0002").unwrap(),
            taf_cap: fee_cap("9.79"),
            taf_cap_basis: TafCapBasis::PerExecution,
            cat_per_share: FeePerShare::parse("0.00001").unwrap(),
        },
        crypto: CryptoFees {
            maker: Bps::parse("15").unwrap(),
            taker: Bps::parse("25").unwrap(),
        },
        calendar: us_2026(),
    }
}

pub fn no_fees() -> Config {
    let mut config = test_default();
    config.equities.sec_rate = FeeRate::parse("0").unwrap();
    config.equities.taf_per_share = FeePerShare::parse("0").unwrap();
    config.equities.cat_per_share = FeePerShare::parse("0").unwrap();
    config
}

pub struct Fill<'a> {
    pub fill_id: &'a str,
    pub order: Option<&'a str>,
    pub instrument: &'a str,
    pub asset_class: AssetClass,
    pub side: Side,
    pub qty: &'a str,
    pub price: &'a str,
    pub liquidity: Option<Liquidity>,
    pub at: &'a str,
}

impl Fill<'_> {
    pub fn input(&self) -> Input {
        Input::Fill(Execution {
            fill_id: self.fill_id.to_owned(),
            client_order_id: self.order.map(str::to_owned),
            instrument: id(self.instrument),
            asset_class: self.asset_class,
            side: self.side,
            qty_gross: Qty::parse(self.qty).unwrap(),
            price: Price::parse(self.price).unwrap(),
            liquidity: self.liquidity,
            executed_at: at(self.at),
        })
    }
}

pub fn equity<'a>(
    fill_id: &'a str,
    instrument: &'a str,
    side: Side,
    qty: &'a str,
    price: &'a str,
    at: &'a str,
) -> Input {
    Fill {
        fill_id,
        order: None,
        instrument,
        asset_class: AssetClass::UsEquity,
        side,
        qty,
        price,
        liquidity: None,
        at,
    }
    .input()
}

pub fn mark(instrument: &str, price: &str) -> Input {
    Input::Mark {
        instrument: id(instrument),
        price: Price::parse(price).unwrap(),
    }
}

pub fn charge(family: FeeFamily, day: &str) -> Input {
    Input::FeesCharged {
        family,
        day: d(day),
    }
}

/// Applies `input`, panicking with the input on error.
pub fn step(account: &Account, input: &Input, config: &Config) -> Account {
    account
        .apply(input, config)
        .unwrap_or_else(|e| panic!("{input:?}: {e}"))
        .account
}

pub fn text<T: ToString>(value: T) -> String {
    value.to_string()
}
