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
    /// Never once implemented: every class has a key.
    pub fn key(self) -> Result<&'static str, NotifyError> {
        Err(NotifyError::Unimplemented { story: "E8-9" })
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
    /// Never once implemented.
    pub fn key(self) -> Result<&'static str, NotifyError> {
        Err(NotifyError::Unimplemented { story: "E8-9" })
    }

    /// The fixed English text every channel renders (spec §4.2).
    ///
    /// # Errors
    /// Never once implemented.
    pub fn text(self) -> Result<&'static str, NotifyError> {
        Err(NotifyError::Unimplemented { story: "E8-9" })
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
    pub const ALL: [Self; 32] = [
        Self::ApprovalRequested, Self::ApprovalReminder, Self::RiskLimit, Self::KillSwitch,
        Self::AgentHeld, Self::AccountRestriction, Self::Protection, Self::ExitStalled,
        Self::Reconciliation, Self::ExternalActivity, Self::AccountState, Self::DataFeedDown,
        Self::IntegrityIncident, Self::CredentialAdded, Self::NewDevice, Self::RecoveryUsed,
        Self::RoleGranted, Self::MemberDeactivated, Self::Deprovisioned, Self::BreakGlass,
        Self::VersionRiskIncreasing, Self::DelegationAdded, Self::ConnectionAdded, Self::WentLive,
        Self::ClientConnected, Self::ChannelLost, Self::DailyBrief, Self::DelegationEnded,
        Self::ModelStatus, Self::ResearchStatus, Self::SpendCap, Self::ApprovalClosed,
    ];

    /// The kind's key as spec §3.2 writes it, for `OwnerAlertSent` and `NoticeIssued`.
    ///
    /// # Errors
    /// Never once implemented.
    pub fn key(self) -> Result<&'static str, NotifyError> {
        Err(NotifyError::Unimplemented { story: "E8-9" })
    }

    /// # Errors
    /// Never once implemented.
    pub fn class(self) -> Result<Class, NotifyError> {
        Err(NotifyError::Unimplemented { story: "E8-9" })
    }

    /// The text a push carries, or `None` for an `info` kind that goes only to the pull channels
    /// and the next brief (spec §3.1).
    ///
    /// # Errors
    /// Never once implemented.
    pub fn text_key(self) -> Result<Option<TextKey>, NotifyError> {
        Err(NotifyError::Unimplemented { story: "E8-9" })
    }
}
