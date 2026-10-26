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
//! `NoticeAttempted` is written at version 3 (journal spec v0.51, DEC-2505): version 2's members
//! (DEC-706's `verdict` last, held by rule 134) and `address_ref` after them, non-null exactly when
//! `channel` is `web_push` and then the only entry of `pii_refs` (rule 215), with the stop
//! `address_retired` on an abandoned web-push attempt only (rules 127 and 216). From v0.51 `append`
//! refuses versions 1 and 2 as `unknown_schema` at `payload`; both still verify as stored. Until
//! DEC-2505's implementation registers version 3, every test that parses an attempt is pending
//! E8-10: the journal refuses version 3 as `unknown_schema`, which is the answer they fail on.
//!
//! The control stream's `NotificationAddressChanged` (v0.51, DEC-2505 items 1 to 4) is closed at
//! version 1 with rules 210 to 214: the address is named by the `event_id` of the record that added
//! it and listed as the only `pii_refs` entry, an `added` carries fresh step-up by the member
//! themself, and a request's records commit with their `notification_address_changed` alert. Until
//! DEC-2505's implementation catalogues it, the journal refuses it as `unknown_event_type`, which
//! is the answer the tests pending E8-14 fail on. A `removed` without step-up rides a
//! `MemberDeactivated` or `MemberRemoved` (rule 214 (c)), which the journal registers only with
//! E9-7, so the test that commits one beside its deactivation is pending E9-7. The active set's
//! fold (DEC-2505 item 5) is not `mandate-journal`'s: no fold of addresses lives here, and the
//! dispatcher and workspace services read it.

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
    kill_switch:safety kill_switch_unapplied:safety agent_held:safety retirement_held:safety \
    account_restriction:safety protection:safety \
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

/// A draft on `stream`, at schema version 3 for `NoticeAttempted` (DEC-2505) and 1 for the rest.
fn envelope(stream: &str, id: u64, event_type: &str, causation: &str, payload: &str) -> Vec<u8> {
    let version = if event_type == "NoticeAttempted" {
        3
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
    alert_of(stream, id, subject, "risk_limit")
}

/// An `OwnerAlertSent` of `kind` on `stream`, with `event_id` `id`, about the event with `event_id`
/// `subject`.
fn alert_of(stream: &str, id: u64, subject: u64, kind: &str) -> Vec<u8> {
    let subject = event_id(subject);
    let payload = format!(r#"{{"subject":"{subject}","kind":"{kind}","owner_command":null}}"#);
    envelope(
        stream,
        id,
        "OwnerAlertSent",
        &format!("\"{subject}\""),
        &payload,
    )
}

/// The control stream every address change is on.
const CTL: &str = "ctl:ws_1";

/// The user principal whose addresses the base address changes are, and another member.
const MEMBER: &str = "01J8Z3M50000000000000000M1";
const OTHER_MEMBER: &str = "01J8Z3M50000000000000000M2";

/// The base `added`'s `event_id`, which is its address.
const ADDED: u64 = 20;

/// The address, added in an earlier batch, that the base `removed` and `retired` name.
const EARLIER: u64 = 10;

/// §9.2's step-up evidence, by passkey, 30 seconds before the envelope's `event_time` [`T`].
const STEP_UP: &str = r#"{"assertion_id":"assert_1","authenticated_at":"2026-09-21T13:59:30.000000000Z","method":"passkey"}"#;

/// The `build` of every `system` actor here.
fn build() -> String {
    format!("sha256:{}", "3".repeat(64))
}

/// A `NotificationAddressChanged` on [`CTL`] with `event_id` `id`, naming `member`'s address
/// `event_id(address)` and listing it as the only `pii_refs` entry. `added` and `removed` are the
/// member's own request, with [`STEP_UP`]; `retired` is a `removed` without step-up by the
/// `system` actor that deactivates the member (identity spec §5.2 step 3).
fn address_change(id: u64, kind: &str, address: u64, member: &str) -> Vec<u8> {
    let address = event_id(address);
    let (action, step_up, actor) = match kind {
        "retired" => (
            "removed",
            "null".to_owned(),
            format!(
                r#"{{"kind":"system","id":"workspace","version":"0.1.0","build":"{}"}}"#,
                build()
            ),
        ),
        action => (
            action,
            STEP_UP.to_owned(),
            format!(r#"{{"kind":"user","id":"{member}","version":"1","build":null}}"#),
        ),
    };
    format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{}","stream_id":"{CTL}",
        "event_type":"NotificationAddressChanged","schema_version":1,"event_time":"{T}",
        "clock_source":"local","causation_id":null,"correlation_id":null,"actor":{actor},
        "config_refs":{{}},"payload":{{"member":"{member}","channel":"web_push","action":"{action}",
        "address_ref":"{address}","step_up":{step_up}}},"artifact_refs":[],"pii_refs":["{address}"]}}"#,
        event_id(id)
    )
    .into_bytes()
}

/// The base draft `name`: `alert` (on the account stream), `opened`, `issued`, or `attempted` (an
/// email attempt, so with no `address_ref`), or one of the control stream's address changes:
/// `added`, `removed` (a request's, with step-up), or `retired` (riding a deactivation, without).
fn base(name: &str) -> Vec<u8> {
    let cause = event_id(100);
    let (id, event_type, causation, payload) = match name {
        "alert" => return alert(STREAM, 101, 100),
        "added" => return address_change(ADDED, "added", ADDED, MEMBER),
        "removed" => return address_change(21, "removed", EARLIER, MEMBER),
        "retired" => return address_change(22, "retired", EARLIER, MEMBER),
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
                "coalesced_into":null,"verdict":null,"address_ref":null}}"#
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
    assert_eq!(kinds.len(), 35, "notifications spec §3.2's 35 kinds");
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

/// Version 3 is the only `NoticeAttempted` `append` admits (DEC-2505 item 8): closed on the notice
/// stream and `wrong_stream` elsewhere, while version 2 in its own shape or with an `address_ref`,
/// version 1 in its own shape, and any later version are `unknown_schema` at `payload`, through
/// `Draft::parse` and `append` alike.
#[test]
fn an_attempt_is_appended_at_version_3_and_refused_at_versions_1_and_2() {
    closed_on(&[("NoticeAttempted", &[NTF])]);
    run(&[
        "attempted | | ok",
        "attempted | ^schema_version=1 | unknown_schema@payload",
        "attempted | ^schema_version=2 | unknown_schema@payload",
        "attempted | ^schema_version=4 | unknown_schema@payload",
    ]);
    let v2 = version_2(&base("attempted"));
    assert_eq!(
        outcome(&v2),
        "unknown_schema@payload",
        "version 2's own shape"
    );
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
        &on_channel(&base("attempted"), "web_push"),
        "status=failed reason=provider_error provider_message_id=null verdict=retryable",
    );
    let opened = journal.append(&ntf, 0, epoch, now(), &[&base("opened"), &base("issued")]);
    assert_eq!(opened.name(), "Committed", "{opened:?}");
    let attempted = journal.append(&ntf, 2, epoch, now(), &[&failed]);
    assert_eq!(attempted.name(), "Committed", "{attempted:?}");
    for (n, older, version) in [(3, v2, 2), (4, v1, 1)] {
        let later = set(&older, &format!("^event_id={}", event_id(n)));
        let refused = match journal.append(&ntf, 3, epoch, now(), &[&later]) {
            AppendOutcome::Invalid { draft, error } => {
                Some((draft, error.reason.code(), error.path))
            }
            _ => None,
        };
        assert_eq!(
            refused,
            Some((0, "unknown_schema", "payload".to_owned())),
            "version {version}"
        );
    }
}

/// `draft`, an attempt, in version 2's shape: `schema_version` 2 and no `address_ref`.
fn version_2(draft: &[u8]) -> Vec<u8> {
    edit(
        &set(draft, "^schema_version=2"),
        "payload.address_ref",
        None,
    )
}

/// `draft`, an attempt, in version 1's shape: `schema_version` 1 and neither `verdict` nor
/// `address_ref`.
fn version_1(draft: &[u8]) -> Vec<u8> {
    edit(
        &set(&version_2(draft), "^schema_version=1"),
        "payload.verdict",
        None,
    )
}

/// The web-push address the attempts here go to: an `added`'s `event_id`.
fn address() -> String {
    event_id(ADDED)
}

/// `draft`, an attempt, on `channel`, naming [`address`] and listing it in `pii_refs` on web push
/// (rule 215).
fn on_channel(draft: &[u8], channel: &str) -> Vec<u8> {
    let draft = edit(draft, "payload.channel", Some(&format!("\"{channel}\"")));
    if channel != "web_push" {
        return draft;
    }
    let named = format!("\"{}\"", address());
    let draft = edit(&draft, "payload.address_ref", Some(&named));
    edit(&draft, "pii_refs", Some(&format!("[{named}]")))
}

/// Version 3 is closed (DEC-720's form): without `verdict` or `address_ref`, or with an unlisted
/// member beside them, an attempt is `schema` at that member, on web push as on every other channel.
#[test]
fn every_member_of_an_attempt_is_required_and_no_unlisted_member_is_admitted() {
    closed(&base("attempted"));
    let failed = set(
        &base("attempted"),
        "status=failed reason=timeout provider_message_id=null verdict=retryable",
    );
    closed(&failed);
    closed(&on_channel(&failed, "web_push"));
}

/// Non-negotiable 6 and NT-1, NT-2 on version 3: an attempt holds no trading content, address, or
/// sentence, its `verdict` and `address_ref` included, and no part of a web-push address beside
/// the `address_ref` that names it.
#[test]
fn no_attempt_can_carry_trading_content() {
    no_unlisted_content("attempted");
    for member in [
        "provider_message_id",
        "coalesced_into",
        "recipient",
        "verdict",
        "address_ref",
    ] {
        no_sentence_in("attempted", member);
    }
    run(&["attempted | recipient=jane@example.com | non_canonical@payload.recipient"]);
    let pushed = on_channel(&base("attempted"), "web_push");
    for member in ADDRESS_PARTS {
        let planted = edit(&pushed, &format!("payload.{member}"), Some(ENDPOINT));
        assert_eq!(outcome(&planted), format!("schema@payload.{member}"));
    }
    let endpoint = edit(&pushed, "payload.address_ref", Some(ENDPOINT));
    assert_eq!(outcome(&endpoint), "non_canonical@payload.address_ref");
}

/// The parts of a web-push address and its key that no record holds (DEC-2505 item 2, NT-2,
/// workspace API API-11): the endpoint, the subscription's keys, the push service and its host, a
/// hash of any of them, and the VAPID `key_id`.
const ADDRESS_PARTS: [&str; 9] = [
    "endpoint",
    "p256dh",
    "auth",
    "push_service",
    "host",
    "endpoint_hash",
    "endpoint_sha256",
    "key_id",
    "vapid_key_id",
];

/// A web-push endpoint, as JSON text.
const ENDPOINT: &str = r#""https://fcm.googleapis.com/fcm/send/dXNlcl9hZGRyZXNz""#;

/// The member types of version 3, each refused in §9.1's order: a non-string `verdict` is
/// `schema`, a string outside `retryable` and `permanent` is `non_canonical`, `address_ref` is a
/// `ulid` or `null`, and an unlisted member or an earlier ill-typed member is reported before it.
#[test]
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
        "attempted | address_ref=not-a-ulid | non_canonical@payload.address_ref",
        "attempted | address_ref=u_1 | non_canonical@payload.address_ref",
        &format!("attempted | address_ref=pii_{ulid} | non_canonical@payload.address_ref"),
        "attempted | address_ref=5 | schema@payload.address_ref",
        "attempted | address_ref=true | schema@payload.address_ref",
        &format!("attempted | address_ref=[\"{ulid}\"] | schema@payload.address_ref"),
        "attempted | verdict=final address_ref=5 | non_canonical@payload.verdict",
        "attempted | zz_unlisted=true address_ref=5 | schema@payload.zz_unlisted",
    ]);
}

/// Every channel, and every outcome rules 127 and 134 admit, at version 3: a `failed` attempt
/// with each verdict its reason takes, every other status with none, on email and on web push at
/// one address, and the web-push stop `address_retired`.
#[test]
fn every_channel_and_outcome_is_admitted_at_version_3() {
    let mut cases = Vec::new();
    for channel in ["email", "phone", "slack", "sms", "telegram"] {
        cases.push(format!("attempted | channel={channel} | ok"));
    }
    let pushed = web_push();
    cases.push(format!("attempted | {pushed} | ok"));
    cases.push(format!(
        "attempted | {pushed} status=abandoned reason=address_retired provider_message_id=null | ok"
    ));
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
        cases.push(format!("attempted | {pushed} {outcome} | ok"));
    }
    run(&cases.iter().map(String::as_str).collect::<Vec<_>>());
}

/// The edits that put the base attempt on web push at [`address`], listed in `pii_refs`.
fn web_push() -> String {
    let named = address();
    format!("channel=web_push address_ref={named} ^pii_refs=[\"{named}\"]")
}

/// Rules 126 to 129 at version 3, and rule 134 after them: `verdict` is refused only on a payload
/// those rules pass, so a wrong verdict never hides an earlier rule's path.
#[test]
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
/// five outcomes with no verdict, nine fixed reasons with one, and two reasons with either. It holds
/// at version 3 unchanged (DEC-2505 item 6).
#[test]
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

/// Rule 134 holds whatever the attempt's other members are: its channel (on web push with its
/// address, as rule 215 requires), attempt number, recipient, and, where rule 129 allows one, the
/// combined message.
#[test]
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
        let draft = on_channel(&attempt_with(attempted, verdict), channel);
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

/// Rule 215: an attempt names its address exactly on web push, and lists it as the only entry of
/// `pii_refs`, which is empty on every other channel. It is checked after rule 134 and before rule
/// 216, and on a payload whose members are well typed.
#[test]
fn an_attempt_names_its_address_exactly_on_web_push_and_lists_it_in_pii_refs() {
    let named = address();
    let other = event_id(99);
    let pushed = web_push();
    let mut cases = vec![
        format!("attempted | {pushed} | ok"),
        "attempted | channel=web_push | schema@payload.address_ref".to_owned(),
        format!(
            "attempted | channel=web_push ^pii_refs=[\"{named}\"] | schema@payload.address_ref"
        ),
        format!("attempted | channel=web_push address_ref={named} | schema@pii_refs"),
        format!(
            "attempted | channel=web_push address_ref={named} ^pii_refs=[\"{other}\"] | schema@pii_refs"
        ),
        format!(
            "attempted | channel=web_push address_ref={named} ^pii_refs=[\"{named}\",\"{other}\"] | schema@pii_refs"
        ),
        format!(
            "attempted | channel=web_push address_ref={named} ^pii_refs=[\"pii_{named}\"] | schema@pii_refs"
        ),
        format!("attempted | ^pii_refs=[\"{named}\"] | schema@pii_refs"),
        format!(
            "attempted | {pushed} status=failed reason=timeout provider_message_id=null verdict=permanent | schema@payload.verdict"
        ),
        format!("attempted | {pushed} attempt=0 | schema@payload.attempt"),
        format!(
            "attempted | address_ref={named} status=abandoned reason=address_retired provider_message_id=null | schema@payload.address_ref"
        ),
        "attempted | channel=web_push address_ref=5 | schema@payload.address_ref".to_owned(),
    ];
    for channel in ["email", "phone", "slack", "sms", "telegram"] {
        cases.push(format!(
            "attempted | channel={channel} address_ref={named} ^pii_refs=[\"{named}\"] | schema@payload.address_ref"
        ));
        cases.push(format!(
            "attempted | channel={channel} address_ref={named} | schema@payload.address_ref"
        ));
    }
    run(&cases.iter().map(String::as_str).collect::<Vec<_>>());
}

/// Rules 127 and 216: `address_retired` is a stop, so only an `abandoned` attempt carries it, and
/// only on web push, where an address can leave the set; every other status, and every other
/// channel, refuses it at `payload.reason`. The other stops stay admitted on web push.
#[test]
fn address_retired_stops_only_an_abandoned_web_push_attempt() {
    let pushed = web_push();
    let retired = "reason=address_retired provider_message_id=null";
    let mut cases = vec![
        format!("attempted | {pushed} status=abandoned {retired} | ok"),
        format!("attempted | {pushed} status=abandoned {retired} coalesced_into=m-1 | ok"),
        format!(
            "attempted | {pushed} status=abandoned reason=not_pending provider_message_id=null | ok"
        ),
        format!(
            "attempted | {pushed} status=abandoned reason=retry_window_ended provider_message_id=null | ok"
        ),
        format!(
            "attempted | {pushed} status=failed {retired} verdict=permanent | schema@payload.reason"
        ),
        format!(
            "attempted | {pushed} status=failed {retired} verdict=retryable | schema@payload.reason"
        ),
        format!(
            "attempted | {pushed} status=delivered reason=address_retired | schema@payload.reason"
        ),
        format!(
            "attempted | {pushed} status=suppressed_quiet_hours {retired} | schema@payload.reason"
        ),
        format!(
            "attempted | {pushed} status=deferred_quiet_hours {retired} | schema@payload.reason"
        ),
        format!(
            "attempted | {pushed} status=abandoned reason=address_retiring provider_message_id=null | non_canonical@payload.reason"
        ),
        format!(
            "attempted | {pushed} status=abandoned {retired} verdict=permanent | schema@payload.verdict"
        ),
    ];
    for channel in ["email", "phone", "slack", "sms", "telegram"] {
        cases.push(format!(
            "attempted | channel={channel} status=abandoned {retired} | schema@payload.reason"
        ));
    }
    run(&cases.iter().map(String::as_str).collect::<Vec<_>>());
}

/// The push channels, in the schema's order.
const CHANNELS: [&str; 6] = ["email", "phone", "slack", "sms", "telegram", "web_push"];

/// An attempt's statuses, in the schema's order.
const STATUSES: [&str; 5] = [
    "delivered",
    "failed",
    "suppressed_quiet_hours",
    "deferred_quiet_hours",
    "abandoned",
];

/// Version 3's reasons, in the schema's order: version 2's and `address_retired`.
const REASONS: [&str; 14] = [
    "timeout",
    "rate_limited",
    "provider_error",
    "address_rejected",
    "auth_failed",
    "too_large",
    "recipient_not_permitted",
    "address_missing",
    "bounced",
    "complained",
    "unsubscribed",
    "not_pending",
    "retry_window_ended",
    "address_retired",
];

/// Rules 127, 215, and 216 as journal spec §9.15 states them, in its report order, on an attempt
/// whose `provider_message_id`, `coalesced_into`, and `verdict` already fit rules 128, 129, and
/// 134, and whose `pii_refs` lists its `address_ref` when it has one: `ok`, or `reason@path`.
fn rules_127_215_216(channel: &str, status: &str, reason: Option<&str>, named: bool) -> String {
    let stops = ["not_pending", "retry_window_ended", "address_retired"];
    let fits = match (status, reason) {
        ("failed", Some(r)) => !stops.contains(&r),
        ("abandoned", Some(r)) => stops.contains(&r),
        ("failed" | "abandoned", None) => false,
        (_, r) => r.is_none(),
    };
    let web = channel == "web_push";
    let failed = if !fits {
        Some("payload.reason")
    } else if named != web {
        Some("payload.address_ref")
    } else if reason == Some("address_retired") && !web {
        Some("payload.reason")
    } else {
        None
    };
    failed.map_or_else(|| "ok".to_owned(), |path| format!("schema@{path}"))
}

/// An attempt on `channel` with `status` and `reason`, naming [`address`] when `named`, whose
/// other members fit rules 128, 129, and 134 by construction: a message id on a delivery or a
/// receipt, no combined message, and on a `failed` attempt the verdict [`FIXED`] gives its reason,
/// or `permanent` where it gives none.
fn attempt_on(channel: &str, status: &str, reason: Option<&str>, named: bool) -> Vec<u8> {
    let json = |v: Option<&str>| v.map_or_else(|| "null".to_owned(), |v| format!("\"{v}\""));
    let receipt = reason.is_some_and(|r| ["bounced", "complained", "unsubscribed"].contains(&r));
    let message = (status == "delivered" || receipt).then_some("m-1");
    let fixed = FIXED
        .iter()
        .find(|(r, _)| Some(*r) == reason)
        .and_then(|(_, v)| *v);
    let verdict = (status == "failed").then(|| fixed.unwrap_or("permanent"));
    let address = address();
    let reference = named.then_some(address.as_str());
    let listed = reference.map_or_else(|| "[]".to_owned(), |a| format!("[\"{a}\"]"));
    let edits = [
        ("payload.channel", json(Some(channel))),
        ("payload.status", json(Some(status))),
        ("payload.reason", json(reason)),
        ("payload.provider_message_id", json(message)),
        ("payload.verdict", json(verdict)),
        ("payload.address_ref", json(reference)),
        ("pii_refs", listed),
    ];
    edits.iter().fold(base("attempted"), |d, (path, value)| {
        edit(&d, path, Some(value))
    })
}

/// Rules 127, 215, and 216 over every channel, status, reason (`null` included), and address
/// (named or not): 900 attempts, each judged against [`rules_127_215_216`], an oracle written from
/// the spec's text. The count admitted is the spec's own: on web push at one address, three
/// statuses with no reason, three stops, and eleven failures; on each of the five other channels
/// with none, the same but for `address_retired`.
#[test]
fn rules_127_215_and_216_hold_over_every_channel_status_reason_and_address() {
    let mut admitted = 0;
    let mut tried = 0;
    let mut differ = Vec::new();
    let reasons = std::iter::once(None).chain(REASONS.iter().copied().map(Some));
    for channel in CHANNELS {
        for status in STATUSES {
            for reason in reasons.clone() {
                for named in [false, true] {
                    tried += 1;
                    let want = rules_127_215_216(channel, status, reason, named);
                    admitted += usize::from(want == "ok");
                    let got = outcome(&attempt_on(channel, status, reason, named));
                    if got != want {
                        differ.push(format!(
                            "{channel} {status} {reason:?} named={named}: got {got}, want {want}"
                        ));
                    }
                }
            }
        }
    }
    assert_eq!(tried, 6 * 5 * 15 * 2);
    assert_eq!(admitted, (3 + 3 + 11) + 5 * (3 + 2 + 11));
    assert!(differ.is_empty(), "{}", differ.join("\n"));
}

/// `draft`, stored at `seq` 1 at `schema_version` `version`: the row §11 verifies.
fn stored(draft: &[u8], version: u64) -> StoredEvent {
    let Value::Object(mut body) = parse(draft).unwrap() else {
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
    StoredEvent {
        stream_id: NTF.to_owned(),
        seq: 1,
        event_id: event_id(2),
        event_type: "NoticeAttempted".to_owned(),
        schema_version: version,
        environment: "paper".to_owned(),
        recorded_at: T.to_owned(),
        prev_hash: Digest::ZERO,
        hash: Digest::of(&body),
        body,
    }
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
    let verified = verify_events(
        &[stored(&failed, 1)],
        TrustedStart::GENESIS,
        &BTreeMap::new(),
    );
    assert_eq!(verified.map(|v| v.next_seq), Ok(2));
}

/// Version 2 is never edited and still replays (journal spec §8, DEC-2505 item 8): a stored
/// version-2 web-push `failed` attempt, which names no address and which `append` refuses from
/// v0.51, passes §11's per-event checks as stored.
#[test]
fn a_stored_version_2_attempt_still_verifies() {
    let failed = set(
        &version_2(&base("attempted")),
        "channel=web_push status=failed reason=provider_error provider_message_id=null verdict=permanent",
    );
    let verified = verify_events(
        &[stored(&failed, 2)],
        TrustedStart::GENESIS,
        &BTreeMap::new(),
    );
    assert_eq!(verified.map(|v| v.next_seq), Ok(2));
}

/// `NotificationAddressChanged` is catalogued on the control stream only: closed there at version
/// 1, `wrong_stream` at `event_type` on every other stream, and any other version `unknown_schema`
/// (DEC-2505 item 1). The three base drafts, an add, a request's removal, and a deactivation's
/// removal, are admitted.
#[test]
fn an_address_change_is_closed_on_the_control_stream_only_at_version_1() {
    closed_on(&[("NotificationAddressChanged", &[CTL])]);
    run(&[
        "added | | ok",
        "removed | | ok",
        "retired | | ok",
        "added | ^schema_version=2 | unknown_schema@payload",
        "added | ^schema_version=0 | unknown_schema@payload",
        "added | ^stream_id=ntf:ws_1 | wrong_stream@event_type",
        "added | ^stream_id=acct:ws_1:ACCT1 | wrong_stream@event_type",
    ]);
}

/// The record is closed (DEC-720's form, as §9.15 closes it): without any one of its members, its
/// step-up evidence's members included, or with an unlisted one, it is `schema` at that member.
#[test]
fn every_member_of_an_address_change_is_required_and_no_unlisted_member_is_admitted() {
    for name in ["added", "removed", "retired"] {
        closed(&base(name));
    }
    for member in ["assertion_id", "authenticated_at", "method"] {
        let path = format!("payload.step_up.{member}");
        let missing = edit(&base("added"), &path, None);
        assert_eq!(outcome(&missing), format!("schema@{path}"));
    }
    run(&["added | step_up.zz_unlisted=true | schema@payload.step_up.zz_unlisted"]);
}

/// NT-2, workspace API API-11, and DEC-2505 item 2: no record holds an endpoint, a key, a push
/// service or its host, a hash of any of them, or the VAPID `key_id`, beside the opaque
/// `address_ref` or inside the step-up evidence; and no trading content or sentence either.
#[test]
fn no_address_change_can_hold_any_part_of_an_address() {
    for name in ["added", "removed", "retired"] {
        no_unlisted_content(name);
        for member in ADDRESS_PARTS {
            let planted = edit(&base(name), &format!("payload.{member}"), Some(ENDPOINT));
            assert_eq!(
                outcome(&planted),
                format!("schema@payload.{member}"),
                "{name}"
            );
        }
        let endpoint = edit(&base(name), "payload.address_ref", Some(ENDPOINT));
        let refused = outcome(&endpoint);
        assert_eq!(refused, "non_canonical@payload.address_ref", "{name}");
    }
    let tucked = edit(&base("added"), "payload.step_up.endpoint", Some(ENDPOINT));
    assert_eq!(outcome(&tucked), "schema@payload.step_up.endpoint");
    run(&["added | member=jane@example.com | non_canonical@payload.member"]);
}

/// The member types (§9.1, §9.12's `member`, §9.2's step-up evidence), each refused in §9.1's
/// order: an unlisted member first, then the listed members in the order given, a non-string
/// `schema` and a string outside the type `non_canonical`.
#[test]
fn an_address_changes_member_types_refuse_foreign_vocabulary() {
    run(&[
        "added | member=not-a-ulid | non_canonical@payload.member",
        "added | member=u_1 | non_canonical@payload.member",
        "added | member=5 | schema@payload.member",
        "added | member=null | schema@payload.member",
        "added | channel=email | non_canonical@payload.channel",
        "added | channel=web_inbox | non_canonical@payload.channel",
        "added | channel=Web_Push | non_canonical@payload.channel",
        "added | channel=null | schema@payload.channel",
        "added | action=replaced | non_canonical@payload.action",
        "added | action=Added | non_canonical@payload.action",
        "added | action=[\"added\"] | schema@payload.action",
        "added | address_ref=not-a-ulid | non_canonical@payload.address_ref",
        &format!("added | address_ref={NOTICE} | non_canonical@payload.address_ref"),
        "added | address_ref=null | schema@payload.address_ref",
        "added | address_ref=5 | schema@payload.address_ref",
        "added | step_up=true | schema@payload.step_up",
        "added | step_up=\"assert_1\" | schema@payload.step_up",
        "added | step_up.assertion_id=5 | schema@payload.step_up.assertion_id",
        "added | step_up.assertion_id=\"\" | non_canonical@payload.step_up.assertion_id",
        "added | step_up.authenticated_at=yesterday | non_canonical@payload.step_up.authenticated_at",
        "added | step_up.authenticated_at=5 | schema@payload.step_up.authenticated_at",
        "added | step_up.method=5 | schema@payload.step_up.method",
        "added | step_up.method=\"\" | non_canonical@payload.step_up.method",
        "added | zz_unlisted=true member=5 | schema@payload.zz_unlisted",
        "added | member=5 channel=email | schema@payload.member",
        "added | channel=email address_ref=5 | non_canonical@payload.channel",
        "added | address_ref=5 step_up=true | schema@payload.address_ref",
    ]);
}

/// Rules 210 and 211: an `added` names its address by its own `event_id`, a `removed` names an
/// earlier one, and `pii_refs` is exactly `[address_ref]`, so redaction reaches the vault entry and
/// the record lists nothing else. Rule 210 is reported before rule 211, and both before rules 212
/// and 213, on a payload whose members are well typed.
#[test]
fn an_address_is_named_by_the_event_that_added_it_and_listed_in_pii_refs() {
    let added = event_id(ADDED);
    let earlier = event_id(EARLIER);
    let removed = event_id(21);
    let retired = event_id(22);
    let other = event_id(99);
    run(&[
        &format!(
            "added | address_ref={earlier} ^pii_refs=[\"{earlier}\"] | schema@payload.address_ref"
        ),
        &format!(
            "removed | address_ref={removed} ^pii_refs=[\"{removed}\"] | schema@payload.address_ref"
        ),
        &format!(
            "retired | address_ref={retired} ^pii_refs=[\"{retired}\"] | schema@payload.address_ref"
        ),
        &format!("removed | address_ref={added} ^pii_refs=[\"{added}\"] | ok"),
        "added | ^pii_refs=[] | schema@pii_refs",
        "removed | ^pii_refs=[] | schema@pii_refs",
        "retired | ^pii_refs=[] | schema@pii_refs",
        &format!("added | ^pii_refs=[\"{other}\"] | schema@pii_refs"),
        &format!("removed | ^pii_refs=[\"{removed}\"] | schema@pii_refs"),
        &format!("added | ^pii_refs=[\"{added}\",\"{other}\"] | schema@pii_refs"),
        &format!("added | ^pii_refs=[\"pii_{added}\"] | schema@pii_refs"),
        &format!("added | address_ref={earlier} | schema@payload.address_ref"),
        &format!("added | address_ref={earlier} channel=email | non_canonical@payload.channel"),
        "added | ^pii_refs=[] step_up=null | schema@pii_refs",
        &format!("added | ^pii_refs=[] ^actor.id={OTHER_MEMBER} | schema@pii_refs"),
    ]);
}

/// The step-up evidence at `authenticated_at` `2026-09-21T{time}Z`, as one `set` edit.
fn authenticated(time: &str) -> String {
    format!("step_up.authenticated_at=2026-09-21T{time}Z")
}

/// Rule 212: an `added` carries step-up, and non-null evidence, on an `added` or a `removed`, is
/// valid at the envelope's `event_time` as rule 102 reads it: `passkey`, or in a non-`live`
/// envelope also `cli_confirm`, authenticated 0 to 300 seconds before. The method is reported
/// before the time, and rule 212 before rule 213. A `removed` without step-up passes the record's
/// rules; the batch decides whether it rides a deactivation (rule 214).
#[test]
fn an_address_change_carries_fresh_step_up_unless_it_rides_a_deactivation() {
    let build = build();
    let stale = authenticated("13:54:59.999999999");
    run(&[
        "added | step_up=null | schema@payload.step_up",
        "removed | step_up=null | ok",
        "retired | | ok",
        "added | step_up.method=cli_confirm | ok",
        "removed | step_up.method=cli_confirm | ok",
        "added | step_up.method=password | schema@payload.step_up.method",
        "added | step_up.method=oidc | schema@payload.step_up.method",
        "added | step_up.method=email_link | schema@payload.step_up.method",
        "added | step_up.method=Passkey | schema@payload.step_up.method",
        "removed | step_up.method=totp | schema@payload.step_up.method",
        "added | ^environment=live | ok",
        "removed | ^environment=live | ok",
        "added | ^environment=live step_up.method=cli_confirm | schema@payload.step_up.method",
        "removed | ^environment=live step_up.method=cli_confirm | schema@payload.step_up.method",
        &format!("added | {} | ok", authenticated("13:55:00.000000000")),
        &format!("added | {stale} | schema@payload.step_up.authenticated_at"),
        &format!("added | {} | ok", authenticated("14:00:00.000000000")),
        &format!(
            "added | {} | schema@payload.step_up.authenticated_at",
            authenticated("14:00:00.000000001")
        ),
        &format!("removed | {stale} | schema@payload.step_up.authenticated_at"),
        &format!(
            "removed | {} | schema@payload.step_up.authenticated_at",
            authenticated("14:05:00.000000000")
        ),
        &format!("added | step_up.method=password {stale} | schema@payload.step_up.method"),
        &format!(
            "added | step_up=null ^actor.kind=system ^actor.build={build} | schema@payload.step_up"
        ),
        &format!(
            "added | {stale} ^actor.id={OTHER_MEMBER} | schema@payload.step_up.authenticated_at"
        ),
    ]);
}

/// Rule 213: with step-up the actor is a `user` who is the `member`, so a member sets and removes
/// only their own addresses (API-6); without step-up, a removal riding a deactivation, it is a
/// `user` or the `system`, as rule 98 has the deactivation's. A `client` is refused at
/// `actor.kind` before the payload is read (rule 83).
#[test]
fn only_the_member_changes_their_own_addresses() {
    let build = build();
    run(&[
        &format!("added | ^actor.id={OTHER_MEMBER} | schema@payload.member"),
        &format!("removed | ^actor.id={OTHER_MEMBER} | schema@payload.member"),
        &format!("added | member={OTHER_MEMBER} | schema@payload.member"),
        &format!("added | member={OTHER_MEMBER} ^actor.id={OTHER_MEMBER} | ok"),
        &format!("added | ^actor.kind=system ^actor.build={build} | schema@actor.kind"),
        &format!("removed | ^actor.kind=system ^actor.build={build} | schema@actor.kind"),
        "added | ^actor.kind=platform_operator | schema@actor.kind",
        "added | ^actor.kind=broker | schema@actor.kind",
        &format!("added | ^actor.kind=agent ^actor.build={build} | schema@actor.kind"),
        &format!("retired | step_up={STEP_UP} | schema@actor.kind"),
        &format!(
            "retired | step_up={STEP_UP} ^actor.kind=user ^actor.id={MEMBER} ^actor.build=null | ok"
        ),
        &format!("retired | ^actor.kind=user ^actor.id={OTHER_MEMBER} ^actor.build=null | ok"),
        &format!("retired | ^actor.kind=user ^actor.id={MEMBER} ^actor.build=null | ok"),
        "retired | ^actor.kind=platform_operator ^actor.build=null | schema@actor.kind",
        "retired | ^actor.kind=broker ^actor.build=null | schema@actor.kind",
        "retired | ^actor.kind=agent | schema@actor.kind",
        "added | ^actor.kind=client ^actor.on_behalf_of=u_1 | schema@actor.kind",
        "added | ^actor.kind=client ^actor.on_behalf_of=u_1 member=5 | schema@actor.kind",
        "retired | ^actor.kind=client ^actor.on_behalf_of=u_1 ^actor.build=null | schema@actor.kind",
        &format!("added | ^actor.id={OTHER_MEMBER} step_up=null | schema@payload.step_up"),
    ]);
}

/// A member's own `added` with `event_id` `id`, which is its address.
fn add(id: u64) -> Vec<u8> {
    address_change(id, "added", id, MEMBER)
}

/// A member's own `removed`, with step-up, with `event_id` `id`, of the address added at `address`.
fn remove(id: u64, address: u64) -> Vec<u8> {
    address_change(id, "removed", address, MEMBER)
}

/// A deactivation's `removed`, without step-up, with `event_id` `id`, of the address added at
/// `address`.
fn retire(id: u64, address: u64) -> Vec<u8> {
    address_change(id, "retired", address, MEMBER)
}

/// The `notification_address_changed` alert on [`CTL`] with `event_id` `id` about `subject`.
fn notify(id: u64, subject: u64) -> Vec<u8> {
    alert_of(CTL, id, subject, "notification_address_changed")
}

/// Rule 214's refusal is reported at the first `NotificationAddressChanged` that breaks (a) to (d)
/// at this path, and rule 130's and (e)'s at the alert's.
const AT_ADDRESS: &str = "payload.address_ref";
const AT_SUBJECT: &str = "payload.subject";

/// One batch: its name, its drafts, and `None` when it is admitted, or every `(draft, path)` at
/// which the spec's report rule lets it be refused as `schema`. Where one batch breaks two clauses
/// whose report places differ, or a clause two records break together, the spec does not say which
/// is first, so each such place is listed.
type Batch = (
    &'static str,
    Vec<Vec<u8>>,
    Option<Vec<(usize, &'static str)>>,
);

/// Judges each batch by `check_batch` after every draft passes `Draft::parse` alone, reporting
/// every one that differs.
fn judge(batches: Vec<Batch>) {
    let mut failed = Vec::new();
    for (name, batch, refused) in batches {
        let drafts: Result<Vec<Draft>, _> =
            batch.iter().map(|d| Draft::parse(d.as_slice())).collect();
        let drafts = match drafts {
            Ok(drafts) => drafts,
            Err(e) => {
                failed.push(format!(
                    "{name}: a draft is refused alone, {}@{}",
                    e.reason.code(),
                    e.path
                ));
                continue;
            }
        };
        let got = check_batch(&drafts)
            .err()
            .map(|(i, e)| (i, e.reason.code(), e.path));
        let fits = match (&got, &refused) {
            (None, None) => true,
            (Some((i, code, path)), Some(places)) => {
                *code == "schema" && places.iter().any(|(j, p)| j == i && *p == path.as_str())
            }
            _ => false,
        };
        if !fits {
            failed.push(format!("{name}: got {got:?}, want {refused:?}"));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Rule 214 on a request's batch, after rule 130: at most one `added` and one `removed`, of one
/// member with one step-up; each `added`, and a lone `removed`, is the subject of a
/// `notification_address_changed` alert, a replacement's `removed` of none; a `removed` without
/// step-up rides a deactivation, which no batch here holds; no `removed` names an address twice or
/// the batch's own `added`; and every such alert names a record that clause alerts. A commit
/// through `append` shows the same: an add with its alert, then a lone removal refused, then the
/// removal with its own alert.
#[test]
fn a_request_commits_its_address_change_with_its_alert_in_one_batch() {
    let other = address_change(21, "removed", EARLIER, OTHER_MEMBER);
    let second = set(&remove(21, EARLIER), "step_up.assertion_id=assert_2");
    let unasked = set(&remove(21, EARLIER), "step_up=null");
    let batches: Vec<Batch> = vec![
        ("an add and its alert", vec![add(20), notify(101, 20)], None),
        (
            "a removal and its alert",
            vec![remove(21, EARLIER), notify(102, 21)],
            None,
        ),
        (
            "a replacement, removal first",
            vec![remove(21, EARLIER), add(20), notify(101, 20)],
            None,
        ),
        (
            "a replacement, add first",
            vec![add(20), remove(21, EARLIER), notify(101, 20)],
            None,
        ),
        (
            "an add without its alert",
            vec![add(20)],
            Some(vec![(0, AT_ADDRESS)]),
        ),
        (
            "a removal without its alert",
            vec![remove(21, EARLIER)],
            Some(vec![(0, AT_ADDRESS)]),
        ),
        (
            "an add alerted as another kind",
            vec![add(20), alert_of(CTL, 101, 20, "new_device")],
            Some(vec![(0, AT_ADDRESS)]),
        ),
        (
            "a replacement without its alert",
            vec![remove(21, EARLIER), add(20)],
            Some(vec![(1, AT_ADDRESS)]),
        ),
        (
            "a replacement whose removal is alerted too",
            vec![
                remove(21, EARLIER),
                add(20),
                notify(101, 20),
                notify(102, 21),
            ],
            Some(vec![(0, AT_ADDRESS), (3, AT_SUBJECT)]),
        ),
        (
            "a replacement alerted on its removal only",
            vec![remove(21, EARLIER), add(20), notify(102, 21)],
            Some(vec![(0, AT_ADDRESS), (1, AT_ADDRESS), (2, AT_SUBJECT)]),
        ),
        (
            "two adds",
            vec![add(20), add(23), notify(101, 20), notify(103, 23)],
            Some(vec![(0, AT_ADDRESS), (1, AT_ADDRESS)]),
        ),
        (
            "two removals",
            vec![
                remove(21, EARLIER),
                remove(24, 11),
                notify(102, 21),
                notify(104, 24),
            ],
            Some(vec![(0, AT_ADDRESS), (1, AT_ADDRESS)]),
        ),
        (
            "a replacement across two members",
            vec![other, add(20), notify(101, 20)],
            Some(vec![(0, AT_ADDRESS), (1, AT_ADDRESS)]),
        ),
        (
            "a replacement under two step-ups",
            vec![second, add(20), notify(101, 20)],
            Some(vec![(0, AT_ADDRESS), (1, AT_ADDRESS)]),
        ),
        (
            "a removal without step-up and no deactivation",
            vec![retire(22, EARLIER)],
            Some(vec![(0, AT_ADDRESS)]),
        ),
        (
            "a member's own removal without step-up",
            vec![unasked],
            Some(vec![(0, AT_ADDRESS)]),
        ),
        (
            "a removal without step-up beside a request",
            vec![retire(22, 11), add(20), notify(101, 20)],
            Some(vec![(0, AT_ADDRESS)]),
        ),
        (
            "a removal of the batch's own add",
            vec![add(20), remove(21, 20), notify(101, 20)],
            Some(vec![(1, AT_ADDRESS)]),
        ),
        (
            "an address alert on another record",
            vec![
                mark_draft(1, "1"),
                alert_of(STREAM, 101, 1, "notification_address_changed"),
            ],
            Some(vec![(1, AT_SUBJECT)]),
        ),
        (
            "two alerts on one add, rule 130 first",
            vec![add(20), notify(101, 20), notify(102, 20)],
            Some(vec![(2, AT_SUBJECT)]),
        ),
        (
            "an alert before its add, rule 130 first",
            vec![notify(101, 20), add(20)],
            Some(vec![(0, AT_SUBJECT)]),
        ),
    ];
    judge(batches);
    let mut journal = MemoryJournal::new();
    let ctl = StreamId::parse(CTL).unwrap();
    let epoch = journal.take_ownership(&ctl);
    let opened = envelope(
        CTL,
        0,
        "StreamOpened",
        "null",
        r#"{"stream_type":"control","workspace_id":"ws_1"}"#,
    );
    let first = journal.append(
        &ctl,
        0,
        epoch,
        now(),
        &[&opened, &add(20), &notify(101, 20)],
    );
    assert_eq!(first.name(), "Committed", "{first:?}");
    let lone = remove(21, 20);
    let refused = match journal.append(&ctl, 3, epoch, now(), &[&lone]) {
        AppendOutcome::Invalid { draft, error } => Some((draft, error.reason.code(), error.path)),
        _ => None,
    };
    assert_eq!(refused, Some((0, "schema", AT_ADDRESS.to_owned())));
    let alerted = journal.append(&ctl, 3, epoch, now(), &[&lone, &notify(102, 21)]);
    assert_eq!(alerted.name(), "Committed", "{alerted:?}");
}

/// An admin's user principal.
const ADMIN: &str = "01J8Z3M50000000000000000A1";

/// A `MemberDeactivated` or `MemberRemoved` (`event_type`) of `member` on [`CTL`] with `event_id`
/// `id`, as journal spec §9.12 closes it: by the `system` actor that writes [`retire`]'s removals,
/// with reason `deprovisioned` or `org_deleted` (rules 97, 98, and 105), or, when `by_admin`, by
/// the [`ADMIN`] user through a session, with reason `admin` (rule 99).
fn ended(id: u64, event_type: &str, member: &str, by_admin: bool) -> Vec<u8> {
    let (actor, by, reason, session) = if by_admin {
        (
            format!(r#"{{"kind":"user","id":"{ADMIN}","version":"1","build":null}}"#),
            ADMIN.to_owned(),
            "admin",
            "\"session_admin\"",
        )
    } else {
        let reason = if event_type == "MemberRemoved" {
            "org_deleted"
        } else {
            "deprovisioned"
        };
        (
            format!(
                r#"{{"kind":"system","id":"workspace","version":"0.1.0","build":"{}"}}"#,
                build()
            ),
            "workspace".to_owned(),
            reason,
            "null",
        )
    };
    format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{}","stream_id":"{CTL}",
        "event_type":"{event_type}","schema_version":1,"event_time":"{T}","clock_source":"local",
        "causation_id":null,"correlation_id":null,"actor":{actor},"config_refs":{{}},
        "payload":{{"member":"{member}","by":"{by}","reason":"{reason}","session_ref":{session}}},
        "artifact_refs":[],"pii_refs":[]}}"#,
        event_id(id)
    )
    .into_bytes()
}

/// Rule 214 (c) and (d) on a deactivation's batch: a `removed` without step-up rides a
/// `MemberDeactivated` or `MemberRemoved` of its own member by its own actor, has no alert, shares
/// its batch with no request, and names no address twice. It needs E9-7's registration of those
/// two records as well as E8-14's of this one, so it is pending until the later of the two.
#[test]
#[ignore = "pending E9-7"]
fn a_removal_without_step_up_rides_its_members_deactivation() {
    let deactivated = || ended(30, "MemberDeactivated", MEMBER, false);
    let by_admin =
        format!("^actor.kind=user ^actor.id={ADMIN} ^actor.version=\"1\" ^actor.build=null");
    let batches: Vec<Batch> = vec![
        (
            "a deactivation and its removals",
            vec![deactivated(), retire(22, EARLIER), retire(23, 11)],
            None,
        ),
        (
            "an organization's deletion and a removal",
            vec![
                ended(30, "MemberRemoved", MEMBER, false),
                retire(22, EARLIER),
            ],
            None,
        ),
        (
            "an admin's deactivation and the admin's removal",
            vec![
                ended(30, "MemberDeactivated", MEMBER, true),
                set(&retire(22, EARLIER), &by_admin),
            ],
            None,
        ),
        (
            "another member's deactivation",
            vec![
                ended(30, "MemberDeactivated", OTHER_MEMBER, false),
                retire(22, EARLIER),
            ],
            Some(vec![(1, AT_ADDRESS)]),
        ),
        (
            "a deactivation by another actor",
            vec![
                ended(30, "MemberDeactivated", MEMBER, true),
                retire(22, EARLIER),
            ],
            Some(vec![(1, AT_ADDRESS)]),
        ),
        (
            "a deactivation's removal alerted",
            vec![deactivated(), retire(22, EARLIER), notify(101, 22)],
            Some(vec![(1, AT_ADDRESS), (2, AT_SUBJECT)]),
        ),
        (
            "one address removed twice",
            vec![deactivated(), retire(22, EARLIER), retire(23, EARLIER)],
            Some(vec![(1, AT_ADDRESS), (2, AT_ADDRESS)]),
        ),
        (
            "a deactivation's removal beside a request",
            vec![deactivated(), retire(22, 11), add(20), notify(101, 20)],
            Some(vec![(1, AT_ADDRESS), (2, AT_ADDRESS)]),
        ),
    ];
    judge(batches);
}
