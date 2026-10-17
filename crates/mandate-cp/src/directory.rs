//! The directory (design §3.2), enrollment (§3.1), routing, and the deployment-scoped calls
//! (CP-11), over the [`Store`] port.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::Digest;
use mandate_identity::Role;
use mandate_time::UtcNanos;

use crate::fleet::{FleetRow, Heartbeat, fleet_row};
use crate::request::{Signed, verify};

/// An opaque identifier, `[A-Za-z0-9_-]`, 1 to 64 bytes: the only form an ID takes here.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ident(String);

impl Ident {
    pub fn parse(text: &str) -> Option<Self> {
        let grammar = text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        (grammar && !text.is_empty() && text.len() <= 64).then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A deployment's API endpoint: `https://` and a host, with an optional port and path, ASCII
/// without whitespace, query, fragment, or user information, at most 256 bytes. It names a
/// deployment, never a person.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Endpoint(String);

impl Endpoint {
    pub fn parse(text: &str) -> Option<Self> {
        let rest = text.strip_prefix("https://")?;
        let host = rest.split(['/', ':']).next().unwrap_or_default();
        let clean =
            text.bytes().all(|b| b.is_ascii_graphic()) && !text.contains(['?', '#', '@', '\\']);
        let named = !host.is_empty()
            && host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
        (clean && named && text.len() <= 256).then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// How a deployment is run (design §1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeploymentMode {
    ManagedCell,
    Hybrid,
}

/// A deployment: a managed cell or a customer site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deployment {
    pub deployment_id: Ident,
    /// The organization that runs it: ours for a managed cell, the customer's for a hybrid site.
    pub owner_org: Ident,
    pub mode: DeploymentMode,
    pub endpoint: Endpoint,
    /// The Ed25519 public key bound at enrollment, `None` until then. The private key never
    /// leaves the deployment (E20-2).
    pub public_key: Option<[u8; 32]>,
}

/// A workspace and the deployment that hosts it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Workspace {
    pub workspace_id: Ident,
    pub org_id: Ident,
    pub deployment_id: Ident,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MemberState {
    Active,
    Deactivated,
}

/// A membership, by IDs and a role name only (E20-1, identity spec ID-15).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Member {
    pub user_id: Ident,
    pub workspace_id: Ident,
    pub role: Role,
    pub state: MemberState,
}

/// Where a client reaches one of its workspaces.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Route {
    pub workspace_id: Ident,
    pub endpoint: Endpoint,
}

/// A single-use enrollment token, kept only as the SHA-256 of its secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub expires_at: UtcNanos,
    pub used: bool,
}

/// What an enrollment bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enrolled {
    pub deployment_id: Ident,
    pub owner_org: Ident,
    pub mode: DeploymentMode,
    /// SHA-256 of the bound public key, which the site journals as `ControlPlaneEnrolled`'s
    /// `certificate_fingerprint` (journal spec §9.17).
    pub key_fingerprint: Digest,
}

/// Why a call was refused. None says more than that the caller may not have what it asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    UnknownToken,
    TokenUsed,
    TokenExpired,
    BadKey,
    UnknownDeployment,
    NotEnrolled,
    Stale,
    BadSignature,
    /// The call names a deployment, workspace or member that is not the caller's (CP-11).
    NotYours,
}

/// The directory's storage, implemented in memory here and over the plane's own database by its
/// server.
pub trait Store {
    fn deployments(&self) -> Vec<Deployment>;
    fn put_deployment(&mut self, deployment: Deployment);
    fn token(&self, deployment_id: &Ident, secret_hash: &Digest) -> Option<Token>;
    fn put_token(&mut self, deployment_id: &Ident, secret_hash: Digest, token: Token);
    fn workspaces(&self) -> Vec<Workspace>;
    fn members(&self) -> Vec<Member>;
    /// Replaces every workspace and membership of `deployment_id`, and nothing else.
    fn replace_directory(
        &mut self,
        deployment_id: &Ident,
        workspaces: Vec<Workspace>,
        members: Vec<Member>,
    );
    fn heartbeat(&self, deployment_id: &Ident) -> Option<(UtcNanos, Heartbeat)>;
    fn put_heartbeat(&mut self, deployment_id: &Ident, at: UtcNanos, heartbeat: Heartbeat);
}

/// The control plane's behaviour over a [`Store`].
pub struct ControlPlane<S> {
    pub store: S,
}

impl<S: Store> ControlPlane<S> {
    /// Stub: issues no token.
    pub fn create_deployment(
        &mut self,
        _deployment: Deployment,
        _secret: [u8; 32],
        _expires_at: UtcNanos,
    ) -> String {
        String::new()
    }

    /// Stub: enrolls nothing.
    pub fn enroll(
        &mut self,
        _deployment_id: &Ident,
        _token: &str,
        _public_key: &[u8],
        _now: UtcNanos,
    ) -> Result<Enrolled, Refused> {
        Err(Refused::UnknownToken)
    }

    /// Stub: authenticates nothing.
    pub fn authenticate(&self, signed: &Signed, now: UtcNanos) -> Result<Ident, Refused> {
        let _ = (
            signed,
            now,
            verify,
            Digest::ZERO,
            fleet_row,
            MemberState::Active,
            BTreeSet::<u8>::new(),
            BTreeMap::<u8, u8>::new(),
        );
        Err(Refused::NotEnrolled)
    }

    /// Stub: records nothing.
    pub fn heartbeat(
        &mut self,
        _signed: &Signed,
        _heartbeat: Heartbeat,
        _now: UtcNanos,
    ) -> Result<(), Refused> {
        Err(Refused::NotEnrolled)
    }

    /// Stub: an empty fleet.
    pub fn fleet(&self, _now: UtcNanos) -> Vec<FleetRow> {
        Vec::new()
    }

    /// Stub: syncs nothing.
    pub fn sync_directory(
        &mut self,
        _signed: &Signed,
        _workspaces: Vec<Workspace>,
        _members: Vec<Member>,
        _now: UtcNanos,
    ) -> Result<(), Refused> {
        Err(Refused::NotEnrolled)
    }

    /// Stub: routes nowhere.
    pub fn routes(
        &self,
        _signed: &Signed,
        _user_id: &Ident,
        _now: UtcNanos,
    ) -> Result<Vec<Route>, Refused> {
        Err(Refused::NotEnrolled)
    }
}

/// The directory in memory: the tests' store, and the shape the database store keeps.
#[derive(Debug, Clone, Default)]
pub struct MemoryStore {
    deployments: BTreeMap<Ident, Deployment>,
    tokens: BTreeMap<(Ident, Digest), Token>,
    workspaces: Vec<Workspace>,
    members: Vec<Member>,
    heartbeats: BTreeMap<Ident, (UtcNanos, Heartbeat)>,
}

impl Store for MemoryStore {
    fn deployments(&self) -> Vec<Deployment> {
        self.deployments.values().cloned().collect()
    }

    fn put_deployment(&mut self, deployment: Deployment) {
        self.deployments
            .insert(deployment.deployment_id.clone(), deployment);
    }

    fn token(&self, deployment_id: &Ident, secret_hash: &Digest) -> Option<Token> {
        self.tokens
            .get(&(deployment_id.clone(), *secret_hash))
            .copied()
    }

    fn put_token(&mut self, deployment_id: &Ident, secret_hash: Digest, token: Token) {
        self.tokens
            .insert((deployment_id.clone(), secret_hash), token);
    }

    fn workspaces(&self) -> Vec<Workspace> {
        self.workspaces.clone()
    }

    fn members(&self) -> Vec<Member> {
        self.members.clone()
    }

    fn replace_directory(
        &mut self,
        deployment_id: &Ident,
        workspaces: Vec<Workspace>,
        members: Vec<Member>,
    ) {
        let gone: BTreeSet<Ident> = self
            .workspaces
            .iter()
            .filter(|w| w.deployment_id == *deployment_id)
            .map(|w| w.workspace_id.clone())
            .collect();
        self.workspaces
            .retain(|w| w.deployment_id != *deployment_id);
        self.members.retain(|m| !gone.contains(&m.workspace_id));
        self.workspaces.extend(workspaces);
        self.members.extend(members);
    }

    fn heartbeat(&self, deployment_id: &Ident) -> Option<(UtcNanos, Heartbeat)> {
        self.heartbeats.get(deployment_id).cloned()
    }

    fn put_heartbeat(&mut self, deployment_id: &Ident, at: UtcNanos, heartbeat: Heartbeat) {
        self.heartbeats
            .insert(deployment_id.clone(), (at, heartbeat));
    }
}
