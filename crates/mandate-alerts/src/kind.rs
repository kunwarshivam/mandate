//! The closed alert set: one kind per row of the infrastructure design's §8.2 table (two where a
//! row names two conditions with different severities), each with its reason code, severity,
//! runbook, and fixed text.

/// A severity, as the quality and release plan (07) defines them. `Informational` pages no one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Sev1,
    Sev2,
    Sev3,
    Informational,
}

impl Severity {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Sev1 => "sev1",
            Self::Sev2 => "sev2",
            Self::Sev3 => "sev3",
            Self::Informational => "informational",
        }
    }
}

/// Every operator alert there is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AlertKind {
    ReconciliationMismatch,
    UnprotectedIntervalLimit,
    ProtectionUnattributed,
    WatchdogFired,
    KillSwitchActivated,
    KillSwitchAutomatedWorkspace,
    GlobalKillSwitch,
    OrderUnknownPastLookup,
    DuplicateOrder,
    MarketDataStale,
    MarketDataStaleAtScale,
    ModelSpendCap,
    ModelGatewayDegraded,
    JournalAppendDegraded,
    StandbyLost,
    UnexpectedFenced,
    VerificationFailed,
    VerificationIncomplete,
    DdlOutsideRelease,
    ColdExportLag,
    AnchorGap,
    ClockOutOfTolerance,
    VaultLeaseRenewalFailing,
    BrokerDegraded,
    CrashLoop,
    ApprovalNotDelivered,
    BackupFailed,
    DrillOverdueOrFailed,
}

impl AlertKind {
    pub const ALL: [Self; 28] = [
        Self::ReconciliationMismatch,
        Self::UnprotectedIntervalLimit,
        Self::ProtectionUnattributed,
        Self::WatchdogFired,
        Self::KillSwitchActivated,
        Self::KillSwitchAutomatedWorkspace,
        Self::GlobalKillSwitch,
        Self::OrderUnknownPastLookup,
        Self::DuplicateOrder,
        Self::MarketDataStale,
        Self::MarketDataStaleAtScale,
        Self::ModelSpendCap,
        Self::ModelGatewayDegraded,
        Self::JournalAppendDegraded,
        Self::StandbyLost,
        Self::UnexpectedFenced,
        Self::VerificationFailed,
        Self::VerificationIncomplete,
        Self::DdlOutsideRelease,
        Self::ColdExportLag,
        Self::AnchorGap,
        Self::ClockOutOfTolerance,
        Self::VaultLeaseRenewalFailing,
        Self::BrokerDegraded,
        Self::CrashLoop,
        Self::ApprovalNotDelivered,
        Self::BackupFailed,
        Self::DrillOverdueOrFailed,
    ];

    /// The reason code a payload carries.
    pub const fn reason(self) -> &'static str {
        match self {
            Self::ReconciliationMismatch => "reconciliation_mismatch",
            Self::UnprotectedIntervalLimit => "unprotected_interval_limit",
            Self::ProtectionUnattributed => "protection_unattributed",
            Self::WatchdogFired => "watchdog_fired",
            Self::KillSwitchActivated => "kill_switch_activated",
            Self::KillSwitchAutomatedWorkspace => "kill_switch_automated_workspace",
            Self::GlobalKillSwitch => "global_kill_switch",
            Self::OrderUnknownPastLookup => "order_unknown_past_lookup",
            Self::DuplicateOrder => "duplicate_order",
            Self::MarketDataStale => "market_data_stale",
            Self::MarketDataStaleAtScale => "market_data_stale_at_scale",
            Self::ModelSpendCap => "model_spend_cap",
            Self::ModelGatewayDegraded => "model_gateway_degraded",
            Self::JournalAppendDegraded => "journal_append_degraded",
            Self::StandbyLost => "synchronous_standby_lost",
            Self::UnexpectedFenced => "unexpected_fenced",
            Self::VerificationFailed => "journal_verification_failed",
            Self::VerificationIncomplete => "journal_verification_incomplete",
            Self::DdlOutsideRelease => "ddl_outside_release",
            Self::ColdExportLag => "cold_export_lag",
            Self::AnchorGap => "anchor_gap",
            Self::ClockOutOfTolerance => "clock_out_of_tolerance",
            Self::VaultLeaseRenewalFailing => "vault_lease_renewal_failing",
            Self::BrokerDegraded => "broker_degraded",
            Self::CrashLoop => "crash_loop",
            Self::ApprovalNotDelivered => "approval_not_delivered",
            Self::BackupFailed => "backup_failed",
            Self::DrillOverdueOrFailed => "drill_overdue_or_failed",
        }
    }

    pub const fn severity(self) -> Severity {
        match self {
            Self::KillSwitchAutomatedWorkspace
            | Self::GlobalKillSwitch
            | Self::DuplicateOrder
            | Self::VerificationFailed
            | Self::DdlOutsideRelease => Severity::Sev1,
            Self::ProtectionUnattributed
            | Self::MarketDataStale
            | Self::ModelGatewayDegraded
            | Self::AnchorGap => Severity::Sev3,
            Self::KillSwitchActivated | Self::ModelSpendCap | Self::VerificationIncomplete => {
                Severity::Informational
            }
            Self::ReconciliationMismatch
            | Self::UnprotectedIntervalLimit
            | Self::WatchdogFired
            | Self::OrderUnknownPastLookup
            | Self::MarketDataStaleAtScale
            | Self::JournalAppendDegraded
            | Self::StandbyLost
            | Self::UnexpectedFenced
            | Self::ColdExportLag
            | Self::ClockOutOfTolerance
            | Self::VaultLeaseRenewalFailing
            | Self::BrokerDegraded
            | Self::CrashLoop
            | Self::ApprovalNotDelivered
            | Self::BackupFailed
            | Self::DrillOverdueOrFailed => Severity::Sev2,
        }
    }

    /// The runbook (design §8.4), as its identifier; the cell's configuration turns it into a
    /// link.
    pub const fn runbook(self) -> &'static str {
        match self {
            Self::ReconciliationMismatch => "RB-01",
            Self::UnprotectedIntervalLimit | Self::ProtectionUnattributed | Self::WatchdogFired => {
                "RB-02"
            }
            Self::KillSwitchActivated
            | Self::KillSwitchAutomatedWorkspace
            | Self::GlobalKillSwitch => "RB-03",
            Self::OrderUnknownPastLookup | Self::DuplicateOrder => "RB-04",
            Self::MarketDataStale | Self::MarketDataStaleAtScale => "RB-05",
            Self::ModelSpendCap | Self::ModelGatewayDegraded => "RB-06",
            Self::JournalAppendDegraded | Self::StandbyLost => "RB-07",
            Self::UnexpectedFenced => "RB-08",
            Self::VerificationFailed | Self::DdlOutsideRelease => "RB-09",
            Self::VerificationIncomplete | Self::ColdExportLag | Self::AnchorGap => "RB-10",
            Self::ClockOutOfTolerance => "RB-11",
            Self::VaultLeaseRenewalFailing => "RB-12",
            Self::BrokerDegraded => "RB-13",
            Self::CrashLoop => "RB-14",
            Self::ApprovalNotDelivered => "RB-15",
            Self::BackupFailed | Self::DrillOverdueOrFailed => "RB-16",
        }
    }

    /// The fixed text a payload carries: what kind of thing happened, never which instrument,
    /// order, amount, or mandate.
    pub const fn text(self) -> &'static str {
        match self {
            Self::ReconciliationMismatch => "Reconciliation found a mismatch; the agent is paused.",
            Self::UnprotectedIntervalLimit => "An unprotected interval reached its limit.",
            Self::ProtectionUnattributed => "A protective leg is unattributed; openings are held.",
            Self::WatchdogFired => "The triggered-stop watchdog fired.",
            Self::KillSwitchActivated => "A kill switch was activated.",
            Self::KillSwitchAutomatedWorkspace => {
                "An automated kill switch was activated for a workspace."
            }
            Self::GlobalKillSwitch => "The global kill switch was activated.",
            Self::OrderUnknownPastLookup => "An order is unknown beyond its lookup window.",
            Self::DuplicateOrder => "A duplicate order was detected.",
            Self::MarketDataStale => "A market data feed is stale.",
            Self::MarketDataStaleAtScale => "Several market data feeds are stale.",
            Self::ModelSpendCap => "A research model spend cap was reached.",
            Self::ModelGatewayDegraded => "The model gateway is failing or refusing calls.",
            Self::JournalAppendDegraded => "Journal appends are failing or slow.",
            Self::StandbyLost => "The synchronous standby was lost.",
            Self::UnexpectedFenced => "A writer was fenced unexpectedly.",
            Self::VerificationFailed => "Journal verification failed.",
            Self::VerificationIncomplete => "Journal verification is incomplete.",
            Self::DdlOutsideRelease => "A DDL or superuser session ran outside a release.",
            Self::ColdExportLag => "The cold export is lagging.",
            Self::AnchorGap => "An anchor has no timestamp.",
            Self::ClockOutOfTolerance => "The clock is out of tolerance.",
            Self::VaultLeaseRenewalFailing => "Vault lease renewal is failing.",
            Self::BrokerDegraded => "A broker connection is failing, limited, or down.",
            Self::CrashLoop => "A process is crash looping; its agent is paused.",
            Self::ApprovalNotDelivered => "An approval request was not delivered.",
            Self::BackupFailed => "A backup failed.",
            Self::DrillOverdueOrFailed => "A restore drill is overdue or failed.",
        }
    }
}
