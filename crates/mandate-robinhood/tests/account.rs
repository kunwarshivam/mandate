//! The agentic account's snapshot as the executor's `BrokerAccount` (DEC-933 item 1), moved here
//! from the shell so the shell and the executor process share one mapping (DEC-2386 item 2).
//! Oracle: DEC-933 item 1's figures, typed by hand.

use mandate_executor::BrokerAccount;
use mandate_num::Usd;
use mandate_robinhood::{AccountSnapshot, broker_account};

fn usd(text: &str) -> Usd {
    Usd::parse(text).unwrap_or(Usd::ZERO)
}

fn snapshot(cash: &str, buying_power: &str) -> AccountSnapshot {
    AccountSnapshot {
        cash: usd(cash),
        buying_power: usd(buying_power),
        positions: Vec::new(),
        open_orders: Vec::new(),
    }
}

/// Cash as reported; both buying powers the lower of cash and buying power; every unpublished
/// figure the one that adds no buying power and lifts no limit.
fn expected(cash: &str, one_x: &str) -> BrokerAccount {
    BrokerAccount {
        status: "ACTIVE".to_owned(),
        crypto_status: "INACTIVE".to_owned(),
        trading_blocked: false,
        account_blocked: false,
        trade_suspended_by_user: false,
        multiplier: 1,
        equity: usd(cash),
        cash: usd(cash),
        buying_power: usd(one_x),
        non_marginable_buying_power: usd(one_x),
        accrued_fees: Usd::ZERO,
        last_equity: Usd::ZERO,
        maintenance_margin: usd(cash),
    }
}

/// Buying power above cash reads as cash: no figure exceeds cash (DEC-620 item 2).
#[test]
fn buying_power_above_cash_reads_as_cash() {
    assert_eq!(
        broker_account(&snapshot("1000.5", "4000")),
        expected("1000.5", "1000.5")
    );
}

/// Buying power below cash, as unsettled proceeds leave it, is taken as reported.
#[test]
fn buying_power_below_cash_is_taken_as_reported() {
    assert_eq!(
        broker_account(&snapshot("1000.5", "250.25")),
        expected("1000.5", "250.25")
    );
}
