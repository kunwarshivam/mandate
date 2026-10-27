//! The service's live start (DEC-2216 item 4, DEC-2326 items 1 and 3, DEC-2282): the founder's
//! vetted entries, read from the live registry file the chart renders, over the HTTPS transport and
//! the key read once from the vault. Every entry the registry takes is registered `evaluating`,
//! with no endpoint, and C is refused while its snapshot is undated, so a live start serves no
//! research call: each is answered `policy_denied`, with nothing reserved and nothing sent. An
//! entry is offered only on a passing gate report the start re-reads and an identity probe, which
//! the gate's build adds; until then a file naming a report starts nothing.

use std::collections::BTreeSet;
use std::path::Path;

use mandate_canon::parse;
use mandate_gateway::guard::GuardContext;
use mandate_gateway::policy::EndpointPolicy;
use mandate_gateway::provider::{Clock, SecretSource, Transport};
use mandate_gateway::registry::{Locality, Registry};
use mandate_gateway::{RateCaps, Settings};
use mandate_research_run::live::{LiveEntry, Refused, live_registry, read_live};
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
    unverified(entries)?;
    let held: Vec<LiveEntry> = entries
        .iter()
        .cloned()
        .map(|live| LiveEntry {
            offered: false,
            ..live
        })
        .collect();
    let (registry, refused) = live_registry(&held, &mut |_, _| Vec::new());
    let service = metered_service(
        config,
        parts,
        Served {
            settings: live_settings(&config.workspace, entries),
            registry,
            transport,
            key,
        },
    )?;
    Ok((service, refused))
}
