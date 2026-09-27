//! E8-1's request content, notification, and quiet-hours clauses of the
//! [M7 brief](../../../docs/project/tasks/M7-escalation-v0.md): one named test per clause, each
//! naming the MC-E case or planted bug (PB-n) it stands for until the M7 spec PR lands the cases
//! (DEC-165 items 1, 3, 7, and 8).
//!
//! Every test but the live one at the end is pending until the implementation PR and fails on
//! `ApprovalError::Unimplemented` (DEC-77, DEC-110).

mod common;

use std::collections::BTreeSet;
use std::num::NonZeroU8;

use common::{DEADLINE, REQUEST_ID, RISK_IMPACT, T0, answer, content, hash, price, request};
use mandate_approval::{
    ApprovalRef, AskablePurpose, AssetClass, Channel, Delivery, QuietHours, RiskClock,
    confirmation_code, content_hash, content_object, deliver_now, notification_for,
    notification_payload,
};
use mandate_canon::{Digest, Value, to_canonical};
use mandate_num::{Qty, Signed};
use mandate_time::{NewYorkTime, UtcNanos};

fn text<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter().try_fold(value, |v, key| v.get(key))?.as_str()
}

/// Every key of `value`, at every nesting level, as a path (`action.qty`, `risk_impact[].cap`):
/// an array contributes its elements' keys under `[]`, so one walk pins the whole shape.
fn key_paths(value: &Value, prefix: &str, out: &mut Vec<String>) {
    match value {
        Value::Object(members) => {
            for (key, member) in members {
                let path = if prefix.is_empty() {
                    key.to_string()
                } else {
                    format!("{prefix}.{key}")
                };
                out.push(path.clone());
                key_paths(member, &path, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                key_paths(item, &format!("{prefix}[]"), out);
            }
        }
        _ => {}
    }
}

/// E8-1, EI-14: the content is mandate spec §6.4's list as DEC-165 item 3 names it, at every
/// nesting level, and nothing else, so no field can be dropped or added unseen.
#[test]
#[ignore = "pending E8-1"]
fn the_content_object_has_exactly_the_listed_fields() {
    let object_value = answer("content_object", content_object(&content()));
    let mut paths = Vec::new();
    key_paths(&object_value, "", &mut paths);
    let paths: BTreeSet<String> = paths.into_iter().collect();
    let expected: BTreeSet<String> = [
        "action",
        "action.asset_class",
        "action.instrument",
        "action.limit",
        "action.order_usd",
        "action.purpose",
        "action.qty",
        "action.side",
        "approvers",
        "approvers.independent",
        "approvers.required",
        "choices",
        "deadline",
        "default",
        "evidence",
        "evidence.combined_score",
        "evidence.combined_score.label",
        "evidence.combined_score.value",
        "evidence.outputs",
        "evidence.outputs[].artifact",
        "evidence.outputs[].event_id",
        "evidence.outputs[].label",
        "reference_mark",
        "reference_mark.price",
        "reference_mark.seq",
        "risk_impact",
        "risk_impact[].cap",
        "risk_impact[].field",
        "risk_impact[].value",
        "trigger",
        "trigger.decided_by",
        "trigger.mandate_version",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(paths, expected);
}

/// E8-1: the proposed action is the bound order, its side is `buy`, and its value is limit × qty.
#[test]
#[ignore = "pending E8-1"]
fn the_action_is_the_bound_order_and_its_value() {
    let object_value = answer("content_object", content_object(&content()));
    let field = |key| text(&object_value, &["action", key]);
    assert_eq!(field("instrument"), Some("asset-equity-1"));
    assert_eq!(field("asset_class"), Some("us_equity"));
    assert_eq!(field("side"), Some("buy"));
    assert_eq!(field("qty"), Some("10"));
    assert_eq!(field("limit"), Some("187.25"));
    assert_eq!(field("order_usd"), Some("1872.5"), "187.25 × 10");
    assert_eq!(field("purpose"), Some("open"));
    assert_eq!(
        text(&object_value, &["trigger", "mandate_version"]),
        Some("v3")
    );
    assert_eq!(
        text(&object_value, &["trigger", "decided_by"]),
        Some("rule:large_order")
    );
}

/// E8-1 "risk impact": the six §6.3 figures at the request, each beside the mandate's own cap,
/// in the fixture's order; facts about the order, never an estimate.
#[test]
#[ignore = "pending E8-1"]
fn the_risk_impact_lists_every_figure_with_its_cap() {
    let object_value = answer("content_object", content_object(&content()));
    let rendered: Vec<(Option<&str>, Option<&str>, Option<&str>)> = object_value
        .get("risk_impact")
        .and_then(Value::as_array)
        .map(|figures| {
            figures
                .iter()
                .map(|f| (text(f, &["field"]), text(f, &["value"]), text(f, &["cap"])))
                .collect()
        })
        .unwrap_or_default();
    let names = [
        "order_usd",
        "position_usd_after",
        "gross_usd_after",
        "bought_today_usd",
        "drawdown",
        "daily_pnl_fraction",
    ];
    let expected: Vec<(Option<&str>, Option<&str>, Option<&str>)> = names
        .iter()
        .zip(RISK_IMPACT)
        .map(|(name, (_, value, cap))| (Some(*name), Some(value), Some(cap)))
        .collect();
    assert_eq!(rendered, expected);
    assert_eq!(
        text(&object_value, &["reference_mark", "price"]),
        Some("187")
    );
}

/// E8-1 and PX-10: the default is skip, stated in §6.4's words, and approve and skip are the only
/// choices, in that order, with neither preselected.
#[test]
#[ignore = "pending E8-1"]
fn the_default_is_skip_and_approve_and_skip_have_equal_weight() {
    let object_value = answer("content_object", content_object(&content()));
    assert_eq!(
        text(&object_value, &["default"]),
        Some("If you do nothing, this action is skipped")
    );
    let choices = object_value.get("choices").and_then(Value::as_array);
    assert_eq!(
        choices.map(|c| c.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["approve", "skip"])
    );
    let deadline = UtcNanos::from_parts(DEADLINE, 0).unwrap().to_string();
    assert_eq!(text(&object_value, &["deadline"]), Some(deadline.as_str()));
}

/// E8-1 and §6.4: the score and each output carry the spec's labels; theses stay references.
#[test]
#[ignore = "pending E8-1"]
fn the_score_and_the_evidence_carry_the_spec_labels() {
    let object_value = answer("content_object", content_object(&content()));
    assert_eq!(
        text(&object_value, &["evidence", "combined_score", "label"]),
        Some("combined model score, not a probability of profit")
    );
    let labels: Vec<&str> = object_value
        .get("evidence")
        .and_then(|e| e.get("outputs"))
        .and_then(Value::as_array)
        .map(|o| o.iter().filter_map(|x| x.get("label")?.as_str()).collect())
        .unwrap_or_default();
    assert_eq!(
        labels,
        ["Output of software you selected", "platform-authored"]
    );
    assert_eq!(
        text(&object_value, &["evidence", "combined_score", "value"]),
        Some("0.62")
    );
    let outputs = object_value
        .get("evidence")
        .and_then(|e| e.get("outputs"))
        .and_then(Value::as_array)
        .map(<[Value]>::to_vec)
        .unwrap_or_default();
    let artifacts: Vec<Option<&Value>> = outputs.iter().map(|o| o.get("artifact")).collect();
    let thesis = Value::Str(Digest::of(b"thesis").to_string());
    assert_eq!(
        artifacts,
        [Some(&Value::Null), Some(&thesis)],
        "theses stay by hash"
    );
}

/// FR-6.2, DEC-126: nothing in the content reads as advice. Owner-written rule text is not in the
/// content yet (DEC-165 item 3); when it is, it is scanned apart from the platform's own text.
#[test]
#[ignore = "pending E8-1"]
fn the_content_never_carries_advice_wording() {
    let bytes = to_canonical(&answer("content_object", content_object(&content())));
    let text = String::from_utf8(bytes).unwrap().to_lowercase();
    assert!(text.contains("approve"), "the content was rendered");
    for word in [
        "recommend",
        "estimate",
        "target",
        "scorecard",
        "expected",
        "suggest",
    ] {
        assert!(!text.contains(word), "the content says {word:?}");
    }
}

/// EI-14: the hash is SHA-256 of the canonical object (journal spec §4).
#[test]
#[ignore = "pending E8-1"]
fn the_content_hash_is_the_sha256_of_the_canonical_object() {
    let object_value = answer("content_object", content_object(&content()));
    let hash = answer("content_hash", content_hash(&content()));
    assert_eq!(hash.0, Digest::of(&to_canonical(&object_value)));
}

/// EI-14, PB-15: what the owner saw is what is bound, so every bound field moves the hash.
#[test]
#[ignore = "pending E8-1"]
fn every_bound_field_moves_the_content_hash() {
    let base = answer("content_hash", content_hash(&content()));
    let mut changed = Vec::new();
    let mut edit = |f: &dyn Fn(&mut mandate_approval::RequestContent)| {
        let mut c = content();
        f(&mut c);
        changed.push(answer("content_hash", content_hash(&c)));
    };
    edit(&|c| c.bound.instrument = "asset-equity-2".to_owned());
    edit(&|c| c.bound.qty = Qty::parse("11").unwrap());
    edit(&|c| c.bound.limit = price("187.26"));
    edit(&|c| c.bound.mandate_version = "v4".to_owned());
    edit(&|c| c.bound.decided_by = "default".to_owned());
    edit(&|c| c.bound.approvers_required = NonZeroU8::new(2).unwrap());
    edit(&|c| c.deadline = RiskClock(DEADLINE + 1));
    edit(&|c| c.bound.reference_mark = None);
    edit(&|c| c.bound.asset_class = AssetClass::Crypto);
    edit(&|c| c.bound.purpose = AskablePurpose::Increase);
    edit(&|c| c.bound.combined_score = Signed::parse("0.99").unwrap());
    edit(&|c| c.risk_impact.truncate(1));
    edit(&|c| c.evidence[1].artifact = Some(Digest::of(b"other thesis")));
    assert_eq!(changed.len(), 13, "every edit ran");
    for (i, h) in changed.iter().enumerate() {
        assert_ne!(*h, base, "edit {i} left the hash unchanged");
    }
}

/// DEC-155 item 4: `approvals show`'s code is bound to one content hash.
#[test]
#[ignore = "pending E8-1"]
fn the_confirmation_code_is_bound_to_the_content_hash() {
    let a = answer("confirmation_code", confirmation_code(&hash("a")));
    let again = answer("confirmation_code", confirmation_code(&hash("a")));
    let b = answer("confirmation_code", confirmation_code(&hash("b")));
    assert_eq!(a, again);
    assert_ne!(a, b);
    assert!(!a.0.is_empty());
}

/// EI-9, PB-9, rule 6: a notification is the opaque id and one generic text, nothing else.
#[test]
#[ignore = "pending E8-1"]
fn a_notification_is_the_opaque_id_and_one_generic_text() {
    let n = answer("notification_for", notification_for(&request()));
    let payload = answer("notification_payload", notification_payload(&n));
    assert_eq!(
        String::from_utf8(to_canonical(&payload)).unwrap(),
        format!(r#"{{"subject":"{REQUEST_ID}","text":"approval_needed"}}"#)
    );
}

fn quiet() -> Option<QuietHours> {
    Some(QuietHours {
        start: NewYorkTime::new(23, 0).unwrap(),
        end: NewYorkTime::new(7, 0).unwrap(),
    })
}

fn deliver(channel: Channel, at: i64) -> Delivery {
    answer("deliver_now", deliver_now(channel, quiet(), RiskClock(at)))
}

/// MC-E29, PB-17, DEC-156 item 6: `cli_inbox` is delivered inside quiet hours.
#[test]
#[ignore = "pending E8-1"]
fn the_cli_inbox_is_delivered_inside_quiet_hours() {
    let two_am_edt = T0 - 8 * 3600;
    assert_eq!(deliver(Channel::CliInbox, two_am_edt), Delivery::Send);
    assert_eq!(
        deliver(Channel::Push, two_am_edt),
        Delivery::SuppressedQuietHours
    );
}

/// MC-E30, PB-17: a push is suppressed from 23:00 and sent from 07:00 New York time, in both DST
/// states.
#[test]
#[ignore = "pending E8-1"]
fn a_push_is_suppressed_from_23_00_and_sent_from_07_00_in_both_dst_states() {
    for (eleven_pm, seven_am) in [
        (1_768_536_000, 1_768_564_800),
        (1_784_170_800, 1_784_199_600),
    ] {
        assert_eq!(deliver(Channel::Push, eleven_pm - 1), Delivery::Send);
        assert_eq!(
            deliver(Channel::Push, eleven_pm),
            Delivery::SuppressedQuietHours
        );
        assert_eq!(
            deliver(Channel::Push, seven_am - 1),
            Delivery::SuppressedQuietHours
        );
        assert_eq!(deliver(Channel::Push, seven_am), Delivery::Send);
    }
    let none = answer(
        "deliver_now",
        deliver_now(Channel::Push, None, RiskClock(1_768_536_000)),
    );
    assert_eq!(none, Delivery::Send);
}

/// Live: an approval reference is built from the request's event id and displays only that id.
#[test]
fn an_approval_ref_displays_only_its_event_id() {
    let r = ApprovalRef::of_requested_event(REQUEST_ID);
    assert_eq!(r.to_string(), REQUEST_ID);
    assert_ne!(ApprovalRef::of_requested_event("other"), r);
}
