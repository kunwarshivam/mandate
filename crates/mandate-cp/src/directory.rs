//! The directory (design §3.2), enrollment (§3.1), routing, and the deployment-scoped calls
//! (CP-11), over the [`Store`] port.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::Digest;
use mandate_identity::Role;
use mandate_time::UtcNanos;

use crate::fleet::{FleetRow, Heartbeat, fleet_row};
use crate::request::{Operation, Signed, verify};
use crate::wire;

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
    /// The deployment already holds a key; a key is replaced only after a revocation.
    AlreadyEnrolled,
    /// The deployment or workspace ID is already registered.
    Exists,
    /// The call's time is outside the skew, or not after the deployment's last accepted call.
    Stale,
    BadSignature,
    /// The call is not the operation it was sent to, or its body is not that operation's closed
    /// canonical shape.
    Malformed,
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
    fn put_workspace(&mut self, workspace: Workspace);
    fn members(&self) -> Vec<Member>;
    /// Replaces every membership of the workspaces of `deployment_id`, and nothing else.
    fn replace_members(&mut self, deployment_id: &Ident, members: Vec<Member>);
    fn heartbeat(&self, deployment_id: &Ident) -> Option<(UtcNanos, Heartbeat)>;
    fn put_heartbeat(&mut self, deployment_id: &Ident, at: UtcNanos, heartbeat: Heartbeat);
    /// The time of the deployment's last accepted call.
    fn last_call(&self, deployment_id: &Ident) -> Option<UtcNanos>;
    fn put_last_call(&mut self, deployment_id: &Ident, at: UtcNanos);
}

/// The control plane's behaviour over a [`Store`].
pub struct ControlPlane<S> {
    pub store: S,
}

impl<S: Store> ControlPlane<S> {
    fn deployment(&self, id: &Ident) -> Option<Deployment> {
        self.store
            .deployments()
            .into_iter()
            .find(|d| d.deployment_id == *id)
    }

    /// An org admin creates `deployment`, unenrolled, and receives its first enrollment token
    /// ([`Self::issue_token`]). An ID already registered is refused, so no creation can take over
    /// or un-enroll another deployment.
    pub fn create_deployment(
        &mut self,
        deployment: Deployment,
        secret: [u8; 32],
        expires_at: UtcNanos,
    ) -> Result<String, Refused> {
        if self.deployment(&deployment.deployment_id).is_some() {
            return Err(Refused::Exists);
        }
        let id = deployment.deployment_id.clone();
        self.store.put_deployment(Deployment {
            public_key: None,
            ..deployment
        });
        self.issue_token(&id, secret, expires_at)
    }

    /// A single-use enrollment token for an unenrolled deployment: the hex of `secret`, 32 random
    /// bytes the caller drew, kept only as its SHA-256 and valid until `expires_at` (E13-1, design
    /// §3.1).
    pub fn issue_token(
        &mut self,
        deployment_id: &Ident,
        secret: [u8; 32],
        expires_at: UtcNanos,
    ) -> Result<String, Refused> {
        let deployment = self
            .deployment(deployment_id)
            .ok_or(Refused::UnknownDeployment)?;
        if deployment.public_key.is_some() {
            return Err(Refused::AlreadyEnrolled);
        }
        let token = Token {
            expires_at,
            used: false,
        };
        self.store
            .put_token(deployment_id, Digest::of(&secret), token);
        Ok(Digest::from_bytes(secret).to_hex())
    }

    /// Revokes a deployment's key: the site sees an outage (design §4), never a trading stop. A
    /// new token can then enroll a new key.
    pub fn revoke(&mut self, deployment_id: &Ident) -> Result<(), Refused> {
        let deployment = self
            .deployment(deployment_id)
            .ok_or(Refused::UnknownDeployment)?;
        self.store.put_deployment(Deployment {
            public_key: None,
            ..deployment
        });
        Ok(())
    }

    /// An org admin places a workspace on a deployment. Only a registered workspace can be named
    /// by the deployment's directory sync, and a hybrid site hosts only its own organization's
    /// workspaces (CP-11).
    pub fn register_workspace(&mut self, workspace: Workspace) -> Result<(), Refused> {
        let deployment = self
            .deployment(&workspace.deployment_id)
            .ok_or(Refused::UnknownDeployment)?;
        if deployment.mode == DeploymentMode::Hybrid && workspace.org_id != deployment.owner_org {
            return Err(Refused::NotYours);
        }
        let held = self
            .store
            .workspaces()
            .iter()
            .any(|w| w.workspace_id == workspace.workspace_id);
        if held {
            return Err(Refused::Exists);
        }
        self.store.put_workspace(workspace);
        Ok(())
    }

    /// Enrollment (design §3.1, E20-2): a known, unused, unexpired token binds `public_key`, the
    /// key generated on site, to its unenrolled deployment, and is spent.
    pub fn enroll(
        &mut self,
        deployment_id: &Ident,
        token: &str,
        public_key: &[u8],
        now: UtcNanos,
    ) -> Result<Enrolled, Refused> {
        let secret = Digest::from_hex(token).ok_or(Refused::UnknownToken)?;
        let hash = Digest::of(secret.as_bytes());
        let stored = self
            .store
            .token(deployment_id, &hash)
            .ok_or(Refused::UnknownToken)?;
        if stored.used {
            return Err(Refused::TokenUsed);
        }
        if now >= stored.expires_at {
            return Err(Refused::TokenExpired);
        }
        let key: [u8; 32] = public_key.try_into().map_err(|_| Refused::BadKey)?;
        let deployment = self
            .deployment(deployment_id)
            .ok_or(Refused::UnknownDeployment)?;
        if deployment.public_key.is_some() {
            return Err(Refused::AlreadyEnrolled);
        }
        let reused = self
            .store
            .deployments()
            .iter()
            .any(|d| d.public_key == Some(key));
        if reused {
            return Err(Refused::BadKey);
        }
        self.store.put_token(
            deployment_id,
            hash,
            Token {
                used: true,
                ..stored
            },
        );
        let enrolled = Enrolled {
            deployment_id: deployment.deployment_id.clone(),
            owner_org: deployment.owner_org.clone(),
            mode: deployment.mode,
            key_fingerprint: Digest::of(&key),
        };
        self.store.put_deployment(Deployment {
            public_key: Some(key),
            ..deployment
        });
        Ok(enrolled)
    }

    /// The deployment a signed call to `operation` comes from: its key is enrolled, the call is
    /// that operation, its time is within the skew and after the deployment's last accepted call,
    /// and its signature verifies. The call's time is then recorded, so no call is accepted twice
    /// and no older call after a newer one.
    pub fn authenticate(
        &mut self,
        signed: &Signed,
        operation: Operation,
        now: UtcNanos,
    ) -> Result<Ident, Refused> {
        if (signed.method.as_str(), signed.path.as_str()) != operation.route() {
            return Err(Refused::Malformed);
        }
        let deployment = self
            .deployment(&signed.deployment_id)
            .ok_or(Refused::UnknownDeployment)?;
        let key = deployment.public_key.ok_or(Refused::NotEnrolled)?;
        verify(signed, &key, now)?;
        let id = deployment.deployment_id;
        if self
            .store
            .last_call(&id)
            .is_some_and(|last| signed.at <= last)
        {
            return Err(Refused::Stale);
        }
        self.store.put_last_call(&id, signed.at);
        Ok(id)
    }

    /// A heartbeat (design §3.4, E20-5): versions and health only, from its own deployment.
    pub fn heartbeat(&mut self, signed: &Signed, now: UtcNanos) -> Result<(), Refused> {
        let caller = self.authenticate(signed, Operation::Heartbeat, now)?;
        let heartbeat = wire::heartbeat(&signed.body).ok_or(Refused::Malformed)?;
        self.store.put_heartbeat(&caller, now, heartbeat);
        Ok(())
    }

    /// The fleet view: every deployment's last heartbeat and whether it is reachable. A silent
    /// deployment is only marked `unreachable`; nothing about it is revoked (CP-8).
    pub fn fleet(&self, now: UtcNanos) -> Vec<FleetRow> {
        self.store
            .deployments()
            .into_iter()
            .map(|d| fleet_row(&d, self.store.heartbeat(&d.deployment_id), now))
            .collect()
    }

    /// Directory sync (design §3.2): a deployment replaces the memberships of the workspaces
    /// registered on it. A member of any other workspace refuses the whole call (CP-11).
    pub fn sync_directory(&mut self, signed: &Signed, now: UtcNanos) -> Result<(), Refused> {
        let caller = self.authenticate(signed, Operation::Directory, now)?;
        let members = wire::members(&signed.body).ok_or(Refused::Malformed)?;
        let hosted = self.hosted(&caller);
        if !members.iter().all(|m| hosted.contains_key(&m.workspace_id)) {
            return Err(Refused::NotYours);
        }
        self.store.replace_members(&caller, members);
        Ok(())
    }

    /// The workspaces registered on `deployment_id`, with their organizations.
    fn hosted(&self, deployment_id: &Ident) -> BTreeMap<Ident, Ident> {
        self.store
            .workspaces()
            .into_iter()
            .filter(|w| w.deployment_id == *deployment_id)
            .map(|w| (w.workspace_id, w.org_id))
            .collect()
    }

    /// The routes of a user the calling deployment hosts: each workspace, in an organization where
    /// the user is an active member on the caller, where they hold an active membership, with its
    /// deployment's endpoint. A user the caller does not host is refused, so no deployment learns
    /// another's users or another organization's workspaces (CP-11).
    pub fn routes(&mut self, signed: &Signed, now: UtcNanos) -> Result<Vec<Route>, Refused> {
        let caller = self.authenticate(signed, Operation::Routes, now)?;
        let user = wire::user(&signed.body).ok_or(Refused::Malformed)?;
        let active: Vec<Member> = self
            .store
            .members()
            .into_iter()
            .filter(|m| m.user_id == user && m.state == MemberState::Active)
            .collect();
        let hosted = self.hosted(&caller);
        let orgs: BTreeSet<&Ident> = active
            .iter()
            .filter_map(|m| hosted.get(&m.workspace_id))
            .collect();
        if orgs.is_empty() {
            return Err(Refused::NotYours);
        }
        let endpoints: BTreeMap<Ident, Endpoint> = self
            .store
            .deployments()
            .into_iter()
            .map(|d| (d.deployment_id, d.endpoint))
            .collect();
        let workspaces: BTreeMap<Ident, Workspace> = self
            .store
            .workspaces()
            .into_iter()
            .map(|w| (w.workspace_id.clone(), w))
            .collect();
        let routes: BTreeSet<Route> = active
            .iter()
            .filter_map(|m| {
                let workspace = workspaces.get(&m.workspace_id)?;
                if !orgs.contains(&workspace.org_id) {
                    return None;
                }
                Some(Route {
                    workspace_id: m.workspace_id.clone(),
                    endpoint: endpoints.get(&workspace.deployment_id)?.clone(),
                })
            })
            .collect();
        Ok(routes.into_iter().collect())
    }
}

/// The directory in memory: the tests' store, and the shape the database store keeps.
#[derive(Debug, Clone, Default)]
pub struct MemoryStore {
    deployments: BTreeMap<Ident, Deployment>,
    tokens: BTreeMap<(Ident, Digest), Token>,
    workspaces: BTreeMap<Ident, Workspace>,
    members: Vec<Member>,
    heartbeats: BTreeMap<Ident, (UtcNanos, Heartbeat)>,
    calls: BTreeMap<Ident, UtcNanos>,
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
        self.workspaces.values().cloned().collect()
    }

    fn put_workspace(&mut self, workspace: Workspace) {
        self.workspaces
            .insert(workspace.workspace_id.clone(), workspace);
    }

    fn members(&self) -> Vec<Member> {
        self.members.clone()
    }

    fn replace_members(&mut self, deployment_id: &Ident, members: Vec<Member>) {
        let hosted: BTreeSet<Ident> = self
            .workspaces
            .values()
            .filter(|w| w.deployment_id == *deployment_id)
            .map(|w| w.workspace_id.clone())
            .collect();
        self.members.retain(|m| !hosted.contains(&m.workspace_id));
        self.members.extend(members);
    }

    fn heartbeat(&self, deployment_id: &Ident) -> Option<(UtcNanos, Heartbeat)> {
        self.heartbeats.get(deployment_id).cloned()
    }

    fn put_heartbeat(&mut self, deployment_id: &Ident, at: UtcNanos, heartbeat: Heartbeat) {
        self.heartbeats
            .insert(deployment_id.clone(), (at, heartbeat));
    }

    fn last_call(&self, deployment_id: &Ident) -> Option<UtcNanos> {
        self.calls.get(deployment_id).copied()
    }

    fn put_last_call(&mut self, deployment_id: &Ident, at: UtcNanos) {
        self.calls.insert(deployment_id.clone(), at);
    }
}
