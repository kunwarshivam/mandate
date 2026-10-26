//! An agent stream's own thesis records replayed as the admission judged them, never judged again
//! ([DEC-2249](../../../docs/project/decisions/DEC-2249.md) items 1 and 2,
//! [DEC-2819](../../../docs/project/decisions/DEC-2819.md)). The runtime's research source and the
//! executor read this one replay, so both hold the same working universe.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::Value;
use mandate_time::UtcNanos;

use crate::{
    AdmissionChange, AdmissionDecision, AssetId, LineageId, LineageState, RefusalReason, ThesisId,
    UniverseEntry, WorkingUniverse, expire_theses,
};

/// One record of an agent stream, as the replay reads it: its type and its payload.
#[derive(Debug, Clone)]
pub struct ThesisRecord<'a> {
    pub event_type: &'a str,
    pub payload: Value,
}

/// What an agent stream's thesis records replay to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replayed {
    /// The working universe at the replay's instant: each instrument a surviving thesis holds.
    pub universe: WorkingUniverse,
    pub lineages: LineageState,
    /// The surviving theses, one per instrument, in instrument order.
    pub active: Vec<UniverseEntry>,
    /// Every instrument any record admitted, its thesis surviving or not: what a position the agent
    /// opened may be held in, and so what its exits and fills name (mandate spec §8.5, removal is
    /// exits-only).
    pub admitted: BTreeSet<AssetId>,
}

/// Why an agent stream's thesis records do not replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReplayError {
    /// A record's member, by name, is missing or not what the journal spec types it as.
    #[error("a thesis record's {member} cannot be read")]
    Unreadable { member: &'static str },
}

/// Replays `records`, in stream order, at `at`, reading each only when the one before it replayed,
/// so a record its reader could not give (an `Err` item) stops the replay where it stands in the
/// stream, as a member a record cannot give does. Each `ThesisProposed` and `ThesisRevised` replays
/// its recorded decision into its lineage through [`LineageState::replay`]; an admitted one becomes
/// its instrument's one universe entry, replacing whatever thesis held it, and joins `admitted`; a
/// `ThesisInvalidated` marks the entry of the thesis it names, while that thesis still holds its
/// instrument. Every other record is passed over. The entries then go through [`expire_theses`] at
/// `at`, and the survivors are the universe and the active theses.
///
/// # Errors
/// The first `Err` item, or [`ReplayError::Unreadable`] naming the first member a record cannot
/// give, including an admitted record with a `reason`, a refused one without one, or a reason that
/// is not a refusal's code, whichever comes first in the stream.
pub fn replay_theses<'a>(
    records: impl IntoIterator<Item = Result<ThesisRecord<'a>, ReplayError>>,
    at: UtcNanos,
) -> Result<Replayed, ReplayError> {
    let unreadable = |member| ReplayError::Unreadable { member };
    let mut lineages = LineageState::default();
    let mut entries: BTreeMap<AssetId, UniverseEntry> = BTreeMap::new();
    let mut admitted = BTreeSet::new();
    for record in records {
        let record = record?;
        let invalidation = record.event_type == "ThesisInvalidated";
        if !invalidation && !matches!(record.event_type, "ThesisProposed" | "ThesisRevised") {
            continue;
        }
        let payload = &record.payload;
        let text = |member| {
            payload
                .get(member)
                .and_then(Value::as_str)
                .ok_or(unreadable(member))
        };
        let thesis = ThesisId::new(text("thesis_id")?).map_err(|_| unreadable("thesis_id"))?;
        if invalidation {
            for entry in entries.values_mut() {
                if entry.thesis_id == thesis {
                    entry.invalidated = true;
                }
            }
            continue;
        }
        let lineage = LineageId::new(text("lineage_id")?).map_err(|_| unreadable("lineage_id"))?;
        let instrument =
            AssetId::new(text("instrument_id")?).map_err(|_| unreadable("instrument_id"))?;
        let revision = payload
            .get("revision")
            .and_then(Value::as_int)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or(unreadable("revision"))?;
        let expires_at =
            UtcNanos::parse_rfc3339(text("expires_at")?).map_err(|_| unreadable("expires_at"))?;
        let is_admitted = match payload.get("admitted") {
            Some(Value::Bool(is_admitted)) => *is_admitted,
            _ => return Err(unreadable("admitted")),
        };
        let decision = match (is_admitted, payload.get("reason")) {
            (true, Some(Value::Null)) => AdmissionDecision::Admitted {
                change: AdmissionChange::Admitted,
            },
            (false, Some(Value::Str(code))) => AdmissionDecision::Refused {
                reason: RefusalReason::parse(code).map_err(|_| unreadable("reason"))?,
            },
            _ => return Err(unreadable("reason")),
        };
        lineages.replay(&lineage, revision, &instrument, decision);
        if is_admitted {
            admitted.insert(instrument.clone());
            entries.insert(
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
    }
    let entries: Vec<UniverseEntry> = entries.into_values().collect();
    let expiry = expire_theses(at, &entries, &lineages).map_err(|_| unreadable("expires_at"))?;
    let WorkingUniverse::Known {
        instruments: kept, ..
    } = &expiry.universe
    else {
        return Err(unreadable("expires_at"));
    };
    let active = entries
        .into_iter()
        .filter(|entry| kept.contains(&entry.instrument))
        .collect();
    Ok(Replayed {
        universe: expiry.universe,
        lineages,
        active,
        admitted,
    })
}
