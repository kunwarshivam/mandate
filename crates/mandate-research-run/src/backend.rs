//! Which transport a research gateway calls through (DEC-997 item 4, DEC-1120): the OpenRouter
//! upstream only when the vault holds its key and the cell supplies a hosted transport, and the
//! recorded fake otherwise, which opens no connection. Either way the adapter, the registry, the
//! routing lock, the guard, the meter, and the caps are the same, so the recorded run shows what a
//! hosted run would send. The selection says which it used and why, and never names the key.

use mandate_canon::{Int, Key, Object, Value, to_canonical};
use mandate_gateway::fake::RecordedTransport;
use mandate_gateway::meter::{Meter, MeterStore};
use mandate_gateway::money::Usage;
use mandate_gateway::openrouter::OpenRouter;
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
        let upstream = &self.upstream;
        match self.reason {
            Reason::VaultedKey => format!(
                "research model gateway: OpenRouter, routed to {upstream} only, with the vaulted key"
            ),
            Reason::NoVaultedKey => format!(
                "research model gateway: the recorded fake (no vaulted key for {upstream}; no \
                 provider is called)"
            ),
            Reason::NoHostedTransport => format!(
                "research model gateway: the recorded fake (a key for {upstream} is vaulted, but \
                 this process has no hosted transport; no provider is called)"
            ),
        }
    }
}

/// The selection for `upstream`: OpenRouter only when `vault` holds its key and `hosted` is true.
pub fn select(vault: &dyn SecretSource, upstream: &str, hosted: bool) -> Selection {
    let (backend, reason) = match (vault.provider_key(upstream).is_ok(), hosted) {
        (false, _) => (Backend::RecordedFake, Reason::NoVaultedKey),
        (true, false) => (Backend::RecordedFake, Reason::NoHostedTransport),
        (true, true) => (Backend::OpenRouter, Reason::VaultedKey),
    };
    Selection {
        backend,
        reason,
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
        match self {
            Self::Recorded(fake) => Some(fake),
            Self::Hosted(_) => None,
        }
    }

    pub fn recorded_mut(&mut self) -> Option<&mut RecordedTransport> {
        match self {
            Self::Recorded(fake) => Some(fake),
            Self::Hosted(_) => None,
        }
    }
}

impl<H: Transport> Transport for ResearchTransport<H> {
    fn send(
        &mut self,
        request: &HttpRequest,
        key: Option<&ProviderKey>,
        deadline: UtcNanos,
        clock: &mut dyn Clock,
    ) -> Delivery {
        match self {
            Self::Hosted(hosted) => hosted.send(request, key, deadline, clock),
            Self::Recorded(fake) => fake.send(request, key, deadline, clock),
        }
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
        Ok(ProviderKey::new(RECORDED_KEY.to_owned()))
    }
}

/// An OpenRouter chat-completions body with `content` as the message, reporting `model` and
/// `provider`, and `usage`: what the recorded fake replays, so a recorded run's reply is the shape
/// a hosted one would read (inference spec §3.2).
pub fn recorded_reply(content: &str, model: &str, provider: &str, usage: Usage) -> Vec<u8> {
    let count = |n: u64| Int::new(n).map_or(Value::Null, Value::Int);
    let message = object(vec![
        ("content", Value::Str(content.to_owned())),
        ("role", Value::Str("assistant".to_owned())),
    ]);
    let choice = object(vec![
        ("finish_reason", Value::Str("stop".to_owned())),
        ("index", count(0)),
        ("message", message),
    ]);
    let body = object(vec![
        ("choices", Value::Array(vec![choice])),
        ("id", Value::Str("gen-recorded".to_owned())),
        ("model", Value::Str(model.to_owned())),
        ("object", Value::Str("chat.completion".to_owned())),
        ("provider", Value::Str(provider.to_owned())),
        (
            "usage",
            object(vec![
                ("completion_tokens", count(usage.output)),
                (
                    "prompt_tokens_details",
                    object(vec![("cached_tokens", count(usage.cached))]),
                ),
                ("prompt_tokens", count(usage.input)),
            ]),
        ),
    ]);
    to_canonical(&body)
}

/// The names are fixed words, each a valid key.
fn object(members: Vec<(&str, Value)>) -> Value {
    let mut out = Object::new();
    for (name, value) in members {
        if let Ok(key) = Key::new(name) {
            out.insert(key, value);
        }
    }
    Value::Object(out)
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
    hosted: Option<H>,
    recorded: RecordedTransport,
    upstream: &str,
) -> (Gateway<ResearchTransport<H>, S, C>, Selection) {
    let selection = select(vault.as_ref(), upstream, hosted.is_some());
    let (transport, secrets): (_, Box<dyn SecretSource>) = match (selection.backend, hosted) {
        (Backend::OpenRouter, Some(hosted)) => (ResearchTransport::Hosted(hosted), vault),
        _ => (ResearchTransport::Recorded(recorded), Box::new(RecordedKey)),
    };
    let gateway = Gateway::new(
        parts.settings,
        parts.registry,
        Box::new(OpenRouter),
        transport,
        secrets,
        parts.meter,
        parts.clock,
        parts.ids,
    );
    (gateway, selection)
}
