//! DEC-533 items 3 and 4 (journal spec v0.17 §9.7): the runtime's `ApprovalResponded` always
//! carries `quorum`, `null` unless check 7 was judged, and the always-`null` `separation_of_duties`
//! and `delegation`; its `ApprovalRevalidated` writes `decided_by_now` as `null`, never a text
//! label, when the re-classification is not an `ask`. Every record of both kinds the runtime writes
//! is handed, in an envelope of this file's own making, to `mandate_journal::Draft::parse`, which
//! shares no code with the runtime's writer, so a record the journal would refuse fails here.
//!
//! Until its change lands the writer omits the three members and writes a text `decided_by_now`
//! for every re-classification, so these tests fail on its answer rather than at a stub (DEC-489's
//! pattern, DEC-137).

mod common;

use std::collections::BTreeSet;

use common::escalation::{
    ASKED_AT, Answer, BOUND_LIMIT, asking_shell, next_control_seq, rebound_shell, tail,
    two_approvers,
};
use common::{AUTHOR, AllowGate, DenyGate, FixedPlan, Ran, TestIds, ports, universe};
use mandate_canon::{Value, to_canonical};
use mandate_journal::{Draft, Invalid};
use mandate_runtime::{Autonomy, EventDraft};

const MANDATE_REF: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";

/// The ask label another trigger gives the re-classification (#731 review).
const OTHER_LABEL: &str = "rule:another";

/// The runs that write each kind of answer record: a grant refused before check 7, a skip, a grant
/// that acts on an `ask` and on an `auto` re-classification, grants skipped on re-validation by a
/// `deny` re-classification, by an `ask` with another label, and by the gate; and, under a request
/// bound to two approvers or to independence, grants check 7 judged and did not admit: `counted`,
/// `duplicate_approver` and `not_independent`.
fn runs() -> Vec<(&'static str, Ran)> {
    let (ids, gate) = (TestIds, AllowGate);
    let mut view = universe(&["AAPL"]);
    view.version = MANDATE_REF.to_owned();
    let asking_plan = FixedPlan::opening(Autonomy::Ask);
    let auto_plan = FixedPlan::opening(Autonomy::Auto);
    let deny_plan = FixedPlan::opening(Autonomy::Deny);
    let other_plan = FixedPlan {
        decided_by: OTHER_LABEL,
        ..FixedPlan::opening(Autonomy::Ask)
    };
    let closing = DenyGate("close_window");
    let asking = ports(&ids, &gate, &asking_plan, &view);
    let mut out = Vec::new();
    let (mut shell, asked) = asking_shell(&asking, Some(BOUND_LIMIT));
    let by_author = Answer {
        responder: AUTHOR.to_owned(),
        ..Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 20)
    }
    .event();
    out.push(("refused", tail(&mut shell, &by_author, &asking)));
    let (mut shell, asked) = asking_shell(&asking, Some(BOUND_LIMIT));
    let skip = Answer::skip(next_control_seq(&shell), &asked, ASKED_AT + 20).event();
    out.push(("skipped", tail(&mut shell, &skip, &asking)));
    for (name, now) in [
        ("act", ports(&ids, &gate, &asking_plan, &view)),
        ("act_auto", ports(&ids, &gate, &auto_plan, &view)),
        ("skip_deny", ports(&ids, &gate, &deny_plan, &view)),
        ("skip_other_label", ports(&ids, &gate, &other_plan, &view)),
        ("skip_gate", ports(&ids, &closing, &asking_plan, &view)),
    ] {
        let (mut shell, asked) = asking_shell(&asking, Some(BOUND_LIMIT));
        let grant = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 30).event();
        out.push((name, tail(&mut shell, &grant, &now)));
    }
    let mut listed = two_approvers();
    listed.version = MANDATE_REF.to_owned();
    let judging = ports(&ids, &gate, &asking_plan, &listed);
    let (mut shell, asked) = rebound_shell(&judging, 2, false);
    let first = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 20).event();
    out.push(("counted", tail(&mut shell, &first, &judging)));
    let again = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 25).event();
    out.push(("duplicate_approver", tail(&mut shell, &again, &judging)));
    let (mut shell, asked) = rebound_shell(&judging, 1, true);
    let by_author = Answer {
        responder: AUTHOR.to_owned(),
        ..Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 20)
    }
    .event();
    out.push(("not_independent", tail(&mut shell, &by_author, &judging)));
    out
}

fn records<'a>(ran: &'a Ran, event_type: &str) -> Vec<&'a EventDraft> {
    ran.drafts
        .iter()
        .filter(|d| d.event_type == event_type)
        .collect()
}

fn digests(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Str(s) if s.len() == 71 && s.starts_with("sha256:") => {
            out.insert(s.clone());
        }
        Value::Array(items) => items.iter().for_each(|v| digests(v, out)),
        Value::Object(members) => members.values().for_each(|v| digests(v, out)),
        _ => {}
    }
}

/// The record in an agent-stream envelope of this file's own making, naming its mandate and listing
/// the payload's references (journal spec §3), parsed by the journal's registered schemas.
fn parse_as_journal_draft(draft: &EventDraft) -> Result<Draft, Invalid> {
    let causation = draft
        .causation_id
        .as_ref()
        .map_or_else(|| "null".to_owned(), |id| format!("\"{}\"", id.0));
    let mut listed = BTreeSet::new();
    digests(&draft.payload, &mut listed);
    let refs: Vec<String> = listed.iter().map(|r| format!("\"{r}\"")).collect();
    let body = format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{}",
        "stream_id":"agent:ws1:agent-a","event_type":"{}","schema_version":1,
        "event_time":"2026-09-21T13:59:58.000000000Z","clock_source":"scheduler",
        "causation_id":{causation},"correlation_id":null,
        "actor":{{"kind":"agent","id":"agent-a","version":"0.1.0","build":"sha256:{}"}},
        "config_refs":{{"mandate_version":"{MANDATE_REF}"}},
        "payload":{},"artifact_refs":[{}],"pii_refs":[]}}"#,
        draft.event_id.0,
        draft.event_type,
        "c".repeat(64),
        String::from_utf8_lossy(&to_canonical(&draft.payload)),
        refs.join(",")
    );
    Draft::parse(body.as_bytes())
}

#[test]
fn every_answer_record_the_runtime_writes_passes_the_journals_check() {
    let mut seen = BTreeSet::new();
    let mut failed = Vec::new();
    for (name, ran) in runs() {
        for event_type in ["ApprovalResponded", "ApprovalRevalidated"] {
            for draft in records(&ran, event_type) {
                seen.insert(event_type);
                if let Err(e) = parse_as_journal_draft(draft) {
                    failed.push(format!("{name} {event_type}: {e:?} in {:?}", draft.payload));
                }
            }
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert_eq!(seen.len(), 2, "both records were written: {seen:?}");
}

/// Rule 48 and the two always-`null` members: `quorum` is present on every copy, `null` before
/// check 7 and on a skip, and exactly the count and independence the request bound on a grant check
/// 7 judged, whether it admitted, counted or refused it.
#[test]
fn a_responded_record_carries_its_quorum_only_where_check_7_was_judged() {
    for (name, ran) in runs() {
        let [responded] = records(&ran, "ApprovalResponded")[..] else {
            panic!("{name}: one ApprovalResponded in {:?}", ran.draft_types())
        };
        let p = &responded.payload;
        for member in ["separation_of_duties", "delegation"] {
            assert_eq!(p.get(member), Some(&Value::Null), "{name}: {member}");
        }
        let quorum = p.get("quorum");
        let bound = match name {
            "refused" | "skipped" => None,
            "counted" | "duplicate_approver" => Some((2, false)),
            "not_independent" => Some((1, true)),
            _ => Some((1, false)),
        };
        let Some((required, independent)) = bound else {
            assert_eq!(quorum, Some(&Value::Null), "{name}: quorum");
            continue;
        };
        let recorded = (
            quorum
                .and_then(|q| q.get("required"))
                .and_then(Value::as_int),
            quorum.and_then(|q| q.get("independent")),
        );
        assert_eq!(
            recorded,
            (Some(required), Some(&Value::Bool(independent))),
            "{name}: {quorum:?}"
        );
    }
}

/// DEC-533 item 4: an `auto` re-classification passes check 10 with no label, written `null`; an
/// `ask` writes its own label, the bound one or another trigger's; a `deny` writes no label either.
#[test]
fn decided_by_now_is_null_unless_the_reclassification_asks() {
    for (name, ran) in runs() {
        for revalidated in records(&ran, "ApprovalRevalidated") {
            let now = revalidated.payload.get("decided_by_now");
            let bound = revalidated.payload.get("decided_by_bound");
            match name {
                "act" | "skip_gate" => assert_eq!(now, bound, "{name}"),
                "act_auto" | "skip_deny" => assert_eq!(now, Some(&Value::Null), "{name}"),
                "skip_other_label" => {
                    assert_eq!(now, Some(&Value::Str(OTHER_LABEL.to_owned())), "{name}");
                    assert_ne!(
                        now, bound,
                        "{name}: the label is the re-classification's own"
                    );
                }
                other => panic!("{other} re-validated"),
            }
        }
    }
}

/// DEC-533 item 4 and DEC-830: an `ask` re-classification whose label is empty names no trigger,
/// so `decided_by_now` is `null`, as for an `auto` or a `deny`, never the empty text (#782 review:
/// the mutant that drops the empty-label guard in `asked_by` survived every other test).
#[test]
fn an_ask_reclassification_with_an_empty_label_writes_a_null_decided_by_now() {
    let (ids, gate) = (TestIds, AllowGate);
    let mut view = universe(&["AAPL"]);
    view.version = MANDATE_REF.to_owned();
    let asking_plan = FixedPlan::opening(Autonomy::Ask);
    let unlabelled_plan = FixedPlan {
        decided_by: "",
        ..FixedPlan::opening(Autonomy::Ask)
    };
    let asking = ports(&ids, &gate, &asking_plan, &view);
    let unlabelled = ports(&ids, &gate, &unlabelled_plan, &view);
    let (mut shell, asked) = asking_shell(&asking, Some(BOUND_LIMIT));
    let grant = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 30).event();
    let ran = tail(&mut shell, &grant, &unlabelled);
    let [revalidated] = records(&ran, "ApprovalRevalidated")[..] else {
        panic!("one ApprovalRevalidated in {:?}", ran.draft_types())
    };
    assert_eq!(
        revalidated.payload.get("decided_by_now"),
        Some(&Value::Null),
        "an empty ask label is written null: {:?}",
        revalidated.payload
    );
    assert!(
        revalidated
            .payload
            .get("decided_by_bound")
            .is_some_and(|b| *b != Value::Null),
        "the bound label is still the request's own: {:?}",
        revalidated.payload
    );
}
