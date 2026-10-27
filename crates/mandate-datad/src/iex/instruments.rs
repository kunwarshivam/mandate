//! The instruments a workspace's IEX generations hold (DEC-2283 item 3, data-plane spec §3.8
//! "Instruments" and "Scope"): the symbol of each US-equity asset id's latest `instrument_snapshot`
//! registered on the control stream (DEC-523), read as the runtime's composer reads them, at most
//! [`BARS_INSTRUMENTS`] in order of each asset id's first registration.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Value, parse};
use mandate_journal::{ArtifactRef, ArtifactSource, StoredEvent, get_artifact};
use mandate_marketdata::model::Symbol;

/// The most instruments a workspace's generations hold, so one workspace's registrations cannot
/// crowd the platform key's budget.
pub const BARS_INSTRUMENTS: usize = 50;

/// One instrument a generation holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instrument {
    pub asset_id: String,
    pub symbol: Symbol,
}

/// What the control stream says the generations hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Registered {
    /// A row is not of the `paper` environment: the source serves paper workspaces only, so
    /// nothing is written.
    NotPaper,
    /// An instrument snapshot's registration or object does not read, so it may name any asset
    /// id and no symbol can be trusted: nothing is written, as the composer then wires no models.
    Unreadable,
    /// The instruments, in order of first registration.
    Instruments(Vec<Instrument>),
}

/// The latest snapshot of one asset id, and the sequence of its first registration.
struct Latest {
    first: u64,
    symbol: Option<String>,
    us_equity: bool,
}

/// The instruments of the workspace whose control stream is `control`, its snapshots read from
/// `artifacts`. Symbols two asset ids share are left out before the cap is applied, so they never
/// count against it. A registration row whose body does not read could be a snapshot, so it gives
/// [`Registered::Unreadable`], a stricter reading than the composer's, which skips it.
pub fn registered(control: &[StoredEvent], artifacts: &dyn ArtifactSource) -> Registered {
    if control.iter().any(|row| row.environment != "paper") {
        return Registered::NotPaper;
    }
    let mut latest: BTreeMap<String, Latest> = BTreeMap::new();
    for row in control
        .iter()
        .filter(|row| row.event_type == "ConfigSnapshotRegistered")
    {
        let Some(snapshot) = snapshot(row, artifacts) else {
            return Registered::Unreadable;
        };
        let Some(snapshot) = snapshot else {
            continue;
        };
        let entry = latest.entry(snapshot.asset_id).or_insert(Latest {
            first: row.seq,
            symbol: None,
            us_equity: false,
        });
        entry.symbol = snapshot.symbol;
        entry.us_equity = snapshot.us_equity;
    }
    let mut holders: BTreeMap<String, usize> = BTreeMap::new();
    for entry in latest.values().filter(|entry| entry.us_equity) {
        if let Some(symbol) = &entry.symbol {
            let count = holders.entry(symbol.clone()).or_default();
            *count = count.saturating_add(1);
        }
    }
    let shared: BTreeSet<&String> = holders
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|(symbol, _)| symbol)
        .collect();
    let mut instruments: Vec<(u64, Instrument)> = latest
        .iter()
        .filter(|(_, entry)| entry.us_equity)
        .filter_map(|(asset_id, entry)| {
            let text = entry.symbol.as_ref()?;
            if shared.contains(text) {
                return None;
            }
            let symbol = Symbol::parse(text)
                .ok()
                .filter(|symbol| !symbol.is_pair())?;
            Some((
                entry.first,
                Instrument {
                    asset_id: asset_id.clone(),
                    symbol,
                },
            ))
        })
        .collect();
    instruments.sort_by_key(|(first, _)| *first);
    Registered::Instruments(
        instruments
            .into_iter()
            .take(BARS_INSTRUMENTS)
            .map(|(_, instrument)| instrument)
            .collect(),
    )
}

/// What one instrument snapshot names.
struct Snapshot {
    asset_id: String,
    symbol: Option<String>,
    us_equity: bool,
}

/// One registration row: `None` when it does not read; `Some(None)` when it registers another
/// kind; otherwise what its snapshot names.
fn snapshot(row: &StoredEvent, artifacts: &dyn ArtifactSource) -> Option<Option<Snapshot>> {
    let body = parse(&row.body).ok()?;
    let payload = body.get("payload")?;
    if payload.get("kind").and_then(Value::as_str) != Some("instrument_snapshot") {
        return Some(None);
    }
    let reference = ArtifactRef::parse(payload.get("content_hash")?.as_str()?)?;
    let object = parse(&get_artifact(artifacts, &reference).ok()?).ok()?;
    Some(Some(Snapshot {
        asset_id: object.get("instrument_id")?.as_str()?.to_owned(),
        symbol: object
            .get("symbol")
            .and_then(Value::as_str)
            .map(str::to_owned),
        us_equity: object.get("asset_class").and_then(Value::as_str) == Some("us_equity"),
    }))
}
