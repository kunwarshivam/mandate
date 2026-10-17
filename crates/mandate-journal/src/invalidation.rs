//! The record journal spec v0.48 §9.21 closes for an invalidated thesis (E19-5, DEC-2202): the agent
//! stream's `ThesisInvalidated`, with rules 165 and 166 on the draft, rule 167 over its `append`
//! batch ([`check_batch`]), and rule 168 over the stream's earlier records ([`stream_rule`]). Every
//! rule only refuses a draft.

use std::collections::BTreeSet;

use mandate_canon::Value;

use crate::schema::Ty;
use crate::{Draft, Invalid, InvalidReason, StreamId, StreamType};

pub(crate) const INVALIDATED: &str = "ThesisInvalidated";
const INVOCATION: &str = "ModelInvocationRecorded";
const PROPOSED: &str = "ThesisProposed";
const REVISED: &str = "ThesisRevised";

/// Whether §9.21 governs `event_type` on `stream`.
pub(crate) fn governs(stream: &StreamId, event_type: &str) -> bool {
    stream.stream_type() == StreamType::Agent && event_type == INVALIDATED
}

/// The §9.21 schema of `event_type` at `schema_version` on `stream`, or `None`.
pub(crate) fn schema(
    event_type: &str,
    schema_version: u64,
    stream: StreamType,
) -> Option<&'static Ty> {
    match (event_type, schema_version, stream) {
        (INVALIDATED, 1, StreamType::Agent) => Some(&THESIS_INVALIDATED),
        _ => None,
    }
}

/// Rules 165 and 166 on a payload its schema has normalized, in rule order: the first that fails is
/// reported. `actor` and `causation_id` are the envelope's.
pub(crate) fn rules(
    event_type: &str,
    stream: StreamType,
    payload: &Value,
    actor: Option<&Value>,
    causation_id: Option<&Value>,
) -> Result<(), Invalid> {
    if (event_type, stream) != (INVALIDATED, StreamType::Agent) {
        return Ok(());
    }
    let p = View(payload);
    let model = p.text("cause") == "model";
    ensure(p.is_null("candidate") != model, "payload.candidate")?;
    if !model {
        ensure(p.list("evidence").is_empty(), "payload.evidence")?;
    }
    ensure(p.is_null("source") == model, "payload.source")?;
    ensure(
        causation_id.is_some_and(|cause| *cause != Value::Null),
        "causation_id",
    )?;
    let writer = actor
        .and_then(|a| a.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    ensure(writer == "agent", "actor.kind")
}

/// Rule 167 over one `append` batch whose drafts each passed [`Draft::parse`], draft by draft in
/// batch order, the first that fails reported by its index:
///
/// - a model's `ThesisInvalidated` names as its cause a `ModelInvocationRecorded` earlier in the
///   batch whose verdict at `candidate` is a judged invalidation, and no earlier
///   `ThesisInvalidated` of the batch names the same verdict;
/// - an agent-stream `ModelInvocationRecorded` has each of its judged invalidation verdicts named
///   by a later `ThesisInvalidated` of the batch.
pub(crate) fn check_batch(drafts: &[Draft]) -> Result<(), (usize, Invalid)> {
    for (i, draft) in drafts.iter().enumerate() {
        if draft.stream_id().stream_type() != StreamType::Agent || draft.schema_version() != 1 {
            continue;
        }
        let payload = payload_of(draft);
        let p = View(&payload);
        match draft.event_type() {
            INVALIDATED if p.text("cause") == "model" => {
                let cause = draft
                    .fields()
                    .get("causation_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let candidate = p.int("candidate");
                let earlier = drafts.get(..i).unwrap_or_default();
                let Some(call) = earlier
                    .iter()
                    .find(|d| d.event_type() == INVOCATION && d.event_id() == cause)
                else {
                    return Err((i, Invalid::new(InvalidReason::Schema, "causation_id")));
                };
                let call = payload_of(call);
                let judged = View(&call)
                    .list("verdicts")
                    .get(usize::try_from(candidate).unwrap_or(usize::MAX))
                    .is_some_and(|v| {
                        View(v).text("kind") == "invalidation" && View(v).text("result") == "judged"
                    });
                if !judged {
                    return Err((i, Invalid::new(InvalidReason::Schema, "payload.candidate")));
                }
                let twice = earlier.iter().any(|d| names(d, cause, candidate));
                if twice {
                    return Err((i, Invalid::new(InvalidReason::Schema, "payload.candidate")));
                }
            }
            INVOCATION => {
                let later = drafts.get(i.saturating_add(1)..).unwrap_or_default();
                for (index, verdict) in p.list("verdicts").iter().enumerate() {
                    let v = View(verdict);
                    if v.text("kind") != "invalidation" || v.text("result") != "judged" {
                        continue;
                    }
                    let candidate = u64::try_from(index).unwrap_or(u64::MAX);
                    if !later.iter().any(|d| names(d, draft.event_id(), candidate)) {
                        return Err((
                            i,
                            Invalid::new(
                                InvalidReason::Schema,
                                &format!("payload.verdicts[{index}].result"),
                            ),
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Whether `draft` is a model's `ThesisInvalidated` naming the verdict `candidate` of the call
/// record `cause`.
fn names(draft: &Draft, cause: &str, candidate: u64) -> bool {
    if draft.event_type() != INVALIDATED {
        return false;
    }
    let payload = payload_of(draft);
    let p = View(&payload);
    p.text("cause") == "model"
        && p.int("candidate") == candidate
        && draft.fields().get("causation_id").and_then(Value::as_str) == Some(cause)
}

/// Rule 168 on one `ThesisInvalidated`, against the stream's records `before` it, in order: no
/// earlier invalidation names its thesis; an earlier admitted `ThesisProposed` or `ThesisRevised`
/// names it with the same lineage and instrument; and that record is the latest admitted one for
/// its instrument, so a superseded thesis is never invalidated in its place. The first that fails.
pub(crate) fn stream_rule(before: &[&Draft], draft: &Draft) -> Result<(), Invalid> {
    if draft.event_type() != INVALIDATED
        || draft.stream_id().stream_type() != StreamType::Agent
        || draft.schema_version() != 1
    {
        return Ok(());
    }
    let mine = payload_of(draft);
    let m = View(&mine);
    let thesis = m.text("thesis_id");
    let instrument = m.text("instrument_id");
    let mut invalidated = BTreeSet::new();
    let mut admitted = None;
    let mut current = None;
    for d in before {
        let payload = payload_of(d);
        let p = View(&payload);
        match d.event_type() {
            INVALIDATED => {
                invalidated.insert(p.text("thesis_id").to_owned());
            }
            PROPOSED | REVISED if p.flag("admitted") => {
                if p.text("thesis_id") == thesis {
                    admitted = Some((
                        p.text("lineage_id").to_owned(),
                        p.text("instrument_id").to_owned(),
                    ));
                }
                if p.text("instrument_id") == instrument {
                    current = Some(p.text("thesis_id").to_owned());
                }
            }
            _ => {}
        }
    }
    ensure(!invalidated.contains(thesis), "payload.thesis_id")?;
    let Some((lineage, admitted_instrument)) = admitted else {
        return Err(Invalid::new(InvalidReason::Schema, "payload.thesis_id"));
    };
    ensure(lineage == m.text("lineage_id"), "payload.lineage_id")?;
    ensure(admitted_instrument == instrument, "payload.instrument_id")?;
    ensure(current.as_deref() == Some(thesis), "payload.thesis_id")
}

fn payload_of(draft: &Draft) -> Value {
    draft
        .fields()
        .get("payload")
        .cloned()
        .unwrap_or(Value::Null)
}

fn ensure(holds: bool, path: &str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::Schema, path))
    }
}

/// A normalized payload, read member by member; a `null` or absent member reads as empty text.
#[derive(Clone, Copy)]
struct View<'a>(&'a Value);

impl<'a> View<'a> {
    fn text(self, member: &str) -> &'a str {
        self.0
            .get(member)
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    fn is_null(self, member: &str) -> bool {
        matches!(self.0.get(member), None | Some(Value::Null))
    }

    fn flag(self, member: &str) -> bool {
        self.0.get(member) == Some(&Value::Bool(true))
    }

    fn int(self, member: &str) -> u64 {
        self.0
            .get(member)
            .and_then(Value::as_int)
            .unwrap_or(u64::MAX)
    }

    fn list(self, member: &str) -> &'a [Value] {
        self.0
            .get(member)
            .and_then(Value::as_array)
            .unwrap_or_default()
    }
}

static THESIS_INVALIDATED: Ty = Ty::Record(&[
    ("thesis_id", Ty::Ident),
    ("lineage_id", Ty::Ident),
    ("instrument_id", Ty::AssetId),
    ("cause", Ty::OneOf(&["model", "source_revoked"])),
    ("candidate", Ty::Nullable(&Ty::Int)),
    ("evidence", Ty::List(&Ty::Str)),
    ("source", Ty::Nullable(&Ty::Str)),
]);
