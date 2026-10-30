//! The service's live start (DEC-2216 item 4, DEC-2326 items 1 and 3, DEC-2282, DEC-2294): the
//! founder's vetted entries, read from the live registry file the chart renders, over the HTTPS
//! transport and the key read once from the vault. Every research entry the registry takes is
//! registered `evaluating`, with no endpoint, and C is refused while its snapshot is undated, so a
//! live start serves no research call: each is answered `policy_denied`, with nothing reserved and
//! nothing sent. A file naming a research gate report starts nothing, since the research gate's
//! runner is not built.
//!
//! The file's chat entries are offered as DEC-2294 says. One that names no gate report is offered,
//! as before, only if it is one of the pins the ruling keeps on ([`UNGATED`]); any other is
//! registered and withheld. One that names a report is offered only when the report reads back,
//! binds the entry's content hash, the built-in probe set and the endpoint and price table the file
//! names, and says `passed`, and the start's identity probe passes ([`crate::gate::verified`],
//! [`crate::gate::identity_probe`]); otherwise it is registered, withheld, and listed with why.

use std::collections::BTreeSet;
use std::path::Path;

use mandate_canon::{Value, parse};
use mandate_chat::ChatEntry;
use mandate_gateway::guard::GuardContext;
use mandate_gateway::policy::EndpointPolicy;
use mandate_gateway::provider::{Clock, SecretSource, Transport};
use mandate_gateway::registry::{
    Endpoint, Locality, ModelRef, Registry, RegistryError, Reported, Status,
};
use mandate_gateway::{RateCaps, Settings};
use mandate_journal::ArtifactRef;
use mandate_research_run::live::{LiveEntry, Refused, live_registry, read_live};
use mandate_research_run::recorded::hosted_endpoint;
use mandate_store::Store;
use mandate_store::meter::JournalMeter;

use mandate_gateway::CallRequest;
use mandate_gateway::wire::{PutRefusal, PutReply, PutRequest, Reply};

use crate::config::Config;
use crate::gate::{Reports, Unverified, built_in, digest_ref, identity_probe, verified};
use crate::gate_socket::Desk;
use crate::recorded::{RATE_MARGIN, WORKSPACE_RATE};
use crate::run::{Parts, Served, StartFailure, metered_service, started_ids};
use crate::serve::Handler;
use crate::service::Service;
use crate::writer::ArtifactWriter;

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
    live_service_with_chat(config, entries, &[], parts, transport, key, None)
        .map(|start| (start.service, start.refused))
}

/// The pins DEC-2294 keeps offered with no gate report, by model id, version and content hash,
/// until the file names a report for them: the chat entries offered when the founder ruled,
/// exactly as they were. Any other chat entry, a new model, a new version, or the same id and
/// version with any pinned content changed, is offered only on a passing report.
pub const UNGATED: [(&str, &str, &str); 2] = [
    (
        "llm.chat_sonnet",
        "1.0.0",
        "sha256:fd69fae48fdb3072ff9f61ebc7b75faa25a392e9ccb66230523388ae59439f6c",
    ),
    (
        "llm.chat_haiku",
        "1.0.0",
        "sha256:4529010899681ebe5ffa58d9d36769dccce47d4dc1464347030a5494b4016e42",
    ),
];

/// Whether `pin` is one of [`UNGATED`]: its id, version and content hash all as there.
pub fn ungated(pin: &ModelRef) -> bool {
    let hash = digest_ref(pin.content_hash);
    UNGATED.iter().any(|(id, version, content)| {
        pin.model_id == *id && pin.model_version == *version && hash == *content
    })
}

/// The lane of the ids the start's identity probes draw from, beside the call ids' (0) and the
/// deployment meter's copies' (1).
pub const PROBE_LANE: u8 = 2;

/// A chat entry the start registered and does not offer, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Withheld {
    pub model_id: String,
    pub reason: Unverified,
}

/// A live start with its chat entries: the service, the entries the registry refused, and the chat
/// entries it registered but does not offer.
pub struct ChatStart<S: Store, C, T> {
    pub service: LiveService<S, C, T>,
    pub refused: Vec<Refused>,
    pub withheld: Vec<Withheld>,
}

/// [`live_service`] with the file's chat entries beside the research entries (DEC-2293 item 13 as
/// DEC-2294 amends it). Every chat entry the registry takes is registered and its upstream allowed.
/// It is offered, with its one endpoint, whose identity every response is still checked against
/// (INF-2), only when:
/// - it names no gate report and is one of [`UNGATED`]; or
/// - it names a report that `reports` reads back and [`verified`] passes, and the start's identity
///   probe through the built service passes.
///
/// Every other chat entry is withheld: registered, never offered, and listed with why.
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
    reports: Option<&dyn Reports>,
) -> Result<ChatStart<S, C, T>, StartFailure> {
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
    let mut withheld = Vec::new();
    let mut gated = Vec::new();
    for chat in chats {
        let pin = chat.entry.content().model_ref();
        let ungated = ungated(&pin);
        let taken = if chat.gate_report.is_none() && ungated {
            offer_chat(&mut registry, chat)
        } else {
            registry.register(chat.entry.content()).map(|_| ())
        };
        match taken {
            Err(error) => refused.push(Refused {
                model_id: pin.model_id,
                error,
            }),
            Ok(()) if chat.gate_report.is_some() => gated.push(chat),
            Ok(()) if !ungated => withheld.push(Withheld {
                model_id: pin.model_id,
                reason: Unverified::Missing,
            }),
            Ok(()) => {}
        }
    }
    let mut settings = live_settings(&config.workspace, entries);
    let allowed: BTreeSet<String> = upstreams(entries)
        .into_iter()
        .chain(chat_upstreams(chats))
        .collect();
    settings.policy = EndpointPolicy::new(BTreeSet::from([Locality::Hosted]), Some(allowed), None);
    let mut service = metered_service(
        config,
        parts,
        Served {
            settings,
            registry,
            transport,
            key,
        },
    )?;
    withheld.extend(offer_gated(
        &mut service,
        &config.workspace,
        &gated,
        reports,
    ));
    Ok(ChatStart {
        service,
        refused,
        withheld,
    })
}

/// Offers each of `gated`, already registered, whose report verifies and whose identity probe
/// passes, and answers the rest with why. A probe runs only for a report that verified, so a start
/// spends on no entry it could not offer.
fn offer_gated<S: Store + 'static, C: Clock, T: Transport>(
    service: &mut LiveService<S, C, T>,
    workspace: &str,
    gated: &[&ChatLive],
    reports: Option<&dyn Reports>,
) -> Vec<Withheld> {
    let built = built_in();
    let now = service.gateway_mut().clock_mut().now();
    let mut ids = started_ids(now, PROBE_LANE);
    gated
        .iter()
        .filter_map(|chat| {
            let verdict = built
                .as_ref()
                .ok_or(Unverified::ProbeSet)
                .and_then(|(set, digest)| {
                    let reports = reports.ok_or(Unverified::Unreadable)?;
                    let reference = chat.gate_report.as_ref().ok_or(Unverified::Missing)?;
                    verified(reports, reference, chat, *digest)?;
                    if identity_probe(service.gateway_mut(), workspace, chat, set, &mut ids) {
                        Ok(())
                    } else {
                        Err(Unverified::Identity)
                    }
                })
                .and_then(|()| {
                    let gateway = service.gateway_mut();
                    let mut registry = gateway.replace_registry(Registry::new());
                    let promoted = promote(&mut registry, chat);
                    let _empty = gateway.replace_registry(registry);
                    promoted.map_err(|_| Unverified::Identity)
                });
            verdict.err().map(|reason| Withheld {
                model_id: chat.entry.model_id.clone(),
                reason,
            })
        })
        .collect()
}

/// One chat entry of the live registry file (DEC-2293 item 13), on its one endpoint, and the gate
/// report the file names for it, if any (DEC-2294).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatLive {
    pub entry: ChatEntry,
    pub endpoint: Endpoint,
    pub gate_report: Option<ArtifactRef>,
}

/// The file's optional `chat` entries, read strictly: each holds an `entry` with `model_id`,
/// `model_version`, `provider`, `model_name`, a dated `snapshot`, `deadline_ms` and
/// `max_output_tokens`, an `endpoint` as a research entry's, and optionally `gate_report`, a
/// `sha256:` reference or `null`. A file with no `chat` member has none.
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
            let entry = ChatEntry::from_registry(item.get("entry")?)?;
            let (endpoint, _) =
                hosted_endpoint(item.get("endpoint")?, &entry.identity.model_name).ok()?;
            let gate_report = match item.get("gate_report") {
                None | Some(Value::Null) => None,
                Some(named) => Some(ArtifactRef::parse(named.as_str()?)?),
            };
            Some(ChatLive {
                entry,
                endpoint,
                gate_report,
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
/// (DEC-2293 item 13: each response's reported identity is still checked against the pin), whatever
/// report it names. Each entry the registry refuses is listed with why.
pub fn register_chat(registry: &mut Registry, chats: &[ChatLive]) -> Vec<Refused> {
    chats
        .iter()
        .filter_map(|chat| {
            offer_chat(registry, chat).err().map(|error| Refused {
                model_id: chat.entry.model_id.clone(),
                error,
            })
        })
        .collect()
}

/// Registers `chat` and offers it on its endpoint: the start's for an ungated pin, and the gate's
/// own registry's.
///
/// # Errors
/// The registry's own refusal.
pub fn offer_chat(registry: &mut Registry, chat: &ChatLive) -> Result<(), RegistryError> {
    registry.register(chat.entry.content())?;
    promote(registry, chat)
}

/// Offers `chat`, already registered, on its endpoint, as answering for the pinned identity.
fn promote(registry: &mut Registry, chat: &ChatLive) -> Result<(), RegistryError> {
    let pin = chat.entry.content().model_ref();
    let reported = Reported {
        model: Some(chat.entry.identity.model_name.clone()),
        provider: Some(chat.endpoint.route.reported_provider.clone()),
    };
    registry.set_status(&pin, Status::Offered)?;
    registry.add_endpoint(&pin, chat.endpoint.clone(), &[reported])
}

/// The live service on the thread that holds its gateway: each call, each put to the research
/// store, and each gate line from the gate socket (DEC-2294).
pub struct LiveHandler<S: Store, C, T> {
    pub service: LiveService<S, C, T>,
    pub writer: Option<ArtifactWriter>,
    pub desk: Desk,
}

impl<S: Store + 'static, C: Clock, T: Transport> Handler for LiveHandler<S, C, T> {
    fn call(&mut self, request: CallRequest) -> Reply {
        self.service.answer(&request)
    }

    fn put(&mut self, put: &PutRequest) -> PutReply {
        self.writer
            .as_mut()
            .map_or(Err(PutRefusal::StoreUnavailable), |writer| writer.put(put))
    }

    fn side(&mut self, line: &str) -> String {
        let reports = self.writer.as_mut().map(|w| -> &mut dyn Reports { w });
        self.desk.answer(line, self.service.gateway_mut(), reports)
    }
}
