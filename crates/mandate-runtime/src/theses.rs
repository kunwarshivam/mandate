//! Each instrument's current thesis and when it ended, folded from the runtime's own agent stream
//! and the account stream's removals (mandate spec §8.6, MI-34, DEC-2391).
//!
//! A thesis is current for its instrument from the latest `ThesisProposed` or `ThesisRevised` with
//! `admitted` `true` that names the instrument (journal spec §9.4). It ends at the first of a
//! `ThesisInvalidated` naming it (§9.22), the risk clock reaching its `expires_at`, a refused
//! thesis record with reason `lineage_retired` in its lineage, or an account-stream
//! `UniverseChanged` (`removed`) naming it with a thesis reason. Whichever comes first ends it,
//! because an end only reduces risk: the order plan then sells the position (§8.3 step 2), and
//! nothing here can open or enlarge one.
//!
//! The ending rules are `mandate_research`'s own, [`expire_theses`] over the folded
//! [`LineageState`], replayed exactly as the daemon's research fold replays them (DEC-2249), so the
//! instruments the plan sells are the ones the research run no longer counts as active.

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_canon::Value;
use mandate_research::{
    AdmissionChange, AdmissionDecision, AssetId, LineageId, LineageState, RefusalReason, ThesisId,
    UniverseEntry, expire_theses,
};
use mandate_time::UtcNanos;

use crate::error::RuntimeError;
use crate::payload;
use crate::types::RiskClock;

/// The `UniverseChanged` reasons that end a thesis (mandate spec §8.6). The others
/// (`eligibility_lost`, `operator_halt`, `version_applied`) leave a position to §2.2's signal exit
/// (DEC-2391 item 3).
const THESIS_REASONS: [&str; 3] = ["thesis_expired", "thesis_invalidated", "lineage_retired"];

/// The agent's theses as the fold holds them: one entry per instrument, and every lineage.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Theses {
    entries: BTreeMap<AssetId, UniverseEntry>,
    lineages: LineageState,
    /// Each admitted thesis's instrument's asset class, as its record states it from reference data
    /// (journal spec §9.4), which the plan needs to price and route the instrument's exit.
    classes: BTreeMap<AssetId, AssetClass>,
}

impl Theses {
    /// A `ThesisProposed` or `ThesisRevised`: its decision replays into its lineage, and an admitted
    /// one becomes its instrument's current thesis, replacing whatever thesis held it.
    pub(crate) fn recorded(&mut self, body: &Value) -> Result<(), RuntimeError> {
        let thesis = thesis_of(body)?;
        let lineage = lineage_of(body)?;
        let instrument = asset_of(body, "instrument_id")?;
        let revision = body
            .get("revision")
            .and_then(Value::as_int)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| payload::non_canonical("revision"))?;
        let expires_at = payload::str_of(body, "expires_at")
            .and_then(|at| UtcNanos::parse_rfc3339(at).ok())
            .ok_or_else(|| payload::non_canonical("expires_at"))?;
        let class = match payload::str_of(body, "asset_class") {
            Some("us_equity") => AssetClass::UsEquity,
            Some("crypto") => AssetClass::Crypto,
            _ => return Err(payload::non_canonical("asset_class")),
        };
        let admitted = match body.get("admitted") {
            Some(Value::Bool(admitted)) => *admitted,
            _ => return Err(payload::non_canonical("admitted")),
        };
        let decision = match (admitted, body.get("reason")) {
            (true, Some(Value::Null)) => AdmissionDecision::Admitted {
                change: AdmissionChange::Admitted,
            },
            (false, Some(Value::Str(code))) => AdmissionDecision::Refused {
                reason: RefusalReason::parse(code).map_err(|_| payload::non_canonical("reason"))?,
            },
            _ => return Err(payload::non_canonical("reason")),
        };
        self.lineages
            .replay(&lineage, revision, &instrument, decision);
        if admitted {
            self.classes.insert(instrument.clone(), class);
            self.entries.insert(
                instrument.clone(),
                UniverseEntry {
                    instrument,
                    thesis_id: thesis,
                    lineage_id: lineage,
                    revision,
                    expires_at,
                    invalidated: false,
                },
            );
        }
        Ok(())
    }

    /// A `ThesisInvalidated`: the thesis it names ends, if it is still its instrument's current
    /// one. One a later admission superseded ends nothing, as journal spec §9.22 rule 187 says.
    pub(crate) fn invalidated(&mut self, body: &Value) -> Result<(), RuntimeError> {
        let thesis = thesis_of(body)?;
        self.end(&thesis);
        Ok(())
    }

    /// An account-stream `UniverseChanged`: a removal of this agent's instrument with a thesis
    /// reason ends the thesis it names. An admission, another reason, or another agent's record
    /// changes nothing here: admission is the thesis records', and the working universe is the
    /// gate's (mandate spec §2.3).
    pub(crate) fn universe_changed(
        &mut self,
        body: &Value,
        agent: &str,
    ) -> Result<(), RuntimeError> {
        if payload::str_of(body, "agent_id") != Some(agent)
            || payload::str_of(body, "change") != Some("removed")
            || !payload::str_of(body, "reason")
                .is_some_and(|reason| THESIS_REASONS.contains(&reason))
        {
            return Ok(());
        }
        let thesis = thesis_of(body)?;
        self.end(&thesis);
        Ok(())
    }

    /// The instruments whose current thesis has ended at `now` (mandate spec §8.6), each as the
    /// order plan names it. Should `now` not be a time the ending rules can compare, or the rules
    /// refuse the entries, which one entry per instrument rules out, every instrument with a thesis
    /// is given: a sell adds no risk and a silent hold might (`AGENTS.md` rule 3). An asset id is
    /// never empty, so each converts to an instrument id.
    pub(crate) fn ended(&self, now: RiskClock) -> BTreeSet<InstrumentId> {
        let entries: Vec<UniverseEntry> = self.entries.values().cloned().collect();
        let ended: Vec<AssetId> = UtcNanos::from_parts(now.secs(), 0)
            .ok()
            .and_then(|at| expire_theses(at, &entries, &self.lineages).ok())
            .map_or_else(
                || self.entries.keys().cloned().collect(),
                |expiry| expiry.removed,
            );
        ended
            .iter()
            .filter_map(|asset| InstrumentId::new(asset.as_str()).ok())
            .collect()
    }

    /// The asset class of every instrument an admitted thesis named, as the order plan names it.
    pub(crate) fn classes(&self) -> BTreeMap<InstrumentId, AssetClass> {
        self.classes
            .iter()
            .filter_map(|(asset, class)| Some((InstrumentId::new(asset.as_str()).ok()?, *class)))
            .collect()
    }

    fn end(&mut self, thesis: &ThesisId) {
        for entry in self.entries.values_mut() {
            if entry.thesis_id == *thesis {
                entry.invalidated = true;
            }
        }
    }
}

fn thesis_of(body: &Value) -> Result<ThesisId, RuntimeError> {
    payload::str_of(body, "thesis_id")
        .and_then(|id| ThesisId::new(id).ok())
        .ok_or_else(|| payload::non_canonical("thesis_id"))
}

fn lineage_of(body: &Value) -> Result<LineageId, RuntimeError> {
    payload::str_of(body, "lineage_id")
        .and_then(|id| LineageId::new(id).ok())
        .ok_or_else(|| payload::non_canonical("lineage_id"))
}

fn asset_of(body: &Value, member: &str) -> Result<AssetId, RuntimeError> {
    payload::str_of(body, member)
        .and_then(|id| AssetId::new(id).ok())
        .ok_or_else(|| payload::non_canonical(member))
}
