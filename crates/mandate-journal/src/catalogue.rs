//! The event catalogue (journal spec §9): which streams may hold each event type, and the
//! `config_refs` keys each requires at append.

use crate::StreamType::{self, Account, Agent, Control, Scheduler};

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

/// The catalogue entry for `event_type`. `TradingDayStarted` and `ClockAdvanced` are owned by the
/// scheduler and copied into account streams by the executor (spec §2); `OwnerAcknowledged` is
/// recorded in the control stream and copied into the account stream, where it is a risk input
/// (mandate spec §5.2, DEC-81). `OwnerCommandRefused` is the copy in its place of a resume or Stop
/// the agent runtime refused, or of an acknowledgment the executor refused, for its step-up
/// (mandate spec §6.1); its payload schema is not closed yet (journal spec §9.1).
pub(crate) fn lookup(event_type: &str) -> Option<Entry> {
    let e = match event_type {
        "StreamOpened" => entry(&[Account, Agent, Control, Scheduler], NONE),
        "TradingDayStarted" | "ClockAdvanced" => entry(&[Account, Scheduler], NONE),
        "KillSwitchActivated" => entry(&[Account, Agent], NONE),
        "OwnerAcknowledged" => entry(&[Account, Control], NONE),
        "OwnerCommandRefused" => entry(&[Account, Agent], NONE),

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

        "ModelInvocationRecorded" => entry(AGENT, &[MOD]),
        "ThesisProposed" | "ThesisRevised" => entry(AGENT, &[MAN, MOD]),
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
        "SurveillanceReportGenerated" | "BacktestRunRecorded" => entry(CONTROL, &[RULE]),
        "MandateVersionCreated"
        | "MandateConfirmed"
        | "PolicyChanged"
        | "WorkspaceProfileAssigned"
        | "ConnectionRevoked"
        | "DisclosureAccepted"
        | "OwnerAlertSent"
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
        | "PersonalDataErased" => entry(CONTROL, NONE),

        "ConnectionEstablished" | "ConnectionCredentialRotated" => entry(&[Control, Account], NONE),
        "ConnectionRefused" => entry(CONTROL, NONE),
        "ConnectionChecked" | "ConnectionStateChanged" | "ConnectionCredentialRefreshed" => {
            entry(ACCOUNT, NONE)
        }
        "ClockOffsetRecorded" | "ClockToleranceExceeded" => entry(&[Scheduler], NONE),
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
