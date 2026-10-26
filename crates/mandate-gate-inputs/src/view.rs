//! One deployment's reviewed agent and instrument as the executor's ports answer them: its mandate
//! version, its one pinned instrument, the registered configuration its fills are accounted under,
//! and the protection an opening is handed with (DEC-2380 items 2 and 5). The paper shell and the
//! executor process read them here, so the two answer the same deployment the same way.

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_executor::{
    AccountFillRefs, AgentId, BindingGateConfigRefs, ExitTier, FillConfigRefs, InstrumentSnapshot,
    MandateVersion, MandateView, ProtectionPrices, equity_bracket_prices,
};
use mandate_num::{Fraction, Price, ShareIncrement};
use mandate_spec::document::Protection;

use crate::error::{InputError, missing};
use crate::reviewed::Reviewed;

/// The fill records' configuration from the registered references: the fee configuration, the
/// trading calendar (which also settles the paper run, DEC-938 item 2) and the instrument
/// snapshot, or none when any is missing, which the journal refuses.
pub fn fill_refs(refs: &BindingGateConfigRefs) -> Option<FillConfigRefs> {
    let calendar = refs.trading_calendar.clone()?;
    Some(FillConfigRefs {
        fee_config: refs.fee_config.clone()?,
        settlement_calendar: calendar.clone(),
        trading_calendar: calendar,
        instrument_snapshot: refs.instrument_snapshot.clone()?,
    })
}

/// The account's own fill configuration from the registered references: the fee configuration and
/// the trading calendar, which also settles the paper run (DEC-938 item 2), or none when either is
/// missing. An unattributed fill in an instrument with no snapshot is accounted under it
/// (DEC-2817).
pub fn account_fill_refs(refs: &BindingGateConfigRefs) -> Option<AccountFillRefs> {
    let calendar = refs.trading_calendar.clone()?;
    Some(AccountFillRefs {
        fee_config: refs.fee_config.clone()?,
        settlement_calendar: calendar.clone(),
        trading_calendar: calendar,
    })
}

/// The mandate's stop-limit offset when its protection is on and sets one; an offset that does
/// not read as a fraction refuses the run rather than leaving a stop-limit without its limit.
///
/// # Errors
/// [`InputError::Num`] for an offset that is not a fraction.
pub fn stop_limit_offset(protection: &Protection) -> Result<Option<Fraction>, InputError> {
    let Some(offset) = protection
        .crypto_stop_limit_offset
        .as_ref()
        .filter(|_| protection.enabled)
    else {
        return Ok(None);
    };
    Ok(Some(Fraction::parse(offset.as_str())?))
}

/// The mandate's protection for an entry at `entry`, as the executor derives it, or none when the
/// mandate disables protection. Enabled protection without a stop distance is refused rather than
/// sent unprotected.
///
/// # Errors
/// [`InputError`] when enabled protection names no stop distance or a distance is unreadable.
pub fn protection_prices(
    protection: &Protection,
    entry: Price,
) -> Result<Option<ProtectionPrices>, InputError> {
    if !protection.enabled {
        return Ok(None);
    }
    let stop_distance = protection
        .stop_distance
        .as_ref()
        .ok_or_else(|| missing("the mandate's stop distance"))?;
    let take_profit_distance = protection
        .take_profit_distance
        .as_ref()
        .map(|distance| Fraction::parse(distance.as_str()))
        .transpose()?;
    Ok(Some(equity_bracket_prices(
        entry,
        Fraction::parse(stop_distance.as_str())?,
        take_profit_distance,
    )?))
}

/// The deployed agent and its one reviewed instrument, keyed by the instrument's asset id.
#[derive(Debug, Clone)]
pub struct DeployedView {
    agent: String,
    asset: String,
    increment: ShareIncrement,
    config_refs: BindingGateConfigRefs,
    stop_limit_offset: Option<Fraction>,
    protection: Protection,
}

impl DeployedView {
    /// `agent`'s view of its deployment's reviewed artifacts.
    ///
    /// # Errors
    /// [`InputError::Num`] for a stop-limit offset that is not a fraction.
    pub fn new(agent: &str, reviewed: &Reviewed) -> Result<Self, InputError> {
        Ok(Self {
            agent: agent.to_owned(),
            asset: reviewed.instrument.asset_id.as_str().to_owned(),
            increment: reviewed.instrument.increment,
            config_refs: reviewed.config_refs.clone(),
            stop_limit_offset: stop_limit_offset(&reviewed.mandate.protection)?,
            protection: reviewed.mandate.protection.clone(),
        })
    }

    fn reviewed(&self, agent: &AgentId, instrument: &InstrumentId) -> bool {
        agent.0 == self.agent && self.instrument(instrument)
    }

    fn instrument(&self, instrument: &InstrumentId) -> bool {
        instrument.as_str() == self.asset
    }

    /// The protection the deployed agent's opening on its instrument at `entry` is handed with;
    /// none when the mandate disables protection.
    ///
    /// # Errors
    /// [`InputError::Missing`] for another agent or instrument, which this deployment cannot
    /// protect, and [`protection_prices`]'s refusals.
    pub fn protection(
        &self,
        agent: &AgentId,
        instrument: &InstrumentId,
        entry: Price,
    ) -> Result<Option<ProtectionPrices>, InputError> {
        if !self.reviewed(agent, instrument) {
            return Err(missing("the deployed agent's reviewed instrument"));
        }
        protection_prices(&self.protection, entry)
    }
}

impl MandateView for DeployedView {
    fn version(&self, agent: &AgentId) -> Option<MandateVersion> {
        (agent.0 == self.agent)
            .then(|| self.config_refs.mandate_version.clone())
            .flatten()
            .map(MandateVersion)
    }

    fn crypto_stop_limit_offset(&self, agent: &AgentId) -> Option<Fraction> {
        self.stop_limit_offset.filter(|_| agent.0 == self.agent)
    }

    fn covers(&self, agent: &AgentId, instrument: &InstrumentId) -> bool {
        self.reviewed(agent, instrument)
    }
}

impl InstrumentSnapshot for DeployedView {
    fn asset_class(&self, instrument: &InstrumentId) -> Option<AssetClass> {
        self.instrument(instrument).then_some(AssetClass::UsEquity)
    }

    fn increment(&self, instrument: &InstrumentId) -> Option<ShareIncrement> {
        self.instrument(instrument).then_some(self.increment)
    }

    fn exit_tier(&self, _instrument: &InstrumentId) -> Option<ExitTier> {
        None
    }

    /// The reviewed instrument's fills are accounted under the registered configuration
    /// (DEC-938 item 2); any other instrument's fill names none, so the journal refuses it.
    fn fill_refs(&self, instrument: &InstrumentId) -> Option<FillConfigRefs> {
        self.instrument(instrument)
            .then(|| fill_refs(&self.config_refs))
            .flatten()
    }

    /// The deployment's registered fee configuration and calendar, whatever the instrument
    /// (DEC-2817).
    fn account_fill_refs(&self) -> Option<AccountFillRefs> {
        account_fill_refs(&self.config_refs)
    }
}
