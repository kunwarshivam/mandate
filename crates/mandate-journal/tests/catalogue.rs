//! The event catalogue (journal spec §9), written out again from the spec's tables: which stream
//! types may hold each event type, and which `config_refs` keys each requires at append.

mod common;

use common::{STREAM, T, edit, event_id, mark_draft};
use mandate_canon::{Digest, Value, parse, to_canonical};
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
/// `OwnerAlertSent` and the notice stream's records are DEC-720's, written out in `tests/notices.rs`.
const SPEC: &[(&str, &[&str], &[&str])] = &[
    ("StreamOpened", &[ACCT, AGENT, CTL, CLOCK], &[]),
    ("IntentReceived", &[ACCT], &[MAN]),
    ("GateDecided", &[ACCT], &[FEE, CAL, INS, RULE, MAN]),
    ("OrderSubmitted", &[ACCT], &[]),
    ("OrderRequestRecorded", &[ACCT], &[]),
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
    ("OwnerCommandRefused", &[ACCT, AGENT], &[]),
    ("MandateVersionApplied", &[ACCT], &[MAN]),
    ("UniverseChanged", &[ACCT], &[MAN]),
    ("RiskDayStarted", &[ACCT], &[MAN]),
    ("RiskLimitTriggered", &[ACCT], &[MAN]),
    ("RiskLimitLifted", &[ACCT], &[MAN]),
    ("HighWaterMarkReset", &[ACCT], &[MAN]),
    ("PositionReleased", &[ACCT], &[MAN]),
    ("InstrumentRestrictionChanged", &[ACCT], &[MAN]),
    ("GoalCompleted", &[ACCT], &[MAN]),
    ("ClockAdvanced", &[ACCT, CLOCK], &[]),
    ("ObservationRecorded", &[AGENT], &[]),
    ("ThesisProposed", &[AGENT], &[MAN, MOD]),
    ("ThesisRevised", &[AGENT], &[MAN, MOD]),
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
    "ReconciliationRun",
    "MandateVersionApplied",
    "UniverseChanged",
];

/// Payload schemas journal spec v0.6 §9.1 closes on the agent stream (E7-9). On an agent stream a
/// `StreamOpened` or `KillSwitchActivated` takes the agent stream's schema, not the account's.
const CLOSED_ON_AGENT: &[&str] = &[
    "StreamOpened",
    "ObservationRecorded",
    "ModelOutputRecorded",
    "DecisionMade",
    "IntentProposed",
    "ApprovalRequested",
    "ApprovalDelivered",
    "AgentModeChanged",
    "KillSwitchActivated",
    "OwnerExitRequested",
];

/// Payload schemas journal spec v0.7 §9.2 closes (E7-10, DEC-261), with the stream types each is
/// closed on. On a control stream a `StreamOpened` takes the control stream's schema. Not
/// `AccountSnapshotRecorded`, which waits for stream K's fee-step writer (DEC-261 item 7).
const CLOSED_BY_SECTION_9_2: &[(&str, &str)] = &[
    ("StreamOpened", CTL),
    ("ConnectionEstablished", CTL),
    ("ConnectionRevoked", CTL),
    ("DisclosureAccepted", CTL),
    ("ConfigSnapshotRegistered", CTL),
    ("MandateVersionCreated", CTL),
    ("MandateConfirmed", CTL),
    ("AgentDeployed", CTL),
    ("AgentStopped", CTL),
    ("OwnerCommandRefused", ACCT),
    ("OwnerCommandRefused", AGENT),
];

/// The records journal spec v0.17 §9.7 closes (DEC-533), each on its one stream with the
/// configuration it names. Until J2's implementation catalogues and closes them, the journal refuses
/// them as not catalogued or not registered; `the_approval_answers_are_catalogued_and_closed_on_their_streams`
/// asserts what they become, so the table above leaves their schema reason alone.
const CLOSED_BY_SECTION_9_7: &[(&str, &str, &[&str])] = &[
    ("ApprovalResponseSubmitted", CTL, &[]),
    ("ApprovalResponded", AGENT, &[MAN]),
    ("ApprovalRevalidated", AGENT, &[MAN]),
];

/// The connection records journal spec v0.20 §9.8 closes (E7-17, DEC-800), with the stream types
/// each is closed on. `ConnectionEstablished` gains the account stream for the executor's copy, so
/// [`stream_types_and_required_config_refs_match_the_spec`] leaves its streams alone;
/// `the_connection_records_are_catalogued_and_closed_on_their_streams` asserts what each becomes.
const CLOSED_BY_E7_17: &[(&str, &[&str])] = &[
    ("ConnectionRefused", &[CTL]),
    ("ConnectionCredentialRotated", &[CTL, ACCT]),
    ("ConnectionChecked", &[ACCT]),
    ("ConnectionStateChanged", &[ACCT]),
    ("ConnectionCredentialRefreshed", &[ACCT]),
];

/// The control-stream records journal spec v0.21 and v0.22 close (§9.9 and §9.10, DEC-670 and
/// DEC-671), with the configuration each names. `ModelInvocationRecorded` is also catalogued on the
/// agent stream, where it stays open, so it is kept out of [`SPEC`] until E10-15's implementation;
/// `the_workspace_api_records_are_catalogued_and_closed_on_the_control_stream` asserts what each
/// becomes.
const CLOSED_BY_E10_15: &[(&str, &[&str])] = &[
    ("MandateDraftSaved", &[]),
    ("ModelInvocationRecorded", &[MOD]),
    ("OwnerRequestSubmitted", &[]),
    ("ClientConnected", &[]),
    ("ClientRevoked", &[]),
];

/// The account stream's snapshot, which §9.2 closes with rule 24 and its registration routes there
/// (DEC-402). It is kept apart from the eleven pairs until that registration is implemented.
const SNAPSHOT_ON_ACCOUNT: (&str, &str) = ("AccountSnapshotRecorded", ACCT);

/// The account stream's risk-state records journal spec §9.3 closes (DEC-403, DEC-404).
const RISK_STATE_ON_ACCOUNT: [(&str, &str); 2] =
    [("MandateVersionApplied", ACCT), ("UniverseChanged", ACCT)];

/// The agent stream's research-agent thesis records journal spec §9.4 closes (DEC-413, DEC-414).
const THESIS_ON_AGENT: [(&str, &str); 2] = [("ThesisProposed", AGENT), ("ThesisRevised", AGENT)];

/// The account stream's closed executor records (DEC-446, DEC-447, DEC-459, DEC-460).
const EXECUTOR_ON_ACCOUNT: [(&str, &str); 9] = [
    ("IntentReceived", ACCT),
    ("GateDecided", ACCT),
    ("OrderSubmitted", ACCT),
    ("OrderRequestRecorded", ACCT),
    ("OrderStateChanged", ACCT),
    ("ProtectionChanged", ACCT),
    ("BrokerPositionObserved", ACCT),
    ("AgentModeApplied", ACCT),
    ("CompensatingEvent", ACCT),
];

fn closed_by_section_9_2(event_type: &str, kind: &str) -> bool {
    CLOSED_BY_SECTION_9_2.contains(&(event_type, kind))
        || (event_type, kind) == SNAPSHOT_ON_ACCOUNT
        || (event_type, kind) == ("AccountStateObserved", ACCT)
        || RISK_STATE_ON_ACCOUNT.contains(&(event_type, kind))
        || THESIS_ON_AGENT.contains(&(event_type, kind))
        || EXECUTOR_ON_ACCOUNT.contains(&(event_type, kind))
}

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
        if *event_type == "ConnectionEstablished" {
            continue;
        }
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
            let section_9_7 = CLOSED_BY_SECTION_9_7
                .iter()
                .any(|(t, _, _)| t == event_type);
            if !(kind == AGENT && CLOSED_ON_AGENT.contains(event_type))
                && !closed_by_section_9_2(event_type, kind)
                && !section_9_7
            {
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

fn with_agent_payload(event_type: &str, payload: &str) -> Vec<u8> {
    let d = draft(event_type, AGENT, &[MAN]);
    edit(&d, "payload", Some(payload))
}

fn approval_requested() -> Vec<u8> {
    let mandate = format!("sha256:{}", "1".repeat(64));
    let body = with_agent_payload(
        "ApprovalRequested",
        &format!(
            r#"{{
            "instrument":"AAPL","asset_class":"us_equity","side":"buy","qty":"2",
            "limit":"150","purpose":"open","mandate_version":"{mandate}",
            "decided_by":"rule:open","combined_score":"0.75",
            "reference_mark":{{"price":"149.5","seq":7}},
            "approvers_required":1,"independent_required":false,
            "deadline":1789999200,"timeout_s":300,"on_timeout":"skip",
            "content":{{
                "action":{{"instrument":"AAPL","asset_class":"us_equity","side":"buy",
                    "qty":"2","limit":"150","order_usd":"300","purpose":"open"}},
                "trigger":{{"mandate_version":"{mandate}","decided_by":"rule:open"}},
                "evidence":{{
                    "combined_score":{{"label":"combined model score, not a probability of profit",
                        "value":"0.75"}},
                    "outputs":[{{"event_id":"{}","artifact":null,
                        "label":"Output of software you selected"}}]
                }},
                "risk_impact":[{{"field":"order_usd","value":"300","cap":null}}],
                "reference_mark":{{"price":"149.5","seq":7}},
                "deadline":"{T}",
                "default":"If you do nothing, this action is skipped",
                "choices":["approve","skip"],
                "approvers":{{"required":1,"independent":false}}
            }},
            "content_hash":"sha256:{}"
        }}"#,
            event_id(8),
            "0".repeat(64)
        ),
    );
    let value = parse(&body).expect("the approval fixture is JSON");
    let content = value
        .get("payload")
        .and_then(|payload| payload.get("content"))
        .expect("the approval fixture has content");
    let content_hash = format!("sha256:{}", Digest::of(&to_canonical(content)));
    let body = edit(
        &body,
        "payload.content_hash",
        Some(&format!("\"{content_hash}\"")),
    );
    let mut refs = [mandate, content_hash];
    refs.sort();
    edit(
        &body,
        "artifact_refs",
        Some(&format!(r#"["{}","{}"]"#, refs[0], refs[1])),
    )
}

fn approval_delivered() -> Vec<u8> {
    with_agent_payload(
        "ApprovalDelivered",
        &format!(
            r#"{{"approval":"{}","channel":"cli_inbox","status":"delivered","message_id":null}}"#,
            event_id(8)
        ),
    )
}

fn broker_position_observed() -> Vec<u8> {
    with_payload(
        "BrokerPositionObserved",
        &[],
        &format!(
            r#"{{"instrument":"AAPL","broker_qty":"7","model_qty":"10","mismatch":true,
            "risk_clock":"{T}"}}"#
        ),
    )
}

fn agent_mode_applied() -> Vec<u8> {
    with_payload(
        "AgentModeApplied",
        &[],
        &format!(
            r#"{{"agent":"*","to":"paused","restriction":"reconciliation:AAPL",
            "originated":true,"risk_clock":"{T}"}}"#
        ),
    )
}

fn compensating_event() -> Vec<u8> {
    with_payload(
        "CompensatingEvent",
        &[],
        &format!(
            r#"{{"subject":"md-order-1","difference":"order_state","from":"submitting",
            "to":"accepted","corrected_event_ids":["{}"],"risk_clock":"{T}"}}"#,
            event_id(9)
        ),
    )
}

#[derive(Clone)]
enum ValuePath {
    Key(String),
    Index(usize),
}

fn member_paths(value: &Value, prefix: Vec<ValuePath>, out: &mut Vec<Vec<ValuePath>>) {
    match value {
        Value::Object(members) => {
            for (key, member) in members {
                let mut path = prefix.clone();
                path.push(ValuePath::Key(key.as_str().to_owned()));
                out.push(path.clone());
                member_paths(member, path, out);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let mut path = prefix.clone();
                path.push(ValuePath::Index(index));
                member_paths(item, path, out);
            }
        }
        Value::Null | Value::Bool(_) | Value::Int(_) | Value::Str(_) => {}
    }
}

fn remove_member(value: &mut Value, path: &[ValuePath]) {
    match path {
        [ValuePath::Key(key)] => {
            let Value::Object(object) = value else {
                panic!("the path names an object")
            };
            object.remove(key.as_str()).expect("the member exists");
        }
        [ValuePath::Key(key), rest @ ..] => {
            let Value::Object(object) = value else {
                panic!("the path names an object")
            };
            remove_member(
                object
                    .get_mut(key.as_str())
                    .expect("the object member exists"),
                rest,
            );
        }
        [ValuePath::Index(index), rest @ ..] => {
            let Value::Array(array) = value else {
                panic!("the path names an array")
            };
            remove_member(
                array.get_mut(*index).expect("the array member exists"),
                rest,
            );
        }
        [] => unreachable!("a member path ends in an object key"),
    }
}

fn path_text(path: &[ValuePath]) -> String {
    let mut text = String::new();
    for part in path {
        match part {
            ValuePath::Key(key) => {
                if !text.is_empty() {
                    text.push('.');
                }
                text.push_str(key);
            }
            ValuePath::Index(index) => text.push_str(&format!("[{index}]")),
        }
    }
    text
}

fn assert_every_payload_member_is_required(bytes: &[u8]) {
    let mut paths = Vec::new();
    let value = parse(bytes).expect("the fixture is JSON");
    let payload = value.get("payload").expect("the fixture has a payload");
    member_paths(
        payload,
        vec![ValuePath::Key("payload".to_owned())],
        &mut paths,
    );
    for path in paths {
        let mut planted = value.clone();
        remove_member(&mut planted, &path);
        let refusal = Draft::parse(&to_canonical(&planted))
            .expect_err("every member of a closed object is required");
        assert_eq!(
            (refusal.reason, refusal.path),
            (InvalidReason::Schema, path_text(&path)),
            "{}",
            path_text(&path)
        );
    }
}

#[test]
fn remaining_tracer_events_are_closed_and_every_member_is_required() {
    for complete in [
        approval_requested(),
        approval_delivered(),
        broker_position_observed(),
        agent_mode_applied(),
        compensating_event(),
    ] {
        assert_eq!(Draft::parse(&complete).map(|_| ()), Ok(()));
        assert_every_payload_member_is_required(&complete);
        let extra = edit(&complete, "payload.unregistered", Some("true"));
        let refusal = Draft::parse(&extra).expect_err("the payload is closed");
        assert_eq!(
            (refusal.reason, refusal.path.as_str()),
            (InvalidReason::Schema, "payload.unregistered")
        );
    }
}

#[test]
fn approval_schemas_pin_nested_types_vocabularies_and_canonical_content() {
    let requested = approval_requested();
    for (path, invalid) in [
        ("asset_class", "\"equity\""),
        ("side", "\"sell\""),
        ("qty", "\"two\""),
        ("purpose", "\"risk_exit\""),
        ("reference_mark.seq", "\"7\""),
        ("approvers_required", "\"1\""),
        ("independent_required", "0"),
        ("deadline", "\"2026-09-21T14:00:00.000000000Z\""),
        ("timeout_s", "\"300\""),
        ("on_timeout", "\"deny\""),
        ("content.action.asset_class", "\"equity\""),
        ("content.action.side", "\"sell\""),
        ("content.action.order_usd", "\"three hundred\""),
        ("content.action.purpose", "\"risk_exit\""),
        ("content.deadline", "\"2026-09-21T14:00:00.100000000Z\""),
        ("content.evidence.outputs", "{}"),
        ("content.risk_impact", "{}"),
        ("content.approvers.required", "\"1\""),
    ] {
        let planted = edit(&requested, &format!("payload.{path}"), Some(invalid));
        let refusal = Draft::parse(&planted).expect_err("the approval member type is exact");
        assert!(
            matches!(
                refusal.reason,
                InvalidReason::Schema | InvalidReason::NonCanonical
            ),
            "{path}: {refusal:?}"
        );
    }
    for (path, valid) in [
        ("asset_class", "\"crypto\""),
        ("purpose", "\"increase\""),
        ("content.action.asset_class", "\"crypto\""),
        ("content.action.purpose", "\"increase\""),
        (
            "content.evidence.outputs",
            &format!(
                r#"[{{"event_id":"{}","artifact":"sha256:{}","label":"platform-authored"}}]"#,
                event_id(7),
                "3".repeat(64)
            ),
        ),
        (
            "content.risk_impact",
            r#"[{"field":"daily_pnl_fraction","value":"0.1","cap":"0.2"}]"#,
        ),
    ] {
        let planted = edit(&requested, &format!("payload.{path}"), Some(valid));
        let parsed = Draft::parse(&planted);
        assert!(
            parsed.is_err(),
            "changing canonical content without its hash must be refused: {path}"
        );
    }
    let delivered = approval_delivered();
    for status in ["delivered", "suppressed_quiet_hours", "failed"] {
        let draft = edit(&delivered, "payload.status", Some(&format!("\"{status}\"")));
        assert_eq!(Draft::parse(&draft).map(|_| ()), Ok(()), "{status}");
    }
    for (path, value) in [
        ("channel", "\"email\""),
        ("status", "\"sent\""),
        ("approval", "\"not-a-ulid\""),
        ("message_id", "\"\""),
    ] {
        let draft = edit(&delivered, &format!("payload.{path}"), Some(value));
        assert!(Draft::parse(&draft).is_err(), "{path}");
    }
}

#[test]
fn reconciliation_schemas_pin_types_and_complete_vocabularies() {
    let position = broker_position_observed();
    for (path, value) in [
        ("broker_qty", "\"seven\""),
        ("model_qty", "10"),
        ("mismatch", "\"true\""),
        ("risk_clock", "\"2026-09-21T14:00:00.100000000Z\""),
    ] {
        let draft = edit(&position, &format!("payload.{path}"), Some(value));
        assert!(Draft::parse(&draft).is_err(), "{path}");
    }
    let mode = agent_mode_applied();
    for state in ["normal", "exits_only", "paused", "stopped"] {
        let draft = edit(&mode, "payload.to", Some(&format!("\"{state}\"")));
        assert_eq!(Draft::parse(&draft).map(|_| ()), Ok(()), "{state}");
    }
    for (path, value) in [
        ("agent", "\"\""),
        ("to", "\"unknown\""),
        ("restriction", "\"\""),
        ("originated", "\"true\""),
    ] {
        let draft = edit(&mode, &format!("payload.{path}"), Some(value));
        assert!(Draft::parse(&draft).is_err(), "{path}");
    }
    let compensation = compensating_event();
    for member in ["from", "to"] {
        for state in [
            "intent",
            "submitting",
            "accepted",
            "partially_filled",
            "pending_cancel",
            "pending_replace",
            "unknown",
            "filled",
            "canceled",
            "rejected",
            "expired",
            "replaced",
            "abandoned",
        ] {
            let draft = edit(
                &compensation,
                &format!("payload.{member}"),
                Some(&format!("\"{state}\"")),
            );
            assert_eq!(Draft::parse(&draft).map(|_| ()), Ok(()), "{member}");
        }
    }
    for (path, value) in [
        ("difference", "\"quantity\""),
        ("from", "\"new\""),
        ("to", "\"new\""),
        ("corrected_event_ids", r#"["not-a-ulid"]"#),
    ] {
        let draft = edit(&compensation, &format!("payload.{path}"), Some(value));
        assert!(Draft::parse(&draft).is_err(), "{path}");
    }
}

fn account_state_observed() -> Vec<u8> {
    with_payload(
        "AccountStateObserved",
        &[],
        &format!(
            r#"{{"status":"ACTIVE","crypto_status":"ACTIVE","trading_blocked":false,
            "account_blocked":false,"trade_suspended_by_user":false,"multiplier":2,
            "equity":"1000","cash":"800","buying_power":"2000",
            "non_marginable_buying_power":"800","accrued_fees":"0",
            "risk_clock":"{T}"}}"#
        ),
    )
}

fn order_state_changed() -> Vec<u8> {
    with_payload(
        "OrderStateChanged",
        &[],
        &format!(
            r#"{{"client_order_id":"md-order-1","state":"accepted","attempted":null,
            "broker_status":null,"filled_qty":null,"reject_code":null,"replaces":null,
            "replaced_by":null,"replaced_by_broker_order_id":null,"lookup":null,
            "ignored":false,"cancel_requested":false,"cancel_confirmed":false,
            "cancel_overdue":false,"adopted":false,"ladder_step":false,"risk_clock":"{T}"}}"#
        ),
    )
}

#[test]
fn order_state_changed_is_closed_and_every_member_is_required() {
    let complete = order_state_changed();
    assert_eq!(Draft::parse(&complete).map(|_| ()), Ok(()));
    for field in [
        "client_order_id",
        "state",
        "attempted",
        "broker_status",
        "filled_qty",
        "reject_code",
        "replaces",
        "replaced_by",
        "replaced_by_broker_order_id",
        "lookup",
        "ignored",
        "cancel_requested",
        "cancel_confirmed",
        "cancel_overdue",
        "adopted",
        "ladder_step",
        "risk_clock",
    ] {
        let missing = edit(&complete, &format!("payload.{field}"), None);
        let refusal = Draft::parse(&missing).expect_err("every transition member is required");
        assert_eq!(
            (refusal.reason, refusal.path),
            (InvalidReason::Schema, format!("payload.{field}")),
            "{field}"
        );
    }
    let extra = edit(&complete, "payload.unregistered", Some("true"));
    let refusal = Draft::parse(&extra).expect_err("the transition payload is closed");
    assert_eq!(
        (refusal.reason, refusal.path.as_str()),
        (InvalidReason::Schema, "payload.unregistered")
    );
}

#[test]
fn order_state_changed_catches_a_planted_writer_omission() {
    let without_ladder_marker = edit(&order_state_changed(), "payload.ladder_step", None);
    let refusal =
        Draft::parse(&without_ladder_marker).expect_err("a writer omitted a required member");
    assert_eq!(
        (refusal.reason, refusal.path.as_str()),
        (InvalidReason::Schema, "payload.ladder_step")
    );
}

#[test]
fn order_state_changed_covers_every_state_and_nullable_evidence_branch() {
    let complete = order_state_changed();
    for field in ["state", "attempted"] {
        for state in [
            "intent",
            "submitting",
            "accepted",
            "partially_filled",
            "pending_cancel",
            "pending_replace",
            "unknown",
            "filled",
            "canceled",
            "rejected",
            "expired",
            "replaced",
            "abandoned",
        ] {
            let value = format!("\"{state}\"");
            let draft = edit(&complete, &format!("payload.{field}"), Some(&value));
            assert_eq!(Draft::parse(&draft).map(|_| ()), Ok(()), "{field}={state}");
        }
    }
    for (field, value) in [
        ("broker_status", "\"accepted\""),
        ("filled_qty", "\"1.25\""),
        ("reject_code", "\"insufficient_buying_power\""),
        ("replaces", "\"md-order-0\""),
        ("replaced_by", "\"md-order-2\""),
        ("replaced_by_broker_order_id", "\"broker-order-2\""),
        ("lookup", "\"absent\""),
    ] {
        let draft = edit(&complete, &format!("payload.{field}"), Some(value));
        assert_eq!(Draft::parse(&draft).map(|_| ()), Ok(()), "{field}");
    }
}

#[test]
fn order_state_changed_rejects_invalid_transition_evidence() {
    let complete = order_state_changed();
    for (field, value, reason) in [
        ("state", "\"new\"", InvalidReason::NonCanonical),
        ("attempted", "\"new\"", InvalidReason::NonCanonical),
        ("lookup", "\"present\"", InvalidReason::NonCanonical),
        ("filled_qty", "\"one\"", InvalidReason::NonCanonical),
        ("broker_status", "\"\"", InvalidReason::NonCanonical),
        ("reject_code", "\"\"", InvalidReason::NonCanonical),
        (
            "replaced_by_broker_order_id",
            "\"\"",
            InvalidReason::NonCanonical,
        ),
    ] {
        let draft = edit(&complete, &format!("payload.{field}"), Some(value));
        let refusal = Draft::parse(&draft).expect_err("invalid transition evidence");
        assert_eq!(
            (refusal.reason, refusal.path),
            (reason, format!("payload.{field}")),
            "{field}"
        );
    }
}

#[test]
fn account_state_observed_is_closed_complete_and_contains_no_sensitive_identity() {
    let complete = account_state_observed();
    assert_eq!(Draft::parse(&complete).map(|_| ()), Ok(()));
    for field in [
        "status",
        "crypto_status",
        "trading_blocked",
        "account_blocked",
        "trade_suspended_by_user",
        "multiplier",
        "equity",
        "cash",
        "buying_power",
        "non_marginable_buying_power",
        "accrued_fees",
        "risk_clock",
    ] {
        let missing = edit(&complete, &format!("payload.{field}"), None);
        let refusal = Draft::parse(&missing).expect_err("every account field is required");
        assert_eq!(
            (refusal.reason, refusal.path),
            (InvalidReason::Schema, format!("payload.{field}")),
            "{field}"
        );
    }
    for field in [
        "extra",
        "account_number",
        "account_id",
        "credentials",
        "personal_data",
    ] {
        let extra = edit(
            &complete,
            &format!("payload.{field}"),
            Some("\"sensitive\""),
        );
        let refusal = Draft::parse(&extra).expect_err("the payload is closed");
        assert_eq!(
            (refusal.reason, refusal.path),
            (InvalidReason::Schema, format!("payload.{field}")),
            "{field}"
        );
    }
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
        (
            r#""checkpoint":null,"differences":0,"result":"clean","risk_clock":"2026-09-21T14:00:01.000000000Z","snapshot_head":1"#,
            with_payload(
                "ReconciliationRun",
                &[],
                r#"{"result":"clean","checkpoint":null,"snapshot_head":1,"differences":0,
            "risk_clock":"2026-09-21T14:00:01.000000000Z"}"#,
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
    let bad_reconciliation = with_payload(
        "ReconciliationRun",
        &[],
        r#"{"result":"unknown","checkpoint":"","snapshot_head":1,"differences":0,
        "risk_clock":"2026-09-21T14:00:01.000000000Z"}"#,
    );
    let e = Draft::parse(&bad_reconciliation).unwrap_err();
    assert_eq!(
        (e.reason, e.path.as_str()),
        (InvalidReason::NonCanonical, "payload.result")
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
    "UniverseChanged",
    "RiskDayStarted",
    "OwnerAcknowledged",
];

/// A valid payload for every registered risk input; registering another one without adding it here
/// fails `every_registered_risk_input_requires_risk_clock`.
const RISK_INPUT_PAYLOADS: &[(&str, &[&str], &str)] = &[
    (
        "MandateVersionApplied",
        &["mandate_version"],
        r#"{"agent_id":"agent_a","old_version":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","new_version":"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","classification":"neutral","step_up":null,"result":"applied","reason":null,"allocation_change":null,"max_loss_from_allocation":null,"risk_clock":"2026-09-21T14:00:01.000000000Z"}"#,
    ),
    (
        "UniverseChanged",
        &["mandate_version"],
        r#"{"agent_id":"agent_a","instrument":"7b4a1c2e-1111-4a2b-9c3d-000000000001","change":"removed","reason":"version_applied","thesis_id":null,"lineage_id":null,"universe_size_after":0,"risk_clock":"2026-09-21T14:00:01.000000000Z"}"#,
    ),
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
        let mut named: Vec<String> = payload
            .match_indices("\"sha256:")
            .filter_map(|(at, _)| payload.get(at..at + 73))
            .map(str::to_owned)
            .collect();
        named.sort();
        named.dedup();
        let draft = edit(
            &with_payload(event_type, refs, payload),
            "artifact_refs",
            Some(&format!("[{}]", named.join(","))),
        );
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

/// §9.1 closes these schemas, so on an agent stream none of them is refused for want of one.
#[test]
fn a_closed_agent_stream_event_is_never_an_unknown_schema() {
    for (event_type, _, required) in SPEC
        .iter()
        .filter(|(event_type, _, _)| CLOSED_ON_AGENT.contains(event_type))
    {
        let refused = Draft::parse(&draft(event_type, AGENT, required))
            .map(|_| ())
            .unwrap_err();
        assert_ne!(refused.reason, InvalidReason::UnknownSchema, "{event_type}");
    }
}

/// Each schema §9.2 closes is a record with exactly its listed members, so a payload with an
/// unlisted member is refused at that member, before anything else in it is read (§9.1's order,
/// which §9.2 keeps).
#[test]
fn a_closed_control_stream_schema_refuses_an_unlisted_member() {
    assert_eq!(
        CLOSED_BY_SECTION_9_2.len(),
        11,
        "§9.2's eleven (type, stream) pairs"
    );
    for (event_type, kind) in CLOSED_BY_SECTION_9_2 {
        let (_, _, required) = SPEC
            .iter()
            .find(|(t, _, _)| t == event_type)
            .unwrap_or_else(|| panic!("{event_type} is in the catalogue"));
        let refused = Draft::parse(&draft(event_type, kind, required))
            .map(|_| ())
            .unwrap_err();
        assert_eq!(
            (refused.reason, refused.path.as_str()),
            (InvalidReason::Schema, "payload.unregistered"),
            "{event_type} in {kind}"
        );
    }
}

/// §9.2 closes these schemas, so on their streams none of them is refused for want of one.
#[test]
fn a_closed_control_stream_event_is_never_an_unknown_schema() {
    for (event_type, kind) in CLOSED_BY_SECTION_9_2 {
        let (_, _, required) = SPEC
            .iter()
            .find(|(t, _, _)| t == event_type)
            .unwrap_or_else(|| panic!("{event_type} is in the catalogue"));
        let refused = Draft::parse(&draft(event_type, kind, required))
            .map(|_| ())
            .unwrap_err();
        assert_ne!(
            refused.reason,
            InvalidReason::UnknownSchema,
            "{event_type} in {kind}"
        );
    }
}

/// The account stream's snapshot is routed to §9.2's checks, never refused for want of a schema
/// (DEC-261 item 7, DEC-402). Stream K's fee-step writer exists and conforms (#456), and `fees`
/// journals it from the change that registers the schema, which the executor's
/// `the_fee_steps_snapshot_is_never_refused_for_its_members` pins.
#[test]
fn account_snapshot_recorded_is_routed_to_section_9_2() {
    let (event_type, kind) = SNAPSHOT_ON_ACCOUNT;
    let refused = Draft::parse(&draft(event_type, kind, &[]))
        .map(|_| ())
        .unwrap_err();
    assert_ne!(refused.reason, InvalidReason::UnknownSchema);
}

/// §9.2 closes the snapshot's schema, so a member it does not list is refused as `schema` at that
/// member, as on the eleven other pairs.
#[test]
fn account_snapshot_recorded_refuses_an_unlisted_member() {
    let (event_type, kind) = SNAPSHOT_ON_ACCOUNT;
    let refused = Draft::parse(&draft(event_type, kind, &[])).map(|_| ());
    assert_eq!(
        refused.map_err(|e| (e.reason, e.path)),
        Err((InvalidReason::Schema, "payload.unregistered".to_owned()))
    );
}

/// Each risk-state record is routed to §9.3's checks, never refused for want of a catalogue entry
/// or a schema (DEC-403, DEC-404).
#[test]
fn risk_state_records_are_routed_to_section_9_3() {
    for (event_type, kind) in RISK_STATE_ON_ACCOUNT {
        let refused = Draft::parse(&draft(event_type, kind, &["mandate_version"]))
            .map(|_| ())
            .unwrap_err();
        assert!(
            !matches!(
                refused.reason,
                InvalidReason::UnknownSchema | InvalidReason::UnknownEventType
            ),
            "{event_type}: {refused:?}"
        );
    }
}

/// §9.3 closes both schemas, so a member neither lists is refused as `schema` at that member.
#[test]
fn risk_state_records_refuse_an_unlisted_member() {
    for (event_type, kind) in RISK_STATE_ON_ACCOUNT {
        let refused = Draft::parse(&draft(event_type, kind, &["mandate_version"])).map(|_| ());
        assert_eq!(
            refused.map_err(|e| (e.reason, e.path)),
            Err((InvalidReason::Schema, "payload.unregistered".to_owned())),
            "{event_type}"
        );
    }
}

/// Each thesis record is routed to §9.4's checks, never refused for want of a catalogue entry or a
/// schema (DEC-413, DEC-414).
#[test]
fn thesis_records_are_routed_to_section_9_4() {
    for (event_type, kind) in THESIS_ON_AGENT {
        let refused = Draft::parse(&draft(event_type, kind, &[MAN, MOD]))
            .map(|_| ())
            .unwrap_err();
        assert!(
            !matches!(
                refused.reason,
                InvalidReason::UnknownSchema | InvalidReason::UnknownEventType
            ),
            "{event_type}: {refused:?}"
        );
    }
}

/// §9.4 closes the shared schema, so a member it does not list is refused as `schema` at that
/// member.
#[test]
fn thesis_records_refuse_an_unlisted_member() {
    for (event_type, kind) in THESIS_ON_AGENT {
        let refused = Draft::parse(&draft(event_type, kind, &[MAN, MOD])).map(|_| ());
        assert_eq!(
            refused.map_err(|e| (e.reason, e.path)),
            Err((InvalidReason::Schema, "payload.unregistered".to_owned())),
            "{event_type}"
        );
    }
}

const SHA_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SHA_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// A control-stream draft of `event_type` with `payload` and its sorted `artifact_refs`.
fn control_draft(event_type: &str, payload: &str, refs: &[&str]) -> Vec<u8> {
    let d = draft(event_type, CTL, &[]);
    let d = edit(&d, "payload", Some(payload));
    let listed: Vec<String> = refs.iter().map(|r| format!("\"{r}\"")).collect();
    edit(
        &d,
        "artifact_refs",
        Some(&format!("[{}]", listed.join(","))),
    )
}

fn disclosure(step_up: &str) -> Vec<u8> {
    control_draft(
        "DisclosureAccepted",
        &format!(
            r#"{{"document":"leveraged_etp","version":"{SHA_A}","user":"user_owner_01","step_up":{step_up}}}"#
        ),
        &[SHA_A],
    )
}

fn refusal_of(draft: &[u8]) -> (InvalidReason, String) {
    let e = Draft::parse(draft).map(|_| ()).unwrap_err();
    (e.reason, e.path)
}

/// §9.2 closes every schema at every depth: `step_up`, the one nested record that carries
/// authentication evidence, refuses an unlisted member at that member, so a raw assertion or a
/// token has nowhere to sit (`AGENTS.md` rules 6 and 7, #429 round 1, B1).
#[test]
fn a_step_up_refuses_an_unlisted_member() {
    let valid = r#"{"assertion_id":"assert_1","authenticated_at":"2026-09-20T13:06:50.000000000Z","method":"webauthn"}"#;
    assert_eq!(
        Draft::parse(&disclosure(valid)).map(|_| ()),
        Ok(()),
        "the control: a well-formed acceptance appends"
    );
    let widened = r#"{"assertion_id":"assert_1","authenticated_at":"2026-09-20T13:06:50.000000000Z","method":"webauthn","token":"reference-fixture-not-a-token"}"#;
    assert_eq!(
        refusal_of(&disclosure(widened)),
        (InvalidReason::Schema, "payload.step_up.token".to_owned())
    );
}

/// `step_up.authenticated_at` is a §4.7 timestamp, never other text (#429 round 1, B1).
#[test]
fn a_step_up_time_is_a_timestamp() {
    let valid = r#"{"assertion_id":"assert_1","authenticated_at":"2026-09-20T13:06:50.000000000Z","method":"webauthn"}"#;
    assert_eq!(
        Draft::parse(&disclosure(valid)).map(|_| ()),
        Ok(()),
        "the control"
    );
    let off_form = r#"{"assertion_id":"assert_1","authenticated_at":"2026-09-20 13:06:50","method":"webauthn"}"#;
    assert_eq!(
        refusal_of(&disclosure(off_form)),
        (
            InvalidReason::NonCanonical,
            "payload.step_up.authenticated_at".to_owned()
        )
    );
}

/// A `provenance` entry is a closed record too: an unlisted member inside it is refused there
/// (#429 round 1, B1).
#[test]
fn a_provenance_entry_refuses_an_unlisted_member() {
    let created = |entry: &str| {
        control_draft(
            "MandateVersionCreated",
            &format!(
                r#"{{"mandate_version":"{SHA_A}","provenance":[{entry}],"record_ref":"{SHA_B}"}}"#
            ),
            &[SHA_A, SHA_B],
        )
    };
    assert_eq!(
        Draft::parse(&created(r#"{"path":"/autonomy","source":"user_entered"}"#)).map(|_| ()),
        Ok(()),
        "the control: a well-formed entry appends"
    );
    assert_eq!(
        refusal_of(&created(
            r#"{"path":"/autonomy","source":"user_entered","quoted_span":"buy dips"}"#
        )),
        (
            InvalidReason::Schema,
            "payload.provenance[0].quoted_span".to_owned()
        )
    );
}

/// §9.7's three records are catalogued on their one stream with the configuration they name, and
/// closed there: a payload §9.7 does not list is refused `schema`, never `unknown_schema`.
#[test]
fn the_approval_answers_are_catalogued_and_closed_on_their_streams() {
    for (event_type, home, refs) in CLOSED_BY_SECTION_9_7 {
        for kind in [ACCT, AGENT, CTL, CLOCK] {
            let refused = Draft::parse(&draft(event_type, kind, refs)).unwrap_err();
            let want = if kind == *home {
                (InvalidReason::Schema, "payload.unregistered".to_owned())
            } else {
                (InvalidReason::WrongStream, "event_type".to_owned())
            };
            assert_eq!(
                (refused.reason, refused.path),
                want,
                "{event_type} in {kind}"
            );
        }
        for missing in refs.iter() {
            let refused = Draft::parse(&draft(event_type, home, &[])).unwrap_err();
            let want = (
                InvalidReason::MissingConfigRef,
                format!("config_refs.{missing}"),
            );
            assert_eq!(
                (refused.reason, refused.path),
                want,
                "{event_type} without {missing}"
            );
        }
    }
}

/// §9.8's connection records are catalogued on their streams and closed there: an unlisted member
/// is refused `schema`, never `unknown_schema`. `ConnectionEstablished` stays closed at version 1 on
/// the control stream and is catalogued on the account stream too, where only the executor's
/// version-2 copy is registered, so a version-1 draft there is `unknown_schema`.
#[test]
fn the_connection_records_are_catalogued_and_closed_on_their_streams() {
    for (event_type, homes) in CLOSED_BY_E7_17 {
        for kind in [ACCT, AGENT, CTL, CLOCK] {
            let refused = Draft::parse(&draft(event_type, kind, &[])).unwrap_err();
            let want = if homes.contains(&kind) {
                (InvalidReason::Schema, "payload.unregistered".to_owned())
            } else {
                (InvalidReason::WrongStream, "event_type".to_owned())
            };
            assert_eq!(
                (refused.reason, refused.path),
                want,
                "{event_type} in {kind}"
            );
        }
    }
    for kind in [ACCT, AGENT, CTL, CLOCK] {
        let refused = Draft::parse(&draft("ConnectionEstablished", kind, &[])).unwrap_err();
        let want = match kind {
            CTL => (InvalidReason::Schema, "payload.unregistered".to_owned()),
            ACCT => (InvalidReason::UnknownSchema, "payload".to_owned()),
            _ => (InvalidReason::WrongStream, "event_type".to_owned()),
        };
        assert_eq!(
            (refused.reason, refused.path),
            want,
            "ConnectionEstablished in {kind}"
        );
    }
}

/// §9.9 and §9.10's records are catalogued on the control stream with the configuration they name,
/// and closed there: an unlisted member is refused `schema`, never `unknown_schema`. The compiler's
/// record stays catalogued, and open, on the agent stream. `OwnerCommandIssued` is catalogued on the
/// control stream too: a payload with no `command` is refused at that member, since §9.11 reads it
/// for every command.
#[test]
fn the_workspace_api_records_are_catalogued_and_closed_on_the_control_stream() {
    let wrong_stream = (InvalidReason::WrongStream, "event_type".to_owned());
    for (event_type, refs) in CLOSED_BY_E10_15 {
        for kind in [ACCT, AGENT, CTL, CLOCK] {
            let want = match kind {
                CTL => (InvalidReason::Schema, "payload.unregistered".to_owned()),
                AGENT if *event_type == "ModelInvocationRecorded" => {
                    (InvalidReason::UnknownSchema, "payload".to_owned())
                }
                _ => wrong_stream.clone(),
            };
            let got = refused(event_type, kind, refs);
            assert_eq!(got, want, "{event_type} in {kind}");
        }
        for missing in refs.iter() {
            let want = (
                InvalidReason::MissingConfigRef,
                format!("config_refs.{missing}"),
            );
            let got = refused(event_type, CTL, &[]);
            assert_eq!(got, want, "{event_type} without {missing}");
        }
    }
    for kind in [ACCT, AGENT, CTL, CLOCK] {
        let want = match kind {
            CTL => (InvalidReason::Schema, "payload.command".to_owned()),
            _ => wrong_stream.clone(),
        };
        let got = refused("OwnerCommandIssued", kind, &[]);
        assert_eq!(got, want, "OwnerCommandIssued in {kind}");
    }
}

/// What `Draft::parse` refuses a catalogue draft with, and where.
fn refused(event_type: &str, kind: &str, refs: &[&str]) -> (InvalidReason, String) {
    let refused = Draft::parse(&draft(event_type, kind, refs)).unwrap_err();
    (refused.reason, refused.path)
}

/// The compiler's record is on the control stream (§9.9), but on the agent stream
/// `ModelInvocationRecorded` stays catalogued and open, needing `model_version`, before and after
/// E10-15 (E15-8 closes it there).
#[test]
fn the_model_invocation_stays_open_on_the_agent_stream() {
    let (mir, unknown) = ("ModelInvocationRecorded", InvalidReason::UnknownSchema);
    assert_eq!(refused(mir, AGENT, &[MOD]), (unknown, "payload".to_owned()));
    let missing = (
        InvalidReason::MissingConfigRef,
        "config_refs.model_version".to_owned(),
    );
    assert_eq!(refused(mir, AGENT, &[]), missing);
    for kind in [ACCT, CLOCK] {
        let wrong_stream = (InvalidReason::WrongStream, "event_type".to_owned());
        assert_eq!(refused(mir, kind, &[MOD]), wrong_stream, "{kind}");
    }
}
