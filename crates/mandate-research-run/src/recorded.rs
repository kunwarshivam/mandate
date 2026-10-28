//! The model half of a recorded research bundle's `research.json` (DEC-2210 items 1 and 2,
//! DEC-2266, DEC-2326 item 3): the pinned entry, its one endpoint, what the endpoint reports, the
//! model's recorded reply and an agentic run's scripted turns, read from the parsed file with every
//! member required and nothing defaulted. The cell's bundle loader and the model-gateway service's
//! `--recorded` mode read the same file through this, so the two cannot drift apart.

use std::collections::BTreeMap;

use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_gateway::money::{PriceTable, TokenPrice, Usage, Usd6};
use mandate_gateway::registry::{Endpoint, Locality, Registry, Reported, Route, Status};
use mandate_gateway::schema::fits;

use crate::entry::{EntryError, ResearchEntry};

/// Why the model half did not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordedError {
    /// The member named is missing, empty, or out of its range.
    Malformed(&'static str),
    /// The gateway's registry refused the entry or its endpoint.
    Unregistered,
}

/// The model's one recorded reply: the output text the fake replays, its latency, and its usage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedReply {
    pub output: String,
    pub latency_ms: u64,
    pub usage: Usage,
}

/// One scripted tool-call reply of an agentic run's turn (DEC-2266): each call's declared tool
/// name and canonical arguments, in reply order, its latency, and its usage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptedTurn {
    pub calls: Vec<(String, String)>,
    pub latency_ms: u64,
    pub usage: Usage,
}

/// The model a recording pins, and what it replays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedModel {
    pub entry: ResearchEntry,
    pub endpoint: Endpoint,
    /// What the endpoint reports it serves, which the registry probes it with.
    pub probe: Reported,
    pub reply: RecordedReply,
    /// The scripted replies of an agentic run's turns before its final one, turn 1 first; empty
    /// for a recording with no `turns` (DEC-2266).
    pub turns: Vec<ScriptedTurn>,
    /// A `monitor` run's final reply, its output the monitor schema's `invalidations` alone (agent
    /// harness spec §6.9), which a monitor run takes at its first turn, with no scripted turn;
    /// `None` for a recording with no `monitor_reply`, whose monitor runs take [`Self::reply`].
    /// Its output names the thesis it invalidates as [`SHOWN_THESIS`], which [`shown_thesis`]
    /// replaces with a thesis the call showed the model.
    pub monitor_reply: Option<RecordedReply>,
}

impl RecordedModel {
    /// The model half of `research`, a parsed `research.json`.
    ///
    /// # Errors
    /// [`RecordedError::Malformed`] naming the first member that is missing, empty, or out of
    /// range.
    pub fn read(research: &Value) -> Result<Self, RecordedError> {
        let member = |name: &'static str| {
            research
                .get(name)
                .map(Field)
                .ok_or(RecordedError::Malformed(name))
        };
        let entry = member("entry")?;
        let endpoint = member("endpoint")?;
        let reply = member("reply")?;
        let pinned = ResearchEntry::recorded(entry.0).map_err(|error| match error {
            EntryError::Member(member) => RecordedError::Malformed(member),
        })?;
        let turns = research
            .get("turns")
            .map(|turns| scripted(turns, &pinned))
            .transpose()?
            .unwrap_or_default();
        let monitor_reply = research
            .get("monitor_reply")
            .map(|monitor| {
                if pinned.content().agentic().is_none() {
                    return Err(RecordedError::Malformed("monitor_reply"));
                }
                replayed(&Field(monitor))
            })
            .transpose()?;
        let (endpoint, probe) = hosted(&endpoint, &pinned.identity.model_name)?;
        Ok(Self {
            entry: pinned,
            endpoint,
            probe,
            reply: replayed(&reply)?,
            turns,
            monitor_reply,
        })
    }

    /// The gateway's registry: the entry, offered, on its one endpoint.
    ///
    /// # Errors
    /// [`RecordedError::Unregistered`] when the registry refuses the entry or the endpoint.
    pub fn registry(&self) -> Result<Registry, RecordedError> {
        registry(&self.entry, &self.endpoint, &self.probe)
    }
}

/// The gateway's registry for `entry` on `endpoint`, probed with `probe`: the entry, offered.
///
/// # Errors
/// [`RecordedError::Unregistered`] when the registry refuses the entry or the endpoint.
pub fn registry(
    entry: &ResearchEntry,
    endpoint: &Endpoint,
    probe: &Reported,
) -> Result<Registry, RecordedError> {
    let content = entry.content();
    let pin = content.model_ref();
    let mut registry = Registry::new();
    registry
        .register(content)
        .map_err(|_| RecordedError::Unregistered)?;
    registry
        .add_endpoint(&pin, endpoint.clone(), std::slice::from_ref(probe))
        .map_err(|_| RecordedError::Unregistered)?;
    registry
        .set_status(&pin, Status::Offered)
        .map_err(|_| RecordedError::Unregistered)?;
    Ok(registry)
}

/// The hosted endpoint `endpoint` describes, and what it reports serving `slug`: the endpoint member
/// of a recording, and of each entry of the live registry file.
pub(crate) fn hosted(
    endpoint: &Field<'_>,
    slug: &str,
) -> Result<(Endpoint, Reported), RecordedError> {
    let reported_provider = endpoint.text("reported_provider")?;
    let prices = PriceTable::new(
        endpoint.per_million("input_per_million")?,
        endpoint.per_million("output_per_million")?,
        endpoint.per_million("cached_per_million")?,
        Digest::of(endpoint.text("price_table")?.as_bytes()),
    )
    .map_err(|_| RecordedError::Malformed("cached_per_million"))?;
    Ok((
        Endpoint {
            id: endpoint.text("id")?.to_owned(),
            host: endpoint.text("host")?.to_owned(),
            region: endpoint.text("region")?.to_owned(),
            locality: Locality::Hosted,
            route: Route {
                upstream: endpoint.text("upstream")?.to_owned(),
                reported_provider: reported_provider.to_owned(),
            },
            prices,
        },
        Reported {
            model: Some(slug.to_owned()),
            provider: Some(reported_provider.to_owned()),
        },
    ))
}

/// One member's value, read strictly.
pub(crate) struct Field<'a>(pub(crate) &'a Value);

impl Field<'_> {
    pub(crate) fn text(&self, member: &'static str) -> Result<&str, RecordedError> {
        self.0
            .get(member)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .ok_or(RecordedError::Malformed(member))
    }

    fn int(&self, member: &'static str) -> Result<u64, RecordedError> {
        self.0
            .get(member)
            .and_then(Value::as_int)
            .ok_or(RecordedError::Malformed(member))
    }

    fn per_million(&self, member: &'static str) -> Result<TokenPrice, RecordedError> {
        Usd6::parse(self.text(member)?)
            .map(TokenPrice::per_million)
            .map_err(|_| RecordedError::Malformed(member))
    }
}

/// What a recorded monitor reply writes for the thesis it invalidates: a runtime's thesis ids are
/// made at admission, so a recording cannot name one, and the double names the thesis the call
/// showed it instead, as a model answers about what it was shown.
pub const SHOWN_THESIS: &str = "$THESIS_ID";

/// `output` with [`SHOWN_THESIS`] replaced by the least of `shown`, the thesis ids the call's
/// `memory` input showed the model; unchanged when it showed none, so an invalidation of no thesis
/// is judged `unknown_thesis` and ends nothing. The id goes into the text unescaped: a journaled
/// thesis id is an identifier (`[A-Za-z0-9_-]`), and one that broke the JSON would only fail the
/// gateway's schema, using nothing.
pub fn shown_thesis(output: &str, shown: &[&str]) -> String {
    match shown.iter().min() {
        Some(thesis) => output.replace(SHOWN_THESIS, thesis),
        None => output.to_owned(),
    }
}

/// The thesis ids a call's `memory` input shows, as [`crate::call`] writes it: a canonical array of
/// objects each naming its `thesis_id`. None when the input is absent or not that array.
pub fn memory_theses(inputs: &BTreeMap<String, String>) -> Vec<String> {
    inputs
        .get("memory")
        .and_then(|text| parse(text.as_bytes()).ok())
        .and_then(|value| {
            value.as_array().map(|theses| {
                theses
                    .iter()
                    .filter_map(|thesis| thesis.get("thesis_id")?.as_str().map(str::to_owned))
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// A recorded reply's members: its output text, latency, and usage.
fn replayed(reply: &Field<'_>) -> Result<RecordedReply, RecordedError> {
    Ok(RecordedReply {
        output: reply.text("output")?.to_owned(),
        latency_ms: reply.int("latency_ms")?,
        usage: Usage {
            input: reply.int("input_tokens")?,
            output: reply.int("output_tokens")?,
            cached: reply.int("cached_tokens")?,
        },
    })
}

/// `research.json`'s `turns` (DEC-2266): refused whole, `Malformed("turns")`, unless the entry
/// names the agentic loop and every turn is one its gateway would accept, so no recording scripts a
/// call the entry does not declare.
fn scripted(turns: &Value, entry: &ResearchEntry) -> Result<Vec<ScriptedTurn>, RecordedError> {
    let refused = RecordedError::Malformed("turns");
    let content = entry.content();
    let (Some(agentic), Some(listed)) = (content.agentic(), turns.as_array()) else {
        return Err(refused);
    };
    let bounds = agentic.loop_bounds;
    if u32::try_from(listed.len()).map_or(true, |n| n >= bounds.max_turns) {
        return Err(refused);
    }
    listed
        .iter()
        .map(|turn| {
            let count = |member: &str| turn.get(member).and_then(Value::as_int);
            let calls = turn.get("calls").and_then(Value::as_array)?;
            let fits_turn = u32::try_from(calls.len())
                .is_ok_and(|n| n > 0 && n <= bounds.max_tool_calls_per_turn);
            if !fits_turn {
                return None;
            }
            let calls = calls
                .iter()
                .map(|call| {
                    let name = call.get("name").and_then(Value::as_str)?;
                    let arguments = call.get("arguments")?;
                    let declared = agentic.tool(name)?;
                    fits(&declared.arguments, arguments).then(|| {
                        (
                            name.to_owned(),
                            String::from_utf8_lossy(&to_canonical(arguments)).into_owned(),
                        )
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(ScriptedTurn {
                calls,
                latency_ms: count("latency_ms")?,
                usage: Usage {
                    input: count("input_tokens")?,
                    output: count("output_tokens")?,
                    cached: count("cached_tokens")?,
                },
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(refused)
}
