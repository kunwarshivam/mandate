//! Writer drafts: the envelope without the journal-assigned fields (journal spec §3), validated
//! and normalized before anything is compared, sequenced, or hashed.

use std::collections::BTreeSet;

use mandate_canon::{Object, Value, parse, to_canonical};
use mandate_time::UtcNanos;

use crate::schema::{Ty, normalize, normalize_record, parse_digest_ref, payload_schema};
use crate::{Environment, Invalid, InvalidReason, StreamId, StreamType, agent, catalogue, control};

static ENVELOPE: &[(&str, Ty)] = &[
    ("envelope_version", Ty::Int),
    ("environment", Ty::OneOf(&["paper", "live", "backtest"])),
    ("event_id", Ty::Ulid),
    ("stream_id", Ty::Str),
    ("event_type", Ty::Str),
    ("schema_version", Ty::Int),
    ("event_time", Ty::Timestamp),
    (
        "clock_source",
        Ty::OneOf(&["broker", "exchange", "local", "scheduler"]),
    ),
    ("causation_id", Ty::Nullable(&Ty::Ulid)),
    ("correlation_id", Ty::Nullable(&Ty::Ulid)),
    (
        "actor",
        Ty::Record(&[
            (
                "kind",
                Ty::OneOf(&["system", "agent", "user", "broker", "platform_operator"]),
            ),
            ("id", Ty::Str),
            ("version", Ty::Str),
            ("build", Ty::Nullable(&Ty::DigestRef)),
        ]),
    ),
    ("config_refs", Ty::OpenObject),
    ("payload", Ty::OpenObject),
    ("artifact_refs", Ty::List(&Ty::DigestRef)),
    ("pii_refs", Ty::List(&Ty::Str)),
];

/// A validated, normalized draft and its canonical bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    fields: Object,
    canonical: Vec<u8>,
    stream_id: StreamId,
    environment: Environment,
}

impl Draft {
    /// Parses and validates one draft: the envelope (§3), canonical values (§4), the catalogue's
    /// stream types and required `config_refs` (§9), and the payload schema. Decimals are
    /// normalized, so equivalent drafts have identical canonical bytes.
    pub fn parse(bytes: &[u8]) -> Result<Self, Invalid> {
        let value = parse(bytes).map_err(|e| Invalid::new(InvalidReason::Json(e.kind), ""))?;
        let object = value
            .as_object()
            .ok_or_else(|| Invalid::new(InvalidReason::Schema, ""))?;
        let mut fields = normalize_record(ENVELOPE, object, "")?;
        let text = |name: &str| fields.get(name).and_then(Value::as_str).unwrap_or_default();
        let int = |name: &str| fields.get(name).and_then(Value::as_int).unwrap_or_default();

        if int("envelope_version") != 1 {
            return Err(Invalid::new(InvalidReason::Schema, "envelope_version"));
        }
        let event_type = text("event_type");
        let entry = catalogue::lookup(event_type)
            .ok_or_else(|| Invalid::new(InvalidReason::UnknownEventType, "event_type"))?;
        let stream_id = StreamId::parse(text("stream_id"))
            .ok_or_else(|| Invalid::new(InvalidReason::NonCanonical, "stream_id"))?;
        if !entry.streams.contains(&stream_id.stream_type()) {
            return Err(Invalid::new(InvalidReason::WrongStream, "event_type"));
        }
        let environment = Environment::parse(text("environment"))
            .ok_or_else(|| Invalid::new(InvalidReason::Schema, "environment"))?;

        let actor = fields.get("actor");
        let internal = matches!(
            actor.and_then(|a| a.get("kind")).and_then(Value::as_str),
            Some("system" | "agent")
        );
        let build = actor.and_then(|a| a.get("build"));
        if internal && build == Some(&Value::Null) {
            return Err(Invalid::new(InvalidReason::Schema, "actor.build"));
        }

        let schema_version = int("schema_version");
        let required_refs =
            catalogue::required_refs(event_type, schema_version, entry.required_refs);
        check_config_refs(
            fields.get("config_refs"),
            required_refs,
            event_type,
            schema_version,
        )?;

        let written = fields.get("payload").unwrap_or(&Value::Null);
        let causation_id = fields.get("causation_id");
        let closed = agent::governs(&stream_id, event_type);
        let controlled = control::governs(&stream_id, event_type);
        let payload = if closed {
            agent::payload(event_type, int("schema_version"), written, causation_id)?
        } else if controlled {
            control::payload(
                event_type,
                int("schema_version"),
                &stream_id,
                written,
                control::Envelope {
                    config_refs: fields.get("config_refs"),
                    actor: fields.get("actor"),
                    causation_id,
                    pii_refs: fields.get("pii_refs"),
                },
            )?
        } else {
            let schema = payload_schema(event_type, int("schema_version"))
                .ok_or_else(|| Invalid::new(InvalidReason::UnknownSchema, "payload"))?;
            normalize(schema, written, "payload")?
        };

        let mut referenced = BTreeSet::new();
        collect_digest_refs(&payload, &mut referenced);
        let listed: Vec<&str> = string_list(fields.get("artifact_refs"));
        if !listed
            .iter()
            .copied()
            .eq(referenced.iter().map(String::as_str))
        {
            return Err(Invalid::new(InvalidReason::ArtifactRefs, "artifact_refs"));
        }
        let pii = string_list(fields.get("pii_refs"));
        if !pii.windows(2).all(|w| matches!(w, [a, b] if a < b)) {
            return Err(Invalid::new(InvalidReason::PiiRefs, "pii_refs"));
        }

        if closed {
            agent::subject_and_copy(event_type, &stream_id, &payload, causation_id)?;
        } else if controlled {
            crate::connections::copy(event_type, int("schema_version"), &payload, causation_id)?;
            control::subject_and_copy(event_type, &stream_id, &payload, causation_id)?;
        } else if event_type == "StreamOpened"
            && subject(&stream_id, &payload).as_deref() != Some(stream_id.as_str())
        {
            return Err(Invalid::new(InvalidReason::StreamMismatch, "stream_id"));
        }

        if let Some(slot) = fields.get_mut("payload") {
            *slot = payload;
        }
        let canonical = to_canonical(&Value::Object(fields.clone()));
        Ok(Self {
            fields,
            canonical,
            stream_id,
            environment,
        })
    }

    pub fn event_id(&self) -> &str {
        self.text("event_id")
    }

    pub fn event_type(&self) -> &str {
        self.text("event_type")
    }

    pub fn schema_version(&self) -> u64 {
        self.fields
            .get("schema_version")
            .and_then(Value::as_int)
            .unwrap_or_default()
    }

    /// The first configuration object a plain append cannot validate without an artifact source.
    ///
    /// A version-2 `ConfigSnapshotRegistered` registers one of the two new kinds or one of version
    /// 1's, which DEC-484 item 3 keeps (version 2 adds to that vocabulary rather than replacing
    /// it). Only the two new kinds carry an object an append must read, so only they are named
    /// here, which is the same condition [`validate_config_artifacts`] applies when an artifact
    /// source is given. Refusing an older kind too would refuse a draft no artifact-aware append
    /// would have checked.
    ///
    /// [`validate_config_artifacts`]: crate::validate_config_artifacts
    pub fn config_artifact_path(&self) -> Option<&'static str> {
        match (self.event_type(), self.schema_version()) {
            ("ModelOutputRecorded", 2) => Some("config_refs.model_registry"),
            ("DecisionMade", 2) => Some("config_refs.policy_set"),
            ("ConfigSnapshotRegistered", 2)
                if matches!(
                    self.registered_kind(),
                    Some(catalogue::POL | catalogue::REG)
                ) =>
            {
                Some("payload.content_hash")
            }
            _ => None,
        }
    }

    /// The `kind` a `ConfigSnapshotRegistered` draft registers.
    fn registered_kind(&self) -> Option<&str> {
        self.fields.get("payload")?.get("kind")?.as_str()
    }

    pub fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    pub fn environment(&self) -> Environment {
        self.environment
    }

    /// The payload's `risk_clock`, which every risk input carries (mandate spec §5.2).
    pub fn risk_clock(&self) -> Option<UtcNanos> {
        self.fields
            .get("payload")
            .and_then(|p| p.get("risk_clock"))
            .and_then(Value::as_str)
            .and_then(|t| UtcNanos::parse(t).ok())
    }

    /// The canonical bytes of the normalized draft; idempotency compares these (spec §5.1).
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }

    pub(crate) fn fields(&self) -> &Object {
        &self.fields
    }

    fn text(&self, name: &str) -> &str {
        self.fields
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or_default()
    }
}

/// `config_refs` holds only known kinds, each a `sha256:` reference, and every kind the event type
/// requires (spec §9). It never contains `null` (spec §4.2).
fn check_config_refs(
    refs: Option<&Value>,
    required: &[&str],
    event_type: &str,
    schema_version: u64,
) -> Result<(), Invalid> {
    let refs = refs
        .and_then(Value::as_object)
        .ok_or_else(|| Invalid::new(InvalidReason::Schema, "config_refs"))?;
    for (kind, value) in refs {
        let path = format!("config_refs.{kind}");
        if !catalogue::CONFIG_REF_KINDS.contains(&kind.as_str()) {
            return Err(Invalid::new(InvalidReason::Schema, path));
        }
        if value.as_str().and_then(parse_digest_ref).is_none() {
            return Err(Invalid::new(InvalidReason::NonCanonical, path));
        }
    }
    if schema_version == 1
        && matches!(event_type, "ModelOutputRecorded" | "DecisionMade")
        && [catalogue::POL, catalogue::REG]
            .iter()
            .any(|kind| refs.contains_key(*kind))
    {
        return Err(Invalid::new(InvalidReason::UnknownSchema, "payload"));
    }
    match required.iter().find(|kind| !refs.contains_key(**kind)) {
        Some(missing) => Err(Invalid::new(
            InvalidReason::MissingConfigRef,
            format!("config_refs.{missing}"),
        )),
        None => Ok(()),
    }
}

fn collect_digest_refs(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Str(s) if parse_digest_ref(s).is_some() => {
            out.insert(s.clone());
        }
        Value::Array(items) => items.iter().for_each(|v| collect_digest_refs(v, out)),
        Value::Object(members) => members.values().for_each(|v| collect_digest_refs(v, out)),
        _ => {}
    }
}

fn string_list(value: Option<&Value>) -> Vec<&str> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// The stream a `StreamOpened` payload describes.
fn subject(stream_id: &StreamId, payload: &Value) -> Option<String> {
    let field = |name: &str| payload.get(name).and_then(Value::as_str);
    match stream_id.stream_type() {
        StreamType::Account => Some(format!(
            "acct:{}:{}",
            field("workspace_id")?,
            field("account_ref")?
        )),
        StreamType::Agent | StreamType::Control | StreamType::Scheduler | StreamType::Notice => {
            None
        }
    }
}
