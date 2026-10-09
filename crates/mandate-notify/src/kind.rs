//! The catalogue (notifications spec §3.1, §3.2, §4.2): every kind, its class, and its text key.

use crate::NotifyError;

/// What a notice asks of its recipient (spec §1.3, §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    /// An approval waits on the recipient.
    Action,
    /// Something that holds, restricts, or protects money happened, or who and what can act on
    /// the account changed. Nothing suppresses it (NT-6).
    Safety,
    /// Nothing the agent may do has changed.
    Info,
}

impl Class {
    /// The class as journaled on `NoticeIssued`: `action`, `safety`, or `info`.
    ///
    /// # Errors
    /// Never: every class has a key.
    pub fn key(self) -> Result<&'static str, NotifyError> {
        Ok(match self {
            Self::Action => "action",
            Self::Safety => "safety",
            Self::Info => "info",
        })
    }
}

/// The closed text set (spec §4.2). Adding a key is a spec change, and no key names a kind more
/// precisely than "approval", "alert", "account change", or "brief".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TextKey {
    ApprovalNeeded,
    AttentionNeeded,
    AccountChanged,
    BriefReady,
}

impl TextKey {
    pub const ALL: [Self; 4] = [
        Self::ApprovalNeeded,
        Self::AttentionNeeded,
        Self::AccountChanged,
        Self::BriefReady,
    ];

    /// The key a payload carries: `approval_needed`, `attention_needed`, `account_changed`, or
    /// `brief_ready`.
    ///
    /// # Errors
    /// Never: every text key has a key.
    pub fn key(self) -> Result<&'static str, NotifyError> {
        Ok(match self {
            Self::ApprovalNeeded => "approval_needed",
            Self::AttentionNeeded => "attention_needed",
            Self::AccountChanged => "account_changed",
            Self::BriefReady => "brief_ready",
        })
    }

    /// The fixed English text every channel renders (spec §4.2).
    ///
    /// # Errors
    /// Never: every text key has its text.
    pub fn text(self) -> Result<&'static str, NotifyError> {
        Ok(match self {
            Self::ApprovalNeeded => "An agent in your workspace needs your approval",
            Self::AttentionNeeded => "Your workspace has a new alert",
            Self::AccountChanged => "There was a change to your account or workspace access",
            Self::BriefReady => "Your daily brief is ready",
        })
    }
}

/// Every row of spec §3.2, in the table's order, which §3.4's "the first row wins" reads. The
/// kind is journaled and shown inside the workspace and never sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NoticeKind {
    ApprovalRequested,
    ApprovalReminder,
    RiskLimit,
    KillSwitch,
    AgentHeld,
    AccountRestriction,
    Protection,
    ExitStalled,
    Reconciliation,
    ExternalActivity,
    AccountState,
    DataFeedDown,
    IntegrityIncident,
    CredentialAdded,
    NewDevice,
    /// A member added or removed one of their own push addresses, with step-up: the only
    /// out-of-band sign that a stolen session added a watcher or silenced the member's safety
    /// notices (spec §3.2, DEC-795 item 6).
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
    ChannelLost,
    DailyBrief,
    DelegationEnded,
    ModelStatus,
    ResearchStatus,
    SpendCap,
    ApprovalClosed,
}

impl NoticeKind {
    #[rustfmt::skip]
    pub const ALL: [Self; 33] = [
        Self::ApprovalRequested, Self::ApprovalReminder, Self::RiskLimit, Self::KillSwitch,
        Self::AgentHeld, Self::AccountRestriction, Self::Protection, Self::ExitStalled,
        Self::Reconciliation, Self::ExternalActivity, Self::AccountState, Self::DataFeedDown,
        Self::IntegrityIncident, Self::CredentialAdded, Self::NewDevice,
        Self::NotificationAddressChanged, Self::RecoveryUsed, Self::RoleGranted,
        Self::MemberDeactivated, Self::Deprovisioned, Self::BreakGlass, Self::VersionRiskIncreasing,
        Self::DelegationAdded, Self::ConnectionAdded, Self::WentLive, Self::ClientConnected,
        Self::ChannelLost, Self::DailyBrief, Self::DelegationEnded, Self::ModelStatus,
        Self::ResearchStatus, Self::SpendCap, Self::ApprovalClosed,
    ];

    /// The kind's key as spec §3.2 writes it, for `OwnerAlertSent` and `NoticeIssued`.
    ///
    /// # Errors
    /// [`NotifyError::Unimplemented`] for [`Self::NotificationAddressChanged`] until E8-9 adds it
    /// (DEC-77); every other kind has a key.
    pub fn key(self) -> Result<&'static str, NotifyError> {
        Ok(match self {
            Self::NotificationAddressChanged => {
                return Err(NotifyError::Unimplemented { story: "E8-9" });
            }
            Self::ApprovalRequested => "approval_requested",
            Self::ApprovalReminder => "approval_reminder",
            Self::RiskLimit => "risk_limit",
            Self::KillSwitch => "kill_switch",
            Self::AgentHeld => "agent_held",
            Self::AccountRestriction => "account_restriction",
            Self::Protection => "protection",
            Self::ExitStalled => "exit_stalled",
            Self::Reconciliation => "reconciliation",
            Self::ExternalActivity => "external_activity",
            Self::AccountState => "account_state",
            Self::DataFeedDown => "data_feed_down",
            Self::IntegrityIncident => "integrity_incident",
            Self::CredentialAdded => "credential_added",
            Self::NewDevice => "new_device",
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
            Self::ChannelLost => "channel_lost",
            Self::DailyBrief => "daily_brief",
            Self::DelegationEnded => "delegation_ended",
            Self::ModelStatus => "model_status",
            Self::ResearchStatus => "research_status",
            Self::SpendCap => "spend_cap",
            Self::ApprovalClosed => "approval_closed",
        })
    }

    /// The kind's class, from spec §3.2's table.
    ///
    /// # Errors
    /// [`NotifyError::Unimplemented`] for [`Self::NotificationAddressChanged`] until E8-9 adds it
    /// (DEC-77); every other kind has a class.
    pub fn class(self) -> Result<Class, NotifyError> {
        Ok(match self {
            Self::NotificationAddressChanged => {
                return Err(NotifyError::Unimplemented { story: "E8-9" });
            }
            Self::ApprovalRequested | Self::ApprovalReminder => Class::Action,
            Self::RiskLimit
            | Self::KillSwitch
            | Self::AgentHeld
            | Self::AccountRestriction
            | Self::Protection
            | Self::ExitStalled
            | Self::Reconciliation
            | Self::ExternalActivity
            | Self::AccountState
            | Self::DataFeedDown
            | Self::IntegrityIncident
            | Self::CredentialAdded
            | Self::NewDevice
            | Self::RecoveryUsed
            | Self::RoleGranted
            | Self::MemberDeactivated
            | Self::Deprovisioned
            | Self::BreakGlass
            | Self::VersionRiskIncreasing
            | Self::DelegationAdded
            | Self::ConnectionAdded
            | Self::WentLive
            | Self::ClientConnected
            | Self::ChannelLost => Class::Safety,
            Self::DailyBrief
            | Self::DelegationEnded
            | Self::ModelStatus
            | Self::ResearchStatus
            | Self::SpendCap
            | Self::ApprovalClosed => Class::Info,
        })
    }

    /// The text a push carries, or `None` for an `info` kind that goes only to the pull channels
    /// and the next brief (spec §3.1).
    ///
    /// # Errors
    /// [`NotifyError::Unimplemented`] for [`Self::NotificationAddressChanged`] until E8-9 adds it
    /// (DEC-77); every other kind has a text key or none.
    pub fn text_key(self) -> Result<Option<TextKey>, NotifyError> {
        Ok(match self {
            Self::NotificationAddressChanged => {
                return Err(NotifyError::Unimplemented { story: "E8-9" });
            }
            Self::ApprovalRequested | Self::ApprovalReminder => Some(TextKey::ApprovalNeeded),
            Self::RiskLimit
            | Self::KillSwitch
            | Self::AgentHeld
            | Self::AccountRestriction
            | Self::Protection
            | Self::ExitStalled
            | Self::Reconciliation
            | Self::ExternalActivity
            | Self::AccountState
            | Self::DataFeedDown
            | Self::IntegrityIncident => Some(TextKey::AttentionNeeded),
            Self::CredentialAdded
            | Self::NewDevice
            | Self::RecoveryUsed
            | Self::RoleGranted
            | Self::MemberDeactivated
            | Self::Deprovisioned
            | Self::BreakGlass
            | Self::VersionRiskIncreasing
            | Self::DelegationAdded
            | Self::ConnectionAdded
            | Self::WentLive
            | Self::ClientConnected
            | Self::ChannelLost => Some(TextKey::AccountChanged),
            Self::DailyBrief => Some(TextKey::BriefReady),
            Self::DelegationEnded
            | Self::ModelStatus
            | Self::ResearchStatus
            | Self::SpendCap
            | Self::ApprovalClosed => None,
        })
    }
}
