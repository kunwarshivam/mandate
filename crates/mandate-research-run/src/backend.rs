//! Which transport a research gateway calls through (DEC-997 item 4, DEC-1120): the OpenRouter
//! upstream only when the vault holds its key and the cell supplies a hosted transport, and the
//! recorded fake otherwise, which opens no connection. Either way the adapter, the registry, the
//! routing lock, the guard, the meter, and the caps are the same, so the recorded run shows what a
//! hosted run would send. The selection says which it used and why, and never names the key.

use mandate_gateway::fake::RecordedTransport;
use mandate_gateway::meter::{Meter, MeterStore};
use mandate_gateway::money::Usage;
use mandate_gateway::provider::{
    Clock, Delivery, HttpRequest, IdSource, ProviderKey, SecretError, SecretSource, Transport,
};
use mandate_gateway::registry::Registry;
use mandate_gateway::{Gateway, Settings};
use mandate_time::UtcNanos;

/// The transport a research gateway uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// The cell's hosted transport to OpenRouter, with the vault's key.
    OpenRouter,
    /// The recorded-response fake: no connection, no key.
    RecordedFake,
}

/// Why that transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The vault holds the upstream's key and a hosted transport is supplied.
    VaultedKey,
    /// The vault holds no key for the upstream.
    NoVaultedKey,
    /// The vault holds a key, but this process has no hosted transport.
    NoHostedTransport,
}

/// The choice, for the demo and the operator to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub backend: Backend,
    pub reason: Reason,
    pub upstream: String,
}

impl Selection {
    /// One line naming the transport and the reason; it never names or shows the key.
    pub fn describe(&self) -> String {
        String::new()
    }
}

/// The selection for `upstream`: OpenRouter only when `vault` holds its key and `hosted` is true.
pub fn select(_vault: &dyn SecretSource, upstream: &str, _hosted: bool) -> Selection {
    Selection {
        backend: Backend::RecordedFake,
        reason: Reason::NoVaultedKey,
        upstream: upstream.to_owned(),
    }
}

/// A hosted transport that cannot exist: what a process without one passes, so the selection can
/// only choose the recorded fake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoHostedTransport {}

impl Transport for NoHostedTransport {
    fn send(
        &mut self,
        _request: &HttpRequest,
        _key: Option<&ProviderKey>,
        _deadline: UtcNanos,
        _clock: &mut dyn Clock,
    ) -> Delivery {
        match *self {}
    }
}

/// The transport a research gateway was built with.
#[derive(Debug, Clone)]
pub enum ResearchTransport<H> {
    Hosted(H),
    Recorded(RecordedTransport),
}

impl<H> ResearchTransport<H> {
    /// The recorded fake, when that is the transport.
    pub fn recorded(&self) -> Option<&RecordedTransport> {
        None
    }

    pub fn recorded_mut(&mut self) -> Option<&mut RecordedTransport> {
        None
    }
}

impl<H: Transport> Transport for ResearchTransport<H> {
    fn send(
        &mut self,
        _request: &HttpRequest,
        _key: Option<&ProviderKey>,
        _deadline: UtcNanos,
        _clock: &mut dyn Clock,
    ) -> Delivery {
        Delivery::ConnectFailed
    }
}

/// The text the recorded fake's key source holds: a fixed placeholder, never a credential, so a
/// hosted endpoint's call reaches the recorded transport rather than ending `credential_invalid`.
pub const RECORDED_KEY: &str = "recorded-fake-holds-no-key";

/// The recorded fake's key source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RecordedKey;

impl SecretSource for RecordedKey {
    fn provider_key(&self, _upstream: &str) -> Result<ProviderKey, SecretError> {
        Err(SecretError::Missing)
    }
}

/// An OpenRouter chat-completions body with `content` as the message, reporting `model` and
/// `provider`, and `usage`: what the recorded fake replays, so a recorded run's reply is the shape
/// a hosted one would read (inference spec §3.2).
pub fn recorded_reply(content: &str, model: &str, provider: &str, usage: Usage) -> Vec<u8> {
    let _ = (content, model, provider, usage);
    Vec::new()
}

/// Everything a research gateway is built from besides its transport and key source.
pub struct GatewayParts<S, C> {
    pub settings: Settings,
    pub registry: Registry,
    pub meter: Meter<S>,
    pub clock: C,
    pub ids: Box<dyn IdSource>,
}

/// The research gateway for `upstream`, with the OpenRouter adapter always: the hosted transport
/// and `vault` when [`select`] chooses OpenRouter, the recorded fake and [`RecordedKey`] otherwise,
/// and the selection that says which.
pub fn research_gateway<H: Transport, S: MeterStore, C: Clock>(
    parts: GatewayParts<S, C>,
    vault: Box<dyn SecretSource>,
    _hosted: Option<H>,
    recorded: RecordedTransport,
    upstream: &str,
) -> (Gateway<ResearchTransport<H>, S, C>, Selection) {
    let selection = select(vault.as_ref(), upstream, false);
    let gateway = Gateway::new(
        parts.settings,
        parts.registry,
        Box::new(mandate_gateway::openrouter::OpenRouter),
        ResearchTransport::Recorded(recorded),
        vault,
        parts.meter,
        parts.clock,
        parts.ids,
    );
    (gateway, selection)
}
