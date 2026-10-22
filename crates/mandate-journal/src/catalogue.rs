//! The event catalogue (journal spec §9): which streams may hold each event type, and the
//! `config_refs` keys each requires at append.

use crate::StreamType::{self, Account, Agent, Control, DeploymentMeter, Meter, Notice, Scheduler};

pub(crate) const FEE: &str = "fee_config";
pub(crate) const CAL: &str = "trading_calendar";
pub(crate) const SET: &str = "settlement_calendar";
pub(crate) const INS: &str = "instrument_snapshot";
pub(crate) const RULE: &str = "rule_set";
pub(crate) const MAN: &str = "mandate_version";
pub(crate) const MOD: &str = "model_version";
pub(crate) const POL: &str = "policy_set";
pub(crate) const REG: &str = "model_registry";

/// Every `config_refs` key the spec defines.
pub(crate) const CONFIG_REF_KINDS: &[&str] = &[FEE, CAL, SET, INS, RULE, MAN, MOD, POL, REG];

pub(crate) struct Entry {
    pub(crate) streams: &'static [StreamType],
    pub(crate) required_refs: &'static [&'static str],
}

const fn entry(streams: &'static [StreamType], required_refs: &'static [&'static str]) -> Entry {
    Entry {
        streams,
        required_refs,
    }
}

const ACCOUNT: &[StreamType] = &[Account];
const AGENT: &[StreamType] = &[Agent];
const CONTROL: &[StreamType] = &[Control];
const NONE: &[&str] = &[];

/// Every event type [`lookup`] admits, in its order. The test below reads `lookup`'s source, so
/// a type added there and not here fails the build's tests.
pub(crate) const EVENT_TYPES: [&str; 131] = [
    "StreamOpened",
    "MeterReserved",
    "MeterSettled",
    "TradingDayStarted",
    "ClockAdvanced",
    "KillSwitchActivated",
    "OwnerAcknowledged",
    "OwnerAcknowledgmentRequested",
    "OwnerCommandRefused",
    "StepUpConfirmed",
    "IntentReceived",
    "GateDecided",
    "FillApplied",
    "LateFillApplied",
    "FeesCharged",
    "SettlementPosted",
    "DividendPaid",
    "CashInLieuPosted",
    "CorporateActionPrepared",
    "CorporateActionApplied",
    "ConductBreachDetected",
    "MandateVersionApplied",
    "RiskDayStarted",
    "RiskLimitTriggered",
    "RiskLimitLifted",
    "HighWaterMarkReset",
    "PositionReleased",
    "InstrumentRestrictionChanged",
    "GoalCompleted",
    "UniverseChanged",
    "OrderSubmitted",
    "OrderRequestRecorded",
    "OrderStateChanged",
    "OrderAbandoned",
    "BrokerExchangeRecorded",
    "MarkUpdated",
    "ProtectionChanged",
    "OwnerConfirmationReceived",
    "BrokerPositionObserved",
    "ReconciliationRun",
    "CompensatingEvent",
    "AccountSnapshotRecorded",
    "AccountStateObserved",
    "RejectObserved",
    "AccountRestrictionChanged",
    "ExternalActivityIngested",
    "RelatedAccountsCoordination",
    "AgentModeApplied",
    "ModelInvocationRecorded",
    "ThesisProposed",
    "ThesisRevised",
    "ThesisInvalidated",
    "RunStarted",
    "ToolCallRecorded",
    "ToolResultRecorded",
    "RunSummaryRecorded",
    "RunEnded",
    "ModelOutputRecorded",
    "DecisionMade",
    "IntentProposed",
    "ApprovalRequested",
    "ApprovalDelivered",
    "ApprovalResponded",
    "ApprovalTimedOut",
    "ApprovalCanceled",
    "ApprovalRevalidated",
    "OwnerExitRequested",
    "ObservationRecorded",
    "AgentModeChanged",
    "AgentDeployed",
    "DeploymentRejected",
    "AgentStopped",
    "SurveillanceReportGenerated",
    "EvaluationRunRecorded",
    "BacktestRunRecorded",
    "MandateVersionCreated",
    "MandateConfirmed",
    "PolicyChanged",
    "WorkspaceProfileAssigned",
    "ConnectionRevoked",
    "DisclosureAccepted",
    "ConfigSnapshotRegistered",
    "ApprovalResponseSubmitted",
    "PlatformOperatorAction",
    "AnchorComputed",
    "VerificationRun",
    "IntegrityIncidentRecorded",
    "SegmentExported",
    "SegmentEvicted",
    "RetentionExtended",
    "LegalHoldChanged",
    "KeyRotated",
    "KeyRevoked",
    "RecordsAccessed",
    "ExportCreated",
    "PersonalDataErased",
    "MandateDraftSaved",
    "OwnerRequestSubmitted",
    "ClientConnected",
    "ClientRevoked",
    "OwnerCommandIssued",
    "MemberActivated",
    "ControlPlaneEnrolled",
    "LicenseApplied",
    "LicenseStateChanged",
    "ReleaseOffered",
    "ReleaseInstalled",
    "ReleaseWithdrawnNoticed",
    "CatalogEntryRegistered",
    "DataBundleImported",
    "UsageReportSealed",
    "ControlPlaneMessageRefused",
    "ProcessStartIssued",
    "ProcessStopIssued",
    "ProcessCrashLooped",
    "CredentialEnrolled",
    "CredentialRemoved",
    "StepUpAsserted",
    "ConnectionEstablished",
    "ConnectionCredentialRotated",
    "ConnectionRefused",
    "ConnectionRequested",
    "ConnectionChecked",
    "ConnectionStateChanged",
    "ConnectionCredentialRefreshed",
    "ClockOffsetRecorded",
    "ClockToleranceExceeded",
    "OwnerAlertSent",
    "NoticeIssued",
    "NoticeAttempted",
    "NotificationAddressChanged",
];

/// The catalogue entry for `event_type`. `TradingDayStarted` and `ClockAdvanced` are owned by the
/// scheduler and copied into account streams by the executor (spec §2); `OwnerAcknowledged` is
/// recorded in the control stream and copied into the account stream, where it is a risk input
/// (mandate spec §5.2, DEC-81). `OwnerCommandRefused` is the copy in its place of a resume or Stop
/// the agent runtime refused, or of an acknowledgment the executor refused, for its step-up
/// (mandate spec §6.1); its payload schema is not closed yet (journal spec §9.1).
pub(crate) fn lookup(event_type: &str) -> Option<Entry> {
    let e = match event_type {
        "StreamOpened" => entry(
            &[
                Account,
                Agent,
                Control,
                Scheduler,
                Notice,
                Meter,
                DeploymentMeter,
            ],
            NONE,
        ),
        "MeterReserved" | "MeterSettled" => entry(&[Meter, DeploymentMeter], NONE),
        "TradingDayStarted" | "ClockAdvanced" => entry(&[Account, Scheduler], NONE),
        "KillSwitchActivated" => entry(&[Account, Agent], NONE),
        "OwnerAcknowledged" | "OwnerAcknowledgmentRequested" => entry(&[Account, Control], NONE),
        "OwnerCommandRefused" => entry(&[Account, Agent], NONE),
        "StepUpConfirmed" => entry(CONTROL, NONE),

        "IntentReceived" => entry(ACCOUNT, &[MAN]),
        "GateDecided" => entry(ACCOUNT, &[FEE, CAL, INS, RULE, MAN]),
        "FillApplied" | "LateFillApplied" => entry(ACCOUNT, &[FEE, CAL, SET, INS]),
        "FeesCharged" => entry(ACCOUNT, &[FEE]),
        "SettlementPosted" | "DividendPaid" | "CashInLieuPosted" => entry(ACCOUNT, &[SET]),
        "CorporateActionPrepared" | "CorporateActionApplied" => entry(ACCOUNT, &[INS]),
        "ConductBreachDetected" => entry(ACCOUNT, &[RULE]),
        "MandateVersionApplied"
        | "RiskDayStarted"
        | "RiskLimitTriggered"
        | "RiskLimitLifted"
        | "HighWaterMarkReset"
        | "PositionReleased"
        | "InstrumentRestrictionChanged"
        | "GoalCompleted"
        | "UniverseChanged" => entry(ACCOUNT, &[MAN]),
        "OrderSubmitted"
        | "OrderRequestRecorded"
        | "OrderStateChanged"
        | "OrderAbandoned"
        | "BrokerExchangeRecorded"
        | "MarkUpdated"
        | "ProtectionChanged"
        | "OwnerConfirmationReceived"
        | "BrokerPositionObserved"
        | "ReconciliationRun"
        | "CompensatingEvent"
        | "AccountSnapshotRecorded"
        | "AccountStateObserved"
        | "RejectObserved"
        | "AccountRestrictionChanged"
        | "ExternalActivityIngested"
        | "RelatedAccountsCoordination"
        | "AgentModeApplied" => entry(ACCOUNT, NONE),

        "ModelInvocationRecorded" => entry(&[Agent, Control], &[MOD]),
        "ThesisProposed" | "ThesisRevised" => entry(AGENT, &[MAN, MOD]),
        "ThesisInvalidated" => entry(AGENT, &[MAN]),
        "RunStarted" | "ToolCallRecorded" | "ToolResultRecorded" | "RunSummaryRecorded"
        | "RunEnded" => entry(AGENT, &[MOD]),
        "ModelOutputRecorded"
        | "DecisionMade"
        | "IntentProposed"
        | "ApprovalRequested"
        | "ApprovalDelivered"
        | "ApprovalResponded"
        | "ApprovalTimedOut"
        | "ApprovalCanceled"
        | "ApprovalRevalidated"
        | "OwnerExitRequested" => entry(AGENT, &[MAN]),
        "ObservationRecorded" | "AgentModeChanged" => entry(AGENT, NONE),

        "AgentDeployed" | "DeploymentRejected" | "AgentStopped" => entry(CONTROL, &[MAN]),
        "SurveillanceReportGenerated" | "EvaluationRunRecorded" => entry(CONTROL, &[RULE]),
        "BacktestRunRecorded" => entry(CONTROL, &[RULE, MAN]),
        "MandateVersionCreated"
        | "MandateConfirmed"
        | "PolicyChanged"
        | "WorkspaceProfileAssigned"
        | "ConnectionRevoked"
        | "DisclosureAccepted"
        | "ConfigSnapshotRegistered"
        | "ApprovalResponseSubmitted"
        | "PlatformOperatorAction"
        | "AnchorComputed"
        | "VerificationRun"
        | "IntegrityIncidentRecorded"
        | "SegmentExported"
        | "SegmentEvicted"
        | "RetentionExtended"
        | "LegalHoldChanged"
        | "KeyRotated"
        | "KeyRevoked"
        | "RecordsAccessed"
        | "ExportCreated"
        | "PersonalDataErased"
        | "MandateDraftSaved"
        | "OwnerRequestSubmitted"
        | "ClientConnected"
        | "ClientRevoked"
        | "OwnerCommandIssued"
        | "MemberActivated"
        | "ControlPlaneEnrolled"
        | "LicenseApplied"
        | "LicenseStateChanged"
        | "ReleaseOffered"
        | "ReleaseInstalled"
        | "ReleaseWithdrawnNoticed"
        | "CatalogEntryRegistered"
        | "DataBundleImported"
        | "UsageReportSealed"
        | "ControlPlaneMessageRefused"
        | "ProcessStartIssued"
        | "ProcessStopIssued"
        | "ProcessCrashLooped"
        | "CredentialEnrolled"
        | "CredentialRemoved"
        | "StepUpAsserted" => entry(CONTROL, NONE),

        "ConnectionEstablished" | "ConnectionCredentialRotated" => entry(&[Control, Account], NONE),
        "ConnectionRefused" | "ConnectionRequested" => entry(CONTROL, NONE),
        "ConnectionChecked" | "ConnectionStateChanged" | "ConnectionCredentialRefreshed" => {
            entry(ACCOUNT, NONE)
        }
        "ClockOffsetRecorded" | "ClockToleranceExceeded" => entry(&[Scheduler], NONE),
        "OwnerAlertSent" => entry(&[Account, Agent, Control], NONE),
        "NoticeIssued" | "NoticeAttempted" => entry(&[Notice], NONE),
        "NotificationAddressChanged" => entry(CONTROL, NONE),
        _ => return None,
    };
    Some(e)
}

pub(crate) fn required_refs(
    event_type: &str,
    schema_version: u64,
    historical: &'static [&'static str],
) -> &'static [&'static str] {
    match (event_type, schema_version) {
        ("ModelOutputRecorded", 2) => &[MAN, REG],
        ("DecisionMade", 2) => &[MAN, POL, REG],
        _ => historical,
    }
}

/// [`EVENT_TYPES`] is exactly the set of names [`lookup`] matches, read from this file's source.
#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{EVENT_TYPES, lookup};

    fn names_in_lookup() -> BTreeSet<&'static str> {
        let source = include_str!("catalogue.rs");
        let body = source
            .split_once("pub(crate) fn lookup(")
            .and_then(|(_, rest)| rest.split_once("\n}\n"))
            .map_or("", |(body, _)| body);
        body.split('"')
            .skip(1)
            .step_by(2)
            .filter(|name| name.chars().all(|c| c.is_ascii_alphabetic()))
            .collect()
    }

    #[test]
    fn event_types_lists_every_type_lookup_admits_and_no_other() {
        let listed: BTreeSet<&str> = EVENT_TYPES.iter().copied().collect();
        assert_eq!(
            listed.len(),
            EVENT_TYPES.len(),
            "EVENT_TYPES repeats a type"
        );
        assert!(listed.contains("StreamOpened") && listed.contains("NoticeAttempted"));
        assert_eq!(listed, names_in_lookup());
        for name in EVENT_TYPES {
            assert!(
                lookup(name).is_some(),
                "{name} is listed but lookup refuses it"
            );
        }
    }
}
