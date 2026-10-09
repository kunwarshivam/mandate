//! Alpaca's account rules: trading spec §7.2's "Account type" and "Day-trading regime" rows as
//! data, declared by the connector so no caller names them for the broker (the first paper trade
//! brief's X-9, slice Q2; DEC-840).

use mandate_accounting::AccountType;

/// Which of trading spec §9.2's day-trading regimes a broker applies. The figures each regime
/// reads (maintenance excess, prior-close equity) are the broker's account answer, not part of
/// the declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclaredRegime {
    IntradayMargin,
    LegacyPdt,
}

/// A broker's account type and day-trading regime, as its connector declares them from the
/// broker's published contract. The answer never depends on a broker call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountRules {
    pub account_type: AccountType,
    pub regime: DeclaredRegime,
}

/// §7.2: "Account type: Alpaca: always margin" and "Day-trading regime: Alpaca:
/// `intraday_margin`".
#[must_use]
pub fn alpaca() -> AccountRules {
    AccountRules {
        account_type: AccountType::Margin,
        regime: DeclaredRegime::IntradayMargin,
    }
}
