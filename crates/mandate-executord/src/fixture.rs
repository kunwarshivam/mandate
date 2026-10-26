//! The fixture wiring the binary runs until a deployment's production ports are wired (DEC-2007
//! item 5, as DEC-1013 item 2 does for the runtime): the core's ports with nothing known. No
//! mandate version, no instrument and no binding-gate fact, so the core admits no opening (an
//! unavailable input is never an allow), while a kill switch, which reads none of them for an agent
//! that holds nothing, is applied as ever.

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_canon::stream_event_id;
use mandate_executor::{
    AccountScope, AgentId, BindingGateInput, BindingGateRequest, BindingGateSource, EventId,
    ExecutorConfig, ExitTier, IdGen, InstrumentSnapshot, MandateVersion, MandateView,
    ProtectionPrices, Seq, WriterEpoch,
};
use mandate_num::{Bps, FeeCap, FeePerShare, FeeRate, Fraction, Price, ShareIncrement};
use mandate_time::{Date, TradingCalendar};

use crate::marks::NoMarks;
use crate::quotes::NoQuotes;
use crate::{Protections, Unplanned, Wiring};

/// The account stream's deterministic event ids, derived from the stream as well as the epoch,
/// head and ordinal ([`stream_event_id`]), so two accounts' streams in one journal never share an
/// id (journal spec §3) and a retry re-derives the same one (§5.1).
pub struct Ids {
    stream: String,
}

impl Ids {
    /// The ids of `scope`'s account stream, `acct:<workspace>:<account>`.
    pub fn of(scope: &AccountScope) -> Self {
        Self {
            stream: format!("acct:{}:{}", scope.workspace.0, scope.account.0),
        }
    }
}

impl IdGen for Ids {
    fn event_id(&self, epoch: WriterEpoch, head: Seq, ordinal: u32) -> EventId {
        EventId(stream_event_id(&self.stream, epoch.0, head.0, ordinal))
    }
}

/// No mandate is known: no version, no stop-limit offset, and no instrument covered.
struct NoMandate;

impl MandateView for NoMandate {
    fn version(&self, _agent: &AgentId) -> Option<MandateVersion> {
        None
    }

    fn crypto_stop_limit_offset(&self, _agent: &AgentId) -> Option<Fraction> {
        None
    }

    fn covers(&self, _agent: &AgentId, _instrument: &InstrumentId) -> bool {
        false
    }
}

/// No instrument is known.
struct NoInstruments;

impl InstrumentSnapshot for NoInstruments {
    fn asset_class(&self, _instrument: &InstrumentId) -> Option<AssetClass> {
        None
    }

    fn increment(&self, _instrument: &InstrumentId) -> Option<ShareIncrement> {
        None
    }

    fn exit_tier(&self, _instrument: &InstrumentId) -> Option<ExitTier> {
        None
    }
}

/// No binding-gate fact is available, so every opening is held (`AGENTS.md` rule 3).
struct NoFacts;

impl BindingGateSource for NoFacts {
    fn input(&self, _request: &BindingGateRequest<'_>) -> Option<BindingGateInput> {
        None
    }
}

/// No mandate version, so no protection to read. An opening handed without one is held at the
/// gate all the same, since [`NoFacts`] admits none (DEC-2033 item 3).
struct NoProtection;

impl Protections for NoProtection {
    fn protection(
        &self,
        _agent: &AgentId,
        _instrument: &InstrumentId,
        _entry: Price,
    ) -> Result<Option<ProtectionPrices>, Unplanned> {
        Ok(None)
    }
}

/// The fixture wiring for `scope`'s account.
///
/// # Errors
/// `fee_config_invalid` when the fixture's fee configuration does not read, which is a defect.
pub fn wiring(scope: &AccountScope) -> Result<Wiring, &'static str> {
    Ok(Wiring {
        ids: Box::new(Ids::of(scope)),
        mandates: Box::new(NoMandate),
        instruments: Box::new(NoInstruments),
        config: ExecutorConfig::PROPOSED,
        fees: fees().ok_or("fee_config_invalid")?,
        gate: Box::new(NoFacts),
        protections: Box::new(NoProtection),
        quotes: Box::new(NoQuotes),
        marked: Box::new(NoMarks),
    })
}

/// The `test_default` fee configuration of the trading-domain reference cases over their
/// `us_2026` calendar (trading spec §6.2, §6.3), as `mandate-executor`'s tests transcribe it: what
/// paper's simulated fees would be booked under. This wiring books no fill.
fn fees() -> Option<mandate_accounting::Config> {
    let date = |text: &str| Date::parse(text).ok();
    Some(mandate_accounting::Config {
        equities: mandate_accounting::EquityFees {
            sec_rate: FeeRate::parse("0.00003").ok()?,
            taf_per_share: FeePerShare::parse("0.0002").ok()?,
            taf_cap: FeeCap::parse("9.79").ok()?,
            fee_rounding: mandate_accounting::FeeRounding::DailyTotal,
            taf_cap_basis: mandate_accounting::TafCapBasis::PerExecution,
            cat_per_share: FeePerShare::parse("0.00001").ok()?,
        },
        crypto: mandate_accounting::CryptoFees {
            maker: Bps::parse("15").ok()?,
            taker: Bps::parse("25").ok()?,
        },
        calendar: TradingCalendar::new(
            date("2026-09-01")?,
            date("2026-12-31")?,
            [date("2026-11-26")?, date("2026-12-25")?],
            [date("2026-10-12")?, date("2026-11-11")?],
        )
        .ok()?,
    })
}

#[cfg(test)]
mod tests {
    use mandate_executor::{BindingGateRequest, Purpose, TimeInForce};

    use super::*;

    /// The fixture wiring knows nothing: no mandate version, offset or covered instrument, no
    /// instrument fact, and no binding-gate fact, so no opening can be admitted on it.
    #[test]
    fn the_fixture_wiring_knows_nothing() -> Result<(), Box<dyn std::error::Error>> {
        let wiring = wiring(&AccountScope {
            account: mandate_executor::AccountRef("acct-a".to_owned()),
            workspace: mandate_executor::WorkspaceId("ws1".to_owned()),
        })?;
        let agent = AgentId("agenta".to_owned());
        let instrument = InstrumentId::new("AAPL")?;
        assert_eq!(wiring.mandates.version(&agent), None);
        assert_eq!(wiring.mandates.crypto_stop_limit_offset(&agent), None);
        assert!(!wiring.mandates.covers(&agent, &instrument));
        assert_eq!(wiring.instruments.asset_class(&instrument), None);
        assert_eq!(wiring.instruments.increment(&instrument), None);
        assert_eq!(wiring.instruments.exit_tier(&instrument), None);
        let request = BindingGateRequest {
            agent: &agent,
            instrument: &instrument,
            side: mandate_accounting::Side::Buy,
            qty: mandate_num::Qty::parse("1")?,
            limit: mandate_num::Price::parse("150")?,
            purpose: Purpose::Open,
            tif: TimeInForce::Day,
            protection: None,
        };
        assert!(wiring.gate.input(&request).is_none());
        Ok(())
    }

    /// The account stream's ids are its stream's [`stream_event_id`]s, so two accounts' first
    /// batches never share one (lane DR's X9).
    #[test]
    fn the_ids_are_the_account_streams_own() {
        let scope = |account: &str| AccountScope {
            account: mandate_executor::AccountRef(account.to_owned()),
            workspace: mandate_executor::WorkspaceId("ws1".to_owned()),
        };
        let id = Ids::of(&scope("acct-a")).event_id(WriterEpoch(2), Seq(31), 4);
        assert_eq!(id.0, stream_event_id("acct:ws1:acct-a", 2, 31, 4));
        assert_ne!(
            Ids::of(&scope("acct-a")).event_id(WriterEpoch(1), Seq(0), 0),
            Ids::of(&scope("acct-b")).event_id(WriterEpoch(1), Seq(0), 0)
        );
    }
}
