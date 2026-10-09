//! Identity spec §4.2's permission matrix, row by row, read by its grammar (§4.2 "Reading the
//! matrix", DEC-641, DEC-816): each row's **S** cell and the columns that grant it. A blank cell
//! is a denial, so a column absent from a row's list grants nothing (ID-2). The `✓ (org)` cells
//! sit only in org-role columns, which act only at an org's scope, so they read as `✓`; the
//! owner-role exception of `✓ (not owner)` is [`crate::change_roles`]'s. A `self` row lists no
//! column: it is granted only at the principal's scope, outside any membership.

use crate::{Permission, Role, StepUp};

/// A column of §4.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Column {
    /// A role's column, OO to Au.
    Member(Role),
    /// Cl, an owner-connected client.
    Client,
    /// SA, a service account.
    ServiceAccount,
    /// HC, the host CLI.
    HostCli,
    /// PO, a platform operator inside a break-glass window.
    PlatformOperator,
}

const OO: Column = Column::Member(Role::OrgOwner);
const OA: Column = Column::Member(Role::OrgAdmin);
const BILL: Column = Column::Member(Role::BillingAdmin);
const WA: Column = Column::Member(Role::WorkspaceAdmin);
const OP: Column = Column::Member(Role::Operator);
const AP: Column = Column::Member(Role::Approver);
const VI: Column = Column::Member(Role::Viewer);
const AU: Column = Column::Member(Role::Auditor);
const CL: Column = Column::Client;
const SA: Column = Column::ServiceAccount;
const HC: Column = Column::HostCli;
const PO: Column = Column::PlatformOperator;

const NONE: StepUp = StepUp::NotRequired;
const S: StepUp = StepUp::Required;

/// The row's **S** cell and its granting columns.
fn row(permission: Permission) -> (StepUp, &'static [Column]) {
    match permission {
        Permission::ViewAgents => (NONE, &[WA, OP, AP, VI, AU, CL]),
        Permission::OwnerRequest => (NONE, &[OP, CL]),
        Permission::DryRun => (NONE, &[OP, CL]),
        Permission::ChatThread => (NONE, &[OP]),
        Permission::ReadRecords => (NONE, &[WA, AU, SA]),
        Permission::DraftVersion => (NONE, &[OP, CL]),
        Permission::ConfirmReducingOrNeutral => (NONE, &[OP]),
        Permission::ConfirmRiskIncreasing => (S, &[OP]),
        Permission::Deploy => (S, &[OP]),
        Permission::Pause => (NONE, &[WA, OP, AP, HC, PO]),
        Permission::HoldNewOpenings => (NONE, &[WA, OP, CL]),
        Permission::LiftClientHold => (S, &[WA, OP]),
        Permission::ResumeOrStop => (S, &[WA, OP]),
        Permission::KillSwitchAgent => (NONE, &[WA, OP, HC, PO]),
        Permission::KillSwitchConnectionOrWorkspace => (NONE, &[WA, OP, HC, PO]),
        Permission::KillSwitchOrg => (NONE, &[OO, OA]),
        Permission::KillSwitchPrivileges => (S, &[OO, OA, WA, OP]),
        Permission::ReenableHaltedScope => (S, &[OO, OA, WA]),
        Permission::OwnerExit => (S, &[OP]),
        Permission::Acknowledge => (S, &[WA, OP]),
        Permission::Approve => (S, &[AP]),
        Permission::Skip => (NONE, &[AP]),
        Permission::RemoveOrNarrowDelegation => (NONE, &[OP]),
        Permission::BrokerConnection => (S, &[WA]),
        Permission::AcceptDisclosure => (S, &[OP]),
        Permission::WorkspacePolicyTighten => (NONE, &[WA]),
        Permission::WorkspacePolicyLoosen => (S, &[WA]),
        Permission::WorkspaceMembers => (StepUp::ForInvite, &[WA]),
        Permission::WorkspaceRoles => (StepUp::ForGrant, &[WA]),
        Permission::ConnectClient => (S, &[OP]),
        Permission::RevokeClient => (NONE, &[WA, OP]),
        Permission::OwnPasskey => (S, &[OO, OA, BILL, WA, OP, AP, VI, AU]),
        Permission::NotificationAddress => (S, &[WA, OP, AP, VI, AU]),
        Permission::ListOwnNotificationAddresses => (NONE, &[WA, OP, AP, VI, AU]),
        Permission::ListOwnMemberships => (NONE, &[]),
        Permission::Leave => (NONE, &[OO, OA, BILL, WA, OP, AP, VI, AU]),
        Permission::OrgPolicyTighten => (NONE, &[OO, OA]),
        Permission::OrgPolicyLoosen => (S, &[OO, OA]),
        Permission::SsoConfiguration => (S, &[OO, OA]),
        Permission::CreateOrArchiveWorkspace => (S, &[OO, OA]),
        Permission::OrgMemberships => (S, &[OO, OA]),
        Permission::ServiceAccounts => (S, &[OO, OA]),
        Permission::Billing => (NONE, &[OO, BILL]),
        Permission::TransferOrDeleteOrg => (S, &[OO]),
        Permission::ApproveBreakGlass => (S, &[OO, WA]),
        Permission::BreakGlassOperational => (NONE, &[PO]),
    }
}

/// What the row's **S** cell asks.
pub(crate) fn step_up(permission: Permission) -> StepUp {
    row(permission).0
}

/// Whether the row's cell in `column` grants it.
pub(crate) fn grants(permission: Permission, column: Column) -> bool {
    row(permission).1.contains(&column)
}

/// The rows refused to everyone until a Proposed decision creates their state: re-enabling a
/// halted scope (DEC-437 item 21).
pub(crate) fn inactive(permission: Permission) -> bool {
    permission == Permission::ReenableHaltedScope
}

/// The `self` rows, granted only at the principal's scope (DEC-816).
pub(crate) fn self_row(permission: Permission) -> bool {
    permission == Permission::ListOwnMemberships
}

/// The rows that yield a [`crate::PrincipalContext`] at a membership's scope: the `own` rows and
/// the leave row (DEC-832 item 7).
pub(crate) fn own_row(permission: Permission) -> bool {
    matches!(
        permission,
        Permission::OwnPasskey
            | Permission::NotificationAddress
            | Permission::ListOwnNotificationAddresses
            | Permission::Leave
    )
}

/// The rows a reduction-only session reaches (§4.5, §6.4): pause and the kill switches' engage.
pub(crate) fn reduction(permission: Permission) -> bool {
    matches!(
        permission,
        Permission::Pause
            | Permission::KillSwitchAgent
            | Permission::KillSwitchConnectionOrWorkspace
            | Permission::KillSwitchOrg
    )
}

/// The risk-reducing rows that no failed read refuses (§4.5, DEC-642 item 10, `AGENTS.md` rule
/// 13): workspace API-7's operations and ID-5's other never-gated reductions.
pub(crate) fn risk_reducing(permission: Permission) -> bool {
    matches!(
        permission,
        Permission::Pause
            | Permission::HoldNewOpenings
            | Permission::OwnerExit
            | Permission::Skip
            | Permission::RemoveOrNarrowDelegation
            | Permission::KillSwitchAgent
            | Permission::KillSwitchConnectionOrWorkspace
            | Permission::KillSwitchOrg
            | Permission::RevokeClient
            | Permission::WorkspacePolicyTighten
            | Permission::OrgPolicyTighten
    )
}
