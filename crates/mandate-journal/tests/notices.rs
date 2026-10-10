//! E8-9 slice S2 (DEC-720): the notice stream `ntf:{workspace_id}` (journal spec §2, v0.12), and
//! the closed `OwnerAlertSent`, notice-stream `StreamOpened`, `NoticeIssued`, and `NoticeAttempted`,
//! with DEC-720's rules N1 to N12. The expected reasons and paths are written out from DEC-720's
//! tables, not read from the journal's own schema. Until the slice's implementation, the journal refuses
//! `ntf:` as a stream id and keeps `OwnerAlertSent` control-only with no schema, which is the
//! answer these tests fail on.
//!
//! A case is one line: `base | edits | expected`. `base` names a draft below; `edits` are
//! space-separated `member=value` pairs on its payload (`^member` for the envelope), where a value
//! that is not JSON (`null`, a number, an array, or a quoted string) is a bare string; `expected`
//! is `ok` or `reason@path`.
//!
//! `NoticeAttempted` is written at version 2 (journal spec v0.40, DEC-706): version 1's members and
//! `verdict` last, non-null exactly when `status` is `failed` and held by rule 134 to the verdict
//! notifications spec §5.2 to §5.4 fix for its reason. From v0.40 `append` refuses version 1 as
//! `unknown_schema` at `payload`. Until DEC-706's implementation registers version 2, every test
//! that parses an attempt is pending: the journal refuses version 2 as `unknown_schema`, which is
//! the answer they fail on.

mod common;

use common::{STREAM, T, edit, event_id, journal_with, mark_draft, now, stream};
use mandate_canon::{Digest, Key, Value, parse, to_canonical};
use mandate_journal::{
    AppendOutcome, Draft, MemoryJournal, StoredEvent, StreamId, StreamType, TrustedStart,
    check_batch, verify_events,
};
use proptest::prelude::*;
use proptest::test_runner::TestRunner;
use std::collections::BTreeMap;

const NTF: &str = "ntf:ws_1";
const NOTICE: &str = "0123456789abcdef0123456789abcdef";
const STREAMS: [&str; 5] = [STREAM, "agent:ws_1:agent_a", "ctl:ws_1", "clock:ws_1", NTF];

/// DEC-720's kinds, with DEC-795 item 6's `notification_address_changed`, in notifications spec
/// §3.2's order, each `kind:class`.
const KINDS: &str = "approval_requested:action approval_reminder:action risk_limit:safety \
    kill_switch:safety agent_held:safety account_restriction:safety protection:safety \
    exit_stalled:safety reconciliation:safety external_activity:safety account_state:safety \
    data_feed_down:safety integrity_incident:safety credential_added:safety new_device:safety \
    notification_address_changed:safety recovery_used:safety role_granted:safety \
    member_deactivated:safety deprovisioned:safety break_glass:safety version_risk_increasing:safety delegation_added:safety \
    connection_added:safety went_live:safety client_connected:safety channel_lost:safety \
    daily_brief:info delegation_ended:info model_status:info research_status:info spend_cap:info \
    approval_closed:info";

/// Rule 134's verdicts, typed from journal spec §9.15 and DEC-706 item 4, never read from the
/// journal: each `failed` reason with the verdict it must carry, `None` where it takes either.
const FIXED: [(&str, Option<&str>); 11] = [
    ("timeout", Some("retryable")),
    ("rate_limited", Some("retryable")),
    ("provider_error", None),
    ("address_rejected", Some("permanent")),
    ("auth_failed", Some("permanent")),
    ("too_large", None),
    ("recipient_not_permitted", Some("permanent")),
    ("address_missing", Some("permanent")),
    ("bounced", Some("permanent")),
    ("complained", Some("permanent")),
    ("unsubscribed", Some("permanent")),
];

/// A draft on `stream`, at schema version 2 for `NoticeAttempted` (DEC-706) and 1 for the rest.
fn envelope(stream: &str, id: u64, event_type: &str, causation: &str, payload: &str) -> Vec<u8> {
    let version = if event_type == "NoticeAttempted" {
        2
    } else {
        1
    };
    format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{}","stream_id":"{stream}",
        "event_type":"{event_type}","schema_version":{version},"event_time":"{T}","clock_source":"local",
        "causation_id":{causation},"correlation_id":null,
        "actor":{{"kind":"system","id":"dispatcher","version":"0.1.0","build":"sha256:{}"}},
        "config_refs":{{}},"payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#,
        event_id(id),
        "3".repeat(64)
    )
    .into_bytes()
}

/// An `OwnerAlertSent` on `stream`, with `event_id` `id`, about the event with `event_id` `subject`.
fn alert(stream: &str, id: u64, subject: u64) -> Vec<u8> {
    let subject = event_id(subject);
    let payload = format!(r#"{{"subject":"{subject}","kind":"risk_limit","owner_command":null}}"#);
    envelope(
        stream,
        id,
        "OwnerAlertSent",
        &format!("\"{subject}\""),
        &payload,
    )
}

/// The base draft `name`: `alert` (on the account stream), `opened`, `issued`, or `attempted`.
fn base(name: &str) -> Vec<u8> {
    let cause = event_id(100);
    let (id, event_type, causation, payload) = match name {
        "alert" => return alert(STREAM, 101, 100),
        "opened" => (
            0,
            "StreamOpened",
            "null".to_owned(),
            r#"{"stream_type":"notice","workspace_id":"ws_1"}"#.to_owned(),
        ),
        "issued" => (
            1,
            "NoticeIssued",
            format!("\"{cause}\""),
            format!(
                r#"{{"notice":"{NOTICE}","kind":"risk_limit","class":"safety","cause":"{cause}",
                "cause_stream":"{STREAM}","recipients":["u_1","u_2"]}}"#
            ),
        ),
        _ => (
            2,
            "NoticeAttempted",
            "null".to_owned(),
            format!(
                r#"{{"notice":"{NOTICE}","recipient":"u_1","channel":"email","attempt":1,
                "status":"delivered","reason":null,"provider_message_id":"<m1@mail.example>",
                "coalesced_into":null,"verdict":null}}"#
            ),
        ),
    };
    envelope(NTF, id, event_type, &causation, &payload)
}

/// `draft` with each `member=value` edit of `edits` applied (see the module docs).
fn set(draft: &[u8], edits: &str) -> Vec<u8> {
    edits.split_whitespace().fold(draft.to_vec(), |d, pair| {
        let (member, value) = pair.split_once('=').unwrap();
        let json =
            parse(value.as_bytes()).map_or_else(|_| format!("\"{value}\""), |_| value.into());
        let path = member
            .strip_prefix('^')
            .map_or_else(|| format!("payload.{member}"), str::to_owned);
        edit(&d, &path, Some(&json))
    })
}

/// `ok`, or the refusal as `reason@path`.
fn outcome(draft: &[u8]) -> String {
    Draft::parse(draft).map_or_else(
        |e| format!("{}@{}", e.reason.code(), e.path),
        |_| "ok".into(),
    )
}

/// Runs `base | edits | expected` cases, reporting every one that differs.
fn run(cases: &[&str]) {
    let failed: Vec<String> = cases
        .iter()
        .filter_map(|case| {
            let [name, edits, want] = case.split('|').map(str::trim).collect::<Vec<_>>()[..] else {
                panic!("`{case}` is not `base | edits | expected`")
            };
            let got = outcome(&set(&base(name), edits));
            (got != want).then(|| format!("{case}: got {got}"))
        })
        .collect();
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

#[test]
fn the_notice_stream_id_parses_to_its_own_stream_type() {
    let id = StreamId::parse(NTF).map(|s| (s.stream_type(), s.as_str().to_owned()));
    assert_eq!(id, Some((StreamType::Notice, NTF.to_owned())));
    for bad in ["ntf:", "ntf:ws_1:x", "ntf:ws 1", "ntf:ws.1", "notice:ws_1"] {
        assert_eq!(StreamId::parse(bad), None, "{bad}");
    }
}

#[test]
fn the_alert_and_notice_records_are_catalogued_and_closed_on_their_streams() {
    let homes: [(&str, &[&str]); 3] = [
        ("OwnerAlertSent", &STREAMS[..3]),
        ("NoticeIssued", &[NTF]),
        ("StreamOpened", &STREAMS),
    ];
    closed_on(&homes);
}

/// Each `(event_type, home)` is closed on the streams of `home` and `wrong_stream` on the others.
fn closed_on(homes: &[(&str, &[&str])]) {
    for &(event_type, home) in homes {
        for kind in STREAMS {
            let draft = envelope(kind, 9, event_type, "null", r#"{"unregistered":true}"#);
            let want = if home.contains(&kind) {
                "schema@payload.unregistered"
            } else {
                "wrong_stream@event_type"
            };
            assert_eq!(outcome(&draft), want, "{event_type} on {kind}");
        }
    }
}

/// Other event types the catalogue holds, account, agent, control, and scheduler alike, the copies
/// a stream owner writes from another stream's event among them. None is admitted on `ntf:`, so no
/// stream owner copies a fact from the notice stream and the dispatcher writes nothing else.
const NOT_ON_NTF: &str = "IntentReceived GateDecided FillApplied MarkUpdated AgentModeApplied \
    OwnerAcknowledged OwnerCommandRefused KillSwitchActivated TradingDayStarted ClockAdvanced \
    ClockToleranceExceeded DecisionMade ApprovalRequested ApprovalDelivered ApprovalResponded \
    AgentModeChanged ApprovalResponseSubmitted ConfigSnapshotRegistered IntegrityIncidentRecorded \
    OwnerAlertSent";

#[test]
fn the_notice_stream_admits_only_its_own_three_records() {
    let opened = envelope(NTF, 9, "StreamOpened", "null", r#"{"unregistered":true}"#);
    assert_eq!(outcome(&opened), "schema@payload.unregistered");
    let types: Vec<&str> = NOT_ON_NTF.split_whitespace().collect();
    assert_eq!(types.len(), 20);
    for event_type in types {
        let draft = envelope(NTF, 9, event_type, "null", r#"{"unregistered":true}"#);
        assert_eq!(
            outcome(&draft),
            "wrong_stream@event_type",
            "{event_type} on {NTF}"
        );
    }
}

#[test]
fn every_member_is_required_and_no_unlisted_member_is_admitted() {
    let alerts = STREAMS[..3].iter().map(|s| alert(s, 101, 100));
    let notices = ["opened", "issued"].map(base);
    alerts.chain(notices).for_each(|draft| closed(&draft));
}

/// `draft` is admitted, and refused without any one of its members or with an unlisted one.
fn closed(draft: &[u8]) {
    assert_eq!(outcome(draft), "ok");
    let Some(Value::Object(members)) = parse(draft).unwrap().get("payload").cloned() else {
        panic!("a payload is an object")
    };
    for member in members.keys() {
        let path = format!("payload.{}", member.as_str());
        assert_eq!(outcome(&edit(draft, &path, None)), format!("schema@{path}"));
    }
    let extra = set(draft, "zz_unlisted=true");
    assert_eq!(outcome(&extra), "schema@payload.zz_unlisted");
}

/// Non-negotiable 6 and NT-1, NT-2: no record admits trading content, an address, or a sentence.
#[test]
fn no_alert_or_notice_record_can_carry_trading_content() {
    for name in ["alert", "opened", "issued"] {
        no_unlisted_content(name);
    }
    for member in ["notice", "cause_stream"] {
        no_sentence_in("issued", member);
    }
    let listed = edit(
        &base("issued"),
        "payload.recipients",
        Some(&format!("[{CONTENT}]")),
    );
    assert_eq!(outcome(&listed), "non_canonical@payload.recipients[0]");
    run(&["issued | recipients=[\"jane@example.com\"] | non_canonical@payload.recipients[0]"]);
}

/// Trading content and an address, as a sentence.
const CONTENT: &str = r#""AAPL buy 10 @ 150.25""#;

/// The base draft `name` refuses a trading or address member planted beside its own.
fn no_unlisted_content(name: &str) {
    for member in [
        "instrument",
        "side",
        "qty",
        "price",
        "text",
        "message",
        "address",
    ] {
        let planted = edit(&base(name), &format!("payload.{member}"), Some(CONTENT));
        assert_eq!(outcome(&planted), format!("schema@payload.{member}"));
    }
}

/// The base draft `name` refuses a sentence in its own `member`.
fn no_sentence_in(name: &str, member: &str) {
    let planted = edit(&base(name), &format!("payload.{member}"), Some(CONTENT));
    assert_eq!(outcome(&planted), format!("non_canonical@payload.{member}"));
}

#[test]
fn the_member_types_refuse_event_ids_and_foreign_vocabulary() {
    let ulid = event_id(5);
    let upper = NOTICE.to_uppercase();
    let short = &NOTICE[1..];
    run(&[
        &format!("issued | notice={ulid} | non_canonical@payload.notice"),
        &format!("issued | notice={upper} | non_canonical@payload.notice"),
        &format!("issued | notice={short} | non_canonical@payload.notice"),
        &format!("issued | notice={NOTICE}0 | non_canonical@payload.notice"),
        "issued | notice=5 | schema@payload.notice",
        "issued | kind=tripwire_fired | non_canonical@payload.kind",
        "issued | class=urgent | non_canonical@payload.class",
        "issued | cause=not-a-ulid | non_canonical@payload.cause",
        "issued | cause_stream=ws_1 | non_canonical@payload.cause_stream",
        "issued | cause_stream=acct:ws_1 | non_canonical@payload.cause_stream",
        "issued | cause_stream=acct:ws.1:A | non_canonical@payload.cause_stream",
        "issued | recipients=u_1 | schema@payload.recipients",
        "opened | stream_type=control | non_canonical@payload.stream_type",
        "alert | subject=not-a-ulid | non_canonical@payload.subject",
        "alert | subject=5 | schema@payload.subject",
        &format!("alert | kind={ulid} | non_canonical@payload.kind"),
        "alert | kind=kill_switch owner_command=not-a-ulid | non_canonical@payload.owner_command",
    ]);
    let attempted = String::from_utf8(base("attempted")).unwrap();
    assert!(attempted.contains(r#""attempt":1,"#));
    let fractional = attempted.replace(r#""attempt":1,"#, r#""attempt":1.5,"#);
    assert_eq!(
        outcome(fractional.as_bytes()),
        "float@",
        "an attempt is never fractional"
    );
}

#[test]
fn every_kind_and_vocabulary_member_is_admitted_where_dec_720_allows_it() {
    let mut cases = Vec::new();
    let kinds: Vec<(&str, &str)> = KINDS
        .split_whitespace()
        .filter_map(|pair| pair.split_once(':'))
        .collect();
    assert_eq!(kinds.len(), 33, "notifications spec §3.2's 33 kinds");
    for (kind, class) in kinds {
        let cause_stream = match kind {
            "approval_requested" | "approval_reminder" => "agent:ws_1:agent_a",
            "channel_lost" => NTF,
            _ => "ctl:ws_1",
        };
        let notice = format!("issued | kind={kind} cause_stream={cause_stream} class=");
        for other in ["action", "safety", "info"] {
            let want = if other == class {
                "ok"
            } else {
                "schema@payload.class"
            };
            cases.push(format!("{notice}{other} | {want}"));
        }
        let alert = match kind {
            "approval_requested" | "approval_reminder" | "channel_lost" => {
                "non_canonical@payload.kind"
            }
            _ => "ok",
        };
        cases.push(format!("alert | kind={kind} | {alert}"));
    }
    run(&cases.iter().map(String::as_str).collect::<Vec<_>>());
}

#[test]
fn the_consistency_rules_refuse_at_their_paths() {
    let other = event_id(2);
    let command = event_id(7);
    run(&[
        "alert | ^causation_id=null | schema@causation_id",
        &format!("alert | ^causation_id={other} | schema@causation_id"),
        &format!("alert | owner_command={command} | schema@payload.owner_command"),
        &format!("alert | kind=kill_switch owner_command={command} | ok"),
        &format!("alert | ^stream_id=ctl:ws_1 kind=kill_switch owner_command={command} | ok"),
        &format!("alert | ^stream_id=agent:ws_1:a kind=kill_switch owner_command={command} | ok"),
        "opened | workspace_id=ws_2 | stream_mismatch@stream_id",
        "issued | recipients=[\"u_2\",\"u_1\"] | schema@payload.recipients",
        "issued | recipients=[\"u_1\",\"u_1\"] | schema@payload.recipients",
        "issued | recipients=[] | ok",
        "issued | ^causation_id=null | schema@causation_id",
        &format!("issued | ^causation_id={other} | schema@causation_id"),
        "issued | kind=approval_requested class=action | stream_mismatch@payload.cause_stream",
        "issued | kind=approval_reminder class=action cause_stream=ctl:ws_1 | stream_mismatch@payload.cause_stream",
        "issued | kind=approval_requested class=action cause_stream=agent:ws_2:a | stream_mismatch@payload.cause_stream",
        "issued | kind=channel_lost | stream_mismatch@payload.cause_stream",
        "issued | kind=channel_lost cause_stream=ntf:ws_2 | stream_mismatch@payload.cause_stream",
        "issued | cause_stream=ctl:ws_2 | stream_mismatch@payload.cause_stream",
        "issued | cause_stream=acct:ws_2:ACCT1 | stream_mismatch@payload.cause_stream",
        "issued | cause_stream=clock:ws_1 | stream_mismatch@payload.cause_stream",
        "issued | cause_stream=ntf:ws_1 | stream_mismatch@payload.cause_stream",
        "issued | cause_stream=agent:ws_1:a | ok",
    ]);
}

/// Rule N12 spans a batch: an alert's subject is an earlier draft of its own batch that is not an
/// alert, and no two alerts in the batch name one subject.
#[test]
fn the_batch_rule_binds_an_alert_to_an_earlier_subject_in_its_batch() {
    let parsed = |draft: Vec<u8>| Draft::parse(&draft).unwrap();
    let mark = || parsed(mark_draft(1, "1"));
    let first = || parsed(alert(STREAM, 101, 1));
    let batches = [
        ("subject first", vec![mark(), first()], None),
        (
            "two subjects, one alert each",
            vec![
                mark(),
                parsed(mark_draft(2, "1")),
                first(),
                parsed(alert(STREAM, 102, 2)),
            ],
            None,
        ),
        ("alone", vec![first()], Some(0)),
        ("subject after", vec![first(), mark()], Some(0)),
        (
            "two alerts",
            vec![mark(), first(), parsed(alert(STREAM, 102, 1))],
            Some(2),
        ),
        (
            "of an alert",
            vec![mark(), first(), parsed(alert(STREAM, 102, 101))],
            Some(2),
        ),
    ];
    for (name, batch, refused_at) in batches {
        let got = check_batch(&batch)
            .err()
            .map(|(i, e)| (i, e.reason.code(), e.path));
        let want = refused_at.map(|i| (i, "schema", "payload.subject".to_owned()));
        assert_eq!(got, want, "{name}");
    }
    let mut journal = journal_with(0);
    let batch = [mark_draft(1, "1"), alert(STREAM, 101, 1)];
    let committed = journal.append(&stream(), 1, 1, now(), &[&batch[0], &batch[1]]);
    assert!(
        matches!(committed, AppendOutcome::Committed(_)),
        "{committed:?}"
    );
    let late = journal.append(&stream(), 3, 1, now(), &[&alert(STREAM, 102, 1)]);
    let refused = match &late {
        AppendOutcome::Invalid { draft, error } => {
            Some((*draft, error.reason.code(), &*error.path))
        }
        _ => None,
    };
    assert_eq!(refused, Some((0, "schema", "payload.subject")), "{late:?}");
}

/// Version 2 is the only `NoticeAttempted` `append` admits (DEC-706 item 6): closed on the notice
/// stream and `wrong_stream` elsewhere, while version 1, in its own shape or with a `verdict`, and
/// any later version are `unknown_schema` at `payload`, through `Draft::parse` and `append` alike.
#[test]
#[ignore = "pending E8-10"]
fn an_attempt_is_appended_at_version_2_and_refused_at_version_1() {
    closed_on(&[("NoticeAttempted", &[NTF])]);
    run(&[
        "attempted | | ok",
        "attempted | ^schema_version=1 | unknown_schema@payload",
        "attempted | ^schema_version=3 | unknown_schema@payload",
    ]);
    let v1 = version_1(&base("attempted"));
    assert_eq!(
        outcome(&v1),
        "unknown_schema@payload",
        "version 1's own shape"
    );
    let mut journal = MemoryJournal::new();
    let ntf = StreamId::parse(NTF).unwrap();
    let epoch = journal.take_ownership(&ntf);
    let failed = set(
        &base("attempted"),
        "status=failed reason=provider_error provider_message_id=null verdict=retryable",
    );
    let opened = journal.append(&ntf, 0, epoch, now(), &[&base("opened"), &base("issued")]);
    assert_eq!(opened.name(), "Committed", "{opened:?}");
    let attempted = journal.append(&ntf, 2, epoch, now(), &[&failed]);
    assert_eq!(attempted.name(), "Committed", "{attempted:?}");
    let later = set(&v1, &format!("^event_id={}", event_id(3)));
    let refused = match journal.append(&ntf, 3, epoch, now(), &[&later]) {
        AppendOutcome::Invalid { draft, error } => Some((draft, error.reason.code(), error.path)),
        _ => None,
    };
    assert_eq!(refused, Some((0, "unknown_schema", "payload".to_owned())));
}

/// `draft`, an attempt, in version 1's shape: `schema_version` 1 and no `verdict`.
fn version_1(draft: &[u8]) -> Vec<u8> {
    edit(&set(draft, "^schema_version=1"), "payload.verdict", None)
}

/// Version 2 is closed (DEC-720's form): without `verdict`, or with an unlisted member beside it,
/// an attempt is `schema` at that member.
#[test]
#[ignore = "pending E8-10"]
fn every_member_of_an_attempt_is_required_and_no_unlisted_member_is_admitted() {
    closed(&base("attempted"));
    let failed = set(
        &base("attempted"),
        "status=failed reason=timeout provider_message_id=null verdict=retryable",
    );
    closed(&failed);
}

/// Non-negotiable 6 and NT-1, NT-2 on version 2: an attempt holds no trading content, address, or
/// sentence, its `verdict` included.
#[test]
#[ignore = "pending E8-10"]
fn no_attempt_can_carry_trading_content() {
    no_unlisted_content("attempted");
    for member in [
        "provider_message_id",
        "coalesced_into",
        "recipient",
        "verdict",
    ] {
        no_sentence_in("attempted", member);
    }
    run(&["attempted | recipient=jane@example.com | non_canonical@payload.recipient"]);
}

/// The member types of version 2, each refused in §9.1's order: a non-string `verdict` is
/// `schema`, a string outside `retryable` and `permanent` is `non_canonical`, and an unlisted
/// member or an earlier ill-typed member is reported before it.
#[test]
#[ignore = "pending E8-10"]
fn an_attempts_member_types_refuse_event_ids_and_foreign_vocabulary() {
    let ulid = event_id(5);
    let over = "x".repeat(257);
    let max = "x".repeat(256);
    let failed = "status=failed reason=timeout provider_message_id=null";
    run(&[
        &format!("attempted | notice={ulid} | non_canonical@payload.notice"),
        "attempted | recipient=\"\" | non_canonical@payload.recipient",
        "attempted | channel=cli_inbox | non_canonical@payload.channel",
        "attempted | channel=web_inbox | non_canonical@payload.channel",
        "attempted | attempt=\"1\" | schema@payload.attempt",
        "attempted | attempt=true | schema@payload.attempt",
        "attempted | status=5 | schema@payload.status",
        "attempted | status=null | schema@payload.status",
        "attempted | reason=true | schema@payload.reason",
        "attempted | reason=[\"timeout\"] | schema@payload.reason",
        "attempted | status=sent | non_canonical@payload.status",
        "attempted | reason=network_down | non_canonical@payload.reason",
        "attempted | status=failed reason=retry_window_elapsed | non_canonical@payload.reason",
        &format!(
            "attempted | provider_message_id={over} | non_canonical@payload.provider_message_id"
        ),
        &format!("attempted | provider_message_id={max} | ok"),
        "attempted | provider_message_id=\"\" | non_canonical@payload.provider_message_id",
        "attempted | provider_message_id=m\u{e9} | non_canonical@payload.provider_message_id",
        "attempted | provider_message_id=\"m\\t1\" | non_canonical@payload.provider_message_id",
        "attempted | verdict=true | schema@payload.verdict",
        "attempted | verdict=0 | schema@payload.verdict",
        "attempted | verdict=[\"permanent\"] | schema@payload.verdict",
        "attempted | verdict=final | non_canonical@payload.verdict",
        "attempted | verdict=Permanent | non_canonical@payload.verdict",
        "attempted | verdict=\"\" | non_canonical@payload.verdict",
        &format!("attempted | {failed} verdict=final | non_canonical@payload.verdict"),
        &format!("attempted | {failed} verdict=true | schema@payload.verdict"),
        "attempted | zz_unlisted=true verdict=final | schema@payload.zz_unlisted",
        "attempted | status=sent verdict=final | non_canonical@payload.status",
        "attempted | channel=fax verdict=7 | non_canonical@payload.channel",
    ]);
}

/// Every channel, and every outcome rules 127 and 134 admit, at version 2: a `failed` attempt
/// with each verdict its reason takes, every other status with none.
#[test]
#[ignore = "pending E8-10"]
fn every_channel_and_outcome_is_admitted_at_version_2() {
    let mut cases = Vec::new();
    for channel in ["email", "phone", "slack", "sms", "telegram", "web_push"] {
        cases.push(format!("attempted | channel={channel} | ok"));
    }
    let failed = "status=failed provider_message_id=null";
    let receipt = "status=failed provider_message_id=m-1 verdict=permanent";
    for outcome in [
        "status=delivered reason=null provider_message_id=m-1 coalesced_into=m-0 verdict=null",
        "status=suppressed_quiet_hours reason=null provider_message_id=null verdict=null",
        "status=deferred_quiet_hours reason=null provider_message_id=null verdict=null",
        "status=abandoned reason=not_pending provider_message_id=null verdict=null",
        "status=abandoned reason=retry_window_ended provider_message_id=null verdict=null",
        &format!("{failed} reason=timeout verdict=retryable"),
        &format!("{failed} reason=rate_limited verdict=retryable"),
        &format!("{failed} reason=provider_error verdict=retryable"),
        &format!("{failed} reason=provider_error verdict=permanent"),
        &format!("{failed} reason=address_rejected verdict=permanent"),
        &format!("{failed} reason=auth_failed verdict=permanent"),
        &format!("{failed} reason=too_large verdict=retryable"),
        &format!("{failed} reason=too_large verdict=permanent"),
        &format!("{failed} reason=recipient_not_permitted verdict=permanent"),
        &format!("{failed} reason=address_missing verdict=permanent"),
        &format!("{receipt} reason=bounced"),
        &format!("{receipt} reason=complained"),
        &format!("{receipt} reason=unsubscribed"),
    ] {
        cases.push(format!("attempted | {outcome} | ok"));
    }
    run(&cases.iter().map(String::as_str).collect::<Vec<_>>());
}

/// Rules 126 to 129 at version 2, and rule 134 after them: `verdict` is refused only on a payload
/// those rules pass, so a wrong verdict never hides an earlier rule's path.
#[test]
#[ignore = "pending E8-10"]
fn an_attempts_consistency_rules_refuse_at_their_paths() {
    let failed = "status=failed provider_message_id=null verdict=permanent";
    let abandoned = "status=abandoned provider_message_id=null";
    run(&[
        "attempted | attempt=0 | schema@payload.attempt",
        "attempted | attempt=-1 | schema@payload.attempt",
        "attempted | reason=timeout | schema@payload.reason",
        &format!(
            "attempted | {abandoned} status=suppressed_quiet_hours reason=not_pending | schema@payload.reason"
        ),
        &format!("attempted | {failed} | schema@payload.reason"),
        &format!("attempted | {abandoned} reason=timeout | schema@payload.reason"),
        &format!("attempted | {failed} reason=not_pending | schema@payload.reason"),
        &format!("attempted | {abandoned} | schema@payload.reason"),
        &format!("attempted | {abandoned} reason=bounced | schema@payload.reason"),
        &format!("attempted | {failed} reason=retry_window_ended | schema@payload.reason"),
        &format!("attempted | {abandoned} reason=recipient_not_permitted | schema@payload.reason"),
        &format!("attempted | {abandoned} reason=address_missing | schema@payload.reason"),
        "attempted | provider_message_id=null | schema@payload.provider_message_id",
        &format!("attempted | {failed} reason=bounced | schema@payload.provider_message_id"),
        &format!("attempted | {failed} reason=complained | schema@payload.provider_message_id"),
        &format!("attempted | {failed} reason=unsubscribed | schema@payload.provider_message_id"),
        &format!(
            "attempted | {failed} reason=timeout provider_message_id=m-1 | schema@payload.provider_message_id"
        ),
        &format!(
            "attempted | {failed} reason=address_missing provider_message_id=m-1 | schema@payload.provider_message_id"
        ),
        "attempted | status=deferred_quiet_hours | schema@payload.provider_message_id",
        &format!(
            "attempted | {abandoned} status=suppressed_quiet_hours coalesced_into=m-1 | schema@payload.coalesced_into"
        ),
        &format!(
            "attempted | {abandoned} status=deferred_quiet_hours coalesced_into=m-1 | schema@payload.coalesced_into"
        ),
        &format!("attempted | {failed} reason=timeout verdict=retryable coalesced_into=m-1 | ok"),
        &format!("attempted | {abandoned} reason=retry_window_ended coalesced_into=m-1 | ok"),
        "attempted | verdict=permanent | schema@payload.verdict",
        &format!("attempted | {failed} reason=timeout | schema@payload.verdict"),
        &format!(
            "attempted | {failed} reason=provider_error verdict=null | schema@payload.verdict"
        ),
        &format!(
            "attempted | {abandoned} reason=not_pending verdict=retryable | schema@payload.verdict"
        ),
        &format!(
            "attempted | {failed} reason=bounced verdict=retryable | schema@payload.provider_message_id"
        ),
        &format!(
            "attempted | {abandoned} status=deferred_quiet_hours coalesced_into=m-1 verdict=retryable | schema@payload.coalesced_into"
        ),
        "attempted | attempt=0 verdict=permanent | schema@payload.attempt",
    ]);
}

/// One outcome rules 127 and 128 admit: `status`, `reason`, and `provider_message_id`, each `null`
/// as `None`.
type Attempt = (&'static str, Option<&'static str>, Option<&'static str>);

/// Every outcome rules 127 and 128 admit: the three statuses with no reason, both stops, and each
/// `failed` reason of [`FIXED`], a receipt naming its message.
fn outcomes() -> Vec<Attempt> {
    let receipt = |reason: &str| ["bounced", "complained", "unsubscribed"].contains(&reason);
    let mut all = vec![
        ("delivered", None, Some("m-1")),
        ("suppressed_quiet_hours", None, None),
        ("deferred_quiet_hours", None, None),
        ("abandoned", Some("not_pending"), None),
        ("abandoned", Some("retry_window_ended"), None),
    ];
    for (reason, _) in FIXED {
        all.push(("failed", Some(reason), receipt(reason).then_some("m-1")));
    }
    all
}

/// Rule 134 as the spec states it, from [`FIXED`]: `ok`, or `schema@payload.verdict`.
fn rule_134(status: &str, reason: Option<&str>, verdict: Option<&str>) -> &'static str {
    let fixed = FIXED.iter().find(|(r, _)| Some(*r) == reason);
    let holds = match (status, verdict) {
        ("failed", Some(v)) => fixed.is_some_and(|(_, f)| f.is_none_or(|f| f == v)),
        ("failed", None) => false,
        (_, verdict) => verdict.is_none(),
    };
    if holds {
        "ok"
    } else {
        "schema@payload.verdict"
    }
}

/// An attempt with `outcome` and `verdict`, each `None` written `null`.
fn attempt_with((status, reason, message): Attempt, verdict: Option<&str>) -> Vec<u8> {
    let json = |v: Option<&str>| v.map_or_else(|| "null".to_owned(), |v| format!("\"{v}\""));
    let edits = [
        ("status", json(Some(status))),
        ("reason", json(reason)),
        ("provider_message_id", json(message)),
        ("verdict", json(verdict)),
    ];
    edits.iter().fold(base("attempted"), |d, (member, value)| {
        edit(&d, &format!("payload.{member}"), Some(value))
    })
}

/// Rule 134 over every outcome rules 127 and 128 admit and every verdict, `null` included: `verdict`
/// is non-null exactly when `status` is `failed`, and a reason the spec fixes takes only its own.
/// The oracle is [`rule_134`], written from the spec's text; the counts are DEC-706 item 4's own:
/// five outcomes with no verdict, nine fixed reasons with one, and two reasons with either.
#[test]
#[ignore = "pending E8-10"]
fn the_verdict_is_set_exactly_on_a_failed_attempt_and_fixed_where_the_reason_fixes_it() {
    let mut admitted = 0;
    let mut differ = Vec::new();
    for attempt in outcomes() {
        for verdict in [None, Some("retryable"), Some("permanent")] {
            let want = rule_134(attempt.0, attempt.1, verdict);
            admitted += usize::from(want == "ok");
            let got = outcome(&attempt_with(attempt, verdict));
            if got != want {
                differ.push(format!("{attempt:?} {verdict:?}: got {got}, want {want}"));
            }
        }
    }
    assert_eq!(outcomes().len(), 16);
    assert_eq!(
        admitted,
        5 + 9 + 2 * 2,
        "each outcome admits only its verdicts"
    );
    assert!(differ.is_empty(), "{}", differ.join("\n"));
}

/// Rule 134 holds whatever the attempt's other members are: its channel, attempt number,
/// recipient, and, where rule 129 allows one, the combined message.
#[test]
#[ignore = "pending E8-10"]
fn rule_134_does_not_depend_on_the_other_members() {
    let channel = prop_oneof![
        Just("email"),
        Just("phone"),
        Just("slack"),
        Just("sms"),
        Just("telegram"),
        Just("web_push"),
    ];
    let members = (
        0usize..16,
        prop::option::of(prop_oneof![Just("retryable"), Just("permanent")]),
        channel,
        1u32..1_000_000,
        "u_[0-9a-z]{1,12}",
        prop::option::of("m-[0-9a-z]{1,40}"),
    );
    let checked = TestRunner::default().run(&members, |members| {
        let (which, verdict, channel, attempt, recipient, coalesced) = members;
        let attempted = outcomes()[which];
        let quiet = attempted.0.ends_with("_quiet_hours");
        let joined = coalesced
            .filter(|_| !quiet)
            .map_or_else(|| "null".to_owned(), |c| format!("\"{c}\""));
        let draft = attempt_with(attempted, verdict);
        let draft = edit(&draft, "payload.channel", Some(&format!("\"{channel}\"")));
        let draft = edit(&draft, "payload.attempt", Some(&attempt.to_string()));
        let draft = edit(
            &draft,
            "payload.recipient",
            Some(&format!("\"{recipient}\"")),
        );
        let draft = edit(&draft, "payload.coalesced_into", Some(&joined));
        let want = rule_134(attempted.0, attempted.1, verdict);
        prop_assert_eq!(outcome(&draft), want);
        Ok(())
    });
    assert_eq!(checked, Ok(()));
}

/// Version 1 is never edited and still replays (journal spec §8, DEC-706 item 6): a stored
/// version-1 `failed` attempt, which `append` no longer admits, passes §11's per-event checks as
/// stored, since verification re-hashes the stored body and never re-reads it against `append`'s
/// schemas.
#[test]
fn a_stored_version_1_attempt_still_verifies() {
    let failed = set(
        &version_1(&base("attempted")),
        "status=failed reason=provider_error provider_message_id=null",
    );
    let Value::Object(mut body) = parse(&failed).unwrap() else {
        panic!("a draft is an object")
    };
    for (name, value) in [
        ("seq", "1".to_owned()),
        ("prev_hash", format!("\"{}\"", Digest::ZERO.to_hex())),
        ("recorded_at", format!("\"{T}\"")),
    ] {
        body.insert(Key::new(name).unwrap(), parse(value.as_bytes()).unwrap());
    }
    let body = to_canonical(&Value::Object(body));
    let row = StoredEvent {
        stream_id: NTF.to_owned(),
        seq: 1,
        event_id: event_id(2),
        event_type: "NoticeAttempted".to_owned(),
        schema_version: 1,
        environment: "paper".to_owned(),
        recorded_at: T.to_owned(),
        prev_hash: Digest::ZERO,
        hash: Digest::of(&body),
        body,
    };
    let verified = verify_events(&[row], TrustedStart::GENESIS, &BTreeMap::new());
    assert_eq!(verified.map(|v| v.next_seq), Ok(2));
}
