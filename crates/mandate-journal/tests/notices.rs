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

mod common;

use common::{STREAM, T, edit, event_id, journal_with, mark_draft, now, stream};
use mandate_canon::{Value, parse};
use mandate_journal::{AppendOutcome, Draft, StreamId, StreamType, check_batch};

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

fn envelope(stream: &str, id: u64, event_type: &str, causation: &str, payload: &str) -> Vec<u8> {
    format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{}","stream_id":"{stream}",
        "event_type":"{event_type}","schema_version":1,"event_time":"{T}","clock_source":"local",
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
                "coalesced_into":null}}"#
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
#[ignore = "pending E8-9"]
fn the_notice_stream_id_parses_to_its_own_stream_type() {
    let id = StreamId::parse(NTF).map(|s| (s.stream_type(), s.as_str().to_owned()));
    assert_eq!(id, Some((StreamType::Notice, NTF.to_owned())));
    for bad in ["ntf:", "ntf:ws_1:x", "ntf:ws 1", "ntf:ws.1", "notice:ws_1"] {
        assert_eq!(StreamId::parse(bad), None, "{bad}");
    }
}

#[test]
#[ignore = "pending E8-9"]
fn the_alert_and_notice_records_are_catalogued_and_closed_on_their_streams() {
    let homes: [(&str, &[&str]); 4] = [
        ("OwnerAlertSent", &STREAMS[..3]),
        ("NoticeIssued", &[NTF]),
        ("NoticeAttempted", &[NTF]),
        ("StreamOpened", &STREAMS),
    ];
    for (event_type, home) in homes {
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
#[ignore = "pending E8-9"]
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
#[ignore = "pending E8-9"]
fn every_member_is_required_and_no_unlisted_member_is_admitted() {
    let alerts = STREAMS[..3].iter().map(|s| alert(s, 101, 100));
    let notices = ["opened", "issued", "attempted"].map(base);
    for draft in alerts.chain(notices) {
        assert_eq!(outcome(&draft), "ok");
        let Some(Value::Object(members)) = parse(&draft).unwrap().get("payload").cloned() else {
            panic!("a payload is an object")
        };
        for member in members.keys() {
            let path = format!("payload.{}", member.as_str());
            assert_eq!(
                outcome(&edit(&draft, &path, None)),
                format!("schema@{path}")
            );
        }
        let extra = set(&draft, "zz_unlisted=true");
        assert_eq!(outcome(&extra), "schema@payload.zz_unlisted");
    }
}

/// Non-negotiable 6 and NT-1, NT-2: no record admits trading content, an address, or a sentence.
#[test]
#[ignore = "pending E8-9"]
fn no_alert_or_notice_record_can_carry_trading_content() {
    let content = r#""AAPL buy 10 @ 150.25""#;
    for name in ["alert", "opened", "issued", "attempted"] {
        for member in [
            "instrument",
            "side",
            "qty",
            "price",
            "text",
            "message",
            "address",
        ] {
            let planted = edit(&base(name), &format!("payload.{member}"), Some(content));
            assert_eq!(outcome(&planted), format!("schema@payload.{member}"));
        }
    }
    for (name, member) in [
        ("attempted", "provider_message_id"),
        ("attempted", "coalesced_into"),
        ("attempted", "recipient"),
        ("issued", "notice"),
        ("issued", "cause_stream"),
    ] {
        let planted = edit(&base(name), &format!("payload.{member}"), Some(content));
        assert_eq!(outcome(&planted), format!("non_canonical@payload.{member}"));
    }
    let listed = edit(
        &base("issued"),
        "payload.recipients",
        Some(&format!("[{content}]")),
    );
    assert_eq!(outcome(&listed), "non_canonical@payload.recipients[0]");
    run(&[
        "attempted | recipient=jane@example.com | non_canonical@payload.recipient",
        "issued | recipients=[\"jane@example.com\"] | non_canonical@payload.recipients[0]",
    ]);
}

#[test]
#[ignore = "pending E8-9"]
fn the_member_types_refuse_event_ids_and_foreign_vocabulary() {
    let ulid = event_id(5);
    let upper = NOTICE.to_uppercase();
    let short = &NOTICE[1..];
    let over = "x".repeat(257);
    let max = "x".repeat(256);
    run(&[
        &format!("issued | notice={ulid} | non_canonical@payload.notice"),
        &format!("issued | notice={upper} | non_canonical@payload.notice"),
        &format!("issued | notice={short} | non_canonical@payload.notice"),
        &format!("issued | notice={NOTICE}0 | non_canonical@payload.notice"),
        &format!("attempted | notice={ulid} | non_canonical@payload.notice"),
        "issued | notice=5 | schema@payload.notice",
        "issued | kind=tripwire_fired | non_canonical@payload.kind",
        "issued | class=urgent | non_canonical@payload.class",
        "issued | cause=not-a-ulid | non_canonical@payload.cause",
        "issued | cause_stream=ws_1 | non_canonical@payload.cause_stream",
        "issued | cause_stream=acct:ws_1 | non_canonical@payload.cause_stream",
        "issued | cause_stream=acct:ws.1:A | non_canonical@payload.cause_stream",
        "issued | recipients=u_1 | schema@payload.recipients",
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
#[ignore = "pending E8-9"]
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
    for channel in ["email", "phone", "slack", "sms", "telegram", "web_push"] {
        cases.push(format!("attempted | channel={channel} | ok"));
    }
    for outcome in [
        "status=delivered reason=null provider_message_id=m-1 coalesced_into=m-0",
        "status=suppressed_quiet_hours reason=null provider_message_id=null",
        "status=deferred_quiet_hours reason=null provider_message_id=null",
        "status=abandoned reason=not_pending provider_message_id=null",
        "status=abandoned reason=retry_window_ended provider_message_id=null",
        "status=failed reason=timeout provider_message_id=null",
        "status=failed reason=rate_limited provider_message_id=null",
        "status=failed reason=provider_error provider_message_id=null",
        "status=failed reason=address_rejected provider_message_id=null",
        "status=failed reason=auth_failed provider_message_id=null",
        "status=failed reason=too_large provider_message_id=null",
        "status=failed reason=recipient_not_permitted provider_message_id=null",
        "status=failed reason=address_missing provider_message_id=null",
        "status=failed reason=bounced provider_message_id=m-1",
        "status=failed reason=complained provider_message_id=m-1",
        "status=failed reason=unsubscribed provider_message_id=m-1",
    ] {
        cases.push(format!("attempted | {outcome} | ok"));
    }
    run(&cases.iter().map(String::as_str).collect::<Vec<_>>());
}

#[test]
#[ignore = "pending E8-9"]
fn the_consistency_rules_refuse_at_their_paths() {
    let other = event_id(2);
    let command = event_id(7);
    let failed = "status=failed provider_message_id=null";
    let abandoned = "status=abandoned provider_message_id=null";
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
        &format!("attempted | {failed} reason=timeout coalesced_into=m-1 | ok"),
        &format!("attempted | {abandoned} reason=retry_window_ended coalesced_into=m-1 | ok"),
    ]);
}

/// Rule N12 spans a batch: an alert's subject is an earlier draft of its own batch that is not an
/// alert, and no two alerts in the batch name one subject.
#[test]
#[ignore = "pending E8-9"]
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
