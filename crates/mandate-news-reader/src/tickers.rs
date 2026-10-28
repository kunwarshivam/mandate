//! Each asset id's ticker (DEC-2280 item 1): the `symbol` in the artifact of the latest
//! `ConfigSnapshotRegistered` of kind `instrument_snapshot` on the control stream whose artifact
//! names that `instrument_id` (DEC-523), the lookup the gate's inputs make. The reader reads no
//! other row of the control stream, and no other artifact.
//!
//! A registration whose artifact is missing, corrupt, not a snapshot object, or names no
//! `instrument_id` names no asset, and is counted. One whose `symbol` is not a ticker leaves its
//! asset with no ticker, as no snapshot does: a later registration wins whatever it holds.

use std::collections::BTreeMap;

use mandate_canon::{Value, parse};
use mandate_journal::{ArtifactRef, ArtifactSource, StoredEvent, get_artifact};
use mandate_sources::symbols::Symbol;

/// The registered tickers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tickers {
    /// Each asset id a readable snapshot names, with its ticker, or `None` when the latest such
    /// snapshot's `symbol` is not a ticker.
    by_asset: BTreeMap<String, Option<Symbol>>,
    /// Registrations of kind `instrument_snapshot` whose artifact names no asset.
    unreadable: u64,
}

impl Tickers {
    /// The ticker of `asset`, if the latest snapshot naming it gives one.
    pub fn ticker(&self, asset: &str) -> Option<&Symbol> {
        self.by_asset.get(asset).and_then(Option::as_ref)
    }

    /// How many registrations named no asset because their artifact could not be read.
    pub fn unreadable(&self) -> u64 {
        self.unreadable
    }
}

/// The tickers `control`'s registrations give, read through `artifacts`, in stream order.
pub fn registered_tickers(control: &[StoredEvent], artifacts: &dyn ArtifactSource) -> Tickers {
    let mut ordered: Vec<&StoredEvent> = control
        .iter()
        .filter(|row| row.event_type == "ConfigSnapshotRegistered")
        .collect();
    ordered.sort_by_key(|row| row.seq);
    let mut tickers = Tickers::default();
    for row in ordered {
        let Some(payload) = parse(&row.body)
            .ok()
            .and_then(|body| body.get("payload").cloned())
        else {
            tickers.unreadable = tickers.unreadable.saturating_add(1);
            continue;
        };
        if payload.get("kind").and_then(Value::as_str) != Some("instrument_snapshot") {
            continue;
        }
        match snapshot(&payload, artifacts) {
            Some((asset, symbol)) => {
                tickers.by_asset.insert(asset, symbol);
            }
            None => tickers.unreadable = tickers.unreadable.saturating_add(1),
        }
    }
    tickers
}

/// The asset id the registration's artifact names and its ticker, or `None` when the artifact
/// cannot be read as a snapshot that names one.
fn snapshot(payload: &Value, artifacts: &dyn ArtifactSource) -> Option<(String, Option<Symbol>)> {
    let reference = payload
        .get("content_hash")
        .and_then(Value::as_str)
        .and_then(ArtifactRef::parse)?;
    let bytes = get_artifact(artifacts, &reference).ok()?;
    let object = parse(&bytes).ok()?;
    let asset = object.get("instrument_id").and_then(Value::as_str)?;
    let symbol = object
        .get("symbol")
        .and_then(Value::as_str)
        .and_then(|text| Symbol::parse(text).ok());
    Some((asset.to_owned(), symbol))
}
