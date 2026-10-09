//! The map's own pieces, on inputs no reference case reaches: a name bound twice or missing, a
//! reference mark or a quorum with a member too many, a cause record of the right type and the
//! wrong reason, a handoff no record authorises, a member supplied with no value, an unstated
//! answer member the runtime writes with a value, a re-validation's label under every
//! re-classification, and an intent whose step reads another mandate version than its request
//! bound.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::Value;
use mandate_journal::Environment;
use mandate_runtime::{
    ApprovalSettings, Effect, EventDraft, EventId, FlattenPlan, IntentBody, IntentHandoff, Purpose,
};
use serde_json::json;

use super::{
    As, Asked, NULL_UNLESS_STATED, Ran, Shell, bind, decided_by_now, extra_fault, handoffs, member,
    object, seconds, strip, text, translate, unstated_fault, unstated_quorum,
};
use crate::ensure;

/// A name binds to one runtime name and a runtime name to one name, and each must be present.
#[test]
fn names_bind_one_to_one() -> Result<(), String> {
    let mut bound = BTreeMap::new();
    bind(&mut bound, Some("ap1"), Some("R1"))?;
    bind(&mut bound, Some("ap1"), Some("R1"))?;
    bind(&mut bound, Some("ap2"), Some("R2"))?;
    for (name, runtime) in [
        (Some("ap1"), Some("R3")),
        (Some("ap3"), Some("R1")),
        (None, Some("R4")),
        (Some("ap4"), None),
    ] {
        ensure(bind(&mut bound, name, runtime).is_err(), || {
            format!("{name:?} bound to {runtime:?}")
        })?;
    }
    ensure(bound.len() == 2, || format!("{bound:?}"))
}

/// A reference mark is exactly `{price, seq}`: a member the map does not name fails it.
#[test]
fn a_reference_mark_has_exactly_its_members() -> Result<(), String> {
    let want = json!({"price": "155", "seq": 2});
    let mark = |extra: bool| -> Result<Value, String> {
        let mut members = vec![("price", text("155")), ("seq", seconds(2)?)];
        if extra {
            members.push(("feed", text("sip")));
        }
        object(members)
    };
    member(As::Canonical, &want, Some(&mark(false)?))?;
    ensure(
        member(As::Canonical, &want, Some(&mark(true)?)).is_err(),
        || "an extra member passed".to_owned(),
    )
}

fn draft(id: &str, event_type: &str, payload: Value) -> EventDraft {
    EventDraft {
        event_id: EventId(id.to_owned()),
        event_type: event_type.to_owned(),
        causation_id: None,
        payload,
    }
}

fn flatten(id: &str) -> Effect {
    Effect::Intent(IntentHandoff {
        intent_id: EventId(id.to_owned()),
        body: IntentBody::Flatten(FlattenPlan {
            cancel_client_order_ids: Vec::new(),
            sells: Vec::new(),
            purpose: Purpose::RiskExit,
            confirmation: None,
        }),
        execution: None,
    })
}

fn order(id: &str) -> Result<Effect, String> {
    Ok(Effect::Intent(IntentHandoff {
        intent_id: EventId(id.to_owned()),
        body: IntentBody::Order {
            instrument: mandate_accounting::InstrumentId::new("i").map_err(|e| format!("{e:?}"))?,
            side: mandate_accounting::Side::Buy,
            qty: mandate_num::Qty::parse("1").map_err(|e| e.to_string())?,
            limit: mandate_num::Price::parse("1").map_err(|e| e.to_string())?,
            purpose: Purpose::Open,
        },
        execution: None,
    }))
}

/// A cause record is set aside only with its type and, where the map names one, its reason.
#[test]
fn a_cause_record_is_set_aside_only_as_named() -> Result<(), String> {
    let paused = draft(
        "1",
        "AgentModeChanged",
        object(vec![("reason", text("owner_pause"))])?,
    );
    let cancel = draft("2", "ApprovalCanceled", Value::Null);
    let mut drafts = vec![paused.clone(), cancel.clone()];
    strip(&mut drafts, &[("AgentModeChanged", "owner_pause")])?;
    ensure(drafts == vec![cancel.clone()], || format!("{drafts:?}"))?;
    let mut drafts = vec![paused.clone(), cancel.clone()];
    ensure(
        strip(&mut drafts, &[("AgentModeChanged", "restriction_changed")]).is_err(),
        || "another reason is not set aside".to_owned(),
    )?;
    let mut drafts = vec![cancel.clone()];
    ensure(
        strip(&mut drafts, &[("AgentModeChanged", "")]).is_err(),
        || "another type is not set aside".to_owned(),
    )?;
    let mut drafts = vec![paused, cancel.clone()];
    strip(&mut drafts, &[("AgentModeChanged", "")])?;
    ensure(drafts == vec![cancel], || format!("{drafts:?}"))?;
    let mut none = Vec::new();
    strip(&mut none, &[("DecisionMade", "")]).map(|_| ())
}

/// A handoff follows the record that authorises it, and every intent is handed exactly once.
#[test]
fn every_handoff_follows_its_record() -> Result<(), String> {
    let intent = || Effect::Journal(draft("i", "IntentProposed", Value::Null));
    let switch = Effect::Journal(draft("k", "KillSwitchActivated", Value::Null));
    let exit = Effect::Journal(draft("x", "OwnerExitRequested", Value::Null));
    let good = [
        vec![intent(), order("i")?],
        vec![switch.clone(), flatten("k")],
        vec![exit.clone(), flatten("x")],
        vec![exit, switch.clone(), flatten("k")],
    ];
    for effects in good {
        let faults = handoffs(&effects);
        ensure(faults.is_empty(), || format!("{effects:?}: {faults:?}"))?;
    }
    let bad = [
        vec![order("i")?, intent()],
        vec![flatten("k")],
        vec![intent(), flatten("i")],
        vec![switch, order("k")?],
        vec![intent()],
        vec![intent(), order("i")?, order("i")?],
    ];
    for effects in bad {
        ensure(!handoffs(&effects).is_empty(), || {
            format!("{effects:?} passed")
        })?;
    }
    Ok(())
}

/// A quorum the runtime records is compared as its canonical value: a different count or
/// independence, or a member too many, fails it, and so does none at all.
#[test]
fn a_recorded_quorum_is_compared() -> Result<(), String> {
    let want = json!({"required": 1, "independent": false});
    let got = |required: i64, independent: bool, extra: bool| -> Result<Value, String> {
        let mut members = vec![
            ("required", seconds(required)?),
            ("independent", Value::Bool(independent)),
        ];
        if extra {
            members.push(("by", text("policy")));
        }
        object(members)
    };
    member(As::Quorum, &want, Some(&got(1, false, false)?))?;
    for wrong in [
        got(2, false, false)?,
        got(1, true, false)?,
        got(1, false, true)?,
    ] {
        ensure(member(As::Quorum, &want, Some(&wrong)).is_err(), || {
            format!("{wrong:?} passed")
        })?;
    }
    ensure(member(As::Quorum, &want, None).is_err(), || {
        "no quorum passed".to_owned()
    })
}

/// An unbound name passes as written, unless it is a runtime name bound to another.
#[test]
fn an_unbound_name_passes_as_written() -> Result<(), String> {
    let bound: BTreeMap<String, String> = [("ap1".to_owned(), "R1".to_owned())].into();
    ensure(translate(&bound, "ap1")? == "R1", || {
        "the bound name".to_owned()
    })?;
    ensure(translate(&bound, "ap9")? == "ap9", || {
        "an unbound name".to_owned()
    })?;
    ensure(translate(&bound, "R1").is_err(), || {
        "a runtime name as written".to_owned()
    })
}

/// A member only the runtime writes is compared when a value is supplied, and only `content` may be
/// supplied with none: a re-validation's `mode`, `band_bp` or `m_req` with no value fails, so none
/// can be switched off without the comparison noticing.
#[test]
fn only_the_content_is_supplied_uncompared() -> Result<(), String> {
    let value = text("normal");
    ensure(
        extra_fault("mode", Some(&value), Some(&value)).is_none(),
        || "an equal member failed".to_owned(),
    )?;
    ensure(
        extra_fault("mode", Some(&value), Some(&text("paused"))).is_some(),
        || "a different member passed".to_owned(),
    )?;
    ensure(extra_fault("mode", Some(&value), None).is_some(), || {
        "a missing member passed".to_owned()
    })?;
    ensure(extra_fault("content", None, Some(&value)).is_none(), || {
        "the content failed".to_owned()
    })?;
    for name in ["mode", "band_bp", "m_req"] {
        ensure(extra_fault(name, None, Some(&value)).is_some(), || {
            format!("`{name}` with no value passed")
        })?;
    }
    Ok(())
}

/// An intent's mandate version is the one its request bound, never the step's `now`: check 8 skips
/// any grant whose `now` differs, so no reference case can separate the two, and this one does.
#[test]
fn an_intent_reads_the_version_its_request_bound() -> Result<(), String> {
    let mut shell = Shell::started(
        ApprovalSettings {
            approvers: BTreeSet::from(["u1".to_owned()]),
            author: "u0".to_owned(),
            timeout_s: 300,
            environment: Environment::Paper,
        },
        0,
    )?;
    shell.asked.insert(
        "ap1".to_owned(),
        Asked {
            id: "R1".to_owned(),
            version: "sha256:v1".to_owned(),
            bound: json!({}),
        },
    );
    shell.approval_ids.insert("ap1".to_owned(), "R1".to_owned());
    let revalidation = draft(
        "E1",
        "ApprovalRevalidated",
        object(vec![("approval", text("R1"))])?,
    );
    let intent = EventDraft {
        causation_id: Some(EventId("E1".to_owned())),
        ..draft("E2", "IntentProposed", object(Vec::new())?)
    };
    let ran = Ran {
        drafts: vec![revalidation],
        effects: Vec::new(),
        clock: 0,
        asked: None,
        now: Some(json!({"mandate_version": "sha256:v2"})),
        decision: None,
    };
    let want = |version: &str| json!({"approval": "ap1", "mandate_version": version});
    let bound = shell.intended(&want("sha256:v1"), &intent, &ran);
    ensure(bound.is_empty(), || {
        format!("the bound version failed: {bound:?}")
    })?;
    ensure(
        !shell.intended(&want("sha256:v2"), &intent, &ran).is_empty(),
        || "`now`'s version passed".to_owned(),
    )
}

fn settings() -> ApprovalSettings {
    ApprovalSettings {
        approvers: BTreeSet::from(["u1".to_owned()]),
        author: "u0".to_owned(),
        timeout_s: 300,
        environment: Environment::Paper,
    }
}

/// Journal spec §9.7 (DEC-533 item 3): an `ApprovalResponded` member a case leaves unstated passes
/// only as `null`. A quorum, or a `false` or a text in `separation_of_duties` or `delegation`, fails
/// it, and so does a `null` in any other unstated member, or in another record.
#[test]
fn an_unstated_answer_member_passes_only_as_null() -> Result<(), String> {
    let quorum = object(vec![
        ("required", seconds(1)?),
        ("independent", Value::Bool(false)),
    ])?;
    for name in ["quorum", "separation_of_duties", "delegation"] {
        ensure(NULL_UNLESS_STATED.contains(&name), || {
            format!("`{name}` is not read as `null`")
        })?;
        ensure(
            unstated_fault("ApprovalResponded", name, &Value::Null).is_none(),
            || format!("a `null` `{name}` failed"),
        )?;
        for got in [quorum.clone(), Value::Bool(false), text("none")] {
            ensure(
                unstated_fault("ApprovalResponded", name, &got).is_some(),
                || format!("an unstated `{name}` of {got:?} passed"),
            )?;
        }
        ensure(
            unstated_fault("ApprovalRevalidated", name, &Value::Null).is_some(),
            || format!("an unstated `{name}` passed on another record"),
        )?;
    }
    ensure(
        unstated_fault("ApprovalResponded", "reason", &Value::Null).is_some(),
        || "an unstated `reason` passed as `null`".to_owned(),
    )
}

/// The same reading through the whole comparison of one draft: a runtime `ApprovalResponded` that
/// writes an unstated member as a value is reported naming it, and as `null` is not.
#[test]
fn a_responded_draft_with_an_unstated_value_fails_naming_it() -> Result<(), String> {
    let mut shell = Shell::started(settings(), 0)?;
    let ran = |payload: Value| Ran {
        drafts: vec![draft("E1", "ApprovalResponded", payload)],
        effects: Vec::new(),
        clock: 0,
        asked: None,
        now: None,
        decision: None,
    };
    let want = json!({"type": "ApprovalResponded", "clock": "1970-01-01T00:00:00Z"});
    let quorum = object(vec![
        ("required", seconds(1)?),
        ("independent", Value::Bool(false)),
    ])?;
    for name in ["quorum", "separation_of_duties", "delegation"] {
        for (got, named) in [(Value::Null, false), (quorum.clone(), true)] {
            let ran = ran(object(vec![
                ("role", text("approver")),
                (name, got.clone()),
            ])?);
            let Some(written) = ran.drafts.first() else {
                return Err("no draft".to_owned());
            };
            let faults = shell.compare_draft(&want, written, &ran);
            let naming = faults
                .iter()
                .any(|fault| fault.contains(&format!("`{name}`")));
            ensure(naming == named, || {
                format!("`{name}` of {got:?}: {faults:?}")
            })?;
        }
    }
    Ok(())
}

/// Rule 48: a step whose grant check 7 judged must state its `quorum`, so the case never reads one
/// as `null`; a step check 7 did not judge need not.
#[test]
fn a_judged_grant_states_its_quorum() -> Result<(), String> {
    let step = |verdict: &str, result: &str, reason: Option<&str>| json!({"verdict": verdict, "result": result, "reason": reason});
    for judged in [
        step("approved", "admitted", None),
        step("approved", "counted", None),
        step("approved", "refused", Some("duplicate_approver")),
        step("approved", "refused", Some("not_independent")),
    ] {
        ensure(unstated_quorum(&judged).is_some(), || {
            format!("{judged} passed with no `quorum`")
        })?;
        let mut members = judged.as_object().cloned().unwrap_or_default();
        members.insert(
            "quorum".to_owned(),
            json!({"required": 1, "independent": false}),
        );
        let stated = serde_json::Value::Object(members);
        ensure(unstated_quorum(&stated).is_none(), || {
            format!("{stated} failed")
        })?;
    }
    for unjudged in [
        step("skipped", "admitted", None),
        step("skipped", "refused", Some("duplicate_approver")),
        step("approved", "refused", Some("not_pending")),
        step("approved", "refused", Some("late")),
    ] {
        ensure(unstated_quorum(&unjudged).is_none(), || {
            format!("{unjudged} failed with no `quorum`")
        })?;
    }
    Ok(())
}

/// DEC-533 item 4: `decided_by_now` is an `ask`'s own label, and `null` for an `auto`, a `deny` or
/// an `ask` with no label; that value is the one expected when the runtime writes none or another.
/// DEC-830 item 2: the one superseded value the runtime wrote before E8-3, the label whatever the
/// decision or `""` with none, also passes. Any third value fails.
#[test]
fn decided_by_now_is_the_ask_label_or_null() -> Result<(), String> {
    let label = |by: &str| text(by);
    let cases = [
        (
            json!({"decision": "auto", "by": null}),
            Value::Null,
            label(""),
            vec![label("rule:big_order")],
        ),
        (
            json!({"decision": "auto", "by": "rule:other"}),
            Value::Null,
            label("rule:other"),
            vec![label(""), label("rule:big_order")],
        ),
        (
            json!({"decision": "deny", "by": "rule:no_more"}),
            Value::Null,
            label("rule:no_more"),
            vec![label(""), label("rule:other")],
        ),
        (
            json!({"decision": "ask", "by": "rule:big_order"}),
            label("rule:big_order"),
            label("rule:big_order"),
            vec![Value::Null, label(""), label("rule:other")],
        ),
        (
            json!({"decision": "ask", "by": null}),
            Value::Null,
            label(""),
            vec![label("rule:other")],
        ),
    ];
    for (classification, spec, superseded, others) in cases {
        for got in [&spec, &superseded] {
            let want = decided_by_now(&classification, Some(got))?;
            ensure(&want == got, || {
                format!("{classification}: {got:?} failed, {want:?} expected")
            })?;
        }
        for got in others.iter().chain([&Value::Bool(false)]) {
            let want = decided_by_now(&classification, Some(got))?;
            ensure(want == spec && &want != got, || {
                format!("{classification}: {got:?} expects {want:?}, not {spec:?}")
            })?;
        }
        let none = decided_by_now(&classification, None)?;
        ensure(none == spec, || {
            format!("{classification}: none written expects {none:?}, not {spec:?}")
        })?;
    }
    Ok(())
}
