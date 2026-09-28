//! The fee configuration the fold charges with (trading-domain spec §6.2, §6.3), built from a dated
//! schedule someone transcribed from the broker's published fee schedule, and validated here.
//!
//! This module holds **no** rates. §6.2 says "rates come from an effective-dated configuration
//! transcribed from Alpaca's fee schedule", so the values are data a deployment supplies (with a
//! content hash in `config_refs`), never constants in the crate (the coordinator's ruling on #174,
//! 5861849917, option (i)). The constructor only refuses a schedule it cannot read exactly.

use mandate_accounting::{Config, CryptoFees, EquityFees, TafCapBasis};
use mandate_num::{Bps, FeeCap, FeePerShare, FeeRate};
use mandate_time::{Date, TradingCalendar};

use crate::error::ExecutorError;

/// One dated fee schedule as transcribed, every figure in canonical decimal text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeeSchedule<'a> {
    /// The first trade date the schedule applies to; it must fall inside the calendar.
    pub effective_from: &'a str,
    /// SEC Section 31, a rate on sell proceeds.
    pub sec_rate: &'a str,
    /// FINRA TAF per share sold, and its cap.
    pub taf_per_share: &'a str,
    pub taf_cap: &'a str,
    /// `per_execution` or `per_order` (§6.2).
    pub taf_cap_basis: &'a str,
    /// CAT per share, buys and sells.
    pub cat_per_share: &'a str,
    /// Crypto maker and taker fees for the account's volume tier, in basis points (§6.3).
    pub crypto_maker_bps: &'a str,
    pub crypto_taker_bps: &'a str,
}

/// The accounting fee configuration for `schedule` over `calendar`. Every figure must parse exactly
/// in its own type, which already refuses a negative amount or rate (DEC-87); the cap basis must be
/// one §6.2 names; and the schedule must take effect on a date the calendar covers. A refusal
/// names the field, and nothing is defaulted (`AGENTS.md` rule 3).
pub fn fee_config(
    schedule: &FeeSchedule<'_>,
    calendar: TradingCalendar,
) -> Result<Config, ExecutorError> {
    let effective_from =
        Date::parse(schedule.effective_from).map_err(|_| refused("effective_from"))?;
    calendar
        .is_trading_day(effective_from)
        .map_err(|_| refused("effective_from"))?;
    Ok(Config {
        equities: EquityFees {
            sec_rate: FeeRate::parse(schedule.sec_rate).map_err(|_| refused("sec_rate"))?,
            taf_per_share: FeePerShare::parse(schedule.taf_per_share)
                .map_err(|_| refused("taf_per_share"))?,
            taf_cap: FeeCap::parse(schedule.taf_cap).map_err(|_| refused("taf_cap"))?,
            taf_cap_basis: match schedule.taf_cap_basis {
                "per_execution" => TafCapBasis::PerExecution,
                "per_order" => TafCapBasis::PerOrder,
                _ => return Err(refused("taf_cap_basis")),
            },
            cat_per_share: FeePerShare::parse(schedule.cat_per_share)
                .map_err(|_| refused("cat_per_share"))?,
        },
        crypto: CryptoFees {
            maker: Bps::parse(schedule.crypto_maker_bps)
                .map_err(|_| refused("crypto_maker_bps"))?,
            taker: Bps::parse(schedule.crypto_taker_bps)
                .map_err(|_| refused("crypto_taker_bps"))?,
        },
        calendar,
    })
}

fn refused(field: &str) -> ExecutorError {
    ExecutorError::NonCanonicalPayload {
        field: field.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use mandate_accounting::TafCapBasis;
    use mandate_num::{Bps, FeeCap, FeePerShare, FeeRate};
    use mandate_time::{Date, TradingCalendar};

    use super::{FeeSchedule, fee_config};
    use crate::error::ExecutorError;

    /// Test figures only, not Alpaca's: the crate holds no real rates.
    const SCHEDULE: FeeSchedule<'static> = FeeSchedule {
        effective_from: "2026-09-01",
        sec_rate: "0.00001",
        taf_per_share: "0.0001",
        taf_cap: "5",
        taf_cap_basis: "per_order",
        cat_per_share: "0.00002",
        crypto_maker_bps: "10",
        crypto_taker_bps: "20",
    };

    fn calendar() -> Result<TradingCalendar, ExecutorError> {
        Ok(TradingCalendar::new(
            Date::parse("2026-09-01")?,
            Date::parse("2026-12-31")?,
            [],
            [],
        )?)
    }

    #[test]
    fn a_readable_schedule_is_the_configuration_figure_for_figure() -> Result<(), ExecutorError> {
        let config = fee_config(&SCHEDULE, calendar()?)?;
        assert_eq!(config.equities.sec_rate, FeeRate::parse("0.00001")?);
        assert_eq!(config.equities.taf_per_share, FeePerShare::parse("0.0001")?);
        assert_eq!(config.equities.taf_cap, FeeCap::parse("5")?);
        assert_eq!(config.equities.taf_cap_basis, TafCapBasis::PerOrder);
        assert_eq!(
            config.equities.cat_per_share,
            FeePerShare::parse("0.00002")?
        );
        assert_eq!(config.crypto.maker, Bps::parse("10")?);
        assert_eq!(config.crypto.taker, Bps::parse("20")?);
        assert_eq!(config.calendar, calendar()?);
        let per_execution = FeeSchedule {
            taf_cap_basis: "per_execution",
            ..SCHEDULE
        };
        assert_eq!(
            fee_config(&per_execution, calendar()?)?
                .equities
                .taf_cap_basis,
            TafCapBasis::PerExecution
        );
        Ok(())
    }

    #[test]
    fn every_unreadable_figure_is_refused_by_name() -> Result<(), ExecutorError> {
        let cases: [(&str, FeeSchedule<'static>); 10] = [
            (
                "effective_from",
                FeeSchedule {
                    effective_from: "2026-13-01",
                    ..SCHEDULE
                },
            ),
            (
                "effective_from",
                FeeSchedule {
                    effective_from: "2027-01-04",
                    ..SCHEDULE
                },
            ),
            (
                "sec_rate",
                FeeSchedule {
                    sec_rate: "-0.1",
                    ..SCHEDULE
                },
            ),
            (
                "taf_per_share",
                FeeSchedule {
                    taf_per_share: "x",
                    ..SCHEDULE
                },
            ),
            (
                "taf_cap",
                FeeSchedule {
                    taf_cap: "-5",
                    ..SCHEDULE
                },
            ),
            (
                "taf_cap_basis",
                FeeSchedule {
                    taf_cap_basis: "per_day",
                    ..SCHEDULE
                },
            ),
            (
                "cat_per_share",
                FeeSchedule {
                    cat_per_share: "",
                    ..SCHEDULE
                },
            ),
            (
                "crypto_maker_bps",
                FeeSchedule {
                    crypto_maker_bps: "-1",
                    ..SCHEDULE
                },
            ),
            (
                "crypto_taker_bps",
                FeeSchedule {
                    crypto_taker_bps: "1e2",
                    ..SCHEDULE
                },
            ),
            (
                "taf_cap_basis",
                FeeSchedule {
                    taf_cap_basis: "",
                    ..SCHEDULE
                },
            ),
        ];
        for (field, schedule) in cases {
            assert_eq!(
                fee_config(&schedule, calendar()?),
                Err(ExecutorError::NonCanonicalPayload {
                    field: field.to_owned()
                }),
                "{schedule:?}"
            );
        }
        Ok(())
    }
}
