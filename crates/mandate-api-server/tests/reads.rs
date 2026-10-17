//! The read-model folds (§4.3, §4.7, DEC-979), against rows built by hand and expected views
//! written out by hand.

use mandate_api_server::reads::{
    AgentView, ApprovalState, Filter, RESOLVED_LISTED, agent, approvals, deployed, inbox, mode,
};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_journal::StoredEvent;

const WS: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Y";

fn at(second: u32) -> String {
    format!(
        "2026-10-17T10:{:02}:{:02}.000000000Z",
        second / 60,
        second % 60
    )
}

/// One stored row: the envelope members the folds read, around `payload` (JSON text).
fn row(stream: &str, seq: u64, event_type: &str, payload: &str) -> StoredEvent {
    row_in(stream, seq, event_type, "paper", payload)
}

fn row_in(
    stream: &str,
    seq: u64,
    event_type: &str,
    environment: &str,
    payload: &str,
) -> StoredEvent {
    let event_id = format!("01JBH3BTZ8K2M4N6P8R0S2T{seq:03}");
    let text = format!(
        r#"{{"environment":"{environment}","event_id":"{event_id}","event_type":"{event_type}","payload":{payload},"stream_id":"{stream}"}}"#
    );
    let body = to_canonical(&parse(text.as_bytes()).expect("JSON"));
    StoredEvent {
        stream_id: stream.to_owned(),
        seq,
        event_id,
        event_type: event_type.to_owned(),
        schema_version: 1,
        environment: environment.to_owned(),
        recorded_at: at(u32::try_from(seq).expect("small")),
        prev_hash: Digest::ZERO,
        hash: Digest::of(&body),
        body,
    }
}

fn ctl() -> String {
    format!("ctl:{WS}")
}

fn version(n: u8) -> String {
    format!("sha256:{}", format!("{n:02x}").repeat(32))
}

fn deploy(seq: u64, agent: &str, n: u8) -> StoredEvent {
    row(
        &ctl(),
        seq,
        "AgentDeployed",
        &format!(
            r#"{{"agent_id":"{agent}","mandate_version":"{}","record_ref":"{}"}}"#,
            version(n),
            version(0xee)
        ),
    )
}

#[test]
fn each_deployed_agent_is_listed_once_in_deployment_order_with_its_latest_deployment() {
    let control = vec![
        row(&ctl(), 1, "StreamOpened", "{}"),
        deploy(2, "agt_b", 1),
        deploy(3, "agt_a", 2),
        row(&ctl(), 4, "OwnerCommandIssued", r#"{"agent":"agt_b"}"#),
        deploy(5, "agt_b", 3),
        row(
            &ctl(),
            6,
            "AgentStopped",
            r#"{"agent_id":"agt_a","connection_id":"c1","reason":"owner_stop","retired_on":"2026-10-17","loss_added":"0"}"#,
        ),
        row_in(
            &ctl(),
            7,
            "AgentDeployed",
            "live",
            &format!(
                r#"{{"agent_id":"agt_c","mandate_version":"{}","record_ref":"{}"}}"#,
                version(4),
                version(0xee)
            ),
        ),
    ];
    assert_eq!(
        deployed(&control),
        vec![
            (
                "agt_b".to_owned(),
                "paper".to_owned(),
                version(3),
                at(5),
                false
            ),
            (
                "agt_a".to_owned(),
                "paper".to_owned(),
                version(2),
                at(3),
                true
            ),
            (
                "agt_c".to_owned(),
                "live".to_owned(),
                version(4),
                at(7),
                false
            ),
        ]
    );
    assert_eq!(deployed(&[]), vec![]);
}

#[test]
fn a_mode_is_the_last_mode_change_and_none_before_any() {
    let stream = format!("agent:{WS}:agt_a");
    assert_eq!(mode(&[]), None);
    let rows = vec![
        row(&stream, 1, "StreamOpened", "{}"),
        row(
            &stream,
            2,
            "AgentModeChanged",
            r#"{"from":"normal","to":"paused","reason":"owner_pause"}"#,
        ),
        row(&stream, 3, "DecisionMade", r#"{"to":"stopped"}"#),
        row(
            &stream,
            4,
            "AgentModeChanged",
            r#"{"from":"paused","to":"exits_only","reason":"x"}"#,
        ),
    ];
    assert_eq!(mode(&rows[..2]), Some("paused".to_owned()));
    assert_eq!(mode(&rows), Some("exits_only".to_owned()));
    assert_eq!(
        mode(&rows[..1]),
        None,
        "nothing reported yet is never normal"
    );
}

fn request(stream: &str, seq: u64, deadline: i64, n: u8) -> StoredEvent {
    row(
        stream,
        seq,
        "ApprovalRequested",
        &format!(
            r#"{{"content":{{"bound":{{"side":"buy","n":{n}}}}},"content_hash":"{}","deadline":{deadline}}}"#,
            version(n)
        ),
    )
}

fn id(seq: u64) -> String {
    format!("01JBH3BTZ8K2M4N6P8R0S2T{seq:03}")
}

fn answer(stream: &str, seq: u64, of: u64, event_type: &str, extra: &str) -> StoredEvent {
    row(
        stream,
        seq,
        event_type,
        &format!(r#"{{"approval":"{}"{extra}}}"#, id(of)),
    )
}

#[test]
fn approval_states_fold_with_the_first_terminal_event_winning() {
    let s = format!("agent:{WS}:agt_a");
    let rows = vec![
        request(&s, 1, 500, 1),
        request(&s, 2, 400, 2),
        request(&s, 3, 300, 3),
        request(&s, 4, 200, 4),
        request(&s, 5, 100, 5),
        request(&s, 6, 600, 6),
        request(&s, 7, 700, 7),
        answer(&s, 10, 1, "ApprovalTimedOut", r#","on_timeout":"skip""#),
        answer(&s, 11, 1, "ApprovalCanceled", r#","reason":"kill_switch""#),
        answer(&s, 12, 2, "ApprovalCanceled", r#","reason":"owner_pause""#),
        answer(&s, 13, 3, "ApprovalRevalidated", r#","result":"act""#),
        answer(
            &s,
            14,
            4,
            "ApprovalResponded",
            r#","result":"admitted","verdict":"skipped""#,
        ),
        answer(&s, 15, 5, "ApprovalRevalidated", r#","result":"skip""#),
        answer(
            &s,
            16,
            6,
            "ApprovalResponded",
            r#","result":"admitted","verdict":"approved""#,
        ),
        answer(
            &s,
            17,
            7,
            "ApprovalResponded",
            r#","result":"refused","verdict":"skipped""#,
        ),
        answer(
            &s,
            18,
            7,
            "ApprovalResponded",
            r#","result":"counted","verdict":"approved""#,
        ),
        answer(&s, 19, 99, "ApprovalTimedOut", r#","on_timeout":"skip""#),
        answer(&s, 20, 3, "ApprovalTimedOut", r#","on_timeout":"skip""#),
    ];
    let folded = approvals("agt_a", &rows);
    let states: Vec<(String, ApprovalState)> = folded
        .iter()
        .map(|a| (a.approval_id.clone(), a.state))
        .collect();
    assert_eq!(
        states,
        vec![
            (id(1), ApprovalState::TimedOut),
            (id(2), ApprovalState::Canceled),
            (id(3), ApprovalState::Acted),
            (id(4), ApprovalState::Skipped),
            (id(5), ApprovalState::Skipped),
            (id(6), ApprovalState::Pending),
            (id(7), ApprovalState::Pending),
        ]
    );
    let first = &folded[0];
    assert_eq!(first.agent_id, "agt_a");
    assert_eq!(first.requested_at, at(1));
    assert_eq!(first.deadline, 500);
    assert_eq!(first.content_hash, version(1));
    let expected_content = parse(br#"{"bound":{"n":1,"side":"buy"}}"#).expect("JSON");
    assert_eq!(first.content, expected_content);
}

#[test]
fn a_request_without_a_deadline_or_hash_is_not_served_as_an_approval() {
    let s = format!("agent:{WS}:agt_a");
    let rows = vec![
        row(
            &s,
            1,
            "ApprovalRequested",
            r#"{"content":{},"content_hash":"sha256:x"}"#,
        ),
        row(&s, 2, "ApprovalRequested", r#"{"content":{},"deadline":5}"#),
        request(&s, 3, 7, 3),
    ];
    let folded = approvals("agt_a", &rows);
    assert_eq!(folded.len(), 1);
    assert_eq!(folded[0].approval_id, id(3));
}

fn views(n: u64) -> Vec<mandate_api_server::reads::ApprovalView> {
    let s = format!("agent:{WS}:agt_a");
    let mut rows = Vec::new();
    for i in 1..=n {
        rows.push(request(&s, i, i64::try_from(1_000 - i).expect("small"), 1));
    }
    approvals("agt_a", &rows)
}

#[test]
fn the_inbox_lists_pending_by_deadline_then_resolved_newest_first() {
    let s = format!("agent:{WS}:agt_a");
    let rows = vec![
        request(&s, 1, 300, 1),
        request(&s, 2, 100, 2),
        request(&s, 3, 200, 3),
        request(&s, 4, 100, 4),
        request(&s, 5, 50, 5),
        answer(&s, 6, 3, "ApprovalTimedOut", r#","on_timeout":"skip""#),
        answer(&s, 7, 5, "ApprovalCanceled", r#","reason":"owner_pause""#),
    ];
    let order = |filter| -> Vec<String> {
        inbox(approvals("agt_a", &rows), filter)
            .into_iter()
            .map(|a| a.approval_id)
            .collect()
    };
    assert_eq!(order(Filter::All), vec![id(2), id(4), id(1), id(5), id(3)]);
    assert_eq!(order(Filter::Pending), vec![id(2), id(4), id(1)]);
    assert_eq!(order(Filter::Resolved), vec![id(5), id(3)]);
}

#[test]
fn the_inbox_lists_every_pending_approval_and_the_hundred_newest_resolved() {
    let pending = views(150);
    assert_eq!(inbox(pending, Filter::All).len(), 150, "every pending one");
    let s = format!("agent:{WS}:agt_a");
    let mut rows = Vec::new();
    for i in 1..=150 {
        rows.push(request(&s, i, 10, 1));
    }
    for i in 1..=150 {
        rows.push(answer(
            &s,
            1_000 + i,
            i,
            "ApprovalTimedOut",
            r#","on_timeout":"skip""#,
        ));
    }
    let listed = inbox(approvals("agt_a", &rows), Filter::Resolved);
    assert_eq!(listed.len(), RESOLVED_LISTED);
    assert_eq!(listed[0].approval_id, id(150));
    assert_eq!(listed[RESOLVED_LISTED - 1].approval_id, id(51));
}

#[test]
fn an_agent_view_counts_its_pending_approvals_and_reports_stopped() {
    let s = format!("agent:{WS}:agt_a");
    let rows = vec![
        row(
            &s,
            1,
            "AgentModeChanged",
            r#"{"from":"normal","to":"paused","reason":"x"}"#,
        ),
        request(&s, 2, 10, 1),
        request(&s, 3, 20, 2),
        request(&s, 4, 30, 3),
        answer(&s, 5, 3, "ApprovalTimedOut", r#","on_timeout":"skip""#),
    ];
    let deployment = (
        "agt_a".to_owned(),
        "paper".to_owned(),
        version(1),
        at(1),
        false,
    );
    assert_eq!(
        agent(&deployment, &rows),
        AgentView {
            agent_id: "agt_a".to_owned(),
            environment: "paper".to_owned(),
            mandate_version: version(1),
            deployed_at: at(1),
            mode: Some("paused".to_owned()),
            pending_approvals: 2,
        }
    );
    let stopped = (
        "agt_a".to_owned(),
        "paper".to_owned(),
        version(1),
        at(1),
        true,
    );
    assert_eq!(agent(&stopped, &rows).mode, Some("stopped".to_owned()));
    assert_eq!(agent(&stopped, &[]).mode, Some("stopped".to_owned()));
    let fresh = (
        "agt_a".to_owned(),
        "paper".to_owned(),
        version(1),
        at(1),
        false,
    );
    assert_eq!(agent(&fresh, &[]).mode, None);
    assert_eq!(agent(&fresh, &[]).pending_approvals, 0);
}

#[test]
fn a_body_that_is_not_json_is_skipped_never_guessed() {
    let s = format!("agent:{WS}:agt_a");
    let mut broken = request(&s, 1, 10, 1);
    broken.body = b"not json".to_vec();
    let rows = vec![broken, request(&s, 2, 20, 2)];
    let folded = approvals("agt_a", &rows);
    assert_eq!(folded.len(), 1);
    assert_eq!(folded[0].approval_id, id(2));
    assert!(matches!(folded[0].content, Value::Object(_)));
}

#[test]
fn resolved_approvals_requested_together_list_by_approval_id() {
    let a = format!("agent:{WS}:agt_a");
    let b = format!("agent:{WS}:agt_b");
    let mut first = request(&a, 1, 10, 1);
    let mut second = request(&b, 2, 10, 2);
    first.recorded_at = at(9);
    second.recorded_at = at(9);
    let mut views = approvals(
        "agt_a",
        &[
            first.clone(),
            answer(&a, 3, 1, "ApprovalTimedOut", r#","on_timeout":"skip""#),
        ],
    );
    views.extend(approvals(
        "agt_b",
        &[
            second.clone(),
            answer(&b, 4, 2, "ApprovalTimedOut", r#","on_timeout":"skip""#),
        ],
    ));
    let listed: Vec<String> = inbox(views.clone(), Filter::Resolved)
        .into_iter()
        .map(|a| a.approval_id)
        .collect();
    assert_eq!(listed, vec![id(2), id(1)]);
    views.reverse();
    let listed: Vec<String> = inbox(views, Filter::Resolved)
        .into_iter()
        .map(|a| a.approval_id)
        .collect();
    assert_eq!(
        listed,
        vec![id(2), id(1)],
        "whatever order the agents are read in"
    );
}
