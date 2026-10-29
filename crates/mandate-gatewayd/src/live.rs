//! The service's live start (DEC-2216 item 4, DEC-2326 items 1 and 3, DEC-2282): the founder's
//! vetted entries, read from the live registry file the chart renders, over the HTTPS transport and
//! the key read once from the vault. Every entry the registry takes is registered `evaluating`,
//! with no endpoint, and C is refused while its snapshot is undated, so a live start serves no
//! research call: each is answered `policy_denied`, with nothing reserved and nothing sent. An
//! entry is offered only on a passing gate report the start re-reads and an identity probe, which
//! the gate's build adds; until then a file naming a report starts nothing.

use std::collections::BTreeSet;
use std::path::Path;

use mandate_canon::{Value, parse};
use mandate_chat::ChatEntry;
use mandate_gateway::guard::GuardContext;
use mandate_gateway::policy::EndpointPolicy;
use mandate_gateway::provider::{Clock, SecretSource, Transport};
use mandate_gateway::registry::{
    Endpoint, Identity, Locality, Registry, RegistryError, Reported, Snapshot, Status,
};
use mandate_gateway::{RateCaps, Settings};
use mandate_research_run::live::{LiveEntry, Refused, live_registry, read_live};
use mandate_research_run::recorded::hosted_endpoint;
use mandate_store::Store;
use mandate_store::meter::JournalMeter;

use crate::config::Config;
use crate::recorded::{RATE_MARGIN, WORKSPACE_RATE};
use crate::run::{Parts, Served, StartFailure, metered_service};
use crate::service::Service;

/// The service the binary serves live.
pub type LiveService<S, C, T> = Service<T, JournalMeter<S>, C>;

/// A live start: the service, and the entries the registry refused, each with why.
pub type LiveStart<S, C, T> = (LiveService<S, C, T>, Vec<Refused>);

/// The live registry file at `path`, read strictly, once, and refused whole when an entry names a
/// gate report, which this start cannot yet verify, before the vault is touched.
///
/// # Errors
/// [`StartFailure::RegistryUnreadable`] when the file cannot be read,
/// [`StartFailure::RegistryMalformed`] when it is not strict JSON or not a live registry, and
/// [`StartFailure::GateReportUnverifiable`] when an entry names a gate report.
pub fn read_registry(path: &Path) -> Result<Vec<LiveEntry>, StartFailure> {
    let bytes = std::fs::read(path).map_err(|_| StartFailure::RegistryUnreadable)?;
    let value = parse(&bytes).map_err(|_| StartFailure::RegistryMalformed)?;
    let entries = read_live(&value).map_err(|_| StartFailure::RegistryMalformed)?;
    unverified(&entries)?;
    Ok(entries)
}

/// Refuses `entries` when one names a gate report (DEC-2216 item 4): offering on one needs its
/// every binding re-read and an identity probe, which the gate's build adds.
fn unverified(entries: &[LiveEntry]) -> Result<(), StartFailure> {
    if entries.iter().any(|live| live.gate_report.is_some()) {
        Err(StartFailure::GateReportUnverifiable)
    } else {
        Ok(())
    }
}

/// The upstreams the endpoints of the entries the registry takes route through, so an entry it
/// refuses, such as C while undated, neither has the key served for its upstream nor that upstream
/// allowed (DEC-2020).
pub fn upstreams(entries: &[LiveEntry]) -> BTreeSet<String> {
    entries
        .iter()
        .filter(|live| Registry::new().register(live.entry.content()).is_ok())
        .map(|live| live.endpoint.route.upstream.clone())
        .collect()
}

/// The gateway's settings for `workspace` over `entries`: hosted, through their upstreams alone,
/// under the cell's rate caps, an agent's at the most turns any entry's run may take (DEC-2316).
pub fn live_settings(workspace: &str, entries: &[LiveEntry]) -> Settings {
    let turns = entries
        .iter()
        .filter_map(|live| live.entry.agentic.as_ref())
        .map(|bounds| bounds.max_turns)
        .max()
        .unwrap_or(1);
    Settings {
        workspace_id: workspace.to_owned(),
        policy: EndpointPolicy::new(
            BTreeSet::from([Locality::Hosted]),
            Some(upstreams(entries)),
            None,
        ),
        guard: GuardContext::default(),
        rate: RateCaps {
            per_workspace: WORKSPACE_RATE,
            per_agent: turns.saturating_add(RATE_MARGIN),
        },
    }
}

/// The live service for `config`'s workspace over `entries`, `transport` and `key`, metered as the
/// recorded service is ([`metered_service`]), with every entry the registry takes registered and
/// none offered, and the entries it refuses.
///
/// # Errors
/// [`StartFailure::GateReportUnverifiable`] when an entry names a gate report, before its meters
/// are opened; then [`metered_service`]'s.
pub fn live_service<S: Store + 'static, C: Clock, T: Transport>(
    config: &Config,
    entries: &[LiveEntry],
    parts: Parts<S, C>,
    transport: T,
    key: Box<dyn SecretSource>,
) -> Result<LiveStart<S, C, T>, StartFailure> {
    live_service_with_chat(config, entries, &[], parts, transport, key)
}

/// [`live_service`] with the file's chat entries offered beside the research entries (DEC-2293
/// item 13): each is registered, offered, and given its one endpoint, whose identity every response
/// is still checked against (INF-2), and its upstream is allowed.
///
/// # Errors
/// As [`live_service`].
pub fn live_service_with_chat<S: Store + 'static, C: Clock, T: Transport>(
    config: &Config,
    entries: &[LiveEntry],
    chats: &[ChatLive],
    parts: Parts<S, C>,
    transport: T,
    key: Box<dyn SecretSource>,
) -> Result<LiveStart<S, C, T>, StartFailure> {
    unverified(entries)?;
    let held: Vec<LiveEntry> = entries
        .iter()
        .cloned()
        .map(|live| LiveEntry {
            offered: false,
            ..live
        })
        .collect();
    let (mut registry, mut refused) = live_registry(&held, &mut |_, _| Vec::new());
    refused.extend(register_chat(&mut registry, chats));
    let mut settings = live_settings(&config.workspace, entries);
    let allowed: BTreeSet<String> = upstreams(entries)
        .into_iter()
        .chain(chat_upstreams(chats))
        .collect();
    settings.policy = EndpointPolicy::new(BTreeSet::from([Locality::Hosted]), Some(allowed), None);
    let service = metered_service(
        config,
        parts,
        Served {
            settings,
            registry,
            transport,
            key,
        },
    )?;
    Ok((service, refused))
}

/// One chat entry of the live registry file (DEC-2293 item 13), offered on its one endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatLive {
    pub entry: ChatEntry,
    pub endpoint: Endpoint,
}

/// The file's optional `chat` entries, read strictly: each holds an `entry` with `model_id`,
/// `model_version`, `provider`, `model_name`, a dated `snapshot`, `deadline_ms` and
/// `max_output_tokens`, and an `endpoint` as a research entry's. A file with no `chat` member has
/// none.
///
/// # Errors
/// [`StartFailure::RegistryUnreadable`] when the file cannot be read, and
/// [`StartFailure::RegistryMalformed`] when it is not strict JSON or an entry lacks a member.
pub fn read_chat(path: &Path) -> Result<Vec<ChatLive>, StartFailure> {
    let bytes = std::fs::read(path).map_err(|_| StartFailure::RegistryUnreadable)?;
    let value = parse(&bytes).map_err(|_| StartFailure::RegistryMalformed)?;
    chat_entries(&value).ok_or(StartFailure::RegistryMalformed)
}

/// The `chat` entries of a parsed registry file; `None` when one does not read.
pub fn chat_entries(file: &Value) -> Option<Vec<ChatLive>> {
    let Some(listed) = file.get("chat") else {
        return Some(Vec::new());
    };
    listed
        .as_array()?
        .iter()
        .map(|item| {
            let entry = item.get("entry")?;
            let text = |name: &str| {
                entry
                    .get(name)
                    .and_then(Value::as_str)
                    .filter(|t| !t.is_empty())
                    .map(str::to_owned)
            };
            let number = |name: &str| entry.get(name).and_then(Value::as_int);
            let model_name = text("model_name")?;
            let (endpoint, _) = hosted_endpoint(item.get("endpoint")?, &model_name).ok()?;
            Some(ChatLive {
                entry: ChatEntry {
                    model_id: text("model_id")?,
                    model_version: text("model_version")?,
                    identity: Identity {
                        provider: text("provider")?,
                        model_name,
                        snapshot: Snapshot::Dated(text("snapshot")?),
                        quantization: None,
                    },
                    deadline_ms: number("deadline_ms")?,
                    max_output_tokens: u32::try_from(number("max_output_tokens")?).ok()?,
                },
                endpoint,
            })
        })
        .collect()
}

/// The upstreams the chat entries' endpoints route through.
pub fn chat_upstreams(chats: &[ChatLive]) -> BTreeSet<String> {
    chats
        .iter()
        .map(|chat| chat.endpoint.route.upstream.clone())
        .collect()
}

/// Registers each chat entry, offers it, and adds its endpoint as answering for the pinned identity
/// (DEC-2293 item 13: no start probe; each response's reported identity is still checked against
/// the pin). Each entry the registry refuses is listed with why.
pub fn register_chat(registry: &mut Registry, chats: &[ChatLive]) -> Vec<Refused> {
    chats
        .iter()
        .filter_map(|chat| {
            let content = chat.entry.content();
            let pin = content.model_ref();
            let reported = Reported {
                model: Some(chat.entry.identity.model_name.clone()),
                provider: Some(chat.endpoint.route.reported_provider.clone()),
            };
            let taken: Result<(), RegistryError> = registry
                .register(content)
                .and_then(|_| registry.set_status(&pin, Status::Offered))
                .and_then(|()| registry.add_endpoint(&pin, chat.endpoint.clone(), &[reported]));
            taken.err().map(|error| Refused {
                model_id: pin.model_id,
                error,
            })
        })
        .collect()
}
