//! E21-4: each alert of the infrastructure design's §8.2 fires from a fixture record or signal,
//! with the table's severity and runbook, and a payload capture finds no symbol, price, quantity,
//! or mandate content (OPS-10).

use mandate_alerts::{
    APPEND_P99_LIMIT_US, AlertError, AlertKind, COLD_EXPORT_LAG_LIMIT_S, CaptureSink, Severity,
    Signal, from_event, from_signal, payload, rendered, send,
};
use mandate_canon::{Digest, Key, Object, Value, to_canonical};
use mandate_journal::StoredEvent;
use mandate_notify::canary::{CANARIES, scan};
use mandate_notify::{NotifyError, SecureRandom};
use proptest::prelude::*;

/// A source counting up from a seed, so ids differ and are predictable in tests.
struct Counter(u8);

impl SecureRandom for Counter {
    fn fill(&mut self, bytes: &mut [u8; 16]) -> Result<(), NotifyError> {
        self.0 = self.0.wrapping_add(1);
        *bytes = [self.0; 16];
        Ok(())
    }
}

struct Dry;

impl SecureRandom for Dry {
    fn fill(&mut self, _: &mut [u8; 16]) -> Result<(), NotifyError> {
        Err(NotifyError::EntropyUnavailable)
    }
}

fn object(members: &[(&str, Value)]) -> Value {
    let mut object = Object::new();
    for (key, value) in members {
        object.insert(Key::new(key).unwrap(), value.clone());
    }
    Value::Object(object)
}

fn s(text: &str) -> Value {
    Value::Str(text.to_owned())
}

/// A stored record of `event_type` whose payload is `members`, on a stream and with an id that
/// hold canaries, so a payload that copied either would be caught.
fn stored(event_type: &str, members: &[(&str, Value)]) -> StoredEvent {
    let body = object(&[
        ("event_type", s(event_type)),
        ("stream_id", s("acct:ws_zqbrokeraccount:zqagentname")),
        ("payload", object(members)),
    ]);
    StoredEvent {
        stream_id: "acct:ws_zqbrokeraccount:zqagentname".to_owned(),
        seq: 7,
        event_id: "01J8ZNB0M000000000000000K1".to_owned(),
        event_type: event_type.to_owned(),
        schema_version: 1,
        environment: "paper".to_owned(),
        recorded_at: "2026-10-17T14:00:00.000000000Z".to_owned(),
        prev_hash: Digest::of(b"prev"),
        hash: Digest::of(b"this"),
        body: to_canonical(&body),
    }
}

/// Every canary NT-1 names, as extra payload members a careless rule might copy.
fn canary_members() -> Vec<(&'static str, Value)> {
    vec![
        ("instrument", s("zqinstrument")),
        ("side", s("zqside")),
        ("qty", s("zqquantity")),
        ("limit_price", s("zqprice")),
        ("notional", s("zqordervalue")),
        ("pnl", s("zqpnl")),
        ("score", s("zqscore")),
        ("thesis", s("zqthesis")),
        ("rule", s("zqrule")),
        ("deadline", s("zqdeadline")),
        ("agent_name", s("zqagentname")),
        ("mandate", s("zqmandate")),
        ("account", s("zqbrokeraccount")),
        ("owner", s("zqpersonal")),
    ]
}

enum Fixture {
    Event(&'static str, Vec<(&'static str, Value)>),
    Signal(Signal),
}

/// The fixture that raises `kind`: a record for a journal-sourced row, a signal otherwise. The
/// match has no wildcard, so a new kind does not compile without one.
fn fixture(kind: AlertKind) -> Fixture {
    use Fixture::{Event, Signal as Sig};
    match kind {
        AlertKind::ReconciliationMismatch => Event(
            "ReconciliationRun",
            vec![
                ("result", s("mismatch")),
                (
                    "differences",
                    Value::Int(mandate_canon::Int::new(2).unwrap()),
                ),
            ],
        ),
        AlertKind::UnprotectedIntervalLimit => {
            Event("ProtectionChanged", vec![("action", s("interval_limit"))])
        }
        AlertKind::ProtectionUnattributed => Event(
            "GateDecided",
            vec![
                ("reason_code", s("protection_unattributed")),
                ("verdict", s("hold")),
            ],
        ),
        AlertKind::WatchdogFired => Event("ProtectionChanged", vec![("action", s("watchdog"))]),
        AlertKind::KillSwitchActivated => Event(
            "KillSwitchActivated",
            vec![("scope", s("agent")), ("initiator", s("owner"))],
        ),
        AlertKind::KillSwitchAutomatedWorkspace => Event(
            "KillSwitchActivated",
            vec![("scope", s("workspace")), ("initiator", s("risk_limit"))],
        ),
        AlertKind::GlobalKillSwitch => Sig(Signal::GlobalKillSwitch),
        AlertKind::OrderUnknownPastLookup => Sig(Signal::OrderUnknownPastLookup),
        AlertKind::DuplicateOrder => Sig(Signal::DuplicateOrder),
        AlertKind::MarketDataStale => Event("OwnerAlertSent", vec![("kind", s("data_feed_down"))]),
        AlertKind::ModelSpendCap => Event("OwnerAlertSent", vec![("kind", s("spend_cap"))]),
        AlertKind::ModelGatewayDegraded => Sig(Signal::ModelGateway {
            errors: 0,
            deadline_misses: 0,
            meter_unavailable: 1,
        }),
        AlertKind::JournalAppendDegraded => Sig(Signal::JournalAppend {
            unavailable: 0,
            ambiguous: 0,
            p99_us: APPEND_P99_LIMIT_US + 1,
        }),
        AlertKind::StandbyLost => Sig(Signal::StandbyLost),
        AlertKind::UnexpectedFenced => Sig(Signal::UnexpectedFenced),
        AlertKind::VerificationFailed => Event("VerificationRun", vec![("result", s("fail"))]),
        AlertKind::VerificationIncomplete => {
            Event("VerificationRun", vec![("result", s("incomplete"))])
        }
        AlertKind::DdlOutsideRelease => Sig(Signal::DdlOutsideRelease),
        AlertKind::ColdExportLag => Sig(Signal::ColdExportLag {
            seconds: COLD_EXPORT_LAG_LIMIT_S + 1,
        }),
        AlertKind::AnchorGap => Event("AnchorComputed", vec![("token", Value::Null)]),
        AlertKind::ClockOutOfTolerance => Event("ClockToleranceExceeded", vec![]),
        AlertKind::VaultLeaseRenewalFailing => Sig(Signal::VaultLeaseRenewal { failures: 1 }),
        AlertKind::BrokerDegraded => Sig(Signal::Broker {
            errors: 0,
            rate_limited: 0,
            outage: true,
        }),
        AlertKind::CrashLoop => Sig(Signal::CrashLoop),
        AlertKind::ApprovalNotDelivered => {
            Event("ApprovalDelivered", vec![("status", s("failed"))])
        }
        AlertKind::BackupFailed => Sig(Signal::BackupFailed),
        AlertKind::DrillOverdueOrFailed => Sig(Signal::DrillOverdueOrFailed),
    }
}

fn raised(fixture: &Fixture, with_canaries: bool) -> Option<AlertKind> {
    match fixture {
        Fixture::Event(event_type, members) => {
            let mut members = members.clone();
            if with_canaries {
                members.extend(canary_members());
            }
            from_event(&stored(event_type, &members)).unwrap()
        }
        Fixture::Signal(signal) => from_signal(*signal),
    }
}

/// The §8.2 table, typed here from the design rather than from the crate: each kind's severity
/// and runbook.
#[rustfmt::skip]
const TABLE: [(AlertKind, Severity, &str); 27] = [
    (AlertKind::ReconciliationMismatch, Severity::Sev2, "RB-01"),
    (AlertKind::UnprotectedIntervalLimit, Severity::Sev2, "RB-02"),
    (AlertKind::ProtectionUnattributed, Severity::Sev3, "RB-02"),
    (AlertKind::WatchdogFired, Severity::Sev2, "RB-02"),
    (AlertKind::KillSwitchActivated, Severity::Informational, "RB-03"),
    (AlertKind::KillSwitchAutomatedWorkspace, Severity::Sev1, "RB-03"),
    (AlertKind::GlobalKillSwitch, Severity::Sev1, "RB-03"),
    (AlertKind::OrderUnknownPastLookup, Severity::Sev2, "RB-04"),
    (AlertKind::DuplicateOrder, Severity::Sev1, "RB-04"),
    (AlertKind::MarketDataStale, Severity::Sev3, "RB-05"),
    (AlertKind::ModelSpendCap, Severity::Informational, "RB-06"),
    (AlertKind::ModelGatewayDegraded, Severity::Sev3, "RB-06"),
    (AlertKind::JournalAppendDegraded, Severity::Sev2, "RB-07"),
    (AlertKind::StandbyLost, Severity::Sev2, "RB-07"),
    (AlertKind::UnexpectedFenced, Severity::Sev2, "RB-08"),
    (AlertKind::VerificationFailed, Severity::Sev1, "RB-09"),
    (AlertKind::VerificationIncomplete, Severity::Informational, "RB-10"),
    (AlertKind::DdlOutsideRelease, Severity::Sev1, "RB-09"),
    (AlertKind::ColdExportLag, Severity::Sev2, "RB-10"),
    (AlertKind::AnchorGap, Severity::Sev3, "RB-10"),
    (AlertKind::ClockOutOfTolerance, Severity::Sev2, "RB-11"),
    (AlertKind::VaultLeaseRenewalFailing, Severity::Sev2, "RB-12"),
    (AlertKind::BrokerDegraded, Severity::Sev2, "RB-13"),
    (AlertKind::CrashLoop, Severity::Sev2, "RB-14"),
    (AlertKind::ApprovalNotDelivered, Severity::Sev2, "RB-15"),
    (AlertKind::BackupFailed, Severity::Sev2, "RB-16"),
    (AlertKind::DrillOverdueOrFailed, Severity::Sev2, "RB-16"),
];

#[test]
fn every_kind_has_the_tables_severity_and_runbook() {
    assert_eq!(TABLE.map(|(kind, _, _)| kind), AlertKind::ALL);
    for (kind, severity, runbook) in TABLE {
        assert_eq!(
            (kind.severity(), kind.runbook()),
            (severity, runbook),
            "{kind:?}"
        );
    }
}

#[test]
fn reason_codes_are_distinct_opaque_snake_case() {
    let mut seen = Vec::new();
    for kind in AlertKind::ALL {
        let reason = kind.reason();
        assert!(
            reason.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') && !reason.is_empty(),
            "{reason}"
        );
        assert!(!seen.contains(&reason), "{reason} twice");
        seen.push(reason);
        assert!(scan(kind.text().as_bytes()).unwrap().is_empty());
    }
    let severities: Vec<&str> = [
        Severity::Sev1,
        Severity::Sev2,
        Severity::Sev3,
        Severity::Informational,
    ]
    .map(Severity::key)
    .to_vec();
    assert_eq!(severities, ["sev1", "sev2", "sev3", "informational"]);
}

#[test]
fn every_alert_fires_from_its_fixture() {
    for kind in AlertKind::ALL {
        assert_eq!(raised(&fixture(kind), false), Some(kind), "{kind:?}");
        assert_eq!(
            raised(&fixture(kind), true),
            Some(kind),
            "{kind:?} with canaries"
        );
    }
}

#[test]
fn records_that_are_not_alerts_raise_nothing() {
    let quiet = [
        ("ReconciliationRun", vec![("result", s("clean"))]),
        ("ReconciliationRun", vec![("result", s("adopted"))]),
        ("ProtectionChanged", vec![("action", s("placed"))]),
        (
            "ProtectionChanged",
            vec![("action", s("unprotected_start"))],
        ),
        ("GateDecided", vec![("reason_code", s("limit_exceeded"))]),
        ("VerificationRun", vec![("result", s("pass"))]),
        ("AnchorComputed", vec![("token", s("art_01"))]),
        ("ApprovalDelivered", vec![("status", s("delivered"))]),
        ("OwnerAlertSent", vec![("kind", s("approval_requested"))]),
        ("OrderStateChanged", vec![("state", s("unknown"))]),
        ("AgentModeApplied", vec![("to", s("paused"))]),
    ];
    for (event_type, members) in quiet {
        assert_eq!(
            from_event(&stored(event_type, &members)),
            Ok(None),
            "{event_type} {members:?}"
        );
    }
}

#[test]
fn a_kill_switch_is_sev1_only_when_automated_at_workspace_scope() {
    let cases = [
        ("agent", "owner", AlertKind::KillSwitchActivated),
        ("workspace", "owner", AlertKind::KillSwitchActivated),
        ("agent", "risk_limit", AlertKind::KillSwitchActivated),
        (
            "connection",
            "platform_operator",
            AlertKind::KillSwitchActivated,
        ),
        (
            "workspace",
            "risk_limit",
            AlertKind::KillSwitchAutomatedWorkspace,
        ),
        (
            "workspace",
            "platform_operator",
            AlertKind::KillSwitchAutomatedWorkspace,
        ),
    ];
    for (scope, initiator, kind) in cases {
        let row = stored(
            "KillSwitchActivated",
            &[("scope", s(scope)), ("initiator", s(initiator))],
        );
        assert_eq!(from_event(&row), Ok(Some(kind)), "{scope} {initiator}");
    }
}

#[test]
fn signals_alert_only_past_their_thresholds() {
    let quiet = [
        Signal::ModelGateway {
            errors: 0,
            deadline_misses: 0,
            meter_unavailable: 0,
        },
        Signal::JournalAppend {
            unavailable: 0,
            ambiguous: 0,
            p99_us: APPEND_P99_LIMIT_US,
        },
        Signal::ColdExportLag {
            seconds: COLD_EXPORT_LAG_LIMIT_S,
        },
        Signal::VaultLeaseRenewal { failures: 0 },
        Signal::Broker {
            errors: 0,
            rate_limited: 0,
            outage: false,
        },
    ];
    for signal in quiet {
        assert_eq!(from_signal(signal), None, "{signal:?}");
    }
    let loud = [
        (
            Signal::ModelGateway {
                errors: 1,
                deadline_misses: 0,
                meter_unavailable: 0,
            },
            AlertKind::ModelGatewayDegraded,
        ),
        (
            Signal::ModelGateway {
                errors: 0,
                deadline_misses: 1,
                meter_unavailable: 0,
            },
            AlertKind::ModelGatewayDegraded,
        ),
        (
            Signal::JournalAppend {
                unavailable: 1,
                ambiguous: 0,
                p99_us: 0,
            },
            AlertKind::JournalAppendDegraded,
        ),
        (
            Signal::JournalAppend {
                unavailable: 0,
                ambiguous: 1,
                p99_us: 0,
            },
            AlertKind::JournalAppendDegraded,
        ),
        (
            Signal::Broker {
                errors: 1,
                rate_limited: 0,
                outage: false,
            },
            AlertKind::BrokerDegraded,
        ),
        (
            Signal::Broker {
                errors: 0,
                rate_limited: 1,
                outage: false,
            },
            AlertKind::BrokerDegraded,
        ),
    ];
    for (signal, kind) in loud {
        assert_eq!(from_signal(signal), Some(kind), "{signal:?}");
    }
    assert_eq!(APPEND_P99_LIMIT_US, 5_000);
    assert_eq!(COLD_EXPORT_LAG_LIMIT_S, 60);
}

#[test]
fn an_unreadable_record_is_an_error_not_silence() {
    let mut row = stored("ReconciliationRun", &[("result", s("mismatch"))]);
    row.body = b"{not json".to_vec();
    assert_eq!(from_event(&row), Err(AlertError::Unreadable));
    row.body = to_canonical(&object(&[("payload", s("mismatch"))]));
    assert_eq!(from_event(&row), Err(AlertError::Unreadable));
    row.body = to_canonical(&object(&[("event_type", s("ReconciliationRun"))]));
    assert_eq!(from_event(&row), Err(AlertError::Unreadable));
}

#[test]
fn the_payload_is_the_id_reason_runbook_severity_and_text_only() {
    let mut sink = CaptureSink::default();
    let alert = send(
        AlertKind::ReconciliationMismatch,
        &mut Counter(0),
        &mut sink,
    )
    .unwrap();
    let id = "01".repeat(16);
    assert_eq!(alert.id().unwrap(), id);
    assert_eq!(alert.kind(), AlertKind::ReconciliationMismatch);
    let expected = format!(
        r#"{{"alert":"{id}","reason":"reconciliation_mismatch","runbook":"RB-01","severity":"sev2","text":"Reconciliation found a mismatch; the agent is paused."}}"#
    );
    assert_eq!(sink.captured, vec![expected.into_bytes()]);
    assert_eq!(rendered(&alert).unwrap(), sink.captured[0]);
    let members: Vec<String> = match payload(&alert).unwrap() {
        Value::Object(object) => object.keys().map(|k| k.as_str().to_owned()).collect(),
        other => panic!("{other:?}"),
    };
    assert_eq!(members, ["alert", "reason", "runbook", "severity", "text"]);
}

#[test]
fn ids_are_fresh_and_never_raised_without_entropy() {
    let mut random = Counter(0);
    let mut sink = CaptureSink::default();
    let first = send(AlertKind::AnchorGap, &mut random, &mut sink).unwrap();
    let second = send(AlertKind::AnchorGap, &mut random, &mut sink).unwrap();
    assert_ne!(first.id().unwrap(), second.id().unwrap());
    assert_eq!(
        send(AlertKind::AnchorGap, &mut Dry, &mut sink),
        Err(AlertError::Entropy)
    );
    assert_eq!(sink.captured.len(), 2);
}

#[test]
fn a_payload_capture_of_every_alert_finds_no_canary() {
    let mut sink = CaptureSink::default();
    let mut random = Counter(0x40);
    for kind in AlertKind::ALL {
        let raised = raised(&fixture(kind), true).unwrap();
        send(raised, &mut random, &mut sink).unwrap();
    }
    assert_eq!(sink.captured.len(), AlertKind::ALL.len());
    let all: Vec<u8> = sink.captured.concat();
    assert_eq!(scan(&all).unwrap(), Vec::<&str>::new());
    for canary in CANARIES {
        assert!(!String::from_utf8_lossy(&all).contains(canary));
    }
    assert!(!String::from_utf8_lossy(&all).contains("01J8ZNB0M000000000000000K1"));
}

proptest! {
    #[test]
    fn what_a_record_holds_beyond_its_trigger_never_reaches_the_payload(
        extra in proptest::collection::vec(("[a-z]{1,12}", "[ -~]{0,24}"), 0..8),
        which in 0usize..AlertKind::ALL.len(),
    ) {
        let kind = AlertKind::ALL[which];
        if let Fixture::Event(event_type, members) = fixture(kind) {
            let mut noisy = members.clone();
            for (key, value) in &extra {
                if !members.iter().any(|(k, _)| k == key) {
                    noisy.push((key.as_str(), s(value)));
                }
            }
            let noisy: Vec<(&str, Value)> = noisy;
            let quiet_kind = from_event(&stored(event_type, &members)).unwrap();
            let noisy_kind = from_event(&stored(event_type, &noisy)).unwrap();
            prop_assert_eq!(noisy_kind, quiet_kind);
            let mut a = CaptureSink::default();
            let mut b = CaptureSink::default();
            send(quiet_kind.unwrap(), &mut Counter(9), &mut a).unwrap();
            send(noisy_kind.unwrap(), &mut Counter(9), &mut b).unwrap();
            prop_assert_eq!(a, b);
        }
    }
}
