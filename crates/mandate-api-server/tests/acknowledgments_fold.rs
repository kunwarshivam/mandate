//! The acknowledgment routes' fold of the account and control streams
//! (`mandate_api_server::acknowledgments`, §3.11 item 12, DEC-2144) on rows built here: each hold
//! ends only by the record journal spec §9.28 names for it, and a record it cannot read fails the
//! fold. The step-up digest's worked vectors are checked against the object the HTTP tests bind.

use mandate_api_server::acknowledgments::{
    Hold, Holder, Raised, Reason, Requested, holds, requests, standing,
};
use mandate_canon::{Digest, parse, to_canonical};
use mandate_journal::StoredEvent;

/// §3.11 item 12's object for `confirms` of the hold, member by member in canonical order, as
/// `versions_http`'s challenges bind it, with `control_head` 42.
fn bound(confirms: &str, request: Option<&str>) -> String {
    let request = request.map_or(String::new(), |id| format!(r#""request":"{id}","#));
    let object = format!(
        r#"{{"agent":"agt_01JB3K6F0C9R2V7N4M8Q1T5W3Z","bid":null,"bid_size":null,"confirms":"{confirms}","control_head":"42","event":"01JBH3BTZ8K2M4N6P8R0S2T4V6","floor":null,"instrument":null,{request}"subject":"reconciliation"}}"#
    );
    let value = parse(object.as_bytes()).expect("JSON");
    format!("sha256:{}", Digest::of(&to_canonical(&value)).to_hex())
}

#[test]
fn the_bound_object_reproduces_the_spec_s_three_vectors() {
    assert_eq!(
        bound("acknowledge", None),
        "sha256:5f5506a3ac32cae637a2cc222393f852035c699430656870ba024702431260a7"
    );
    assert_eq!(
        bound("acknowledge", Some("01JBH3BTZ8K2M4N6P8R0S2T4V7")),
        "sha256:b9367c62d1201059c5a9f2cf0bf35e637d27a5934e174b779276a2fcbc7af765"
    );
    assert_eq!(
        bound("request_ack", None),
        "sha256:b15c59e4e09394ddf0f5430c04569276c27c7a957ac68c683e53b15e6b393ebc"
    );
}

fn row(seq: u64, event_type: &str, schema_version: u64, payload: &str) -> StoredEvent {
    let event_id = format!("01JBH3BTZ8K2M4N6P8R0S2T{seq:03}");
    let body = to_canonical(
        &parse(format!(r#"{{"event_id":"{event_id}","payload":{payload}}}"#).as_bytes())
            .expect("JSON"),
    );
    StoredEvent {
        stream_id: "acct:w:a".to_owned(),
        seq,
        event_id,
        event_type: event_type.to_owned(),
        schema_version,
        environment: "paper".to_owned(),
        recorded_at: format!("2026-10-17T11:00:{seq:02}.000000000Z"),
        prev_hash: Digest::ZERO,
        hash: Digest::of(&body),
        body,
    }
}

fn events(rows: &[StoredEvent]) -> Vec<String> {
    standing(rows)
        .expect("readable")
        .into_iter()
        .map(|hold| hold.event)
        .collect()
}

#[test]
fn a_limit_stands_until_its_own_agent_s_lift_or_trigger_of_the_same_limit() {
    let rows = [
        row(
            1,
            "RiskLimitTriggered",
            1,
            r#"{"agent_id":"a","limit":"max_daily_loss"}"#,
        ),
        row(
            2,
            "RiskLimitTriggered",
            1,
            r#"{"agent_id":"b","limit":"max_daily_loss"}"#,
        ),
        row(
            3,
            "RiskLimitLifted",
            1,
            r#"{"agent_id":"b","limit":"profit_stop"}"#,
        ),
        row(
            4,
            "RiskLimitTriggered",
            1,
            r#"{"agent_id":"a","limit":"profit_stop"}"#,
        ),
        row(
            5,
            "RiskLimitLifted",
            1,
            r#"{"agent_id":"a","limit":"profit_stop"}"#,
        ),
    ];
    assert_eq!(
        events(&rows),
        [rows[0].event_id.clone(), rows[1].event_id.clone()]
    );
    let again = row(
        6,
        "RiskLimitTriggered",
        1,
        r#"{"agent_id":"b","limit":"max_daily_loss"}"#,
    );
    let mut more = rows.to_vec();
    more.push(again.clone());
    assert_eq!(
        events(&more),
        [rows[0].event_id.clone(), again.event_id.clone()]
    );
    let held = standing(&more).expect("readable");
    assert_eq!(held[0].holder, Holder::Agent("a".to_owned()));
    assert_eq!(held[0].reason, Reason::Limit("max_daily_loss".to_owned()));
    assert_eq!(held[0].mode, None);
    assert_eq!(held[0].raised_at, rows[0].recorded_at);
}

#[test]
fn a_restriction_holds_the_mode_its_state_gives_until_its_cause_lifts_or_changes() {
    let rows = [
        row(
            1,
            "AccountRestrictionChanged",
            1,
            r#"{"cause":"broker_reject","restriction":"closing_only"}"#,
        ),
        row(
            2,
            "AccountRestrictionChanged",
            1,
            r#"{"cause":"broker_notice","restriction":"blocked"}"#,
        ),
        row(
            3,
            "AccountRestrictionLifted",
            1,
            r#"{"cause":"connection_unavailable"}"#,
        ),
    ];
    let held = standing(&rows).expect("readable");
    assert_eq!(
        held.iter()
            .map(|h| (h.mode, h.holder.clone()))
            .collect::<Vec<_>>(),
        [
            (Some("exits_only"), Holder::Account),
            (Some("paused"), Holder::Account)
        ]
    );
    let lifted = [
        rows[0].clone(),
        rows[1].clone(),
        row(
            4,
            "AccountRestrictionLifted",
            1,
            r#"{"cause":"broker_reject"}"#,
        ),
    ];
    assert_eq!(events(&lifted), [rows[1].event_id.clone()]);
    let other = row(
        5,
        "AccountRestrictionChanged",
        1,
        r#"{"cause":"broker_reject","restriction":"active"}"#,
    );
    assert_eq!(
        standing(&[other]),
        None,
        "a state that holds no mode is unreadable"
    );
}

#[test]
fn a_reconciliation_hold_is_the_executor_s_own_until_the_same_agent_and_restriction_apply_again() {
    let hold = |seq, agent: &str, to: &str, restriction: &str, originated: bool| {
        row(
            seq,
            "AgentModeApplied",
            1,
            &format!(
                r#"{{"agent":"{agent}","originated":{originated},"restriction":"{restriction}","to":"{to}"}}"#
            ),
        )
    };
    let rows = [
        hold(1, "a", "exits_only", "reconciliation:SPY", true),
        hold(2, "*", "paused", "reconciliation:SPY", true),
        hold(3, "a", "paused", "reconciliation:fees", false),
        hold(4, "a", "stopped", "reconciliation:cash", true),
        hold(5, "a", "paused", "kill_switch", true),
        hold(6, "b", "normal", "reconciliation:SPY", true),
        hold(7, "a", "paused", "reconciliation:", true),
    ];
    let held = standing(&rows).expect("readable");
    assert_eq!(held.len(), 2);
    assert_eq!(
        held[0].reason,
        Reason::Reconciliation {
            restriction: "reconciliation:SPY".to_owned(),
            kind: "instrument".to_owned(),
            instrument: Some("SPY".to_owned()),
        }
    );
    assert_eq!(held[1].holder, Holder::Account);
    assert_eq!(held[1].mode, Some("paused"));
    let removed = [
        rows[0].clone(),
        hold(8, "a", "normal", "reconciliation:SPY", true),
    ];
    assert_eq!(events(&removed), Vec::<String>::new());
    let mut kinds = Vec::new();
    for (seq, subject) in [
        (9, "cash"),
        (10, "fees"),
        (11, "external_activity"),
        (12, "incomplete"),
    ] {
        let held = standing(&[hold(
            seq,
            "*",
            "paused",
            &format!("reconciliation:{subject}"),
            true,
        )])
        .expect("readable");
        kinds.push(held[0].reason.clone());
    }
    assert!(kinds.iter().all(|r| matches!(
        r,
        Reason::Reconciliation {
            instrument: None,
            ..
        }
    )));
    let unreadable = row(
        13,
        "AgentModeApplied",
        1,
        r#"{"agent":"a","originated":"yes","restriction":"reconciliation:SPY","to":"paused"}"#,
    );
    assert_eq!(standing(&[unreadable]), None);
}

#[test]
fn a_request_is_spent_by_any_acknowledgment_naming_it_and_a_version_1_one_is_a_limit_s() {
    let asked = |seq, version, subject: &str| {
        row(
            seq,
            "OwnerAcknowledgmentRequested",
            version,
            &format!(r#"{{"agent":"a","event":"e","requested_by":"bob"{subject}}}"#),
        )
    };
    let rows = [
        asked(1, 1, ""),
        asked(2, 2, r#","subject":"restriction""#),
        asked(3, 2, r#","subject":7"#),
        row(
            4,
            "OwnerAcknowledged",
            2,
            r#"{"request":"01JBH3BTZ8K2M4N6P8R0S2T002"}"#,
        ),
        row(5, "OwnerAcknowledged", 2, r#"{"request":null}"#),
    ];
    let read = requests(&rows).expect("readable");
    let summary: Vec<(&str, bool)> = read
        .iter()
        .map(|(request, spent)| (request.subject.as_str(), *spent))
        .collect();
    assert_eq!(
        summary,
        [("limit", false), ("restriction", true), ("", false)]
    );
    assert_eq!(read[0].0.requested_by, "bob");
    let unreadable = row(
        6,
        "OwnerAcknowledgmentRequested",
        2,
        r#"{"agent":7,"event":"e","requested_by":"bob","subject":"limit"}"#,
    );
    assert_eq!(requests(&[unreadable]), None);
}

/// A hold blocks an agent only on that agent's own account stream: an account-wide hold of
/// another account blocks no agent here, and an agent's own hold blocks no other agent.
#[test]
fn a_hold_blocks_only_its_own_agent_or_the_agents_of_its_own_account() {
    let raised = |holder| Raised {
        event: "e".to_owned(),
        holder,
        reason: Reason::Cause("broker_notice".to_owned()),
        mode: Some("paused"),
        raised_at: "2026-10-17T11:00:00.000000000Z".to_owned(),
    };
    let hold = |holder| Hold {
        raised: raised(holder),
        agent_id: "a".to_owned(),
        account: "acct:w:x".to_owned(),
        request: None,
    };
    let account_wide = hold(Holder::Account);
    assert!(account_wide.blocks("a", "acct:w:x"));
    assert!(account_wide.blocks("b", "acct:w:x"));
    assert!(!account_wide.blocks("b", "acct:w:y"));
    let own = hold(Holder::Agent("a".to_owned()));
    assert!(own.blocks("a", "acct:w:x"));
    assert!(!own.blocks("b", "acct:w:x"));
    assert!(!own.blocks("a", "acct:w:y"));
}

/// The list attaches to a hold only an unspent request for that very event and subject: a spent
/// one, another hold's, and one for another subject of the same hold are each left off, and the
/// hold is posted to its own agent; the latest matching unspent request is attached, and the hold
/// is posted to the agent that request names (§3.11 item 12).
#[test]
fn a_hold_lists_only_an_unspent_request_for_its_own_event_and_subject() {
    let rows = vec![
        row(
            1,
            "RiskLimitTriggered",
            1,
            r#"{"agent_id":"a","limit":"max_daily_loss"}"#,
        ),
        row(
            2,
            "RiskLimitTriggered",
            1,
            r#"{"agent_id":"b","limit":"max_daily_loss"}"#,
        ),
    ];
    let held = rows[0].event_id.clone();
    let other = rows[1].event_id.clone();
    let asked = |id: &str, agent: &str, event: &str, subject: &str| Requested {
        event_id: id.to_owned(),
        agent: agent.to_owned(),
        event: event.to_owned(),
        subject: subject.to_owned(),
        requested_by: "bob".to_owned(),
    };
    let mut requested = vec![
        (asked("r1", "spent", &held, "limit"), true),
        (asked("r2", "elsewhere", &other, "limit"), false),
        (asked("r3", "restricted", &held, "restriction"), false),
    ];
    let accounts = [(
        "acct:w:a".to_owned(),
        rows,
        vec!["a".to_owned(), "b".to_owned()],
    )];
    let listed = |requested: &[(Requested, bool)]| -> Vec<(String, String, Option<String>)> {
        holds(&accounts, requested)
            .expect("readable")
            .into_iter()
            .map(|hold| {
                (
                    hold.raised.event,
                    hold.agent_id,
                    hold.request.map(|r| r.event_id),
                )
            })
            .collect()
    };
    assert_eq!(
        listed(&requested),
        [
            (held.clone(), "a".to_owned(), None),
            (other.clone(), "elsewhere".to_owned(), Some("r2".to_owned())),
        ]
    );
    requested.push((asked("r4", "first", &held, "limit"), false));
    requested.push((asked("r5", "latest", &held, "limit"), false));
    assert_eq!(
        listed(&requested),
        [
            (held, "latest".to_owned(), Some("r5".to_owned())),
            (other, "elsewhere".to_owned(), Some("r2".to_owned())),
        ]
    );
}
