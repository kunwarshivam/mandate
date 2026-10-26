//! The agentic account's snapshot as the executor's `BrokerAccount` (DEC-933 item 1), moved here
//! from the shell so the shell and the executor process share one mapping (DEC-2386 item 2).
//! Oracle: DEC-933 item 1's figures, typed by hand.

use mandate_executor::BrokerAccount;
use mandate_num::Usd;
use mandate_robinhood::{AccountSnapshot, broker_account};

type Error = Box<dyn std::error::Error>;

fn usd(text: &str) -> Result<Usd, Error> {
    Ok(Usd::parse(text)?)
}

fn snapshot(cash: &str, buying_power: &str) -> Result<AccountSnapshot, Error> {
    Ok(AccountSnapshot {
        cash: usd(cash)?,
        buying_power: usd(buying_power)?,
        positions: Vec::new(),
        open_orders: Vec::new(),
    })
}

/// Cash as reported; both buying powers the lower of cash and buying power; every unpublished
/// figure the one that adds no buying power and lifts no limit.
fn expected(cash: &str, one_x: &str) -> Result<BrokerAccount, Error> {
    Ok(BrokerAccount {
        status: "ACTIVE".to_owned(),
        crypto_status: "INACTIVE".to_owned(),
        trading_blocked: false,
        account_blocked: false,
        trade_suspended_by_user: false,
        multiplier: 1,
        equity: usd(cash)?,
        cash: usd(cash)?,
        buying_power: usd(one_x)?,
        non_marginable_buying_power: usd(one_x)?,
        accrued_fees: Usd::ZERO,
        last_equity: Usd::ZERO,
        maintenance_margin: usd(cash)?,
    })
}

/// Buying power above cash reads as cash: no figure exceeds cash (DEC-620 item 2).
#[test]
fn buying_power_above_cash_reads_as_cash() -> Result<(), Error> {
    assert_eq!(
        broker_account(&snapshot("1000.5", "4000")?),
        expected("1000.5", "1000.5")?
    );
    Ok(())
}

/// Buying power below cash, as unsettled proceeds leave it, is taken as reported.
#[test]
fn buying_power_below_cash_is_taken_as_reported() -> Result<(), Error> {
    assert_eq!(
        broker_account(&snapshot("1000.5", "250.25")?),
        expected("1000.5", "250.25")?
    );
    Ok(())
}

/// An empty account reads as empty: no buying power out of nothing.
#[test]
fn an_empty_account_has_no_buying_power() -> Result<(), Error> {
    assert_eq!(broker_account(&snapshot("0", "0")?), expected("0", "0")?);
    Ok(())
}
