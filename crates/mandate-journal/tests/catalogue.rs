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

/// Journal spec §9, with §2's copies into account streams: `ClockAdvanced`, and `OwnerAcknowledged`
/// as a risk input (mandate spec §5.2, DEC-81).
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
    ("OwnerAcknowledged", &[ACCT, CTL], &[]),
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

/// Payload schemas journal spec v0.6 §9.1 closes on the agent stream (E7-9). On an agent stream a
/// `StreamOpened` or `KillSwitchActivated` takes the agent stream's schema, not the account's.
const CLOSED_ON_AGENT: &[&str] = &[
    "StreamOpened",
    "ObservationRecorded",
    "ModelOutputRecorded",
    "DecisionMade",
    "IntentProposed",
    "AgentModeChanged",
    "KillSwitchActivated",
    "OwnerExitRequested",
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
            if !(kind == AGENT && CLOSED_ON_AGENT.contains(event_type)) {
                assert_eq!(with_all.reason, expected, "{event_type} in {kind}");
            }
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
            "qty_gross":"10","price":"150.000","trade_date":"2026-09-21","risk_clock":"2026-09-21T14:00:01.000000000Z",
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
        "qty_gross":"10","price":"150","trade_date":"2026-02-30","risk_clock":"2026-09-21T14:00:01.000000000Z","fees":[]}"#,
    );
    let e = Draft::parse(&bad_date).unwrap_err();
    assert_eq!(
        (e.reason, e.path.as_str()),
        (InvalidReason::NonCanonical, "payload.trade_date")
    );
    let fill = |clock: &str| {
        with_payload(
            "FillApplied",
            &[FEE, CAL, SET, INS],
            &format!(
                r#"{{"fill_id":"f","client_order_id":"c-1","instrument_id":"i","side":"buy",
                "qty_gross":"10","price":"150","trade_date":"2026-09-21","risk_clock":{clock},"fees":[]}}"#
            ),
        )
    };
    let timed = Draft::parse(&fill("\"2026-09-21T14:00:01.000000000Z\"")).unwrap();
    assert_eq!(
        timed.risk_clock().map(|t| t.secs()),
        Some(1_789_999_201),
        "FillApplied carries risk_clock when present"
    );
    let e = Draft::parse(&fill("\"2026-09-21T14:00:01.250000000Z\"")).unwrap_err();
    assert_eq!(
        (e.reason, e.path.as_str()),
        (InvalidReason::NonCanonical, "payload.risk_clock")
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

/// Mandate spec §5.2 risk inputs, other than the copied `ClockAdvanced`, which is the clock itself.
const RISK_INPUTS: &[&str] = &[
    "MarkUpdated",
    "FillApplied",
    "LateFillApplied",
    "FeesCharged",
    "CorporateActionApplied",
    "CashInLieuPosted",
    "CompensatingEvent",
    "MandateVersionApplied",
    "RiskDayStarted",
    "OwnerAcknowledged",
];

/// A valid payload for every registered risk input; registering another one without adding it here
/// fails `every_registered_risk_input_requires_risk_clock`.
const RISK_INPUT_PAYLOADS: &[(&str, &[&str], &str)] = &[
    (
        "MarkUpdated",
        &[],
        r#"{"instrument_id":"i","price":"1","source":"quote","feed":"iex","risk_clock":"2026-09-21T14:00:01.000000000Z"}"#,
    ),
    (
        "FillApplied",
        &[FEE, CAL, SET, INS],
        r#"{"fill_id":"f","client_order_id":"c-1","instrument_id":"i","side":"buy","qty_gross":"1",
        "price":"1","trade_date":"2026-09-21","risk_clock":"2026-09-21T14:00:01.000000000Z","fees":[]}"#,
    ),
];

#[test]
fn every_registered_risk_input_requires_risk_clock() {
    for event_type in REGISTERED.iter().filter(|t| RISK_INPUTS.contains(t)) {
        let (_, refs, payload) = RISK_INPUT_PAYLOADS
            .iter()
            .find(|(t, _, _)| t == event_type)
            .unwrap_or_else(|| panic!("add a valid {event_type} payload to RISK_INPUT_PAYLOADS"));
        let draft = with_payload(event_type, refs, payload);
        assert!(
            Draft::parse(&draft).is_ok(),
            "{event_type} payload is valid"
        );
        let e = Draft::parse(&edit(&draft, "payload.risk_clock", None)).unwrap_err();
        assert_eq!(
            (e.reason, e.path.as_str()),
            (InvalidReason::Schema, "payload.risk_clock"),
            "{event_type} requires risk_clock (DEC-81)"
        );
    }
}

/// Each schema §9.1 closes is a record with exactly its listed members, so a payload with an
/// unlisted member is refused at that member, before anything else in it is read (§9.1's order).
#[test]
#[ignore = "pending E7-9"]
fn a_closed_agent_stream_schema_refuses_an_unlisted_member() {
    for (event_type, _, required) in SPEC
        .iter()
        .filter(|(event_type, _, _)| CLOSED_ON_AGENT.contains(event_type))
    {
        let refused = Draft::parse(&draft(event_type, AGENT, required))
            .map(|_| ())
            .unwrap_err();
        assert_eq!(
            (refused.reason, refused.path.as_str()),
            (InvalidReason::Schema, "payload.unregistered"),
            "{event_type}"
        );
    }
}
