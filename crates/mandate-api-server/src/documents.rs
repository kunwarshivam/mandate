//! The mandate document a deployed version names, read from the content-addressed store
//! (DEC-1020 items 1 and 2): checked against its digest, parsed as a mandate, and summarized as the
//! envelope the agent read serves. The store is read, never written.

use mandate_canon::{Key, Object, Value, parse};
use mandate_journal::{ArtifactError, ArtifactRef, ArtifactSource, get_artifact};
use mandate_spec::document::Mandate;

/// What the agent read takes from a mandate document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    /// The document's `name`.
    pub label: String,
    /// `read-models/agent`'s `MandateSummary`, its values the document's own.
    pub mandate: Value,
    /// The connection the document names, for the account's positions.
    pub connection_id: String,
    /// Whether the document pins its universe (`universe.pinned`): an unpinned mandate's agent is
    /// a research agent, whose openings wait on a multi-instrument risk state once the account
    /// has traded (DEC-2143, DEC-2819 item 4).
    pub pinned: bool,
}

/// The envelope of the document `version` names in `store`: `None` when the store does not hold
/// it, it does not hash to `version`, or it does not parse as a mandate (an encrypted document
/// included). A digest mismatch is logged by name, with no content.
pub fn envelope(store: &dyn ArtifactSource, version: &str) -> Option<Envelope> {
    let reference = ArtifactRef::parse(version)?;
    let bytes = match get_artifact(store, &reference) {
        Ok(bytes) => bytes,
        Err(ArtifactError::Corrupt) => {
            eprintln!("mandate document: digest check failed");
            return None;
        }
        Err(_) => return None,
    };
    let document = parse(&bytes).ok()?;
    Mandate::parse(&document).ok()?;
    let member = |path: &[&str]| path.iter().try_fold(&document, |v, k| v.get(k)).cloned();
    let text = |path: &[&str]| member(path).and_then(|v| v.as_str().map(str::to_owned));
    let mut summary = Object::new();
    for (name, path) in SUMMARY {
        summary.insert(Key::new(name).ok()?, member(path)?);
    }
    Some(Envelope {
        label: text(&["name"])?,
        mandate: Value::Object(summary),
        connection_id: text(&["connection_id"])?,
        pinned: match member(&["universe", "pinned"])? {
            Value::Bool(pinned) => pinned,
            _ => return None,
        },
    })
}

/// The summary's members, each at its path in the document (DEC-1020 item 2).
const SUMMARY: [(&str, &[&str]); 10] = [
    ("allocation_usd", &["capital", "allocation_usd"]),
    (
        "max_loss_from_allocation",
        &["capital", "max_loss_from_allocation"],
    ),
    ("goal", &["goal", "type"]),
    ("goal_end_date", &["goal", "end_date"]),
    ("asset_classes", &["universe", "asset_classes"]),
    ("max_position_usd", &["risk", "max_position_usd"]),
    ("max_order_usd", &["risk", "max_order_usd"]),
    ("max_orders_per_day", &["risk", "max_orders_per_day"]),
    ("max_daily_loss", &["risk", "max_daily_loss"]),
    ("max_drawdown", &["risk", "max_drawdown"]),
];

/// The bytes `store` holds under `reference`, checked against it; `None` when the store does not
/// hold them or they do not hash to it. A digest mismatch is logged by `name`, with no content.
pub fn checked(store: &dyn ArtifactSource, reference: &ArtifactRef, name: &str) -> Option<Vec<u8>> {
    match get_artifact(store, reference) {
        Ok(bytes) => Some(bytes),
        Err(ArtifactError::Corrupt) => {
            eprintln!("{name}: digest check failed");
            None
        }
        Err(_) => None,
    }
}
