//! Builders shared by the accounting tests.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use mandate_accounting::{
    Account, AssetClass, CashDividend, Config, CorporateAction, CryptoFees, EquityFees, Execution,
    FeeFamily, Input, InstrumentId, Liquidity, Position, Side, Split, TafCapBasis,
};
use mandate_num::{
    Bps, CostBasis, FeeCap, FeePerShare, FeeRate, Price, Qty, ShareIncrement, SignedQty,
    SplitRatio, Usd,
};
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

pub fn ratio(new: u64, old: u64) -> SplitRatio {
    SplitRatio::new(new, old).unwrap()
}

pub fn split_of(
    instrument: &str,
    ex_date: &str,
    (new, old): (u64, u64),
    increment: ShareIncrement,
    cash_in_lieu_price: Option<&str>,
) -> Split {
    Split {
        instrument: id(instrument),
        ex_date: d(ex_date),
        ratio: ratio(new, old),
        increment,
        cash_in_lieu_price: cash_in_lieu_price.map(|p| Price::parse(p).unwrap()),
    }
}

pub fn split(
    instrument: &str,
    ex_date: &str,
    new_old: (u64, u64),
    increment: ShareIncrement,
    cash_in_lieu_price: Option<&str>,
) -> Input {
    Input::CorporateAction(CorporateAction::Split(split_of(
        instrument,
        ex_date,
        new_old,
        increment,
        cash_in_lieu_price,
    )))
}

pub fn dividend(instrument: &str, ex_date: &str, pay_date: &str, per_share: &str) -> Input {
    Input::CorporateAction(CorporateAction::CashDividend(
        CashDividend::new(
            id(instrument),
            d(ex_date),
            d(pay_date),
            Price::parse(per_share).unwrap(),
        )
        .unwrap(),
    ))
}

pub fn dividend_paid(instrument: &str, ex_date: &str) -> Input {
    Input::DividendPaid {
        instrument: id(instrument),
        ex_date: d(ex_date),
    }
}

pub fn cash_in_lieu_posted(instrument: &str, amount: &str) -> Input {
    Input::CashInLieuPosted {
        instrument: id(instrument),
        amount: usd(amount),
    }
}

/// An account with `settled` cash and one position.
pub fn holding(settled: &str, instrument: &str, qty: &str, basis: &str) -> Account {
    Account::opening(
        usd(settled),
        [(
            id(instrument),
            Position::new(
                SignedQty::parse(qty).unwrap(),
                CostBasis::parse(basis).unwrap(),
            )
            .unwrap(),
        )],
    )
}

/// Rebuilds an input from the text of every field, as a journal round trip would.
pub fn round_trip(input: &Input) -> Input {
    match input {
        Input::Fill(e) => Input::Fill(Execution {
            fill_id: e.fill_id.clone(),
            client_order_id: e.client_order_id.clone(),
            instrument: id(e.instrument.as_str()),
            asset_class: e.asset_class,
            side: e.side,
            qty_gross: Qty::parse(&e.qty_gross.to_string()).unwrap(),
            price: Price::parse(&e.price.to_string()).unwrap(),
            liquidity: e.liquidity,
            executed_at: UtcNanos::parse(&e.executed_at.to_string()).unwrap(),
        }),
        Input::Mark { instrument, price } => Input::Mark {
            instrument: id(instrument.as_str()),
            price: Price::parse(&price.to_string()).unwrap(),
        },
        Input::FeesCharged { family, day } => Input::FeesCharged {
            family: *family,
            day: d(&day.to_string()),
        },
        Input::SettlementPosted { date } => Input::SettlementPosted {
            date: d(&date.to_string()),
        },
        Input::CorporateAction(CorporateAction::Split(s)) => {
            Input::CorporateAction(CorporateAction::Split(Split {
                instrument: id(s.instrument.as_str()),
                ex_date: d(&s.ex_date.to_string()),
                ratio: SplitRatio::new(
                    s.ratio.new_shares().to_string().parse().unwrap(),
                    s.ratio.old_shares().to_string().parse().unwrap(),
                )
                .unwrap(),
                increment: s.increment,
                cash_in_lieu_price: s
                    .cash_in_lieu_price
                    .map(|p| Price::parse(&p.to_string()).unwrap()),
            }))
        }
        Input::CorporateAction(CorporateAction::CashDividend(c)) => {
            Input::CorporateAction(CorporateAction::CashDividend(
                CashDividend::new(
                    id(c.instrument().as_str()),
                    d(&c.ex_date().to_string()),
                    d(&c.pay_date().to_string()),
                    Price::parse(&c.per_share().to_string()).unwrap(),
                )
                .unwrap(),
            ))
        }
        Input::DividendPaid {
            instrument,
            ex_date,
        } => Input::DividendPaid {
            instrument: id(instrument.as_str()),
            ex_date: d(&ex_date.to_string()),
        },
        Input::CashInLieuPosted { instrument, amount } => Input::CashInLieuPosted {
            instrument: id(instrument.as_str()),
            amount: usd(&amount.to_string()),
        },
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
