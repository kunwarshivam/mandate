//! The allowed label set (infrastructure design §8.1): opaque IDs and enumerations only. A key is
//! one of [`LabelKey`]'s six, and each key takes only its own value type, so a symbol, side,
//! quantity, price, mandate field, or person has no way in.

use crate::TelemetryError;

/// The longest label value, in bytes.
pub const VALUE_BOUND: usize = 64;

/// Every label key a metric may carry (design §8.1). There is no seventh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LabelKey {
    Workspace,
    Agent,
    Account,
    Process,
    Reason,
    Release,
}

impl LabelKey {
    pub const ALL: [Self; 6] = [
        Self::Workspace,
        Self::Agent,
        Self::Account,
        Self::Process,
        Self::Reason,
        Self::Release,
    ];

    /// The key as exported.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::Agent => "agent",
            Self::Account => "account_ref",
            Self::Process => "process_type",
            Self::Reason => "reason_code",
            Self::Release => "release",
        }
    }

    /// The key whose exported name is `key`, if any.
    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }

    /// Whether `value` is a value this key may carry, as exported.
    pub fn admits(self, value: &str) -> bool {
        match self {
            Self::Workspace | Self::Agent | Self::Account | Self::Reason => {
                OpaqueId::parse(value).is_ok()
            }
            Self::Process => ProcessType::parse(value).is_some(),
            Self::Release => Release::parse(value).is_ok(),
        }
    }
}

/// The process types of a workspace deployment (design §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProcessType {
    /// One deployment's runtime and executor in one process (E21-2, `mandate-agentd`).
    AgentProcess,
    AgentRuntime,
    Executor,
    Scheduler,
    ApiServer,
    DeploymentManager,
    Dispatcher,
}

impl ProcessType {
    pub const ALL: [Self; 7] = [
        Self::AgentProcess,
        Self::AgentRuntime,
        Self::Executor,
        Self::Scheduler,
        Self::ApiServer,
        Self::DeploymentManager,
        Self::Dispatcher,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::AgentProcess => "agent_process",
            Self::AgentRuntime => "agent_runtime",
            Self::Executor => "executor",
            Self::Scheduler => "scheduler",
            Self::ApiServer => "api_server",
            Self::DeploymentManager => "deployment_manager",
            Self::Dispatcher => "dispatcher",
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key() == key)
    }
}

/// An opaque identifier: 1 to 64 characters of `[a-z0-9_]` starting with a letter, the shape of
/// the journal's workspace, agent and account references and of DEC-710 item 4's recipient. A
/// value that is all digits, or holds a `.`, `-`, or capital letter, is never one, so no price,
/// quantity, or ticker as brokers print it fits.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpaqueId(String);

impl OpaqueId {
    /// # Errors
    /// [`TelemetryError::NotOpaque`] for anything but the shape above.
    pub fn parse(id: &str) -> Result<Self, TelemetryError> {
        let mut bytes = id.bytes();
        let starts_with_letter = bytes.next().is_some_and(|b| b.is_ascii_lowercase());
        let rest = bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        if starts_with_letter && rest && id.len() <= VALUE_BOUND {
            Ok(Self(id.to_owned()))
        } else {
            Err(TelemetryError::NotOpaque)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A reason code: fixed `snake_case` text written in the code, never built from data. It takes
/// only a `&'static str`, so no runtime value can become one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReasonCode(&'static str);

impl ReasonCode {
    /// # Errors
    /// [`TelemetryError::NotOpaque`] for anything but 1 to 64 characters of `[a-z0-9_]` starting
    /// with a letter.
    pub fn new(code: &'static str) -> Result<Self, TelemetryError> {
        OpaqueId::parse(code).map(|_| Self(code))
    }

    /// This crate's own codes in [`crate::reason`], checked there by [`crate::lint::reasons`].
    pub(crate) const fn fixed(code: &'static str) -> Self {
        Self(code)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// A release: the build's version or commit, 1 to 64 characters of `[a-z0-9._+-]` starting with
/// a letter or digit, holding at least one letter or `.`-separated part, so a bare number is
/// never one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Release(String);

impl Release {
    /// # Errors
    /// [`TelemetryError::NotOpaque`] for anything but the shape above.
    pub fn parse(release: &str) -> Result<Self, TelemetryError> {
        let allowed = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._+-".contains(&b);
        let first_ok = release
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
        let not_a_number = release
            .bytes()
            .filter(|b| !b.is_ascii_digit())
            .any(|b| b != b'.')
            || release.bytes().filter(|b| *b == b'.').count() >= 2;
        if first_ok && not_a_number && release.len() <= VALUE_BOUND && release.bytes().all(allowed)
        {
            Ok(Self(release.to_owned()))
        } else {
            Err(TelemetryError::NotOpaque)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A set of labels, at most one value per key, each value of its key's own type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Labels(Vec<(LabelKey, String)>);

impl Labels {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn workspace(self, id: &OpaqueId) -> Self {
        self.with(LabelKey::Workspace, id.as_str())
    }

    pub fn agent(self, id: &OpaqueId) -> Self {
        self.with(LabelKey::Agent, id.as_str())
    }

    pub fn account(self, id: &OpaqueId) -> Self {
        self.with(LabelKey::Account, id.as_str())
    }

    pub fn process(self, process: ProcessType) -> Self {
        self.with(LabelKey::Process, process.key())
    }

    pub fn reason(self, reason: ReasonCode) -> Self {
        self.with(LabelKey::Reason, reason.as_str())
    }

    pub fn release(self, release: &Release) -> Self {
        self.with(LabelKey::Release, release.as_str())
    }

    /// The keys present, in [`LabelKey`] order.
    pub fn keys(&self) -> Vec<LabelKey> {
        self.0.iter().map(|(key, _)| *key).collect()
    }

    /// The pairs as exported, in [`LabelKey`] order.
    pub fn pairs(&self) -> impl Iterator<Item = (&'static str, &str)> {
        self.0
            .iter()
            .map(|(key, value)| (key.key(), value.as_str()))
    }

    fn with(mut self, key: LabelKey, value: &str) -> Self {
        self.0.retain(|(k, _)| *k != key);
        self.0.push((key, value.to_owned()));
        self.0.sort_by_key(|(k, _)| *k);
        self
    }
}
