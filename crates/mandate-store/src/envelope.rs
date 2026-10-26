//! The journal envelope a long-running writer wraps each draft in (journal spec §3), without the
//! members the journal assigns at append. The scheduler and the runtime process compose theirs here
//! (E21-29, E21-30; DEC-1012). Every draft names its writer's build, and `artifact_refs` lists every
//! `sha256:` reference in the payload, so the append can re-hash what it names.

use std::collections::BTreeSet;

use mandate_canon::{Digest, Int, Key, Object, Value, to_canonical};
use mandate_journal::Environment;
use mandate_time::UtcNanos;

/// Who writes, where, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope<'a> {
    pub stream: &'a str,
    pub environment: Environment,
    /// `system` or `agent` (journal spec §3).
    pub actor_kind: &'a str,
    pub actor_id: &'a str,
    /// The writer's crate and version, `name/version`, which the actor's `build` digests.
    pub build: &'a str,
    pub version: &'a str,
    pub event_time: UtcNanos,
}

/// One draft's own members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fields<'a> {
    pub event_id: &'a str,
    pub event_type: &'a str,
    pub schema_version: u64,
    pub causation_id: Option<&'a str>,
    /// The run the draft belongs to (journal spec §9.23 rule 171, DEC-2208 item 5); `None` for a
    /// record of no run.
    pub correlation_id: Option<&'a str>,
    pub config_refs: &'a Object,
    pub payload: &'a Value,
}

/// The canonical bytes of one draft, with `clock_source` `scheduler`: every long-running writer
/// times its records by the scheduler's ticks (journal spec §5.4, mandate spec §5.2). `None` only for
/// a schema version the canonical form cannot hold.
pub fn draft_bytes(envelope: &Envelope<'_>, fields: &Fields<'_>) -> Option<Vec<u8>> {
    draft_bytes_with_pii(envelope, fields, &[])
}

/// [`draft_bytes`] for a record that names personal data by reference: `pii_refs` holds the opaque
/// vault references the payload carries, sorted and de-duplicated as journal spec §3 requires (the
/// executor's `ConnectionChecked` names its account's, rule 62).
pub fn draft_bytes_with_pii(
    envelope: &Envelope<'_>,
    fields: &Fields<'_>,
    pii_refs: &[&str],
) -> Option<Vec<u8>> {
    body_bytes(envelope, fields, pii_refs, ClockSource::Scheduler)
}

/// The clock a writer times its records by, as journal spec §3's `clock_source` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockSource {
    /// The scheduler's ticks, which every long-running writer but the news reader times by.
    Scheduler,
    /// The host clock: the news reader times each record by its receipt of the item (DEC-2278).
    Local,
}

impl ClockSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Scheduler => "scheduler",
            Self::Local => "local",
        }
    }
}

/// [`draft_bytes`] for a writer that times its records by `clock_source`.
pub fn draft_bytes_with_clock(
    envelope: &Envelope<'_>,
    fields: &Fields<'_>,
    clock_source: ClockSource,
) -> Option<Vec<u8>> {
    body_bytes(envelope, fields, &[], clock_source)
}

fn body_bytes(
    envelope: &Envelope<'_>,
    fields: &Fields<'_>,
    pii_refs: &[&str],
    clock_source: ClockSource,
) -> Option<Vec<u8>> {
    let pii: BTreeSet<&str> = pii_refs.iter().copied().collect();
    let build = format!("sha256:{}", Digest::of(envelope.build.as_bytes()).to_hex());
    let actor = object(vec![
        ("build", Value::Str(build)),
        ("id", text(envelope.actor_id)),
        ("kind", text(envelope.actor_kind)),
        ("version", text(envelope.version)),
    ])?;
    let mut references = BTreeSet::new();
    collect_digest_refs(fields.payload, &mut references);
    let body = object(vec![
        ("actor", actor),
        (
            "artifact_refs",
            Value::Array(references.into_iter().map(Value::Str).collect()),
        ),
        (
            "causation_id",
            fields.causation_id.map_or(Value::Null, text),
        ),
        ("clock_source", text(clock_source.as_str())),
        ("config_refs", Value::Object(fields.config_refs.clone())),
        (
            "correlation_id",
            fields.correlation_id.map_or(Value::Null, text),
        ),
        ("environment", text(envelope.environment.as_str())),
        ("envelope_version", int(1)?),
        ("event_id", text(fields.event_id)),
        ("event_time", Value::Str(envelope.event_time.to_string())),
        ("event_type", text(fields.event_type)),
        ("payload", fields.payload.clone()),
        (
            "pii_refs",
            Value::Array(pii.into_iter().map(text).collect()),
        ),
        ("schema_version", int(fields.schema_version)?),
        ("stream_id", text(envelope.stream)),
    ])?;
    Some(to_canonical(&body))
}

/// Every `sha256:` reference in `value`, which journal spec §3 lists in `artifact_refs`.
fn collect_digest_refs(value: &Value, references: &mut BTreeSet<String>) {
    match value {
        Value::Str(s) => {
            if s.strip_prefix("sha256:")
                .and_then(Digest::from_hex)
                .is_some()
            {
                references.insert(s.clone());
            }
        }
        Value::Array(values) => values
            .iter()
            .for_each(|v| collect_digest_refs(v, references)),
        Value::Object(values) => values
            .values()
            .for_each(|v| collect_digest_refs(v, references)),
        Value::Null | Value::Bool(_) | Value::Int(_) => {}
    }
}

/// A string value.
pub fn text(s: &str) -> Value {
    Value::Str(s.to_owned())
}

/// An integer value; `None` past the canonical form's range (journal spec §4.4).
pub fn int(n: u64) -> Option<Value> {
    Int::new(n).map(Value::Int)
}

/// An object of `members`; `None` for a member name the key grammar refuses (§4.1).
pub fn object(members: Vec<(&'static str, Value)>) -> Option<Value> {
    let mut map = Object::new();
    for (name, value) in members {
        map.insert(Key::new(name).ok()?, value);
    }
    Some(Value::Object(map))
}
