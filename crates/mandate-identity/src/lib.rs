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
//! [`Authorized`] for a workspace's scope, or from consuming an [`OrgContext`] or a
//! [`PrincipalContext`] that [`authorize`] built; its fields are private and it has no other
//! constructor (ID-8, DEC-642, DEC-832). None of these compile outside the crate:
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
//! **A sensitive data API can demand the permission too** (DEC-655): it takes a
//! [`demand::Permitted`] witness, which only [`TenantContext::require`] yields, and only for a
//! context authorized for the witness's permission ([`demand`]).
//!
//! **Org scope and one's own data get sealed contexts too** (DEC-832). An org-scope authorization
//! yields an [`OrgContext`], carrying the organization's workspaces as the store enumerated them,
//! never a caller's list; it is not a [`Tenant`], so no data API takes it, and it reaches a
//! workspace only through [`OrgContext::into_workspace`] or [`OrgContext::into_every_workspace`].
//! An `own` row, the leave row, or a `self` row yields a [`PrincipalContext`] bound to the
//! authenticated principal. Neither can be built outside the crate, and an `OrgContext` is not a
//! `Tenant`:
//!
//! ```compile_fail,E0451
//! use mandate_identity::{OrgContext, OrgId, Permission, PrincipalId, PrincipalKind, SessionRef};
//! let _ = OrgContext {
//!     org: OrgId(1),
//!     principal: PrincipalId(3),
//!     kind: PrincipalKind::User,
//!     session: SessionRef(4),
//!     permission: Permission::KillSwitchOrg,
//!     membership_unverified: false,
//!     workspaces: Some(std::collections::BTreeSet::new()),
//! };
//! ```
//!
//! ```compile_fail,E0451
//! use mandate_identity::{
//!     Permission, PrincipalContext, PrincipalId, PrincipalKind, Scope, SessionRef,
//! };
//! let _ = PrincipalContext {
//!     principal: PrincipalId(3),
//!     kind: PrincipalKind::User,
//!     session: SessionRef(4),
//!     permission: Permission::OwnPasskey,
//!     scope: Scope::Principal,
//!     membership_unverified: false,
//! };
//! ```
//!
//! ```compile_fail,E0277
//! fn data_api(_: &impl mandate_identity::Tenant) {}
//! fn org_scope(context: &mandate_identity::OrgContext) {
//!     data_api(context)
//! }
//! ```
//!
//! The control: the same data API takes a `TenantContext`.
//!
//! ```
//! fn data_api(_: &impl mandate_identity::Tenant) {}
//! fn workspace_scope(context: &mandate_identity::TenantContext) {
//!     data_api(context)
//! }
//! ```
//!
//! **A member deactivated before an outage stays refused through it** (DEC-642 item 10's pin, the
//! test `a_deactivated_member_is_refused_through_an_outage`). This crate does not owe it:
//! `MemberDeactivated` revokes the member's sessions and client tokens and suspends their passkey
//! credentials in the workspace store's transaction (E9-7), so the session-record read of §6.2
//! (`mandate-authn`, E9-7) finds no live session and no [`Session`] reaches [`authorize`]; E9-7 owes
//! the test, over the store and the session read, and E9-8 its isolation run with a route-2 attempt.
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
//!     fn workspaces(
//!         &self,
//!         _: mandate_identity::OrgId,
//!     ) -> Result<std::collections::BTreeSet<mandate_identity::WorkspaceId>, LookupFailed> {
//!         Ok(std::collections::BTreeSet::new())
//!     }
//!     fn workspace_org(
//!         &self,
//!         _: mandate_identity::WorkspaceId,
//!     ) -> Result<Option<mandate_identity::OrgId>, LookupFailed> {
//!         Ok(None)
//!     }
//! }
//! ```
//!
//! **Roles change only through their checks** (ID-13, §4.5): [`change_roles`] refuses a change to
//! its author's own roles, the org owner role to anyone but an org owner, and a change that leaves
//! no active org owner or workspace admin (§5.2), and yields the step-up the change needs (ID-4,
//! DEC-654). Refusals carry DEC-643's codes ([`Refusal`]).
//!
//! **Memberships fold from the control stream** (E9-7, DEC-657): [`MembershipFold`] reads
//! journal spec §9.12's records to each member's state and roles and `workspace_users` (§5.3,
//! ID-7).
//!
//! Every entry point is pure: no clock, no randomness, no I/O, ordered collections only.

use std::{
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
};

use mandate_identity_seal::{LookupSeal, Seal};
use mandate_time::UtcNanos;

pub mod demand;
mod matrix;
mod membership;
mod permission;

use matrix::Column;
pub use membership::{
    InvitationId, InvitationState, MembershipEvent, MembershipFold, MembershipRecord,
    RecordRefusal, check_independence,
};
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

/// Why a text is not a ULID's: it must be 26 characters of uppercase Crockford base32, the first at
/// most `7`, as the journal validates it (journal spec §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UlidTextError {
    /// Not 26 characters of uppercase Crockford base32 whose first is at most `7`.
    #[error("not 26 characters of uppercase Crockford base32 whose first is at most 7")]
    Invalid,
}

const ULID_LEN: usize = 26;
const CROCKFORD: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The 26 uppercase Crockford base32 digits of `value`, most significant first. The first digit is
/// at most `7` because 128 bits leave 3 for it.
fn encode_ulid(mut value: u128) -> Result<String, UlidTextError> {
    let mut reversed = String::with_capacity(ULID_LEN);
    for _ in 0..ULID_LEN {
        let digit = usize::try_from(value % 32).map_err(|_| UlidTextError::Invalid)?;
        let ch = CROCKFORD.chars().nth(digit).ok_or(UlidTextError::Invalid)?;
        reversed.push(ch);
        value /= 32;
    }
    Ok(reversed.chars().rev().collect())
}

/// The value of a canonical ULID text: exactly 26 characters of the uppercase Crockford alphabet,
/// the first at most `7`; anything else, such as lowercase, `I`, `L`, `O`, `U` or a space, is
/// [`UlidTextError::Invalid`].
fn decode_ulid(text: &str) -> Result<u128, UlidTextError> {
    if text.chars().count() != ULID_LEN
        || !text.starts_with(['0', '1', '2', '3', '4', '5', '6', '7'])
    {
        return Err(UlidTextError::Invalid);
    }
    text.chars().try_fold(0u128, |acc, ch| {
        let digit = CROCKFORD
            .chars()
            .position(|c| c == ch)
            .and_then(|d| u128::try_from(d).ok())
            .ok_or(UlidTextError::Invalid)?;
        acc.checked_mul(32)
            .and_then(|shifted| shifted.checked_add(digit))
            .ok_or(UlidTextError::Invalid)
    })
}

impl PrincipalId {
    /// The principal's ULID as the journal spells it: 26 characters of uppercase Crockford base32.
    pub fn to_ulid_text(self) -> Result<String, UlidTextError> {
        encode_ulid(self.0)
    }

    /// The principal a ULID's text names, or [`UlidTextError::Invalid`].
    pub fn from_ulid_text(text: &str) -> Result<Self, UlidTextError> {
        decode_ulid(text).map(Self)
    }
}

impl OrgId {
    /// The organization's ULID as the journal spells it: 26 characters of uppercase Crockford base32.
    pub fn to_ulid_text(self) -> Result<String, UlidTextError> {
        encode_ulid(self.0)
    }

    /// The organization a ULID's text names, or [`UlidTextError::Invalid`].
    pub fn from_ulid_text(text: &str) -> Result<Self, UlidTextError> {
        decode_ulid(text).map(Self)
    }
}

impl WorkspaceId {
    /// The workspace's ULID as the journal spells it: 26 characters of uppercase Crockford base32.
    pub fn to_ulid_text(self) -> Result<String, UlidTextError> {
        encode_ulid(self.0)
    }

    /// The workspace a ULID's text names, or [`UlidTextError::Invalid`].
    pub fn from_ulid_text(text: &str) -> Result<Self, UlidTextError> {
        decode_ulid(text).map(Self)
    }
}

impl SessionRef {
    /// The session reference's ULID as the journal spells it: 26 characters of uppercase Crockford base32.
    pub fn to_ulid_text(self) -> Result<String, UlidTextError> {
        encode_ulid(self.0)
    }

    /// The session reference a ULID's text names, or [`UlidTextError::Invalid`].
    pub fn from_ulid_text(text: &str) -> Result<Self, UlidTextError> {
        decode_ulid(text).map(Self)
    }
}

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
    /// Whether it is an org role, acting only at an organization's scope (§4.1).
    fn is_org(self) -> bool {
        matches!(self, Self::OrgOwner | Self::OrgAdmin | Self::BillingAdmin)
    }

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
    /// A workspace's scope, where only workspace roles and the non-member columns act. The pair is
    /// the route's, so [`authorize`] checks it against the workspace's own record in the store
    /// ([`MembershipLookup::workspace_org`]) before anything else but an inactive row: a workspace
    /// this deployment does not host, or one under another organization, is refused
    /// `no_membership` for every principal kind (ID-8, DEC-832 item 2), and a context only ever
    /// carries the pair the store holds, unless the record could not be read; then only a
    /// risk-reducing row is granted, its context flagged `membership_unverified` ([`authorize`]).
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
/// private, and only an allowed dependent of `mandate-identity-seal` builds one: the workspace
/// store, and test support (DEC-642 items 4 and 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Membership {
    member: PrincipalId,
    scope: Scope,
    state: MembershipState,
    roles: BTreeMap<Role, UtcNanos>,
}

impl Membership {
    /// A membership, for a holder of the seal: each role with the instant it becomes effective.
    pub fn new(
        seal: Seal,
        member: PrincipalId,
        scope: Scope,
        state: MembershipState,
        roles: BTreeMap<Role, UtcNanos>,
    ) -> Self {
        let _: Seal = seal;
        Self {
            member,
            scope,
            state,
            roles,
        }
    }

    /// The member.
    pub fn member(&self) -> PrincipalId {
        self.member
    }

    /// The org or workspace it is a membership of.
    pub fn scope(&self) -> Scope {
        self.scope
    }

    /// Whether it holds `role` effective at `now`: its effective-from instant is at or before it.
    fn effective(&self, role: Role, now: UtcNanos) -> bool {
        self.roles.get(&role).is_some_and(|from| *from <= now)
    }
}

/// The membership of `member` that reaches `scope` (`active` or `cooling_off`, §5.1), if any.
fn reaching(memberships: &[Membership], member: PrincipalId, scope: Scope) -> Option<&Membership> {
    memberships.iter().find(|m| {
        m.member == member
            && m.scope == scope
            && matches!(
                m.state,
                MembershipState::Active | MembershipState::CoolingOff
            )
    })
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

    /// Whether this risk-reducing row was authorized without the store's answer: from the session's
    /// roles snapshot because the membership read failed (identity spec §4.5, DEC-642 item 10), or,
    /// when the workspace's own record could not be read, with its pair unchecked by the record
    /// ([`authorize`]); the committed event records it.
    pub fn membership_unverified(&self) -> bool {
        self.membership_unverified
    }

    /// The one permission authorized.
    pub fn permission(&self) -> Permission {
        self.permission
    }

    /// The witness a sensitive data API demands (DEC-655 item 2): `Ok` exactly when this context
    /// was authorized for `P`'s permission, and otherwise [`Refusal::Forbidden`]. The witness
    /// borrows this context, so it reports this context's workspace, organization, and principal.
    pub fn require<P: demand::RequiredPermission>(
        &self,
    ) -> Result<demand::Permitted<'_, P>, Refusal> {
        if self.permission == P::PERMISSION {
            Ok(demand::Permitted {
                context: self,
                demanded: PhantomData,
            })
        } else {
            Err(Refusal::Forbidden)
        }
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
    impl<P: super::demand::RequiredPermission> Sealed for super::demand::Permitted<'_, P> {}
}

/// What an authorization at an organization's scope yields (identity spec §4.5, DEC-832 items 1 to
/// 3). Sealed as [`TenantContext`] is: private fields, built only by [`authorize`], no `Default`,
/// no `Clone`, never stored or serialized. It is not a [`Tenant`], so no data API takes it; it
/// reaches workspace data only by being consumed, through [`Self::into_workspace`] or
/// [`Self::into_every_workspace`], into `TenantContext`s carrying the same principal, permission,
/// `session_ref`, and `membership_unverified`.
#[derive(Debug, PartialEq, Eq)]
pub struct OrgContext {
    org: OrgId,
    principal: PrincipalId,
    kind: PrincipalKind,
    session: SessionRef,
    permission: Permission,
    membership_unverified: bool,
    workspaces: Option<BTreeSet<WorkspaceId>>,
}

impl OrgContext {
    /// The organization authorized.
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

    /// The one permission authorized.
    pub fn permission(&self) -> Permission {
        self.permission
    }

    /// Whether this risk-reducing row was authorized from the session's roles snapshot because the
    /// membership read failed (DEC-642 item 10).
    pub fn membership_unverified(&self) -> bool {
        self.membership_unverified
    }

    /// The organization's workspaces as [`MembershipLookup::workspaces`] enumerated them at
    /// authorization time, never named by the caller; `None` when the membership store could not
    /// answer, so the route's workspace is checked against its own record instead (DEC-832 item 4).
    pub fn workspaces(&self) -> Option<&BTreeSet<WorkspaceId>> {
        self.workspaces.as_ref()
    }

    /// The one-workspace way (DEC-832 item 3): the context for the route's `workspace`, only when it
    /// is in the enumerated set, or, with no set, when its own record names this organization;
    /// otherwise `no_membership`. With no set and the record unreadable too, it is
    /// [`OneWorkspace::Pending`], never a refusal: nothing establishes the route's workspace as this
    /// organization's, and granting it unchecked would let one organization's kill switch reach
    /// another's workspace, so the client retries it with the same idempotency key, showing it
    /// pending, as DEC-832 item 4 retries a workspace not yet committed (identity spec §4.5).
    pub fn into_workspace(
        self,
        lookup: &impl MembershipLookup,
        workspace: WorkspaceId,
    ) -> Result<OneWorkspace, Refusal> {
        let in_org = match &self.workspaces {
            Some(set) => set.contains(&workspace),
            None => match lookup.workspace_org(workspace) {
                Ok(record) => record == Some(self.org),
                Err(LookupFailed) => return Ok(OneWorkspace::Pending),
            },
        };
        match in_org {
            true => Ok(OneWorkspace::Context(self.tenant(workspace))),
            false => Err(Refusal::NoMembership),
        }
    }

    /// The every-workspace way (DEC-832 items 3 and 5): one context per workspace of the enumerated
    /// set, in the set's order, for a write one request makes to all of them; never a set the caller
    /// names and never part of one. With no enumerated set (built during a membership-store outage)
    /// it is [`EveryWorkspace::NoSetYet`], not a refusal: the caller treats it as pending and
    /// retries, never as a denial. No route calls it until a route story decides it (identity spec
    /// §4.5). It never returns `Err` today; the `Result` keeps the signature the route stories use.
    pub fn into_every_workspace(self) -> Result<EveryWorkspace, Refusal> {
        Ok(match &self.workspaces {
            Some(set) => EveryWorkspace::Contexts(set.iter().map(|w| self.tenant(*w)).collect()),
            None => EveryWorkspace::NoSetYet,
        })
    }

    /// The context for one of the organization's workspaces, bound to this context's principal,
    /// session, permission, and flag.
    fn tenant(&self, workspace: WorkspaceId) -> TenantContext {
        TenantContext {
            org: self.org,
            workspace,
            principal: self.principal,
            kind: self.kind,
            session: self.session,
            permission: self.permission,
            membership_unverified: self.membership_unverified,
        }
    }
}

/// What [`OrgContext::into_workspace`] yields when it refuses nothing.
#[derive(Debug, PartialEq, Eq)]
pub enum OneWorkspace {
    /// The route's workspace, which the enumerated set or its own record places in the organization.
    Context(TenantContext),
    /// Neither the enumerated set nor the workspace's own record could be read: not a refusal and
    /// not a grant. The caller retries with the same idempotency key and reports the workspace
    /// pending (DEC-832 item 4).
    Pending,
}

/// What [`OrgContext::into_every_workspace`] yields when it refuses nothing.
#[derive(Debug, PartialEq, Eq)]
pub enum EveryWorkspace {
    /// One context per workspace of the set the store enumerated, in the set's order.
    Contexts(Vec<TenantContext>),
    /// The context was built during a membership-store outage, so no set was enumerated: never a
    /// caller-named or partial set, and never a denial; the caller retries until a set is read.
    NoSetYet,
}

/// What an authorization of one's own data yields (identity spec §4.5, DEC-832 item 7): an `own`
/// row (enrolling or removing one's own passkey), the leave row, or a `self` row at the principal's
/// scope. Sealed as [`TenantContext`] is. It binds the authenticated principal and the scope of the
/// membership the row is used through ([`Scope::Principal`] for a `self` row); the credential-store
/// APIs for one's own passkeys and the leave operation take only it and read the subject from it.
#[derive(Debug, PartialEq, Eq)]
pub struct PrincipalContext {
    principal: PrincipalId,
    kind: PrincipalKind,
    session: SessionRef,
    permission: Permission,
    scope: Scope,
    membership_unverified: bool,
}

impl PrincipalContext {
    /// The authenticated principal, the only subject the context reaches.
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

    /// The one permission authorized.
    pub fn permission(&self) -> Permission {
        self.permission
    }

    /// The scope of the membership the row is used through.
    pub fn scope(&self) -> Scope {
        self.scope
    }

    /// Whether the membership read failed and the session's roles snapshot answered (DEC-642 item
    /// 10).
    pub fn membership_unverified(&self) -> bool {
        self.membership_unverified
    }

    /// The context for its control-stream writes: its workspace's, bound to the same principal and
    /// permission (DEC-832 item 7); `no_membership` at an organization's or the principal's scope,
    /// whose writes are not the request path's.
    pub fn into_tenant(self) -> Result<TenantContext, Refusal> {
        match self.scope {
            Scope::Workspace { org, workspace } => Ok(TenantContext {
                org,
                workspace,
                principal: self.principal,
                kind: self.kind,
                session: self.session,
                permission: self.permission,
                membership_unverified: self.membership_unverified,
            }),
            Scope::Org(_) | Scope::Principal => Err(Refusal::NoMembership),
        }
    }
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

impl<P: demand::RequiredPermission> Tenant for demand::Permitted<'_, P> {
    fn workspace(&self) -> WorkspaceId {
        self.context.workspace()
    }
    fn org(&self) -> OrgId {
        self.context.org()
    }
    fn principal(&self) -> PrincipalId {
        self.context.principal()
    }
    fn kind(&self) -> PrincipalKind {
        self.context.kind()
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
/// It is sealed (DEC-642 items 4 and 7): only an allowed dependent of `mandate-identity-seal`
/// implements it, the workspace store and test support.
pub trait MembershipLookup: LookupSeal {
    /// The memberships the query names.
    fn memberships(&self, query: &MembershipQuery) -> Result<Vec<Membership>, LookupFailed>;

    /// The organization's workspaces (those not archived) that this deployment hosts, from the fold
    /// of the organization's events in its store: the set an [`OrgContext`] carries (DEC-832 item
    /// 2). It fails when the membership store does.
    fn workspaces(&self, org: OrgId) -> Result<BTreeSet<WorkspaceId>, LookupFailed>;

    /// The workspace's own record: the organization it names, or `None` when this deployment hosts
    /// no such workspace. [`authorize`] checks a [`Scope::Workspace`] pair with it, and an
    /// [`OrgContext`] built during a membership-store outage checks the route's workspace with it
    /// (identity spec §4.5, DEC-832 item 4). What a failure of it yields is [`authorize`]'s and
    /// [`OrgContext::into_workspace`]'s to say; neither refuses a risk-reducing row for it.
    fn workspace_org(&self, workspace: WorkspaceId) -> Result<Option<OrgId>, LookupFailed>;
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
/// row only (DEC-642 items 9 and 10). Its fields are private, and only an allowed dependent of
/// `mandate-identity-seal` builds one: §6's code (`mandate-authn`) and test support.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    reference: SessionRef,
    kind: SessionKind,
    snapshot: Vec<Membership>,
}

impl Session {
    /// A session, for a holder of the seal, from the session record read for this request.
    pub fn new(
        seal: Seal,
        reference: SessionRef,
        kind: SessionKind,
        snapshot: Vec<Membership>,
    ) -> Self {
        let _: Seal = seal;
        Self {
            reference,
            kind,
            snapshot,
        }
    }
}

/// A granted authorization. Each arm carries a sealed context nobody outside this crate can build,
/// so a hand-made `Authorized` reaches no data.
#[derive(Debug, PartialEq, Eq)]
pub enum Authorized {
    /// One's own data: a `self` row at the principal's scope, or an `own` row or the leave row at
    /// the scope of the membership it is used through (DEC-832 item 7).
    Principal {
        /// What the row's **S** cell asks.
        step_up: StepUp,
        /// The context bound to the authenticated principal.
        context: PrincipalContext,
    },
    /// Any other row at an organization's scope (DEC-832 items 1 to 3).
    Org {
        /// What the row's **S** cell asks.
        step_up: StepUp,
        /// The context carrying the organization's workspaces from the store.
        context: OrgContext,
    },
    /// Any other row at a workspace's scope.
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
            Self::Principal { step_up, .. }
            | Self::Org { step_up, .. }
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
        }
    }
}

/// Applies identity spec §4.2 by its grammar (ID-2, §4.5) at `now`, reading the principal's (or a
/// client's user's) memberships through `lookup` itself; a role counts once its effective-from
/// instant is at or before `now`.
///
/// What it yields is sealed (DEC-642, DEC-832): a [`TenantContext`] for a workspace's scope, an
/// [`OrgContext`] for an organization's, with the organization's workspaces read through `lookup`
/// at this call, and a [`PrincipalContext`] for an `own`, leave, or `self` row. A
/// [`Scope::Workspace`] whose pair the store does not hold is refused `no_membership`, after an
/// inactive row and before anything else.
///
/// When the workspace's own record cannot be read ([`MembershipLookup::workspace_org`] fails), the
/// pair cannot be checked: a row that is not risk-reducing is refused `membership_unavailable` for
/// every principal kind, and a risk-reducing row is never refused for it (`AGENTS.md` rule 13). A
/// user or a client is then authorized from its session's roles snapshot as DEC-642 item 10 says,
/// the snapshot entry's own pair, which the store folded, standing in for the record; a service
/// account, the host CLI, or a platform operator by its column, whose workspace its own
/// registration, issuance, or window names; each context flagged `membership_unverified`
/// (identity spec §4.5).
pub fn authorize(
    lookup: &impl MembershipLookup,
    principal: &Principal,
    session: &Session,
    scope: Scope,
    permission: Permission,
    now: UtcNanos,
) -> Result<Authorized, Refusal> {
    if matrix::inactive(permission) {
        return Err(Refusal::InactivePermission);
    }
    let record_unread = match scope {
        Scope::Workspace { org, workspace } => match lookup.workspace_org(workspace) {
            Ok(record) if record == Some(org) => false,
            Ok(_) => return Err(Refusal::NoMembership),
            Err(LookupFailed) if matrix::risk_reducing(permission) => true,
            Err(LookupFailed) => return Err(Refusal::MembershipUnavailable),
        },
        Scope::Principal | Scope::Org(_) => false,
    };
    let grant = Grant {
        lookup,
        session,
        scope,
        permission,
        now,
        record_unread,
    };
    let membership_unverified = grant.of(principal)?;
    if session.kind == SessionKind::ReductionOnly && !matrix::reduction(permission) {
        return Err(Refusal::ReductionOnly);
    }
    let (id, kind) = identity(principal);
    let step_up = matrix::step_up(permission);
    if matrix::self_row(permission) || matrix::own_row(permission) {
        let context = PrincipalContext {
            principal: id,
            kind,
            session: session.reference,
            permission,
            scope,
            membership_unverified,
        };
        return Ok(Authorized::Principal { step_up, context });
    }
    match scope {
        Scope::Org(org) => Ok(Authorized::Org {
            step_up,
            context: OrgContext {
                org,
                principal: id,
                kind,
                session: session.reference,
                permission,
                membership_unverified,
                workspaces: lookup.workspaces(org).ok(),
            },
        }),
        Scope::Workspace { org, workspace } => Ok(Authorized::Workspace {
            step_up,
            tenant: TenantContext {
                org,
                workspace,
                principal: id,
                kind,
                session: session.reference,
                permission,
                membership_unverified,
            },
        }),
        Scope::Principal => Err(Refusal::NoMembership),
    }
}

/// The authenticated principal's ID and kind, which every context binds.
fn identity(principal: &Principal) -> (PrincipalId, PrincipalKind) {
    match principal {
        Principal::User { id } => (*id, PrincipalKind::User),
        Principal::Client { id, .. } => (*id, PrincipalKind::Client),
        Principal::ServiceAccount { id, .. } => (*id, PrincipalKind::ServiceAccount),
        Principal::Agent { id } => (*id, PrincipalKind::Agent),
        Principal::Process { id } => (*id, PrincipalKind::Process),
        Principal::HostCli { id, .. } => (*id, PrincipalKind::HostCli),
        Principal::PlatformOperator { id, .. } => (*id, PrincipalKind::PlatformOperator),
    }
}

/// One authorization past its inactive-row and pair checks: whether the row is granted at the
/// scope, and if so whether the store's answer was missing (`membership_unverified`).
struct Grant<'a, L> {
    lookup: &'a L,
    session: &'a Session,
    scope: Scope,
    permission: Permission,
    now: UtcNanos,
    /// The workspace's own record could not be read, on a risk-reducing row.
    record_unread: bool,
}

impl<L: MembershipLookup> Grant<'_, L> {
    /// The principal's column at the scope: users and clients through memberships, the other
    /// columns through what their own record names, and no column for an agent or a process.
    fn of(&self, principal: &Principal) -> Result<bool, Refusal> {
        if self.scope == Scope::Principal {
            return match (matrix::self_row(self.permission), principal) {
                (true, Principal::User { .. }) => Ok(false),
                (true, Principal::Agent { .. } | Principal::Process { .. }) | (false, _) => {
                    Err(Refusal::NoMembership)
                }
                (true, _) => Err(Refusal::Forbidden),
            };
        }
        match principal {
            Principal::User { id } => self.member(*id),
            Principal::Client {
                on_behalf_of,
                workspace,
                ..
            } => {
                if !self.in_workspace(*workspace) {
                    return Err(Refusal::NoMembership);
                }
                let unverified = self.member(*on_behalf_of)?;
                self.column(true, Column::Client).map(|_| unverified)
            }
            Principal::ServiceAccount { workspaces, .. } => self.column(
                workspaces.iter().any(|w| self.in_workspace(*w)),
                Column::ServiceAccount,
            ),
            Principal::HostCli { workspace, .. } => {
                self.column(self.in_workspace(*workspace), Column::HostCli)
            }
            Principal::PlatformOperator { window, .. } => self.column(
                window.is_some_and(|w| self.in_workspace(w)),
                Column::PlatformOperator,
            ),
            Principal::Agent { .. } | Principal::Process { .. } => Err(Refusal::NoMembership),
        }
    }

    /// Whether the scope is `workspace`'s.
    fn in_workspace(&self, workspace: WorkspaceId) -> bool {
        matches!(self.scope, Scope::Workspace { workspace: w, .. } if w == workspace)
    }

    /// A non-member column: `no_membership` outside its scope, `forbidden` where its cell is blank.
    fn column(&self, inside: bool, column: Column) -> Result<bool, Refusal> {
        match (inside, matrix::grants(self.permission, column)) {
            (false, _) => Err(Refusal::NoMembership),
            (true, true) => Ok(self.record_unread),
            (true, false) => Err(Refusal::Forbidden),
        }
    }

    /// The member's own membership at the scope, read live, or from the session's roles snapshot
    /// when the read fails on a risk-reducing row (DEC-642 item 10) or the workspace's record could
    /// not be read; only the scope's kind of role acts there (§4.1).
    fn member(&self, member: PrincipalId) -> Result<bool, Refusal> {
        let live;
        let (memberships, unverified): (&[Membership], bool) = match self.record_unread {
            true => (&self.session.snapshot, true),
            false => match self.lookup.memberships(&MembershipQuery {
                member: Some(member),
                scope: self.scope,
            }) {
                Ok(read) => {
                    live = read;
                    (&live, false)
                }
                Err(LookupFailed) if matrix::risk_reducing(self.permission) => {
                    (&self.session.snapshot, true)
                }
                Err(LookupFailed) => return Err(Refusal::MembershipUnavailable),
            },
        };
        let reaching = reaching(memberships, member, self.scope).ok_or(Refusal::NoMembership)?;
        let org_scope = matches!(self.scope, Scope::Org(_));
        let granted = Role::ALL.into_iter().any(|role| {
            reaching.effective(role, self.now)
                && role.is_org() == org_scope
                && matrix::grants(self.permission, Column::Member(role))
        });
        match granted {
            true => Ok(unverified),
            false => Err(Refusal::Forbidden),
        }
    }
}

/// Checks a role or membership change in `scope` (§4.5): its rows through [`authorize`] (the
/// author's own deactivation is the leave row), then
/// ID-13, the reserved owner role, and the last-owner and last-admin rules on the state after it.
/// It reads every membership of the scope through `lookup`, and judges roles at `now`.
///
/// It yields the step-up the whole change needs, resolved (DEC-654 item 1): [`StepUp::Required`]
/// when a part of it does (a grant at a workspace's scope, ID-4; any change through the org
/// memberships row), otherwise [`StepUp::NotRequired`]; never `ForInvite` or `ForGrant`.
///
/// The rows it uses (DEC-654 item 4): the roles row for a grant or a removal, and for an empty
/// change, so nothing answers a principal the row would refuse; the members row for deactivating
/// another member; the leave row for the author's own deactivation. A grant to a principal with no
/// membership reaching the scope, or a removal from or a deactivation of one with no membership in
/// the scope (none, or one `removed`, `expired`, or `revoked`), is `forbidden`, before `own_roles`
/// (DEC-654 item 7). An active owner or admin is a member `active` in the scope with the role
/// effective at `now`, or granted it in the change (DEC-654 item 2).
pub fn change_roles(
    lookup: &impl MembershipLookup,
    author: &Principal,
    session: &Session,
    scope: Scope,
    change: &RoleChange,
    now: UtcNanos,
) -> Result<StepUp, Refusal> {
    let (roles_row, members_row, admin, last) = match scope {
        Scope::Org(_) => (
            Permission::OrgMemberships,
            Permission::OrgMemberships,
            Role::OrgOwner,
            Refusal::LastOwner,
        ),
        Scope::Workspace { .. } | Scope::Principal => (
            Permission::WorkspaceRoles,
            Permission::WorkspaceMembers,
            Role::WorkspaceAdmin,
            Refusal::LastAdmin,
        ),
    };
    let (me, _) = identity(author);
    let entries = || change.grants.iter().chain(&change.removals);
    let granting = !change.grants.is_empty();
    let rows = [
        (
            entries().next().is_some() || change.deactivations.is_empty(),
            roles_row,
        ),
        (change.deactivations.iter().any(|d| *d != me), members_row),
        (change.deactivations.contains(&me), Permission::Leave),
    ];
    let mut step_up = StepUp::NotRequired;
    for (used, row) in rows {
        if used
            && needs_step_up(
                authorize(lookup, author, session, scope, row, now)?,
                granting,
            )
        {
            step_up = StepUp::Required;
        }
    }
    let org_scope = matches!(scope, Scope::Org(_));
    if entries().any(|(_, role)| role.is_org() != org_scope) {
        return Err(Refusal::Forbidden);
    }
    let memberships: Vec<Membership> = lookup
        .memberships(&MembershipQuery {
            member: None,
            scope,
        })
        .map_err(|LookupFailed| Refusal::MembershipUnavailable)?
        .into_iter()
        .filter(|m| m.scope == scope)
        .collect();
    let mut reduced = change
        .removals
        .iter()
        .map(|(who, _)| who)
        .chain(&change.deactivations);
    if change
        .grants
        .iter()
        .any(|(who, _)| reaching(&memberships, *who, scope).is_none())
        || reduced.any(|who| {
            !memberships.iter().any(|m| {
                m.member == *who
                    && !matches!(
                        m.state,
                        MembershipState::Removed
                            | MembershipState::Expired
                            | MembershipState::Revoked
                    )
            })
        })
    {
        return Err(Refusal::Forbidden);
    }
    if entries().any(|(who, _)| *who == me) {
        return Err(Refusal::OwnRoles);
    }
    let takes_owner = entries().any(|(_, role)| *role == Role::OrgOwner)
        || memberships.iter().any(|m| {
            m.member != me
                && change.deactivations.contains(&m.member)
                && m.roles.contains_key(&Role::OrgOwner)
        });
    let owner = reaching(&memberships, me, scope).is_some_and(|m| m.effective(Role::OrgOwner, now));
    if takes_owner && !owner {
        return Err(Refusal::OwnerRoleReserved);
    }
    let kept = memberships.iter().any(|m| {
        m.state == MembershipState::Active
            && !change.deactivations.contains(&m.member)
            && ((m.effective(admin, now) && !change.removals.contains(&(m.member, admin)))
                || change.grants.contains(&(m.member, admin)))
    });
    match kept {
        true => Ok(step_up),
        false => Err(last),
    }
}

/// Whether a row [`change_roles`] uses asks for step-up for this change: always for `S`, for
/// `S for grant` only when the change grants a role, and never for `S for invite`, since a change
/// invites no one (DEC-654 item 1).
fn needs_step_up(authorized: Authorized, granting: bool) -> bool {
    match authorized.step_up() {
        StepUp::Required => true,
        StepUp::ForGrant => granting,
        StepUp::NotRequired | StepUp::ForInvite => false,
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    reason = "ADR-0001 ES-09 excepts tests from the safety-critical denies; these tests sit in the \
              crate because `Session`, `Membership`, and `MembershipLookup` are sealed to it (DEC-642)"
)]
mod tests;
