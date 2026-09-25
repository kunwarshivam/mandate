//! The event catalogue (journal spec §9), written out again from the spec's tables: which stream
//! types may hold each event type, and which `config_refs` keys each requires at append.

mod common;

use common::{STREAM, T, edit, event_id, mark_draft};
use mandate_journal::{Draft, InvalidReason};

const FEE: &str = "fee_config";
const CAL: &str = "trading_calendar";
const SET: &str = "settlement_calendar";
const INS: &str = "instrument_snapshot";
const RULE: &str = "rule_set";
const MAN: &str = "mandate_version";
const MOD: &str = "model_version";

const ACCT: &str = "acct";
const AGENT: &str = "agent";
const CTL: &str = "ctl";
const CLOCK: &str = "clock";

/// Journal spec §9 (and §2 for `ClockAdvanced`, which the executor copies into account streams).
const SPEC: &[(&str, &[&str], &[&str])] = &[
    ("StreamOpened", &[ACCT, AGENT, CTL, CLOCK], &[]),
    ("IntentReceived", &[ACCT], &[MAN]),
    ("GateDecided", &[ACCT], &[FEE, CAL, INS, RULE, MAN]),
    ("OrderSubmitted", &[ACCT], &[]),
    ("OrderStateChanged", &[ACCT], &[]),
    ("OrderAbandoned", &[ACCT], &[]),
    ("BrokerExchangeRecorded", &[ACCT], &[]),
    ("FillApplied", &[ACCT], &[FEE, CAL, SET, INS]),
    ("LateFillApplied", &[ACCT], &[FEE, CAL, SET, INS]),
    ("FeesCharged", &[ACCT], &[FEE]),
    ("MarkUpdated", &[ACCT], &[]),
    ("SettlementPosted", &[ACCT], &[SET]),
    ("DividendPaid", &[ACCT], &[SET]),
    ("CashInLieuPosted", &[ACCT], &[SET]),
    ("CorporateActionPrepared", &[ACCT], &[INS]),
    ("CorporateActionApplied", &[ACCT], &[INS]),
    ("ProtectionChanged", &[ACCT], &[]),
    ("BrokerPositionObserved", &[ACCT], &[]),
    ("ReconciliationRun", &[ACCT], &[]),
    ("CompensatingEvent", &[ACCT], &[]),
    ("AccountSnapshotRecorded", &[ACCT], &[]),
    ("AccountStateObserved", &[ACCT], &[]),
    ("RejectObserved", &[ACCT], &[]),
    ("AccountRestrictionChanged", &[ACCT], &[]),
    ("ExternalActivityIngested", &[ACCT], &[]),
    ("RelatedAccountsCoordination", &[ACCT], &[]),
    ("ConductBreachDetected", &[ACCT], &[RULE]),
    ("AgentModeApplied", &[ACCT], &[]),
    ("TradingDayStarted", &[ACCT, CLOCK], &[]),
    ("KillSwitchActivated", &[ACCT, AGENT], &[]),
    ("MandateVersionApplied", &[ACCT], &[MAN]),
    ("RiskDayStarted", &[ACCT], &[MAN]),
    ("RiskLimitTriggered", &[ACCT], &[MAN]),
    ("RiskLimitLifted", &[ACCT], &[MAN]),
    ("HighWaterMarkReset", &[ACCT], &[MAN]),
    ("PositionReleased", &[ACCT], &[MAN]),
    ("InstrumentRestrictionChanged", &[ACCT], &[MAN]),
    ("GoalCompleted", &[ACCT], &[MAN]),
    ("ClockAdvanced", &[ACCT, CLOCK], &[]),
    ("ObservationRecorded", &[AGENT], &[]),
    ("ModelInvocationRecorded", &[AGENT], &[MOD]),
    ("ModelOutputRecorded", &[AGENT], &[MAN]),
    ("DecisionMade", &[AGENT], &[MAN]),
    ("IntentProposed", &[AGENT], &[MAN]),
    ("ApprovalRequested", &[AGENT], &[MAN]),
    ("ApprovalDelivered", &[AGENT], &[MAN]),
    ("ApprovalResponded", &[AGENT], &[MAN]),
    ("ApprovalTimedOut", &[AGENT], &[MAN]),
    ("ApprovalCanceled", &[AGENT], &[MAN]),
    ("AgentModeChanged", &[AGENT], &[]),
    ("OwnerExitRequested", &[AGENT], &[MAN]),
    ("MandateVersionCreated", &[CTL], &[]),
    ("MandateConfirmed", &[CTL], &[]),
    ("AgentDeployed", &[CTL], &[MAN]),
    ("DeploymentRejected", &[CTL], &[MAN]),
    ("AgentStopped", &[CTL], &[MAN]),
    ("PolicyChanged", &[CTL], &[]),
    ("WorkspaceProfileAssigned", &[CTL], &[]),
    ("ConnectionEstablished", &[CTL], &[]),
    ("ConnectionRevoked", &[CTL], &[]),
    ("DisclosureAccepted", &[CTL], &[]),
    ("OwnerAlertSent", &[CTL], &[]),
    ("OwnerAcknowledged", &[CTL], &[]),
    ("ConfigSnapshotRegistered", &[CTL], &[]),
    ("SurveillanceReportGenerated", &[CTL], &[RULE]),
    ("BacktestRunRecorded", &[CTL], &[RULE]),
    ("PlatformOperatorAction", &[CTL], &[]),
    ("AnchorComputed", &[CTL], &[]),
    ("VerificationRun", &[CTL], &[]),
    ("IntegrityIncidentRecorded", &[CTL], &[]),
    ("SegmentExported", &[CTL], &[]),
    ("SegmentEvicted", &[CTL], &[]),
    ("RetentionExtended", &[CTL], &[]),
    ("LegalHoldChanged", &[CTL], &[]),
    ("KeyRotated", &[CTL], &[]),
    ("KeyRevoked", &[CTL], &[]),
    ("RecordsAccessed", &[CTL], &[]),
    ("ExportCreated", &[CTL], &[]),
    ("PersonalDataErased", &[CTL], &[]),
    ("ClockOffsetRecorded", &[CLOCK], &[]),
    ("ClockToleranceExceeded", &[CLOCK], &[]),
];

/// Payload schemas registered by E5-1.
const REGISTERED: &[&str] = &[
    "StreamOpened",
    "IntentReceived",
    "GateDecided",
    "OrderSubmitted",
    "FillApplied",
    "MarkUpdated",
];

fn stream_of(kind: &str) -> &'static str {
    match kind {
        ACCT => STREAM,
        AGENT => "agent:ws_1:agent_a",
        CTL => "ctl:ws_1",
        _ => "clock:ws_1",
    }
}

fn refs_json(refs: &[&str]) -> String {
    let members: Vec<String> = refs
        .iter()
        .map(|r| format!("\"{r}\":\"sha256:{}\"", "1".repeat(64)))
        .collect();
    format!("{{{}}}", members.join(","))
}

fn draft(event_type: &str, kind: &str, refs: &[&str]) -> Vec<u8> {
    let d = edit(
        &mark_draft(1, "1"),
        "event_type",
        Some(&format!("\"{event_type}\"")),
    );
    let d = edit(&d, "stream_id", Some(&format!("\"{}\"", stream_of(kind))));
    let d = edit(&d, "config_refs", Some(&refs_json(refs)));
    edit(&d, "payload", Some(r#"{"unregistered":true}"#))
}

#[test]
#[ignore = "pending E5-1"]
fn stream_types_and_required_config_refs_match_the_spec() {
    for (event_type, streams, required) in SPEC {
        for kind in [ACCT, AGENT, CTL, CLOCK] {
            let reason = |refs: &[&str]| {
                Draft::parse(&draft(event_type, kind, refs))
                    .map(|_| ())
                    .unwrap_err()
            };
            let with_all = reason(required);
            if !streams.contains(&kind) {
                assert_eq!(
                    with_all.reason,
                    InvalidReason::WrongStream,
                    "{event_type} in {kind}"
                );
                continue;
            }
            let expected = if REGISTERED.contains(event_type) {
                InvalidReason::Schema
            } else {
                InvalidReason::UnknownSchema
            };
            assert_eq!(with_all.reason, expected, "{event_type} in {kind}");
            for missing in required.iter() {
                let rest: Vec<&str> = required.iter().copied().filter(|r| r != missing).collect();
                let e = reason(&rest);
                assert_eq!(
                    (e.reason, e.path),
                    (
                        InvalidReason::MissingConfigRef,
                        format!("config_refs.{missing}")
                    ),
                    "{event_type} without {missing}"
                );
            }
        }
    }
    let e = Draft::parse(&draft("FillReversed", ACCT, &[])).unwrap_err();
    assert_eq!(e.reason, InvalidReason::UnknownEventType);
}

fn with_payload(event_type: &str, refs: &[&str], payload: &str) -> Vec<u8> {
    let d = draft(event_type, ACCT, refs);
    edit(&d, "payload", Some(payload))
}

#[test]
#[ignore = "pending E5-1"]
fn registered_schemas_accept_their_payloads() {
    let intent = event_id(7);
    let cases = [
        (
            r#""qty":"10""#,
            with_payload(
                "IntentReceived",
                &[MAN],
                &format!(
                    r#"{{"intent_id":"{intent}","agent_id":"agent_a","instrument_id":"i","side":"buy",
                "type":"limit","tif":"day","qty":"10.0","limit_price":null,"purpose":"open"}}"#
                ),
            ),
        ),
        (
            r#""attempt":1"#,
            with_payload(
                "OrderSubmitted",
                &[],
                r#"{"client_order_id":"c-1","attempt":1,"instrument_id":"i","side":"sell",
            "type":"market","tif":"day","qty":"1","limit_price":null}"#,
            ),
        ),
        (
            r#""amount":"0.0001","asset":"USD","kind":"cat""#,
            with_payload(
                "FillApplied",
                &[FEE, CAL, SET, INS],
                r#"{"fill_id":"f","client_order_id":"c-1","instrument_id":"i","side":"buy",
            "qty_gross":"10","price":"150.000","trade_date":"2026-09-21",
            "fees":[{"kind":"cat","amount":"0.00010","asset":"USD","status":"accrued"}]}"#,
            ),
        ),
    ];
    for (expected, bytes) in cases {
        let d = Draft::parse(&bytes).unwrap_or_else(|e| panic!("{e}"));
        let text = String::from_utf8(d.canonical_bytes().to_vec()).unwrap();
        assert!(text.contains(expected), "{expected} in {text}");
    }
    let bad_date = with_payload(
        "FillApplied",
        &[FEE, CAL, SET, INS],
        r#"{"fill_id":"f","client_order_id":"c-1","instrument_id":"i","side":"buy",
        "qty_gross":"10","price":"150","trade_date":"2026-02-30","fees":[]}"#,
    );
    let e = Draft::parse(&bad_date).unwrap_err();
    assert_eq!(
        (e.reason, e.path.as_str()),
        (InvalidReason::NonCanonical, "payload.trade_date")
    );
    let bad_attempt = with_payload(
        "OrderSubmitted",
        &[],
        r#"{"client_order_id":"c-1","attempt":"1","instrument_id":"i","side":"sell",
        "type":"market","tif":"day","qty":"1","limit_price":null}"#,
    );
    let e = Draft::parse(&bad_attempt).unwrap_err();
    assert_eq!(
        (e.reason, e.path.as_str()),
        (InvalidReason::Schema, "payload.attempt")
    );
}

#[test]
#[ignore = "pending E5-1"]
fn artifact_refs_cover_references_nested_in_arrays() {
    let digest = format!("sha256:{}", "a".repeat(64));
    let gate = |artifacts: &str| {
        let payload = format!(
            r#"{{"intent_id":"{}","verdict":"allow","reason_code":null,"data_profile":"iex",
            "quotes_used":[{{"instrument_id":"i","bid":"1","ask":"2","as_of":"{T}","feed":"iex"}}],
            "marks_used":[],"checks":[{{"id":"collar","result":"pass",
            "inputs":{{"evidence":["{digest}"]}},"computed":{{}}}}]}}"#,
            event_id(2)
        );
        let d = with_payload("GateDecided", &[FEE, CAL, INS, RULE, MAN], &payload);
        edit(&d, "artifact_refs", Some(artifacts))
    };
    assert!(Draft::parse(&gate(&format!("[\"{digest}\"]"))).is_ok());
    let e = Draft::parse(&gate("[]")).unwrap_err();
    assert_eq!(e.reason, InvalidReason::ArtifactRefs);
}
