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

/// A version-3 deployment (journal spec v0.84, DEC-2131 item 4) is listed as its version 2: by
/// its agent and version, `step_up` ignored. The API's version reads (`versions::active`,
/// `versions::last_confirmed`) read the same members at every version.
#[test]
fn a_version_3_deployment_is_listed_as_its_version_2() {
    let stepped = versioned(
        row(
            &ctl(),
            2,
            "AgentDeployed",
            &format!(
                r#"{{"agent_id":"agt_a","mandate_version":"{}","record_ref":"{}","connection_id":"c1","step_up":{{"assertion_id":"01JAA3P0A000000000000000H1","authenticated_at":"2026-10-17T10:00:01.000000000Z","method":"passkey"}}}}"#,
                version(1),
                version(0xee)
            ),
        ),
        3,
    );
    let confirmed = versioned(
        row(
            &ctl(),
            1,
            "MandateConfirmed",
            &format!(
                r#"{{"mandate_version":"{}","confirmed_paths":["/capital"],"record_ref":"{}","agent_id":null,"base_version":null,"step_up":{{"assertion_id":"01JAA3P0A000000000000000H1","authenticated_at":"2026-10-17T10:00:00.000000000Z","method":"passkey"}}}}"#,
                version(1),
                version(0xee)
            ),
        ),
        3,
    );
    let control = vec![confirmed, stepped];
    assert_eq!(
        deployed(&control),
        vec![(
            "agt_a".to_owned(),
            "paper".to_owned(),
            version(1),
            at(2),
            false
        )]
    );
    assert_eq!(
        mandate_api_server::versions::active(&control, "agt_a").map(|(_, v)| v),
        Some(version(1))
    );
    assert_eq!(
        mandate_api_server::versions::last_confirmed(&control).map(|(_, v)| v),
        Some(version(1))
    );
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
            r#"{{"content":{{{ACTION},"bound":{{"side":"buy","n":{n}}}}},"content_hash":"{}","deadline":{deadline}}}"#,
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
    let expected_content =
        parse(format!(r#"{{{ACTION},"bound":{{"n":1,"side":"buy"}}}}"#).as_bytes()).expect("JSON");
    assert_eq!(first.content, expected_content);
    let expected_order = parse(br#"{"instrument":"b0c2a3e4-5f60-4718-9a2b-3c4d5e6f7081","asset_class":"us_equity","side":"buy","qty":"3","limit":"101.5","order_usd":"304.5","purpose":"open","tif":null,"protection":null}"#).expect("JSON");
    assert_eq!(first.order, expected_order);
}

#[test]
fn a_request_without_a_deadline_hash_or_action_is_not_served_as_an_approval() {
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
        row(
            &s,
            4,
            "ApprovalRequested",
            r#"{"content":{"bound":{}},"content_hash":"sha256:x","deadline":5}"#,
        ),
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

fn payload(json: &str) -> mandate_canon::Value {
    mandate_canon::parse(json.as_bytes()).expect("JSON")
}

const ACTION: &str = r#""action":{"instrument":"b0c2a3e4-5f60-4718-9a2b-3c4d5e6f7081","asset_class":"us_equity","side":"buy","qty":"3","limit":"101.5","order_usd":"304.5","purpose":"open"}"#;

#[test]
fn a_version_one_request_binds_its_action_with_no_execution_plan() {
    let bound =
        mandate_api_server::reads::order(&payload(&format!(r#"{{"content":{{{ACTION}}}}}"#)))
            .expect("an order");
    assert_eq!(
        bound,
        payload(
            r#"{"instrument":"b0c2a3e4-5f60-4718-9a2b-3c4d5e6f7081","asset_class":"us_equity","side":"buy","qty":"3","limit":"101.5","order_usd":"304.5","purpose":"open","tif":null,"protection":null}"#
        )
    );
}

#[test]
fn a_version_two_request_adds_its_time_in_force_and_protection() {
    let with = |execution: &str| {
        mandate_api_server::reads::order(&payload(&format!(
            r#"{{"content":{{{ACTION},"execution":{execution}}}}}"#
        )))
        .expect("an order")
    };
    let protected = with(
        r#"{"asset_class":"us_equity","tif":"day","protection_required":true,"protection":{"stop":"95","take_profit":"112"}}"#,
    );
    assert_eq!(protected.get("tif"), Some(&payload(r#""day""#)));
    assert_eq!(
        protected.get("protection"),
        Some(&payload(r#"{"stop":"95","take_profit":"112"}"#))
    );
    let bare = with(
        r#"{"asset_class":"crypto","tif":"ioc","protection_required":false,"protection":null}"#,
    );
    assert_eq!(bare.get("tif"), Some(&payload(r#""ioc""#)));
    assert_eq!(bare.get("protection"), Some(&mandate_canon::Value::Null));
}

#[test]
fn a_request_without_a_whole_action_binds_no_order() {
    assert_eq!(
        mandate_api_server::reads::order(&payload(r#"{"content":{}}"#)),
        None
    );
    assert_eq!(mandate_api_server::reads::order(&payload(r#"{}"#)), None);
    let partial = r#"{"content":{"action":{"instrument":"x","asset_class":"us_equity","side":"buy","qty":"3","limit":"1","order_usd":"3"}}}"#;
    assert_eq!(
        mandate_api_server::reads::order(&payload(partial)),
        None,
        "no purpose"
    );
}

fn versioned(mut row: StoredEvent, schema_version: u64) -> StoredEvent {
    row.schema_version = schema_version;
    row
}

#[test]
fn a_connection_binds_the_account_its_latest_version_two_establishment_names() {
    let control = vec![
        row(&ctl(), 1, "StreamOpened", "{}"),
        row(
            &ctl(),
            2,
            "ConnectionRequested",
            r#"{"connection_id":"conn_1","account_ref":"acct_requested"}"#,
        ),
        versioned(
            row(
                &ctl(),
                3,
                "ConnectionEstablished",
                r#"{"connection_id":"conn_1","account_ref":"acct_one"}"#,
            ),
            2,
        ),
        row(
            &ctl(),
            4,
            "ConnectionEstablished",
            r#"{"connection_id":"conn_1","account_ref":"acct_v1"}"#,
        ),
        versioned(
            row(
                &ctl(),
                5,
                "ConnectionEstablished",
                r#"{"connection_id":"conn_1","account_ref":"acct_two"}"#,
            ),
            2,
        ),
        versioned(
            row(
                &ctl(),
                6,
                "ConnectionEstablished",
                r#"{"connection_id":"conn_2","account_ref":"acct_other"}"#,
            ),
            2,
        ),
        row(
            &ctl(),
            7,
            "ConnectionRequested",
            r#"{"connection_id":"conn_3","account_ref":"acct_pending"}"#,
        ),
        row(
            &ctl(),
            8,
            "ConnectionRequested",
            r#"{"connection_id":"conn_3","account_ref":"acct_pending_2"}"#,
        ),
    ];
    let bound = |id| mandate_api_server::reads::account_ref(&control, id);
    assert_eq!(
        bound("conn_1").as_deref(),
        Some("acct_two"),
        "the latest v2, never v1"
    );
    assert_eq!(bound("conn_2").as_deref(), Some("acct_other"));
    assert_eq!(
        bound("conn_3").as_deref(),
        Some("acct_pending_2"),
        "else the latest request"
    );
    assert_eq!(bound("conn_4"), None);

    let later = |payload: &str| {
        let mut rows = control.clone();
        rows.push(versioned(
            row(&ctl(), 9, "ConnectionEstablished", payload),
            2,
        ));
        mandate_api_server::reads::account_ref(&rows, "conn_1")
    };
    assert_eq!(
        later(r#"{"connection_id":"conn_1"}"#),
        None,
        "the latest row decides, even one that names no account"
    );
    assert_eq!(
        later(r#"{"connection_id":"conn_1","account_ref":"../ws2:acct"}"#),
        None,
        "an account ref outside the id grammar forms no stream name"
    );
    assert_eq!(
        later(r#"{"connection_id":"conn_1","account_ref":""}"#),
        None
    );
    assert_eq!(
        later(r#"{"connection_id":"conn_1","account_ref":"acct-3_x"}"#).as_deref(),
        Some("acct-3_x")
    );

    let mut v1_last = control.clone();
    v1_last.push(row(
        &ctl(),
        9,
        "ConnectionEstablished",
        r#"{"connection_id":"conn_1","account_ref":"acct_v1_late"}"#,
    ));
    v1_last.push(versioned(
        row(
            &ctl(),
            10,
            "ConnectionRequested",
            r#"{"connection_id":"conn_1","account_ref":"acct_req_v2"}"#,
        ),
        2,
    ));
    assert_eq!(
        mandate_api_server::reads::account_ref(&v1_last, "conn_1").as_deref(),
        Some("acct_two"),
        "a later version-1 establishment, or a request at another version, never binds"
    );
}

#[test]
fn the_accounts_positions_are_its_latest_reconciliation_per_instrument() {
    let acct = format!("acct:{WS}:acct_1");
    let observed = |seq, instrument: &str, model: &str, broker: &str, mismatch: bool| {
        row(
            &acct,
            seq,
            "BrokerPositionObserved",
            &format!(
                r#"{{"instrument":"{instrument}","broker_qty":"{broker}","model_qty":"{model}","mismatch":{mismatch},"risk_clock":"2026-10-17T10:00:00Z"}}"#
            ),
        )
    };
    let rows = vec![
        row(&acct, 1, "StreamOpened", "{}"),
        observed(2, "zzz", "5", "5", false),
        observed(3, "aaa", "2", "2", false),
        observed(4, "zzz", "7", "6", true),
        observed(5, "mmm", "0", "0", false),
        row(&acct, 6, "FillApplied", r#"{"instrument_id":"aaa"}"#),
        observed(7, "nnn", "0", "1", true),
        observed(8, "aaa", "0", "0", false),
    ];
    let positions = mandate_api_server::reads::account_positions(&rows);
    let expected: Vec<Value> = [
        (r#""nnn""#, "0", "1", true, 7),
        (r#""zzz""#, "7", "6", true, 4),
    ]
    .iter()
    .map(|(instrument, qty, broker, mismatch, seq)| {
        parse(
            format!(
                r#"{{"instrument":{instrument},"qty":"{qty}","broker_qty":"{broker}","mismatch":{mismatch},"observed_at":"{}"}}"#,
                at(*seq)
            )
            .as_bytes(),
        )
        .expect("JSON")
    })
    .collect();
    assert_eq!(
        positions, expected,
        "flat instruments are left out, the rest in instrument order"
    );
    assert!(mandate_api_server::reads::account_positions(&[]).is_empty());
}
