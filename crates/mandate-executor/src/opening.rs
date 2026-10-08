//! Two figures an equity opening needs that the builder does not size: the protective prices of
//! its bracket, since protection is the executor's to place (DEC-130 item 18), and the account's
//! 1× buying power, which the gate's check 7 compares against (trading-domain spec §7.2, DEC-470
//! item 5).

use mandate_num::{Adverse, Fraction, Price, TickRule, Usd};

use crate::error::ExecutorError;
use crate::types::{BrokerAccount, ProtectionPrices};

/// The resting protection for an equity entry at `entry`, from the mandate's distances as fractions
/// of the entry price (mandate spec §3, `protection`): the stop at `entry × (1 − stop_distance)` and
/// the take-profit at `entry × (1 + take_profit_distance)`. Both are sell prices, so each rounds up
/// onto the Reg NMS grid (trading-domain spec §2.1): a stop that triggers sooner and a take-profit
/// that asks more, never the reverse (DEC-470 item 3).
///
/// The take-profit's exact product is first truncated to the 9 places a price holds, as
/// [`Price::collar_bound`] does for a buy's bound. That can only lower it, by at most one tick, and
/// only for an entry or distance whose product has no digit between the tick and the ninth place but
/// one past it. A lower take-profit sells a gain sooner and adds no risk.
///
/// # Errors
/// [`ExecutorError::Num`] when a price reaches zero or leaves the range a price holds.
pub fn equity_bracket_prices(
    entry: Price,
    stop_distance: Fraction,
    take_profit_distance: Option<Fraction>,
) -> Result<ProtectionPrices, ExecutorError> {
    let stop = entry
        .collar_bound(stop_distance, Adverse::Down)?
        .on_tick(TickRule::RegNmsEquity, Adverse::Down)?;
    let take_profit = take_profit_distance
        .map(|distance| {
            entry
                .collar_bound(distance, Adverse::Up)?
                .on_tick(TickRule::RegNmsEquity, Adverse::Down)
        })
        .transpose()?;
    Ok(ProtectionPrices { stop, take_profit })
}

impl BrokerAccount {
    /// §7.2's enforced 1× cap as the broker reports it: the lesser of its cash and its
    /// non-marginable buying power, so no margin the account's `multiplier` offers is spent
    /// (DEC-129 item 7, DEC-470 item 5).
    pub fn one_x_buying_power(&self) -> Usd {
        self.cash.min(self.non_marginable_buying_power)
    }
}

#[cfg(test)]
mod tests {
    use mandate_num::{Fraction, NumError, Price, Usd};

    use super::equity_bracket_prices;
    use crate::error::ExecutorError;
    use crate::types::{BrokerAccount, ProtectionPrices};

    type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

    fn price(text: &str) -> Result<Price> {
        Ok(Price::parse(text)?)
    }

    fn fraction(text: &str) -> Result<Fraction> {
        Ok(Fraction::parse(text)?)
    }

    /// Hand-calculated: 255.20 × 0.95 = 242.44 and 255.20 × 1.10 = 280.72, both already on the cent
    /// grid. 199.99 × 0.95 = 189.9905 rounds up to 190.00, not down to 189.99; 199.99 × 1.10 =
    /// 219.989 rounds up to 219.99, not down to 219.98. Below one dollar the grid is 0.0001:
    /// 0.5003 × 0.95 = 0.475285 rounds up to 0.4753.
    #[test]
    fn each_protective_price_rounds_up_onto_the_reg_nms_grid() -> Result<()> {
        let cases = [
            ("255.2", "0.05", Some("0.1"), "242.44", Some("280.72")),
            ("199.99", "0.05", Some("0.1"), "190", Some("219.99")),
            ("0.5003", "0.05", None, "0.4753", None),
        ];
        for (entry, stop, take_profit, expected_stop, expected_take_profit) in cases {
            let take_profit = take_profit.map(fraction).transpose()?;
            let expected = ProtectionPrices {
                stop: price(expected_stop)?,
                take_profit: expected_take_profit.map(price).transpose()?,
            };
            assert_eq!(
                equity_bracket_prices(price(entry)?, fraction(stop)?, take_profit)?,
                expected,
                "entry {entry}"
            );
        }
        Ok(())
    }

    /// A stop distance of the whole price would rest a stop at zero, which is no price.
    #[test]
    fn a_stop_at_zero_is_refused() -> Result<()> {
        assert_eq!(
            equity_bracket_prices(price("255.2")?, Fraction::ONE, None),
            Err(ExecutorError::Num(NumError::NotPositive))
        );
        Ok(())
    }

    fn account(cash: &str, non_marginable: &str) -> Result<BrokerAccount> {
        Ok(BrokerAccount {
            status: "ACTIVE".to_owned(),
            crypto_status: "ACTIVE".to_owned(),
            trading_blocked: false,
            account_blocked: false,
            trade_suspended_by_user: false,
            multiplier: 4,
            equity: Usd::parse("1000000")?,
            cash: Usd::parse(cash)?,
            buying_power: Usd::parse("4000000")?,
            non_marginable_buying_power: Usd::parse(non_marginable)?,
            accrued_fees: Usd::ZERO,
            last_equity: Usd::parse("1000000")?,
            maintenance_margin: Usd::ZERO,
        })
    }

    /// Whichever of cash and non-marginable buying power is lower binds, and the 4× margin figure
    /// never does.
    #[test]
    fn one_x_buying_power_is_the_lesser_of_cash_and_non_marginable() -> Result<()> {
        assert_eq!(
            account("1000000", "999998.99")?.one_x_buying_power(),
            Usd::parse("999998.99")?
        );
        assert_eq!(
            account("500", "999998.99")?.one_x_buying_power(),
            Usd::parse("500")?
        );
        Ok(())
    }
}
