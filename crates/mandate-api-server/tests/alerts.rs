//! The alerts read's fold (`read-models/alerts`; workspace API spec §4.7; DEC-2141): every
//! `OwnerAlertSent` on the streams the control stream names, most recent first, each with its
//! class, stream, subject, and notice, and only the asking member's own deliveries. The oracles
//! are the alerts each test lays down and the schema's own kind table, never the fold's reading of
//! the rows.

use mandate_api_server::alerts::{
    AlertView, DeliveryView, Followed, NoticeView, Stream, Unreadable, WITHHELD, alerts, class,
    followed, recipient,
};
use mandate_canon::{Digest, parse, to_canonical};
use mandate_journal::StoredEvent;
use proptest::prelude::*;

const WS: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Y";
const ME: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Z";
const OTHER: &str = "01JB3K6F0C9R2V7N4M8Q1T5W41";

/// Rows laid stream by stream, with event ids and times unique and increasing across all of them.
#[derive(Default)]
struct Journal {
    next: u64,
}

impl Journal {
    fn row(&mut self, stream: &str, seq: u64, event_type: &str, payload: &str) -> StoredEvent {
        self.next += 1;
        let n = self.next;
        let event_id = format!("01JBH3BTZ8K2M4N6P8R0{n:06}");
        let text = format!(
            r#"{{"environment":"paper","event_id":"{event_id}","event_type":"{event_type}","payload":{payload},"stream_id":"{stream}"}}"#
        );
        let body = to_canonical(&parse(text.as_bytes()).expect("JSON"));
        StoredEvent {
            stream_id: stream.to_owned(),
            seq,
            event_id,
            event_type: event_type.to_owned(),
            schema_version: 1,
            environment: "paper".to_owned(),
            recorded_at: format!("2026-10-17T10:{:02}:{:02}.000000000Z", n / 60, n % 60),
            prev_hash: Digest::ZERO,
            hash: Digest::of(&body),
            body,
        }
    }
}

fn agent(id: &str) -> String {
    format!("agent:{WS}:{id}")
}

fn account(id: &str) -> String {
    format!("acct:{WS}:{id}")
}

fn ntf() -> String {
    format!("ntf:{WS}")
}

fn alert_payload(kind: &str, subject: &str, command: Option<&str>) -> String {
    let command = command.map_or("null".to_owned(), |c| format!(r#""{c}""#));
    format!(r#"{{"kind":"{kind}","owner_command":{command},"subject":"{subject}"}}"#)
}

fn issued(notice: &str, kind: &str, cause: &str, stream: &str) -> String {
    format!(
        r#"{{"cause":"{cause}","cause_stream":"{stream}","class":"safety","kind":"{kind}","notice":"{notice}","recipients":["p_{}"]}}"#,
        ME.to_lowercase()
    )
}

fn attempted(notice: &str, who: &str, channel: &str, attempt: u64, status: &str) -> String {
    format!(
        r#"{{"attempt":{attempt},"channel":"{channel}","coalesced_into":null,"notice":"{notice}","provider_message_id":null,"reason":null,"recipient":"p_{}","status":"{status}"}}"#,
        who.to_lowercase()
    )
}

const N1: &str = "0123456789abcdef0123456789abcdef";
const N2: &str = "fedcba9876543210fedcba9876543210";

/// The kinds `read-models/alerts` admits, each with its class, read from the schema file itself.
fn schema_kinds() -> Vec<(String, String)> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../schemas/workspace-api/read-models/alerts.schema.json"
    );
    let schema: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).expect("the schema")).expect("JSON");
    let names = |value: &serde_json::Value| -> Vec<String> {
        value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    let alert = &schema["$defs"]["Alert"];
    let kinds = names(&alert["properties"]["kind"]["enum"]);
    let info = names(&alert["allOf"][0]["if"]["properties"]["kind"]["enum"]);
    assert!(kinds.len() >= 20 && !info.is_empty(), "{kinds:?} {info:?}");
    kinds
        .into_iter()
        .map(|kind| {
            let class = if info.contains(&kind) {
                "info"
            } else {
                "safety"
            };
            (kind, class.to_owned())
        })
        .collect()
}

/// The nine kinds about members and access, written out from notifications spec §3.2's recipient
/// column (one member, or admins alone), not read from the code.
const MEMBER_AND_ACCESS: [&str; 9] = [
    "credential_added",
    "new_device",
    "notification_address_changed",
    "recovery_used",
    "role_granted",
    "member_deactivated",
    "deprovisioned",
    "break_glass",
    "client_connected",
];

#[test]
fn every_kind_the_schema_admits_has_its_class_and_no_other_kind_has_one() {
    for (kind, want) in schema_kinds() {
        assert_eq!(class(&kind), Some(want.as_str()), "{kind}");
    }
    for refused in [
        "approval_requested",
        "approval_reminder",
        "channel_lost",
        "",
        "Risk_Limit",
    ]
    .into_iter()
    .chain(MEMBER_AND_ACCESS)
    {
        assert_eq!(class(refused), None, "{refused}");
    }
    assert_eq!(WITHHELD, MEMBER_AND_ACCESS);
}

#[test]
fn a_members_recipient_is_p_and_their_id_in_lowercase() {
    assert_eq!(recipient(ME), "p_01jb3k6f0c9r2v7n4m8q1t5w3z");
}

#[test]
fn the_control_stream_names_each_agent_and_account_once_in_first_named_order() {
    let mut j = Journal::default();
    let ctl = format!("ctl:{WS}");
    let control = vec![
        j.row(
            &ctl,
            1,
            "ConnectionRequested",
            r#"{"account_ref":"ACCT1","connection_id":"c1"}"#,
        ),
        j.row(
            &ctl,
            2,
            "AgentDeployed",
            r#"{"agent_id":"agt_a","mandate_version":"v"}"#,
        ),
        j.row(
            &ctl,
            3,
            "ConnectionEstablished",
            r#"{"account_ref":"ACCT1","connection_id":"c1"}"#,
        ),
        j.row(&ctl, 4, "OwnerCommandIssued", r#"{"command":"pause"}"#),
        j.row(
            &ctl,
            5,
            "AgentDeployed",
            r#"{"agent_id":"agt_b","mandate_version":"v"}"#,
        ),
        j.row(
            &ctl,
            6,
            "AgentDeployed",
            r#"{"agent_id":"agt_a","mandate_version":"w"}"#,
        ),
        j.row(
            &ctl,
            7,
            "ConnectionEstablished",
            r#"{"account_ref":"ACCT2","connection_id":"c2"}"#,
        ),
    ];
    assert_eq!(
        followed(&control),
        (
            vec!["agt_a".to_owned(), "agt_b".to_owned()],
            vec!["ACCT1".to_owned(), "ACCT2".to_owned()]
        )
    );
}

/// A user's kill switch on an agent stream, notified and delivered to me by email after one
/// failure, and to another member, whose attempts I never see; a protection alert on an account
/// stream with no notice yet. The later alert is listed first.
#[test]
fn each_alert_carries_its_stream_subject_and_my_own_deliveries() -> Result<(), Unreadable> {
    let mut j = Journal::default();
    let a = agent("agt_a");
    let acct = account("ACCT1");
    let opened = j.row(&acct, 1, "StreamOpened", "{}");
    let protection = j.row(&acct, 2, "ProtectionChanged", r#"{"action":"placed"}"#);
    let protected = j.row(
        &acct,
        3,
        "OwnerAlertSent",
        &alert_payload("protection", &protection.event_id, None),
    );
    let command = "01JBH3BTZ8K2M4N6P8R0COMMND";
    let killed = j.row(&a, 1, "KillSwitchActivated", r#"{"scope":"agent"}"#);
    let kill = j.row(
        &a,
        2,
        "OwnerAlertSent",
        &alert_payload("kill_switch", &killed.event_id, Some(command)),
    );
    let notices = vec![
        j.row(
            &ntf(),
            1,
            "NoticeIssued",
            &issued(N1, "kill_switch", &kill.event_id, &a),
        ),
        j.row(
            &ntf(),
            2,
            "NoticeAttempted",
            &attempted(N1, ME, "email", 1, "failed"),
        ),
        j.row(
            &ntf(),
            3,
            "NoticeAttempted",
            &attempted(N1, OTHER, "email", 1, "delivered"),
        ),
        j.row(
            &ntf(),
            4,
            "NoticeAttempted",
            &attempted(N1, ME, "email", 2, "delivered"),
        ),
    ];
    let delivered_at = notices[3].recorded_at.clone();
    let streams = [
        Stream {
            followed: Followed::Account("ACCT1".to_owned()),
            rows: &[opened, protection.clone(), protected.clone()],
        },
        Stream {
            followed: Followed::Agent("agt_a".to_owned()),
            rows: &[killed.clone(), kill.clone()],
        },
    ];
    let got = alerts(&streams, &notices, &recipient(ME))?;
    assert_eq!(
        got,
        vec![
            AlertView {
                alert_id: kill.event_id.clone(),
                kind: "kill_switch".to_owned(),
                class: "safety",
                raised_at: kill.recorded_at.clone(),
                followed: Followed::Agent("agt_a".to_owned()),
                subject_id: killed.event_id.clone(),
                subject_type: "KillSwitchActivated".to_owned(),
                owner_command: Some(command.to_owned()),
                notice: Some(NoticeView {
                    issued_at: notices[0].recorded_at.clone(),
                    delivery: vec![DeliveryView {
                        channel: "email".to_owned(),
                        status: "delivered".to_owned(),
                        attempt: 2,
                        recorded_at: delivered_at,
                    }],
                }),
            },
            AlertView {
                alert_id: protected.event_id.clone(),
                kind: "protection".to_owned(),
                class: "safety",
                raised_at: protected.recorded_at.clone(),
                followed: Followed::Account("ACCT1".to_owned()),
                subject_id: protection.event_id.clone(),
                subject_type: "ProtectionChanged".to_owned(),
                owner_command: None,
                notice: None,
            },
        ]
    );
    Ok(())
}

/// A notice whose attempts are all another member's has no delivery for me; a member and access
/// alert on the control stream is withheld, neither served nor a failure.
#[test]
fn a_notice_only_others_were_sent_has_no_delivery_and_a_member_alert_is_withheld()
-> Result<(), Unreadable> {
    let mut j = Journal::default();
    let ctl = format!("ctl:{WS}");
    let capped = j.row(&ctl, 1, "SpendCapReached", "{}");
    let alert = j.row(
        &ctl,
        2,
        "OwnerAlertSent",
        &alert_payload("spend_cap", &capped.event_id, None),
    );
    let recovery = j.row(&ctl, 3, "RecoveryUsed", "{}");
    let withheld = j.row(
        &ctl,
        4,
        "OwnerAlertSent",
        &alert_payload("recovery_used", &recovery.event_id, None),
    );
    let notices = vec![
        j.row(
            &ntf(),
            1,
            "NoticeIssued",
            &issued(N2, "spend_cap", &alert.event_id, &ctl),
        ),
        j.row(
            &ntf(),
            2,
            "NoticeAttempted",
            &attempted(N2, OTHER, "email", 1, "delivered"),
        ),
    ];
    let got = alerts(
        &[Stream {
            followed: Followed::Control,
            rows: &[capped, alert.clone(), recovery, withheld],
        }],
        &notices,
        &recipient(ME),
    )?;
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].alert_id, alert.event_id);
    assert_eq!(got[0].class, "info");
    assert_eq!(
        got[0].notice,
        Some(NoticeView {
            issued_at: notices[0].recorded_at.clone(),
            delivery: Vec::new(),
        })
    );
    Ok(())
}

/// An alert that does not read, names a kind no alert takes, names no subject, or names a subject
/// its stream does not hold (rule 130) is never skipped: the whole read fails (`AGENTS.md` rule 3).
#[test]
fn an_alert_that_does_not_read_fails_the_read_rather_than_shortening_it() {
    let mut j = Journal::default();
    let ctl = format!("ctl:{WS}");
    let mut broken = j.row(&ctl, 1, "OwnerAlertSent", "{}");
    broken.body = b"not json".to_vec();
    let cases = [
        broken,
        j.row(
            &ctl,
            2,
            "OwnerAlertSent",
            &alert_payload("approval_requested", N1, None),
        ),
        j.row(
            &ctl,
            3,
            "OwnerAlertSent",
            r#"{"kind":"risk_limit","owner_command":null}"#,
        ),
        j.row(
            &ctl,
            4,
            "OwnerAlertSent",
            r#"{"kind":7,"owner_command":null,"subject":"x"}"#,
        ),
        j.row(
            &ctl,
            5,
            "OwnerAlertSent",
            &alert_payload("risk_limit", "01JBH3BTZ8K2M4N6P8R0ABSENT", None),
        ),
    ];
    for row in cases {
        let streams = [Stream {
            followed: Followed::Control,
            rows: std::slice::from_ref(&row),
        }];
        assert_eq!(
            alerts(&streams, &[], &recipient(ME)),
            Err(Unreadable),
            "{row:?}"
        );
    }
}

/// One alert the generator lays: on which stream, its kind, and whether it is notified, with my
/// attempts and another member's.
#[derive(Debug, Clone)]
struct Laid {
    stream: usize,
    kind: usize,
    notified: bool,
    mine: Vec<(bool, u64, bool)>,
    others: u64,
}

fn laid() -> impl Strategy<Value = Laid> {
    (
        0usize..3,
        0usize..32,
        any::<bool>(),
        proptest::collection::vec((any::<bool>(), 1u64..4, any::<bool>()), 0..4),
        0u64..3,
    )
        .prop_map(|(stream, kind, notified, mine, others)| Laid {
            stream,
            kind,
            notified,
            mine,
            others,
        })
}

proptest! {
    /// Whatever alerts lie on the streams: each is listed once, most recently raised first, with
    /// its kind's class from the schema, and with my latest attempt per channel only. The oracle
    /// is the laid list.
    #[test]
    fn every_alert_is_listed_once_newest_first_with_my_latest_attempt_per_channel(
        laid in proptest::collection::vec(laid(), 0..10),
    ) {
        let kinds = schema_kinds();
        let mut j = Journal::default();
        let names = [
            (Followed::Control, format!("ctl:{WS}")),
            (Followed::Agent("agt_a".to_owned()), agent("agt_a")),
            (Followed::Account("ACCT1".to_owned()), account("ACCT1")),
        ];
        let mut rows: Vec<Vec<StoredEvent>> = vec![Vec::new(), Vec::new(), Vec::new()];
        let mut notices = Vec::new();
        let mut ntf_seq = 0u64;
        let mut want = Vec::new();
        for (n, alert) in laid.iter().enumerate() {
            let (kind, class) = &kinds[alert.kind % kinds.len()];
            let (followed, stream) = &names[alert.stream];
            let seq = u64::try_from(rows[alert.stream].len()).expect("a count") + 1;
            let subject = j.row(stream, seq, "SubjectHappened", "{}");
            let command = (kind == "kill_switch").then(|| format!("01JBH3BTZ8K2M4N6P8RC{n:06}"));
            let row = j.row(
                stream,
                seq + 1,
                "OwnerAlertSent",
                &alert_payload(kind, &subject.event_id, command.as_deref()),
            );
            rows[alert.stream].push(subject.clone());
            rows[alert.stream].push(row.clone());
            let notice = if alert.notified {
                let id = format!("{n:032x}");
                ntf_seq += 1;
                let issued_row = j.row(&ntf(), ntf_seq, "NoticeIssued", &issued(&id, kind, &row.event_id, stream));
                notices.push(issued_row.clone());
                let mut latest: Vec<DeliveryView> = Vec::new();
                for (web, attempt, ok) in &alert.mine {
                    let channel = if *web { "web_push" } else { "email" };
                    let status = if *ok { "delivered" } else { "failed" };
                    ntf_seq += 1;
                    let at = j.row(&ntf(), ntf_seq, "NoticeAttempted", &attempted(&id, ME, channel, *attempt, status));
                    latest.retain(|d| d.channel != channel);
                    latest.push(DeliveryView {
                        channel: channel.to_owned(),
                        status: status.to_owned(),
                        attempt: *attempt,
                        recorded_at: at.recorded_at.clone(),
                    });
                    notices.push(at);
                    for k in 0..alert.others {
                        ntf_seq += 1;
                        notices.push(j.row(&ntf(), ntf_seq, "NoticeAttempted", &attempted(&id, OTHER, channel, k + 9, "delivered")));
                    }
                }
                latest.sort_by(|a, b| a.channel.cmp(&b.channel));
                Some(NoticeView { issued_at: issued_row.recorded_at.clone(), delivery: latest })
            } else {
                None
            };
            want.push(AlertView {
                alert_id: row.event_id.clone(),
                kind: kind.clone(),
                class: if class == "info" { "info" } else { "safety" },
                raised_at: row.recorded_at.clone(),
                followed: followed.clone(),
                subject_id: subject.event_id.clone(),
                subject_type: "SubjectHappened".to_owned(),
                owner_command: command,
                notice,
            });
        }
        want.reverse();
        let streams: Vec<Stream> = names
            .iter()
            .zip(rows.iter())
            .map(|((followed, _), rows)| Stream { followed: followed.clone(), rows })
            .collect();
        prop_assert_eq!(alerts(&streams, &notices, &recipient(ME)), Ok(want));
    }
}
