//! The map's own pieces, on inputs no reference case reaches: a name bound twice or missing, a
//! reference mark or a quorum with a member too many, a cause record of the right type and the
//! wrong reason, a handoff no record authorises, a member supplied with no value, and an intent
//! whose step reads another mandate version than its request bound.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::Value;
use mandate_journal::Environment;
use mandate_runtime::{
    ApprovalSettings, Effect, EventDraft, EventId, FlattenPlan, IntentBody, IntentHandoff, Purpose,
};
use serde_json::json;

use super::{
    As, Asked, Ran, Shell, bind, extra_fault, handoffs, member, object, seconds, strip, text,
    translate,
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
