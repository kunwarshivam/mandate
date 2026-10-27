//! The live registry file (DEC-2282 item 6, DEC-2326 item 1): the research role's vetted entries,
//! each with its one endpoint, its prices, and the status the platform means to offer it at. The
//! gateway's registry decides what is offered: an entry it refuses (an undated snapshot from a
//! provider that dates none, INF-1), one whose endpoint fails the identity probe (inference spec
//! §4.3), and one whose harness content differs from the role's (DEC-2282 item 2) are each listed as
//! refused and hold no endpoint, so no call reaches them, while the rest are offered.

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
    /// The reference (`sha256:` and 64 lowercase hex digits) of the gate report the file names as
    /// this entry's pass (DEC-2216 item 4), or `None`. The service re-reads the report and its
    /// every binding before it offers anything on its say-so.
    pub gate_report: Option<String>,
}

/// An entry the registry did not take as the file asks, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub model_id: String,
    pub error: RegistryError,
}

/// The entries of `file`, a parsed live registry file, read strictly: `entries` is a non-empty
/// array, and each holds an `entry` as a recording's, an `endpoint` as a recording's, a `status` of
/// `offered` or `evaluating`, and optionally a `gate_report` reference.
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
            let gate_report = item
                .get("gate_report")
                .map(|reference| {
                    reference
                        .as_str()
                        .filter(|text| is_reference(text))
                        .map(str::to_owned)
                        .ok_or(RecordedError::Malformed("gate_report"))
                })
                .transpose()?;
            Ok(LiveEntry {
                entry,
                endpoint,
                offered,
                gate_report,
            })
        })
        .collect()
}

/// The registry of `entries`. Each is registered. One the file offers is then offered, which the
/// registry refuses for another role's harness content, and only then probed through `probe`, and
/// given its endpoint when the probe answers for it; a failed probe puts it back to `evaluating`.
/// So an entry holds an endpoint, and a call can reach it, only when it is offered and its probe
/// passed. One the file holds `evaluating` is registered with no endpoint. Every refusal is listed
/// with its error, once; an entry the registry refuses to register is never probed, so a live probe
/// spends nothing on it.
pub fn live_registry(
    entries: &[LiveEntry],
    probe: &mut dyn FnMut(&Endpoint, &PinnedContent) -> Vec<Reported>,
) -> (Registry, Vec<Refused>) {
    let mut registry = Registry::new();
    let mut refused = Vec::new();
    for live in entries {
        let content = live.entry.content();
        let pin = content.model_ref();
        let taken = registry.register(content.clone()).and_then(|_| {
            if !live.offered {
                return Ok(());
            }
            registry.set_status(&pin, Status::Offered)?;
            let reported = probe(&live.endpoint, &content);
            registry
                .add_endpoint(&pin, live.endpoint.clone(), &reported)
                .or_else(|error| {
                    registry.set_status(&pin, Status::Evaluating)?;
                    Err(error)
                })
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

/// Whether `text` is an artifact reference: `sha256:` and 64 lowercase hexadecimal digits.
fn is_reference(text: &str) -> bool {
    text.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
    })
}
