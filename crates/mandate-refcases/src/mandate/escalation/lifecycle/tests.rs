//! The map's own pieces, on inputs no reference case reaches: a cause record of the right type
//! and the wrong reason, a handoff no record authorises, a name bound twice, a hash that is not
//! its content's, a quorum the runtime records, and a runtime object with a member too many.

use std::collections::BTreeMap;

use mandate_canon::Value;
use mandate_runtime::{
    Effect, EventDraft, EventId, FlattenPlan, IntentBody, IntentHandoff, Purpose,
};
use serde_json::json;

use super::{
    As, Recorded, bind, extra_fault, fields_are, handoffs, member, object, recorded, stated_hash,
    strip, text, translate,
};
use crate::ensure;

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
    strip(&mut none, &[("DecisionMade", "")])
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

/// A name binds to one runtime name and a runtime name to one name; an unbound name passes as
/// written unless it is a runtime name bound to another.
#[test]
fn names_bind_one_to_one() -> Result<(), String> {
    let mut bound = BTreeMap::new();
    bind(&mut bound, "ap1", "R1")?;
    bind(&mut bound, "ap1", "R1")?;
    bind(&mut bound, "ap2", "R2")?;
    ensure(bind(&mut bound, "ap1", "R3").is_err(), || {
        "a name rebound".to_owned()
    })?;
    ensure(bind(&mut bound, "ap3", "R1").is_err(), || {
        "a runtime name rebound".to_owned()
    })?;
    ensure(translate(&bound, "ap2")? == "R2", || {
        "the bound name".to_owned()
    })?;
    ensure(translate(&bound, "ap9")? == "ap9", || {
        "an unbound name".to_owned()
    })?;
    ensure(translate(&bound, "R1").is_err(), || {
        "a runtime name as written".to_owned()
    })
}

/// The runtime's hash is its content's digest, and nothing else.
#[test]
fn a_hash_must_be_its_contents_digest() -> Result<(), String> {
    let content = object(vec![("k", text("v"))])?;
    let digest = format!(
        "sha256:{}",
        mandate_canon::Digest::of(&mandate_canon::to_canonical(&content))
    );
    let right = draft(
        "r",
        "ApprovalRequested",
        object(vec![
            ("content", content.clone()),
            ("content_hash", text(&digest)),
        ])?,
    );
    ensure(stated_hash(&right)? == digest, || "the digest".to_owned())?;
    for payload in [
        object(vec![
            ("content", object(vec![("k", text("w"))])?),
            ("content_hash", text(&digest)),
        ])?,
        object(vec![("content_hash", text(&digest))])?,
        object(vec![("content", content)])?,
    ] {
        ensure(
            stated_hash(&draft("r", "ApprovalRequested", payload)).is_err(),
            || "a hash that is not the content's".to_owned(),
        )?;
    }
    Ok(())
}

/// A quorum the runtime records is compared member by member, and a runtime object with a member
/// the map does not name fails.
#[test]
fn a_recorded_quorum_is_compared() -> Result<(), String> {
    let want = json!({"required": 1, "independent": false});
    let got = |required: u64, independent: bool| -> Result<Value, String> {
        object(vec![
            ("required", super::int(required)?),
            ("independent", Value::Bool(independent)),
        ])
    };
    member(As::Quorum, &want, Some(&got(1, false)?))?;
    for wrong in [got(2, false)?, got(1, true)?] {
        ensure(member(As::Quorum, &want, Some(&wrong)).is_err(), || {
            format!("{wrong:?} passed")
        })?;
    }
    let extra = object(vec![
        ("required", super::int(1)?),
        ("independent", Value::Bool(false)),
        ("by", text("policy")),
    ])?;
    ensure(member(As::Quorum, &want, Some(&extra)).is_err(), || {
        "an extra member".to_owned()
    })?;
    ensure(
        fields_are(&extra, &["required", "independent"]).is_err(),
        || "an extra member".to_owned(),
    )?;
    fields_are(&got(1, false)?, &["required", "independent"])
}

/// A `decided_by` the classifier does not name is recorded as `""`, and a value of the wrong shape
/// fails.
#[test]
fn a_recorded_value_reads_its_shape() -> Result<(), String> {
    ensure(recorded(Recorded::Label, &json!(null))? == text(""), || {
        "a label".to_owned()
    })?;
    ensure(
        recorded(Recorded::Text, &json!(null))? == Value::Null,
        || "a text".to_owned(),
    )?;
    ensure(recorded(Recorded::Flag, &json!("true")).is_err(), || {
        "a flag".to_owned()
    })?;
    ensure(recorded(Recorded::Decimal, &json!(true)).is_err(), || {
        "a decimal".to_owned()
    })
}

/// A runtime-only member holds what the map says it must, and a different value, an unreadable
/// expectation, or a missing member each fault; one another check reads is left to it.
#[test]
fn a_runtime_only_member_is_checked() -> Result<(), String> {
    let responded = draft(
        "r",
        "ApprovalResponded",
        object(vec![("role", text("approver"))])?,
    );
    ensure(
        extra_fault(&responded, "role", Some(Ok(text("approver")))).is_none(),
        || "the role holds".to_owned(),
    )?;
    for value in [Some(Ok(text("observer"))), Some(Err("unread".to_owned()))] {
        ensure(extra_fault(&responded, "role", value).is_some(), || {
            "a fault".to_owned()
        })?;
    }
    ensure(
        extra_fault(&responded, "on_timeout", Some(Ok(text("skip")))).is_some(),
        || "a missing member".to_owned(),
    )?;
    ensure(extra_fault(&responded, "content", None).is_none(), || {
        "left to another check".to_owned()
    })
}
