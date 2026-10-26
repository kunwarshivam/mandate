//! The reviewed production inputs of one deployment, read once from the confirmed version and the
//! effective registrations and judged on the bytes hashed into the executor's config references
//! (DEC-470 item 2; E19-11, DEC-505), moved here from the paper shell (DEC-2380 item 5) so the
//! executor process reads its deployment the same way.

use std::time::Duration;

use mandate_accounting::InstrumentId;
use mandate_canon::{Digest, Value};
use mandate_domain::{AssetClass, AssetId};
use mandate_executor::{BindingGateConfigRefs, ExecutorConfig};
use mandate_num::{Qty, ShareIncrement};
use mandate_risk::{EtpClass, Exchange, GateConfig};
use mandate_time::UtcNanos;

use crate::control::{Configuration, ConfirmedVersion, SnapshotExchange};
use crate::error::{InputError, missing};
use crate::rules::{
    CALENDAR_MEMBERS, RULE_MEMBERS, artifact_from, environment_text, fee_artifact, fees, reference,
    required_text, rule_set,
};

/// One whole share, the quantity grid the instrument artifact admits.
const WHOLE_SHARE: &str = "1";

/// What the instrument artifact states, each field read by the run: the symbol the broker is read
/// by, the listing exchange, the quantity grid, and the ETP classification.
pub struct ReviewedInstrument {
    pub asset_id: AssetId,
    pub symbol: InstrumentId,
    pub asset_class: AssetClass,
    pub exchange: SnapshotExchange,
    pub gate_exchange: Exchange,
    pub increment: ShareIncrement,
    pub increment_qty: Qty,
    pub etp: EtpClass,
    pub etp_classified_at: UtcNanos,
}

/// The reviewed E7-7 artifacts of one deployment, each judged on the bytes hashed into
/// `config_refs`.
pub struct Reviewed {
    pub mandate: mandate_spec::Mandate,
    pub model_id: String,
    pub model_version: String,
    pub model_hash: Digest,
    pub fees: mandate_accounting::Config,
    pub instrument: ReviewedInstrument,
    pub quote_max_age: Duration,
    pub config_refs: BindingGateConfigRefs,
    pub gate_config: GateConfig,
    pub executor_config: ExecutorConfig,
}

impl Reviewed {
    /// The deployment's artifacts from the confirmed version and the effective registrations on the
    /// control stream (E19-11, DEC-505), never from a file. The deployment's stream must be in the
    /// mandate's environment, and the model, fee schedule, calendar, rule set and instrument
    /// snapshot must each have been registered on that same stream (DEC-952 item 1, DEC-957). The
    /// fee schedule must name the mandate's environment. Then: the mandate and its one pinned
    /// model, whose registered content is a DEC-504 content object naming the pin's own `model_id`
    /// and `model_version` under the pinned hash; the registered fee schedule, calendar and rule
    /// set; and the instrument from the DEC-523 snapshot, which must be the mandate's pinned asset
    /// and symbol. The config references hash the registered bytes.
    ///
    /// # Errors
    /// [`InputError::Missing`] for a registered object the run cannot use.
    pub fn from_registered(
        confirmed: &ConfirmedVersion,
        configuration: &Configuration,
    ) -> Result<Self, InputError> {
        let mandate = confirmed.mandate().clone();
        on_its_own_stream(confirmed, configuration)?;
        let [pinned] = mandate.universe.pinned_instruments.as_slice() else {
            return Err(missing("one pinned production instrument"));
        };
        let [model] = mandate.behavior.signal_models.as_slice() else {
            return Err(missing("the one reviewed signal model"));
        };
        let registered = &configuration.model_version;
        let content = mandate_canon::parse(&registered.bytes)
            .map_err(|_| missing("the registered model content"))?;
        let member = |name: &str| content.get(name).and_then(Value::as_str);
        if model.content_hash != registered.content_hash
            || member("kind") != Some("quant_model_content")
            || member("model_id") != Some(model.id.as_str())
            || member("model_version") != Some(model.version.as_str())
        {
            return Err(missing("the registered model"));
        }
        let snapshot = &configuration.instrument;
        if pinned.asset_class != AssetClass::UsEquity
            || snapshot.instrument_id != pinned.asset_id.as_str()
            || snapshot.symbol != pinned.symbol
        {
            return Err(missing("the registered instrument"));
        }
        let instrument = reviewed_instrument(&pinned.asset_id, &pinned.symbol, snapshot)?;
        let model = model.clone();
        Self::assemble(mandate, configuration, &model, instrument)
    }

    /// A research deployment's artifacts for one admitted instrument (DEC-2819 item 3): the
    /// confirmed version's model is its one signal model that admits instruments, which only an
    /// unpinned version has (the confirmed version passed V-036, at most one such model, and
    /// V-037, none in a pinned version), and
    /// that admits instruments, whose registration `configuration` read under its own hash. The
    /// instrument is the registered snapshot's, a US equity the mandate's asset classes allow. The
    /// fee schedule, calendar and rule set are read as [`Self::from_registered`] reads them, every
    /// binding on the deployment's own stream.
    ///
    /// # Errors
    /// [`InputError::Missing`] for a registered object the run cannot use, a pinned version, or a
    /// version with no one admitting model.
    pub fn from_admitted(
        confirmed: &ConfirmedVersion,
        configuration: &Configuration,
    ) -> Result<Self, InputError> {
        let mandate = confirmed.mandate().clone();
        on_its_own_stream(confirmed, configuration)?;
        let Some(model) = mandate
            .behavior
            .signal_models
            .iter()
            .find(|model| model.admits_instruments)
        else {
            return Err(missing("the one model that admits instruments"));
        };
        if model.content_hash != configuration.model_version.content_hash {
            return Err(missing("the registered admitting model"));
        }
        let snapshot = &configuration.instrument;
        if !mandate
            .universe
            .asset_classes
            .contains(&AssetClass::UsEquity)
        {
            return Err(missing("an asset class the mandate allows"));
        }
        let asset_id = AssetId::parse(&snapshot.instrument_id)
            .map_err(|_| missing("the admitted instrument's asset id"))?;
        let instrument = reviewed_instrument(&asset_id, &snapshot.symbol, snapshot)?;
        let model = model.clone();
        Self::assemble(mandate, configuration, &model, instrument)
    }

    /// The artifacts of `mandate`, `model` and `instrument` from `configuration`'s fee schedule,
    /// calendar and rule set, the config references hashing the registered bytes.
    fn assemble(
        mandate: mandate_spec::Mandate,
        configuration: &Configuration,
        model: &mandate_spec::document::SignalModel,
        instrument: ReviewedInstrument,
    ) -> Result<Self, InputError> {
        let fee = fee_artifact(configuration.fee_config.bytes.clone())?;
        let calendar = artifact_from(
            configuration.trading_calendar.bytes.clone(),
            CALENDAR_MEMBERS,
        )?;
        let rules = artifact_from(configuration.rule_set.bytes.clone(), RULE_MEMBERS)?;
        let fees = fees(&fee, &calendar)?;
        if required_text(&fee.value, "environment")? != environment_text(mandate.environment) {
            return Err(missing("a fee schedule for the mandate's environment"));
        }
        let (quote_max_age, gate_config, executor_config) = rule_set(&rules)?;
        let canonical_mandate = mandate
            .canonical_bytes()
            .map_err(|_| missing("the mandate's canonical artifact"))?;
        let config_refs = BindingGateConfigRefs::complete(
            reference(&fee.bytes),
            reference(&calendar.bytes),
            reference(&configuration.instrument_snapshot.bytes),
            reference(&rules.bytes),
            reference(&canonical_mandate),
        );
        Ok(Self {
            model_id: model.id.as_str().to_owned(),
            model_version: model.version.clone(),
            model_hash: model.content_hash,
            mandate,
            fees,
            instrument,
            quote_max_age,
            config_refs,
            gate_config,
            executor_config,
        })
    }
}

/// Refuses unless every binding of `configuration` was journaled on the deployment's own control
/// stream, in the mandate's environment (DEC-952 item 1, DEC-957).
fn on_its_own_stream(
    confirmed: &ConfirmedVersion,
    configuration: &Configuration,
) -> Result<(), InputError> {
    let deployed_on = confirmed.deployed_on();
    let bindings = [
        &configuration.model_version,
        &configuration.fee_config,
        &configuration.trading_calendar,
        &configuration.rule_set,
        &configuration.instrument_snapshot,
    ];
    if deployed_on.environment != confirmed.mandate().environment
        || bindings
            .iter()
            .any(|registered| registered.origin != *deployed_on)
    {
        return Err(missing(
            "every binding journaled on the deployment's own control stream",
        ));
    }
    Ok(())
}

/// The reviewed plain, whole-share US equity `asset_id`, `symbol`, as `snapshot` lists it.
fn reviewed_instrument(
    asset_id: &AssetId,
    symbol: &str,
    snapshot: &crate::control::InstrumentSnapshot,
) -> Result<ReviewedInstrument, InputError> {
    Ok(ReviewedInstrument {
        asset_id: asset_id.clone(),
        symbol: InstrumentId::new(symbol)
            .map_err(|_| missing("the registered instrument symbol"))?,
        asset_class: AssetClass::UsEquity,
        exchange: snapshot.exchange,
        gate_exchange: match snapshot.exchange {
            SnapshotExchange::Arca => Exchange::Arca,
            SnapshotExchange::Nasdaq => Exchange::Nasdaq,
        },
        increment: ShareIncrement::Whole,
        increment_qty: Qty::parse(WHOLE_SHARE)?,
        etp: EtpClass::Plain,
        etp_classified_at: snapshot.etp_classified_at,
    })
}
