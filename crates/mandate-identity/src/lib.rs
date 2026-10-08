#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The pure identity core ([identity spec](../../../docs/specs/identity.md) §3 to §5, backlog E9-2
//! and E9-8): who a principal is, which memberships reach a scope, and the authorization step.
//!
//! **One authority.** [`authorize`] applies the permission matrix of identity spec §4.2, read by
//! its grammar (§4.2 "Reading the matrix", DEC-641), and nothing else (ID-2). Org roles never act
//! at a workspace's scope and workspace roles never at an org's (§4.1, no inheritance).
//!
//! **The only way to name a workspace below the API.** A [`TenantContext`] comes only out of an
//! [`Authorized`] for a workspace's scope; its fields are private and it has no other constructor
//! (ID-8, DEC-642). None of these compile outside the crate:
//!
//! ```compile_fail,E0451
//! use mandate_identity::{
//!     OrgId, Permission, PrincipalId, PrincipalKind, SessionRef, TenantContext, WorkspaceId,
//! };
//! let _ = TenantContext {
//!     org: OrgId(1),
//!     workspace: WorkspaceId(2),
//!     principal: PrincipalId(3),
//!     kind: PrincipalKind::User,
//!     session: SessionRef(4),
//!     permission: Permission::ViewAgents,
//!     membership_unverified: false,
//! };
//! ```
//!
//! ```compile_fail,E0599
//! let _ = mandate_identity::TenantContext::default();
//! ```
//!
//! ```compile_fail,E0308
//! fn copy(context: &mandate_identity::TenantContext) -> mandate_identity::TenantContext {
//!     context.clone()
//! }
//! ```
//!
//! **Sessions, memberships, and the membership lookup are sealed** (DEC-642 items 4, 7, 9): their
//! fields are private and the lookup's supertrait is unreachable, so none of these compile either:
//!
//! ```compile_fail,E0451
//! use mandate_identity::{Session, SessionKind, SessionRef};
//! let _ = Session {
//!     reference: SessionRef(1),
//!     kind: SessionKind::Full,
//!     snapshot: Vec::new(),
//! };
//! ```
//!
//! ```compile_fail,E0451
//! use mandate_identity::{Membership, MembershipState, OrgId, PrincipalId, Scope};
//! let _ = Membership {
//!     member: PrincipalId(1),
//!     scope: Scope::Org(OrgId(2)),
//!     state: MembershipState::Active,
//!     roles: std::collections::BTreeMap::new(),
//! };
//! ```
//!
//! ```compile_fail,E0277
//! use mandate_identity::{LookupFailed, Membership, MembershipLookup, MembershipQuery};
//! struct Forged;
//! impl MembershipLookup for Forged {
//!     fn memberships(&self, _: &MembershipQuery) -> Result<Vec<Membership>, LookupFailed> {
//!         Ok(Vec::new())
//!     }
//! }
//! ```
//!
//! **Roles change only through their checks** (ID-13, §4.5): [`change_roles`] refuses a change to
//! its author's own roles, the org owner role to anyone but an org owner, and a change that leaves
//! no active org owner or workspace admin (§5.2). Refusals carry DEC-643's codes ([`Refusal`]).
//!
//! Every entry point is pure: no clock, no randomness, no I/O, ordered collections only.

use std::collections::{BTreeMap, BTreeSet};

use mandate_time::UtcNanos;

mod permission;

pub use permission::Permission;

/// A principal's opaque ID, a ULID (identity spec §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrincipalId(pub u128);

/// An organization's opaque ID, a ULID (§3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OrgId(pub u128);

/// A workspace's opaque ID, a ULID (§3.2). No data API takes one bare (ID-8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceId(pub u128);

/// A session's opaque reference (identity spec §6.3, §12.2): what every committed event names as
/// `session_ref`, never the cookie or the token. For the host CLI, its registration's ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionRef(pub u128);

/// The principal kinds of identity spec §3.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PrincipalKind {
    /// A person.
    User,
    /// An owner-connected agent acting for one user (DEC-141).
    Client,
    /// An organization's automation, read and export only in v1.
    ServiceAccount,
    /// A deployed agent's runtime; it holds no column of §4.2.
    Agent,
    /// Executor, scheduler, workspace services; no column of §4.2.
    Process,
    /// The on-host command line of a workspace deployment (§6.4 route 3).
    HostCli,
    /// Platform staff, only inside break-glass (§10.3).
    PlatformOperator,
}

/// An authenticated principal, with what its kind's column of §4.2 is scoped to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Principal {
    /// A person; reaches a scope only through a membership.
    User {
        /// Its opaque ID.
        id: PrincipalId,
    },
    /// A client, scoped to one user in one workspace.
    Client {
        /// Its opaque ID.
        id: PrincipalId,
        /// The user it acts for, whose memberships bound it (DEC-641 item 2).
        on_behalf_of: PrincipalId,
        /// The one workspace its token names.
        workspace: WorkspaceId,
    },
    /// A service account, scoped to the workspaces it names.
    ServiceAccount {
        /// Its opaque ID.
        id: PrincipalId,
        /// The workspaces it was issued for.
        workspaces: BTreeSet<WorkspaceId>,
    },
    /// An agent's runtime.
    Agent {
        /// Its opaque ID.
        id: PrincipalId,
    },
    /// A platform process.
    Process {
        /// Its opaque ID.
        id: PrincipalId,
    },
    /// The host CLI, registered for its deployment's own workspace.
    HostCli {
        /// Its registration's ULID.
        id: PrincipalId,
        /// The deployment's own workspace.
        workspace: WorkspaceId,
        /// The registering admin.
        on_behalf_of: PrincipalId,
    },
    /// A platform operator.
    PlatformOperator {
        /// Its opaque ID.
        id: PrincipalId,
        /// The workspace of a customer-approved break-glass window in force, if any.
        window: Option<WorkspaceId>,
    },
}

/// The roles of identity spec §4.1. The first three are org roles, the rest workspace roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    /// OO.
    OrgOwner,
    /// OA.
    OrgAdmin,
    /// Bill.
    BillingAdmin,
    /// WA.
    WorkspaceAdmin,
    /// Op.
    Operator,
    /// Ap.
    Approver,
    /// Vi.
    Viewer,
    /// Au.
    Auditor,
}

impl Role {
    /// Every role, in §4.2's column order.
    pub const ALL: [Self; 8] = [
        Self::OrgOwner,
        Self::OrgAdmin,
        Self::BillingAdmin,
        Self::WorkspaceAdmin,
        Self::Operator,
        Self::Approver,
        Self::Viewer,
        Self::Auditor,
    ];
}

/// What a row's **S** cell asks of step-up (§4.2, DEC-641 item 1). Verifying evidence is §7's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepUp {
    /// A blank cell.
    NotRequired,
    /// `S`: every use.
    Required,
    /// `S for invite`: an invitation only.
    ForInvite,
    /// `S for grant`: a grant only.
    ForGrant,
}

/// Where an action applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    /// The principal's own scope, for the `self` rows of §4.2: its own data only, no workspace or
    /// organization, and no membership read (identity spec §4.5, DEC-816).
    Principal,
    /// An organization's scope, where only org roles act.
    Org(OrgId),
    /// A workspace's scope, where only workspace roles and the non-member columns act.
    Workspace {
        /// The workspace's organization.
        org: OrgId,
        /// The workspace.
        workspace: WorkspaceId,
    },
}

/// The membership states of identity spec §5.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(
    missing_docs,
    reason = "each variant is the §5.1 state of the same name"
)]
pub enum MembershipState {
    Invited,
    CoolingOff,
    Active,
    Deactivated,
    Removed,
    Expired,
    Revoked,
}

/// A user's membership in one scope, as the workspace store folds it from the control stream (§5):
/// the member, the org or workspace, its state, and its roles, each with the instant it becomes
/// effective, the end of its cool-off (§8.3); a role grants nothing before then. Its fields are
/// private and only the store builds one (DEC-642 item 4); until E9-7 adds that store, only this
/// crate's tests do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Membership {
    member: PrincipalId,
    scope: Scope,
    state: MembershipState,
    roles: BTreeMap<Role, UtcNanos>,
}

/// A change to roles or memberships in one scope, checked by [`change_roles`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RoleChange {
    /// Roles granted, by member.
    pub grants: BTreeSet<(PrincipalId, Role)>,
    /// Roles removed, by member.
    pub removals: BTreeSet<(PrincipalId, Role)>,
    /// Memberships deactivated; the author's own is leaving.
    pub deactivations: BTreeSet<PrincipalId>,
}

/// The workspace context only the authorization step constructs (ID-8, DEC-642). Every data API
/// over a workspace takes `&TenantContext` and reads the workspace from it.
#[derive(Debug, PartialEq, Eq)]
pub struct TenantContext {
    org: OrgId,
    workspace: WorkspaceId,
    principal: PrincipalId,
    kind: PrincipalKind,
    session: SessionRef,
    permission: Permission,
    membership_unverified: bool,
}

impl TenantContext {
    /// The workspace authorized.
    pub fn workspace(&self) -> WorkspaceId {
        self.workspace
    }

    /// The workspace's organization.
    pub fn org(&self) -> OrgId {
        self.org
    }

    /// The authenticated principal.
    pub fn principal(&self) -> PrincipalId {
        self.principal
    }

    /// The authenticated principal's kind.
    pub fn kind(&self) -> PrincipalKind {
        self.kind
    }

    /// The session the request came through.
    pub fn session(&self) -> SessionRef {
        self.session
    }

    /// Whether this risk-reducing row was authorized from the session's roles snapshot because the
    /// membership read failed (identity spec §4.5, DEC-642 item 10); the committed event records it.
    pub fn membership_unverified(&self) -> bool {
        self.membership_unverified
    }

    /// The one permission authorized.
    pub fn permission(&self) -> Permission {
        self.permission
    }
}

/// What every data API over a workspace takes: a [`TenantContext`] for a request, or, from E9-8,
/// the `SystemContext` of `mandate-identity-system` for the deployment's own background processes
/// (DEC-642 items 3 and 5 to 7). It is sealed, so no other type implements it, and none of its
/// implementors takes a bare workspace ID:
///
/// ```compile_fail,E0277
/// struct Forged;
/// impl mandate_identity::Tenant for Forged {
///     fn workspace(&self) -> mandate_identity::WorkspaceId { mandate_identity::WorkspaceId(1) }
///     fn org(&self) -> mandate_identity::OrgId { mandate_identity::OrgId(1) }
///     fn principal(&self) -> mandate_identity::PrincipalId { mandate_identity::PrincipalId(1) }
///     fn kind(&self) -> mandate_identity::PrincipalKind { mandate_identity::PrincipalKind::User }
/// }
/// ```
pub trait Tenant: sealed::Sealed {
    /// The workspace whose data may be reached.
    fn workspace(&self) -> WorkspaceId;
    /// The workspace's organization.
    fn org(&self) -> OrgId;
    /// Who acts: the authenticated principal, or the process's workload identity.
    fn principal(&self) -> PrincipalId;
    /// The actor's kind.
    fn kind(&self) -> PrincipalKind;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::TenantContext {}
}

impl Tenant for TenantContext {
    fn workspace(&self) -> WorkspaceId {
        self.workspace
    }
    fn org(&self) -> OrgId {
        self.org
    }
    fn principal(&self) -> PrincipalId {
        self.principal
    }
    fn kind(&self) -> PrincipalKind {
        self.kind
    }
}

/// The key of the one read authorizing needs before any context exists (DEC-642 item 4), which
/// [`authorize`] and [`change_roles`] build themselves: the principal, or a client's user, and the
/// scope. The membership lookup takes only this and returns only [`Membership`]s; no data API
/// accepts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MembershipQuery {
    /// Whose memberships, or `None` for every membership of the scope (the last-owner and
    /// last-admin count).
    pub member: Option<PrincipalId>,
    /// In which scope.
    pub scope: Scope,
}

/// The membership store's one read path (identity spec §4.5, §6.2): uncached, memberships only.
///
/// It is sealed (DEC-642 items 4 and 7): only the workspace store implements it, through the seal
/// crate E9-8 adds; until then nothing outside this crate does, and its tests' doubles are
/// `cfg(test)` only.
pub trait MembershipLookup: sealed::Sealed {
    /// The memberships the query names.
    fn memberships(&self, query: &MembershipQuery) -> Result<Vec<Membership>, LookupFailed>;
}

/// The lookup could not answer; the step grants nothing on it (`membership_unavailable`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LookupFailed;

/// What a session can do (identity spec §6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    /// An ordinary session.
    Full,
    /// A session kept after the provider became unreachable (route 1) or opened by a local passkey
    /// (route 2): pause and the kill switches' engage rows only.
    ReductionOnly,
}

/// An authenticated session (identity spec §4.5, §6.2), built from the session record read for this
/// request: its opaque reference, which every committed event names (ID-1), its kind, and its roles
/// snapshot: the principal's (or a client's user's) memberships that reach their scopes, one per
/// workspace or organization, as of the last successful membership read. A failed read falls back
/// to the snapshot's entry for the request's own scope, and never another's, on a risk-reducing
/// row only (DEC-642 items 9 and 10). Its fields are private, and only §6's code (`mandate-authn`)
/// builds one; until then only this crate's tests do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    reference: SessionRef,
    kind: SessionKind,
    snapshot: Vec<Membership>,
}

/// A granted authorization. Only a workspace's scope carries a [`TenantContext`], which nobody
/// outside this crate can build, so a hand-made `Authorized` reaches no workspace data.
#[derive(Debug, PartialEq, Eq)]
pub enum Authorized {
    /// At the principal's own scope (a `self` row): its own data only, no context.
    Principal {
        /// What the row's **S** cell asks.
        step_up: StepUp,
    },
    /// At an organization's scope.
    Org {
        /// What the row's **S** cell asks.
        step_up: StepUp,
    },
    /// At a workspace's scope.
    Workspace {
        /// What the row's **S** cell asks.
        step_up: StepUp,
        /// The context for the request's data calls.
        tenant: TenantContext,
    },
}

impl Authorized {
    /// What the row's **S** cell asks.
    pub fn step_up(&self) -> StepUp {
        match self {
            Self::Principal { step_up }
            | Self::Org { step_up }
            | Self::Workspace { step_up, .. } => *step_up,
        }
    }
}

/// The closed client scopes of workspace API §3.8 (DEC-436 item 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(
    missing_docs,
    reason = "each variant is the §3.8 scope of the same name"
)]
pub enum ClientScope {
    Read,
    Request,
    Propose,
    DryRun,
    Hold,
}

impl ClientScope {
    /// Every scope, in §3.8's order.
    pub const ALL: [Self; 5] = [
        Self::Read,
        Self::Request,
        Self::Propose,
        Self::DryRun,
        Self::Hold,
    ];

    /// The scope's wire name.
    pub fn code(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Request => "request",
            Self::Propose => "propose",
            Self::DryRun => "dry_run",
            Self::Hold => "hold",
        }
    }
}

/// The step-up action kinds of workspace API §3.6, one per action identity spec ID-4 lists that
/// the API serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(
    missing_docs,
    reason = "each variant is the §3.6 kind of the same name"
)]
pub enum StepUpActionKind {
    ConfirmVersion,
    Deploy,
    Approve,
    Connection,
    Resume,
    Stop,
    Acknowledge,
    OwnerExit,
    KillSwitchPrivilege,
    Disclosure,
    PolicyLoosen,
    MemberInvite,
    RoleGrant,
    ClientConnect,
    CredentialEnrol,
    NotificationAddress,
    BreakGlassApprove,
    LiftHold,
}

impl StepUpActionKind {
    /// Every kind, in §3.6's order.
    pub const ALL: [Self; 18] = [
        Self::ConfirmVersion,
        Self::Deploy,
        Self::Approve,
        Self::Connection,
        Self::Resume,
        Self::Stop,
        Self::Acknowledge,
        Self::OwnerExit,
        Self::KillSwitchPrivilege,
        Self::Disclosure,
        Self::PolicyLoosen,
        Self::MemberInvite,
        Self::RoleGrant,
        Self::ClientConnect,
        Self::CredentialEnrol,
        Self::NotificationAddress,
        Self::BreakGlassApprove,
        Self::LiftHold,
    ];

    /// The kind's wire name.
    pub fn code(self) -> &'static str {
        match self {
            Self::ConfirmVersion => "confirm_version",
            Self::Deploy => "deploy",
            Self::Approve => "approve",
            Self::Connection => "connection",
            Self::Resume => "resume",
            Self::Stop => "stop",
            Self::Acknowledge => "acknowledge",
            Self::OwnerExit => "owner_exit",
            Self::KillSwitchPrivilege => "kill_switch_privilege",
            Self::Disclosure => "disclosure",
            Self::PolicyLoosen => "policy_loosen",
            Self::MemberInvite => "member_invite",
            Self::RoleGrant => "role_grant",
            Self::ClientConnect => "client_connect",
            Self::CredentialEnrol => "credential_enrol",
            Self::NotificationAddress => "notification_address",
            Self::BreakGlassApprove => "break_glass_approve",
            Self::LiftHold => "lift_hold",
        }
    }
}

/// The step-up methods (identity spec §6.1, §7.3): `passkey` anywhere, `cli_confirm` in `paper`
/// only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StepUpMethod {
    /// A WebAuthn assertion with user verification, verified by workspace services.
    Passkey,
    /// The CLI's typed confirmation (DEC-155), `paper` only.
    CliConfirm,
}

impl StepUpMethod {
    /// The method's wire name, as the journal records it.
    pub fn code(self) -> &'static str {
        match self {
            Self::Passkey => "passkey",
            Self::CliConfirm => "cli_confirm",
        }
    }
}

/// A step-up challenge's ID as the journal records it, in its text form (journal spec §9.2,
/// `assertion_id: text`; identity spec §7.2). It has the shape of `mandate-approval`'s
/// `AssertionId`: both crates sit at layer 1, so neither depends on the other, this one serving
/// step-up issuance and verification and that one the runtime; the API converts, and E9-4 pins that
/// the two agree (DEC-644).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssertionId(pub String);

/// Step-up evidence as the journal records it (journal spec §9.2; mandate spec §6.1). Verifying it
/// is §7.2's, not this type's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepUpEvidence {
    /// The challenge the assertion answered.
    pub assertion_id: AssertionId,
    /// When the user verified on the authenticator.
    pub authenticated_at: UtcNanos,
    /// How.
    pub method: StepUpMethod,
}

/// The refusals of identity spec §4.5 (DEC-643).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// No membership reaches the scope, or the principal is outside its column's scope.
    #[error("no membership reaches this scope")]
    NoMembership,
    /// The scope is reached, but nothing grants the row.
    #[error("no role grants this permission")]
    Forbidden,
    /// The row is inactive until a Proposed decision creates its state.
    #[error("this permission is inactive")]
    InactivePermission,
    /// The change grants or removes a role of its own author (ID-13).
    #[error("a principal never changes its own roles")]
    OwnRoles,
    /// Only an org owner grants or removes the org owner role.
    #[error("only an org owner changes the org owner role")]
    OwnerRoleReserved,
    /// A reduction-only session asked for a row other than pause or a kill switch's engage.
    #[error("this session may only pause or engage a kill switch")]
    ReductionOnly,
    /// The membership lookup could not answer, so nothing was granted.
    #[error("memberships could not be read")]
    MembershipUnavailable,
    /// The change leaves the organization with no active org owner.
    #[error("the organization would have no active owner")]
    LastOwner,
    /// The change leaves the workspace with no active workspace admin.
    #[error("the workspace would have no active admin")]
    LastAdmin,
    /// The stub of a story not yet implemented. It goes when E9-2 is implemented, so no caller
    /// matches on it.
    #[error("{story} has not been implemented yet")]
    Unimplemented {
        /// The story.
        story: &'static str,
    },
}

impl Refusal {
    /// The stable code of identity spec §4.5.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NoMembership => "no_membership",
            Self::Forbidden => "forbidden",
            Self::InactivePermission => "inactive_permission",
            Self::ReductionOnly => "reduction_only",
            Self::MembershipUnavailable => "membership_unavailable",
            Self::OwnRoles => "own_roles",
            Self::OwnerRoleReserved => "owner_role_reserved",
            Self::LastOwner => "last_owner",
            Self::LastAdmin => "last_admin",
            Self::Unimplemented { .. } => "unimplemented",
        }
    }
}

/// Applies identity spec §4.2 by its grammar (ID-2, §4.5) at `now`, reading the principal's (or a
/// client's user's) memberships through `lookup` itself; a role counts once its effective-from
/// instant is at or before `now`.
pub fn authorize(
    lookup: &impl MembershipLookup,
    principal: &Principal,
    session: &Session,
    scope: Scope,
    permission: Permission,
    now: UtcNanos,
) -> Result<Authorized, Refusal> {
    let _ = (lookup, principal, session, scope, permission, now);
    Err(Refusal::Unimplemented { story: "E9-2" })
}

/// Checks a role or membership change in `scope` (§4.5): its rows through [`authorize`] (the
/// author's own deactivation is the leave row), then
/// ID-13, the reserved owner role, and the last-owner and last-admin rules on the state after it.
/// It reads every membership of the scope through `lookup`, and judges roles at `now`.
pub fn change_roles(
    lookup: &impl MembershipLookup,
    author: &Principal,
    session: &Session,
    scope: Scope,
    change: &RoleChange,
    now: UtcNanos,
) -> Result<(), Refusal> {
    let _ = (lookup, author, session, scope, change, now);
    Err(Refusal::Unimplemented { story: "E9-2" })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "ADR-0001 ES-09 excepts tests from the safety-critical denies; these tests sit in the \
              crate because `Session`, `Membership`, and `MembershipLookup` are sealed to it (DEC-642)"
)]
mod tests;
