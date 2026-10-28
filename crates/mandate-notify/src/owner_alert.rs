//! The kinds a stream's owner may raise as `OwnerAlertSent` (spec §3.2, §5.5; journal spec §9.15,
//! DEC-2514): every [`NoticeKind`] but the two approval kinds, whose cause is the
//! `ApprovalRequested` itself, and `channel_lost`, the dispatcher's own. The journal refuses those
//! three on an `OwnerAlertSent` as `non_canonical`, so leaving them out of the type means no core
//! can build an alert that would refuse the batch it rides in (`AGENTS.md` rule 13), and an alert
//! with no kind cannot be written at all.

use crate::NoticeKind;

/// One owner-raisable kind. Closed: a new one is a spec change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OwnerAlertKind {
    RiskLimit,
    KillSwitch,
    KillSwitchUnapplied,
    AgentHeld,
    RetirementHeld,
    AccountRestriction,
    ConnectionUnavailable,
    Protection,
    ProtectionUnattributed,
    AttributionDeficit,
    ExitStalled,
    Reconciliation,
    ExternalActivity,
    AccountState,
    DataFeedDown,
    IntegrityIncident,
    CredentialAdded,
    NewDevice,
    NotificationAddressChanged,
    RecoveryUsed,
    RoleGranted,
    MemberDeactivated,
    Deprovisioned,
    BreakGlass,
    VersionRiskIncreasing,
    DelegationAdded,
    ConnectionAdded,
    WentLive,
    ClientConnected,
    DailyBrief,
    DelegationEnded,
    ModelStatus,
    ResearchStatus,
    SpendCap,
    ApprovalClosed,
}

impl OwnerAlertKind {
    #[rustfmt::skip]
    pub const ALL: [Self; 35] = [
        Self::RiskLimit, Self::KillSwitch, Self::KillSwitchUnapplied, Self::AgentHeld,
        Self::RetirementHeld, Self::AccountRestriction, Self::ConnectionUnavailable,
        Self::Protection, Self::ProtectionUnattributed, Self::AttributionDeficit,
        Self::ExitStalled, Self::Reconciliation, Self::ExternalActivity,
        Self::AccountState, Self::DataFeedDown, Self::IntegrityIncident, Self::CredentialAdded,
        Self::NewDevice, Self::NotificationAddressChanged, Self::RecoveryUsed, Self::RoleGranted,
        Self::MemberDeactivated, Self::Deprovisioned, Self::BreakGlass,
        Self::VersionRiskIncreasing, Self::DelegationAdded, Self::ConnectionAdded, Self::WentLive,
        Self::ClientConnected, Self::DailyBrief, Self::DelegationEnded, Self::ModelStatus,
        Self::ResearchStatus, Self::SpendCap, Self::ApprovalClosed,
    ];

    /// The notice kind this is, whose key `OwnerAlertSent` journals.
    #[must_use]
    pub fn notice(self) -> NoticeKind {
        match self {
            Self::RiskLimit => NoticeKind::RiskLimit,
            Self::KillSwitch => NoticeKind::KillSwitch,
            Self::KillSwitchUnapplied => NoticeKind::KillSwitchUnapplied,
            Self::AgentHeld => NoticeKind::AgentHeld,
            Self::RetirementHeld => NoticeKind::RetirementHeld,
            Self::AccountRestriction => NoticeKind::AccountRestriction,
            Self::ConnectionUnavailable => NoticeKind::ConnectionUnavailable,
            Self::Protection => NoticeKind::Protection,
            Self::ProtectionUnattributed => NoticeKind::ProtectionUnattributed,
            Self::AttributionDeficit => NoticeKind::AttributionDeficit,
            Self::ExitStalled => NoticeKind::ExitStalled,
            Self::Reconciliation => NoticeKind::Reconciliation,
            Self::ExternalActivity => NoticeKind::ExternalActivity,
            Self::AccountState => NoticeKind::AccountState,
            Self::DataFeedDown => NoticeKind::DataFeedDown,
            Self::IntegrityIncident => NoticeKind::IntegrityIncident,
            Self::CredentialAdded => NoticeKind::CredentialAdded,
            Self::NewDevice => NoticeKind::NewDevice,
            Self::NotificationAddressChanged => NoticeKind::NotificationAddressChanged,
            Self::RecoveryUsed => NoticeKind::RecoveryUsed,
            Self::RoleGranted => NoticeKind::RoleGranted,
            Self::MemberDeactivated => NoticeKind::MemberDeactivated,
            Self::Deprovisioned => NoticeKind::Deprovisioned,
            Self::BreakGlass => NoticeKind::BreakGlass,
            Self::VersionRiskIncreasing => NoticeKind::VersionRiskIncreasing,
            Self::DelegationAdded => NoticeKind::DelegationAdded,
            Self::ConnectionAdded => NoticeKind::ConnectionAdded,
            Self::WentLive => NoticeKind::WentLive,
            Self::ClientConnected => NoticeKind::ClientConnected,
            Self::DailyBrief => NoticeKind::DailyBrief,
            Self::DelegationEnded => NoticeKind::DelegationEnded,
            Self::ModelStatus => NoticeKind::ModelStatus,
            Self::ResearchStatus => NoticeKind::ResearchStatus,
            Self::SpendCap => NoticeKind::SpendCap,
            Self::ApprovalClosed => NoticeKind::ApprovalClosed,
        }
    }

    /// The key `OwnerAlertSent.kind` journals (journal spec §9.15's `kind`).
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::RiskLimit => "risk_limit",
            Self::KillSwitch => "kill_switch",
            Self::KillSwitchUnapplied => "kill_switch_unapplied",
            Self::AgentHeld => "agent_held",
            Self::RetirementHeld => "retirement_held",
            Self::AccountRestriction => "account_restriction",
            Self::ConnectionUnavailable => "connection_unavailable",
            Self::Protection => "protection",
            Self::ProtectionUnattributed => "protection_unattributed",
            Self::AttributionDeficit => "attribution_deficit",
            Self::ExitStalled => "exit_stalled",
            Self::Reconciliation => "reconciliation",
            Self::ExternalActivity => "external_activity",
            Self::AccountState => "account_state",
            Self::DataFeedDown => "data_feed_down",
            Self::IntegrityIncident => "integrity_incident",
            Self::CredentialAdded => "credential_added",
            Self::NewDevice => "new_device",
            Self::NotificationAddressChanged => "notification_address_changed",
            Self::RecoveryUsed => "recovery_used",
            Self::RoleGranted => "role_granted",
            Self::MemberDeactivated => "member_deactivated",
            Self::Deprovisioned => "deprovisioned",
            Self::BreakGlass => "break_glass",
            Self::VersionRiskIncreasing => "version_risk_increasing",
            Self::DelegationAdded => "delegation_added",
            Self::ConnectionAdded => "connection_added",
            Self::WentLive => "went_live",
            Self::ClientConnected => "client_connected",
            Self::DailyBrief => "daily_brief",
            Self::DelegationEnded => "delegation_ended",
            Self::ModelStatus => "model_status",
            Self::ResearchStatus => "research_status",
            Self::SpendCap => "spend_cap",
            Self::ApprovalClosed => "approval_closed",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    /// Every owner-raisable kind is exactly the catalogue less the three the journal refuses on an
    /// `OwnerAlertSent`, and its key is its notice kind's.
    #[test]
    fn the_owner_kinds_are_the_catalogue_less_the_approval_kinds_and_channel_lost() {
        let refused = [
            NoticeKind::ApprovalRequested,
            NoticeKind::ApprovalReminder,
            NoticeKind::ChannelLost,
        ];
        let owned: BTreeSet<NoticeKind> = OwnerAlertKind::ALL
            .iter()
            .map(|kind| kind.notice())
            .collect();
        let expected: BTreeSet<NoticeKind> = NoticeKind::ALL
            .iter()
            .copied()
            .filter(|kind| !refused.contains(kind))
            .collect();
        assert_eq!(owned, expected);
        assert_eq!(owned.len(), OwnerAlertKind::ALL.len());
        for kind in OwnerAlertKind::ALL {
            assert_eq!(Ok(kind.key()), kind.notice().key(), "{kind:?}");
        }
    }
}
