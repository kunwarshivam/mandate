//! The live registry file (DEC-2282 item 6, DEC-2326 item 1): the research role's vetted entries,
//! each with its one endpoint, its prices, and the status the platform means to offer it at. The
//! gateway's registry decides what is offered: an entry it refuses (an undated snapshot from a
//! provider that dates none, INF-1), one whose endpoint fails the identity probe (inference spec
//! §4.3), and one whose harness content differs from the role's (DEC-2282 item 2) are each listed as
//! refused, and stay out of every pin's reach, while the rest are offered.

use mandate_canon::Value;
use mandate_gateway::registry::{
    Endpoint, PinnedContent, Registry, RegistryError, Reported, Status,
};

use crate::entry::{EntryError, ResearchEntry};
use crate::recorded::{Field, RecordedError, hosted};

/// One entry of the live registry file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveEntry {
    pub entry: ResearchEntry,
    pub endpoint: Endpoint,
    /// Whether the file asks for the entry to be offered; `false` holds it `evaluating`.
    pub offered: bool,
}

/// An entry the registry did not take as the file asks, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub model_id: String,
    pub error: RegistryError,
}

/// The entries of `file`, a parsed live registry file, read strictly: `entries` is a non-empty
/// array, and each holds an `entry` as a recording's, an `endpoint` as a recording's, and a
/// `status` of `offered` or `evaluating`.
///
/// # Errors
/// [`RecordedError::Malformed`] naming the first member that is missing, empty, or out of range.
pub fn read_live(file: &Value) -> Result<Vec<LiveEntry>, RecordedError> {
    let entries = file
        .get("entries")
        .and_then(Value::as_array)
        .filter(|entries| !entries.is_empty())
        .ok_or(RecordedError::Malformed("entries"))?;
    entries
        .iter()
        .map(|item| {
            let member = |name: &'static str| item.get(name).ok_or(RecordedError::Malformed(name));
            let offered = match Field(item).text("status")? {
                "offered" => true,
                "evaluating" => false,
                _ => return Err(RecordedError::Malformed("status")),
            };
            let entry = ResearchEntry::recorded(member("entry")?).map_err(|error| match error {
                EntryError::Member(member) => RecordedError::Malformed(member),
            })?;
            let (endpoint, _) = hosted(&Field(member("endpoint")?), &entry.identity.model_name)?;
            Ok(LiveEntry {
                entry,
                endpoint,
                offered,
            })
        })
        .collect()
}

/// The registry of `entries`: each registered, its endpoint added after `probe` answers for it (an
/// entry the registry refuses is never probed, so a live probe spends nothing on it), and
/// offered when the file asks. An entry the registry refuses at any step is listed with the error,
/// once, and taken no further, so it is unknown, or held `evaluating` with no endpoint; nothing
/// refused is ever offered.
pub fn live_registry(
    entries: &[LiveEntry],
    probe: &mut dyn FnMut(&Endpoint, &PinnedContent) -> Vec<Reported>,
) -> (Registry, Vec<Refused>) {
    let mut registry = Registry::new();
    let mut refused = Vec::new();
    for live in entries {
        let content = live.entry.content();
        let pin = content.model_ref();
        let taken = registry
            .register(content.clone())
            .and_then(|_| {
                let reported = probe(&live.endpoint, &content);
                registry.add_endpoint(&pin, live.endpoint.clone(), &reported)
            })
            .and_then(|()| {
                if live.offered {
                    registry.set_status(&pin, Status::Offered)
                } else {
                    Ok(())
                }
            });
        if let Err(error) = taken {
            refused.push(Refused {
                model_id: pin.model_id,
                error,
            });
        }
    }
    (registry, refused)
}
