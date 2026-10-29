//! `mandate_journal::holds` (journal spec §9.28, DEC-2830, DEC-2144): each hold ends only by the
//! record §9.28 names for it, an acknowledgment copy ends a limit's trigger unless the next record
//! refuses it, a record the fold cannot read fails it, and the overlay never reads an unreadable
//! policy as `false`. Seeded from the workspace API's fold tests, whose fold this replaces.

use mandate_canon::{Digest, parse, to_canonical};
use mandate_journal::StoredEvent;
use mandate_journal::holds::{
    Hold, Holder, Raised, Reason, Requested, Unreadable, independence_required, requests, standing,
};

fn caused(
    seq: u64,
    event_type: &str,
    schema_version: u64,
    causation: Option<&str>,
    payload: &str,
) -> StoredEvent {
    let event_id = format!("01JBH3BTZ8K2M4N6P8R0S2T{seq:03}");
    let causation = causation.map_or("null".to_owned(), |id| format!(r#""{id}""#));
    let body = to_canonical(
        &parse(
            format!(
                r#"{{"causation_id":{causation},"event_id":"{event_id}","payload":{payload}}}"#
            )
            .as_bytes(),
        )
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

fn row(seq: u64, event_type: &str, schema_version: u64, payload: &str) -> StoredEvent {
    caused(seq, event_type, schema_version, None, payload)
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
        Err(Unreadable),
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
    assert_eq!(
        kinds,
        ["cash", "fees", "external_activity", "incomplete"].map(|kind| Reason::Reconciliation {
            restriction: format!("reconciliation:{kind}"),
            kind: kind.to_owned(),
            instrument: None,
        })
    );
    let unreadable = row(
        13,
        "AgentModeApplied",
        1,
        r#"{"agent":"a","originated":"yes","restriction":"reconciliation:SPY","to":"paused"}"#,
    );
    assert_eq!(standing(&[unreadable]), Err(Unreadable));
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
    assert_eq!(requests(&[unreadable]), Err(Unreadable));
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

fn trigger(seq: u64, agent: &str, limit: &str) -> StoredEvent {
    row(
        seq,
        "RiskLimitTriggered",
        1,
        &format!(r#"{{"agent_id":"{agent}","limit":"{limit}"}}"#),
    )
}

fn copy(seq: u64, original: &str, event: &str) -> StoredEvent {
    caused(
        seq,
        "OwnerAcknowledged",
        2,
        Some(original),
        &format!(r#"{{"event":"{event}","subject":"limit"}}"#),
    )
}

fn refusal(seq: u64, cause: &str, command: &str) -> StoredEvent {
    caused(
        seq,
        "OwnerCommandRefused",
        1,
        Some(cause),
        &format!(r#"{{"command":"{command}","reason":"step_up_missing"}}"#),
    )
}

/// The executor's reading of a limit's acknowledgment (`mandate-executor`'s `acknowledge.rs`): a
/// copy naming a standing trigger ends every standing trigger of its agent's limit, and only its
/// own refusal as the very next record restores them.
#[test]
fn an_acknowledgment_copy_ends_its_trigger_unless_the_next_record_refuses_it() {
    let first = trigger(1, "a", "max_daily_loss");
    let other = trigger(2, "b", "max_daily_loss");
    let named = first.event_id.clone();
    let ended = [first.clone(), other.clone(), copy(3, "ctl1", &named)];
    assert_eq!(events(&ended), std::slice::from_ref(&other.event_id));
    let refused = [
        first.clone(),
        other.clone(),
        copy(3, "ctl1", &named),
        refusal(4, "ctl1", "acknowledge"),
    ];
    assert_eq!(events(&refused), [named.clone(), other.event_id.clone()]);
    for (late, why) in [
        (
            vec![
                copy(3, "ctl1", &named),
                trigger(4, "c", "x"),
                refusal(5, "ctl1", "acknowledge"),
            ],
            "a refusal that is not the next record",
        ),
        (
            vec![copy(3, "ctl1", &named), refusal(4, "ctl2", "acknowledge")],
            "another copy's refusal",
        ),
        (
            vec![copy(3, "ctl1", &named), refusal(4, "ctl1", "resume")],
            "a refusal of another command",
        ),
    ] {
        let mut rows = vec![first.clone()];
        rows.extend(late);
        assert!(!events(&rows).contains(&named), "{why} restores nothing");
    }
    let unknown = [first.clone(), copy(2, "ctl1", "01JNOTATRIGGER000000000000")];
    assert_eq!(
        events(&unknown),
        std::slice::from_ref(&named),
        "a copy of no trigger ends nothing"
    );
    let no_cause = [
        first,
        row(
            2,
            "OwnerAcknowledged",
            2,
            &format!(r#"{{"event":"{named}"}}"#),
        ),
    ];
    assert_eq!(standing(&no_cause), Err(Unreadable));
}

fn member(seq: u64, kind: &str, payload: &str) -> StoredEvent {
    row(seq, kind, 1, payload)
}

#[test]
fn independence_is_the_latest_membership_record_s_and_never_false_when_it_does_not_read() {
    assert!(!independence_required(&[]), "none is false");
    let off = member(
        1,
        "MemberActivated",
        r#"{"independent_approval_required":false}"#,
    );
    let on = member(
        2,
        "MemberRoleChanged",
        r#"{"independent_approval_required":true}"#,
    );
    let unrelated = member(
        3,
        "PolicyChanged",
        r#"{"independent_approval_required":false}"#,
    );
    assert!(independence_required(&[off.clone(), on.clone(), unrelated]));
    assert!(!independence_required(&[on.clone(), off.clone()]));
    let reactivated = member(
        4,
        "MemberReactivated",
        r#"{"independent_approval_required":true}"#,
    );
    assert!(independence_required(&[off.clone(), reactivated]));
    for unreadable in [r#"{"independent_approval_required":"no"}"#, r#"{}"#] {
        assert!(
            independence_required(&[off.clone(), member(5, "MemberRoleChanged", unreadable)]),
            "{unreadable}"
        );
    }
    let bad_body = StoredEvent {
        body: b"not json".to_vec(),
        ..off.clone()
    };
    assert!(independence_required(&[bad_body]));
}

#[test]
fn a_reason_names_its_subject() {
    assert_eq!(Reason::Limit("x".to_owned()).subject(), "limit");
    assert_eq!(Reason::Cause("x".to_owned()).subject(), "restriction");
    assert_eq!(
        Reason::Reconciliation {
            restriction: "reconciliation:cash".to_owned(),
            kind: "cash".to_owned(),
            instrument: None,
        }
        .subject(),
        "reconciliation"
    );
}

/// Each hold carries its latest unspent request of the same event and subject, and is posted to
/// that request's agent, else its own agent, else the account's first deployed agent; an
/// account-wide hold on an account with no agent and no request is left out.
#[test]
fn a_hold_is_posted_to_its_request_s_agent_or_its_own_or_the_account_s_first() {
    let limit = trigger(1, "b", "max_daily_loss");
    let account_hold = row(
        2,
        "AgentModeApplied",
        1,
        r#"{"agent":"*","originated":true,"restriction":"reconciliation:cash","to":"paused"}"#,
    );
    let asked = |seq: u64, event: &str, subject: &str, by: &str| {
        row(
            seq,
            "OwnerAcknowledgmentRequested",
            2,
            &format!(
                r#"{{"agent":"{by}","event":"{event}","requested_by":"bob","subject":"{subject}"}}"#
            ),
        )
    };
    let older = asked(10, &limit.event_id, "limit", "b");
    let newer = asked(11, &limit.event_id, "limit", "b");
    let other_subject = asked(12, &account_hold.event_id, "limit", "a");
    let spent = asked(13, &account_hold.event_id, "reconciliation", "a");
    let answer = row(
        14,
        "OwnerAcknowledged",
        2,
        &format!(r#"{{"request":"{}"}}"#, spent.event_id),
    );
    let control = [older, newer.clone(), other_subject, spent, answer];
    let requested = requests(&control).expect("readable");
    let accounts = [(
        "acct:w:x".to_owned(),
        vec![limit.clone(), account_hold.clone()],
        vec!["a".to_owned(), "b".to_owned()],
    )];
    let listed = mandate_journal::holds::holds(&accounts, &requested).expect("readable");
    let summary: Vec<(String, String, Option<String>)> = listed
        .iter()
        .map(|h| {
            (
                h.raised.event.clone(),
                h.agent_id.clone(),
                h.request.as_ref().map(|r| r.event_id.clone()),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (limit.event_id.clone(), "b".to_owned(), Some(newer.event_id)),
            (account_hold.event_id.clone(), "a".to_owned(), None),
        ]
    );
    assert!(listed.iter().all(|h| h.account == "acct:w:x"));
    let nobody = [(
        "acct:w:y".to_owned(),
        vec![account_hold],
        Vec::<String>::new(),
    )];
    assert_eq!(
        mandate_journal::holds::holds(&nobody, &[]).map(|h| h.len()),
        Ok(0)
    );
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
        mandate_journal::holds::holds(&accounts, requested)
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
