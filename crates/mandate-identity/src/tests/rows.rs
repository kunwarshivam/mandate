//! The test's own map from §4.2's rows and columns to the crate's names, written by hand from
//! the spec. A row the spec adds, renames, or drops fails the parse in `matrix.rs`.

use crate::{Permission, Role};

/// Each row's permission text starts with exactly one of these, in the spec's order.
pub(crate) const ROWS: [(&str, Permission); 46] = [
    ("View agents", Permission::ViewAgents),
    ("Make an owner request", Permission::OwnerRequest),
    ("Dry run", Permission::DryRun),
    ("Chat thread", Permission::ChatThread),
    ("Read records", Permission::ReadRecords),
    ("Draft a mandate version", Permission::DraftVersion),
    (
        "Confirm a version: reducing",
        Permission::ConfirmReducingOrNeutral,
    ),
    (
        "Confirm a version: risk-increasing",
        Permission::ConfirmRiskIncreasing,
    ),
    ("Deploy", Permission::Deploy),
    ("Pause", Permission::Pause),
    ("Hold new openings", Permission::HoldNewOpenings),
    ("Lift a hold", Permission::LiftClientHold),
    ("Resume, Stop", Permission::ResumeOrStop),
    ("Kill switch, agent scope", Permission::KillSwitchAgent),
    (
        "Kill switch, connection or workspace",
        Permission::KillSwitchConnectionOrWorkspace,
    ),
    ("Kill switch, org scope", Permission::KillSwitchOrg),
    (
        "Kill switch, any scope: privileges",
        Permission::KillSwitchPrivileges,
    ),
    ("Re-enable a halted scope", Permission::ReenableHaltedScope),
    ("Owner exit", Permission::OwnerExit),
    ("Acknowledge", Permission::Acknowledge),
    ("Answer an approval: approve", Permission::Approve),
    ("Answer an approval: skip", Permission::Skip),
    (
        "Remove or narrow a delegation",
        Permission::RemoveOrNarrowDelegation,
    ),
    (
        "Connect, change, or revoke a broker",
        Permission::BrokerConnection,
    ),
    ("Accept a disclosure", Permission::AcceptDisclosure),
    (
        "Workspace policy: tighten",
        Permission::WorkspacePolicyTighten,
    ),
    (
        "Workspace policy: loosen",
        Permission::WorkspacePolicyLoosen,
    ),
    ("Invite, deactivate, remove", Permission::WorkspaceMembers),
    (
        "Grant or remove a workspace role",
        Permission::WorkspaceRoles,
    ),
    ("Connect a client", Permission::ConnectClient),
    ("Revoke a client", Permission::RevokeClient),
    ("Enrol or remove one's own passkey", Permission::OwnPasskey),
    (
        "Add or remove one's own notification address",
        Permission::NotificationAddress,
    ),
    (
        "List one's own notification addresses",
        Permission::ListOwnNotificationAddresses,
    ),
    (
        "List one's own workspace memberships",
        Permission::ListOwnMemberships,
    ),
    ("Leave: deactivate one's own membership", Permission::Leave),
    ("Org policy: tighten", Permission::OrgPolicyTighten),
    ("Org policy: loosen", Permission::OrgPolicyLoosen),
    ("SSO configuration", Permission::SsoConfiguration),
    (
        "Create or archive a workspace",
        Permission::CreateOrArchiveWorkspace,
    ),
    ("Org memberships and org roles", Permission::OrgMemberships),
    (
        "Issue or revoke a service account",
        Permission::ServiceAccounts,
    ),
    ("Billing", Permission::Billing),
    ("Transfer org ownership", Permission::TransferOrDeleteOrg),
    (
        "Approve a break-glass request",
        Permission::ApproveBreakGlass,
    ),
    ("Restart a process", Permission::BreakGlassOperational),
];

pub(crate) const ROLE_COLUMNS: [(&str, Role); 8] = [
    ("OO", Role::OrgOwner),
    ("OA", Role::OrgAdmin),
    ("Bill", Role::BillingAdmin),
    ("WA", Role::WorkspaceAdmin),
    ("Op", Role::Operator),
    ("Ap", Role::Approver),
    ("Vi", Role::Viewer),
    ("Au", Role::Auditor),
];
pub(crate) const ORG_ROLES: [Role; 3] = [Role::OrgOwner, Role::OrgAdmin, Role::BillingAdmin];

/// Rows the spec gains with PR #766 (Leave, identity spec v0.2, DEC-641 item 5) and PR #811 (the three
/// `self` rows, v0.3, DEC-816), which may be absent from `docs/specs/identity.md` only until those
/// PRs merge; I1 tests part 2, which merges after both, empties this list. Every other row must be
/// in the spec.
pub(crate) const OWED: [Permission; 4] = [
    Permission::Leave,
    Permission::NotificationAddress,
    Permission::ListOwnNotificationAddresses,
    Permission::ListOwnMemberships,
];

/// The §4.2 rows of workspace API-7's risk-reducing operations, mapped by workspace API §3.7 (away
/// mode maps to "Remove or narrow a delegation" there), and ID-5's other never-gated reductions:
/// a failed membership read never refuses them (identity spec §4.5, DEC-642 item 10).
pub(crate) const RISK_REDUCING: [&str; 11] = [
    "Pause",
    "Hold new openings",
    "Owner exit",
    "Answer an approval: skip",
    "Remove or narrow a delegation",
    "Kill switch, agent scope: engage",
    "Kill switch, connection or workspace scope: engage",
    "Kill switch, org scope: engage",
    "Revoke a client",
    "Workspace policy: tighten",
    "Org policy: tighten",
];
