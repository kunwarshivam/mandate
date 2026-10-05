//! The journal envelope the shell wraps every draft in, and the deterministic ids it injects.
//!
//! The cores never compose an envelope, so they cannot forge `seq`, `prev_hash`, or `recorded_at`
//! (mandate-runtime's `EventDraft`). The shell composes it here, and the environment it writes is a
//! constant: there is no input from which a draft could be built with any environment but `paper`
//! (TI-7, `AGENTS.md` rule 8).

use std::collections::BTreeSet;

use mandate_canon::{Int, Key, Object, Value, to_canonical};
use mandate_journal::Environment;
use mandate_time::UtcNanos;

use crate::error::ShellError;

/// The only environment the shell can write (TI-7).
pub const ENVIRONMENT: Environment = Environment::Paper;

/// The version the envelope's actor carries.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The agent stream of one deployment: `agent:{workspace}:{agent}` (journal spec §2). The same
/// string `mandate_runtime` folds, so a draft the shell appends is one the runtime follows.
pub fn agent_stream(workspace: &str, agent: &str) -> String {
    format!("agent:{workspace}:{agent}")
}

/// The account stream of one broker account: `acct:{workspace}:{account_ref}`, whose subject is an
/// opaque internal id and never the broker's account number (journal spec §6.4, TI-8).
pub fn account_stream(workspace: &str, account_ref: &str) -> String {
    format!("acct:{workspace}:{account_ref}")
}

/// Who writes a stream's drafts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Writer {
    /// The agent runtime, on its agent stream.
    Agent,
    /// The executor, the account stream's single writer.
    Executor,
}

/// Everything an envelope needs besides the draft itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope<'a> {
    pub stream: &'a str,
    pub writer: Writer,
    pub actor_id: &'a str,
    /// The injected clock's reading. The shell never reads a wall clock (ES-21).
    pub event_time: UtcNanos,
}

/// One draft as the journal takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftFields<'a> {
    pub event_id: &'a str,
    pub event_type: &'a str,
    pub schema_version: u64,
    pub causation_id: Option<&'a str>,
    pub config_refs: &'a Object,
    pub payload: &'a Value,
}

/// The canonical bytes of one draft: the envelope of journal spec §3 without the fields the journal
/// assigns at append.
pub fn draft_bytes(
    envelope: &Envelope<'_>,
    draft: &DraftFields<'_>,
) -> Result<Vec<u8>, ShellError> {
    let actor_kind = match envelope.writer {
        Writer::Agent => "agent",
        Writer::Executor => "system",
    };
    let actor = object(vec![
        ("build", text(&build_ref())),
        ("id", text(envelope.actor_id)),
        ("kind", text(actor_kind)),
        ("version", text(VERSION)),
    ])?;
    let causation = match draft.causation_id {
        Some(id) => text(id),
        None => Value::Null,
    };
    let mut references = BTreeSet::new();
    collect_digest_refs(draft.payload, &mut references);
    let fields = object(vec![
        ("actor", actor),
        (
            "artifact_refs",
            Value::Array(references.into_iter().map(Value::Str).collect()),
        ),
        ("causation_id", causation),
        ("clock_source", text("scheduler")),
        ("config_refs", Value::Object(draft.config_refs.clone())),
        ("correlation_id", Value::Null),
        ("environment", text(ENVIRONMENT.as_str())),
        ("envelope_version", int(1)?),
        ("event_id", text(draft.event_id)),
        ("event_time", text(&envelope.event_time.to_string())),
        ("event_type", text(draft.event_type)),
        ("payload", draft.payload.clone()),
        ("pii_refs", Value::Array(Vec::new())),
        ("schema_version", int(draft.schema_version)?),
        ("stream_id", text(envelope.stream)),
    ])?;
    Ok(to_canonical(&fields))
}

fn collect_digest_refs(value: &Value, references: &mut BTreeSet<String>) {
    match value {
        Value::Str(text) => {
            if text
                .strip_prefix("sha256:")
                .and_then(mandate_canon::Digest::from_hex)
                .is_some()
            {
                references.insert(text.clone());
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_digest_refs(value, references);
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                collect_digest_refs(value, references);
            }
        }
        Value::Null | Value::Bool(_) | Value::Int(_) => {}
    }
}

/// The build the actor names: a content reference over the crate's name and version. It is not a
/// reproducible-build digest, which is M9's (ES-01); it only makes every draft name the code that
/// wrote it.
fn build_ref() -> String {
    let digest =
        mandate_canon::Digest::of(concat!("mandate-shell/", env!("CARGO_PKG_VERSION")).as_bytes());
    format!("sha256:{}", digest.to_hex())
}

fn text(s: &str) -> Value {
    Value::Str(s.to_owned())
}

fn int(n: u64) -> Result<Value, ShellError> {
    Int::new(n)
        .map(Value::Int)
        .ok_or(ShellError::Envelope { field: "integer" })
}

fn object(members: Vec<(&'static str, Value)>) -> Result<Value, ShellError> {
    let mut map = Object::new();
    for (name, value) in members {
        let key = Key::new(name).map_err(|_| ShellError::Envelope { field: name })?;
        map.insert(key, value);
    }
    Ok(Value::Object(map))
}

/// Which stream an id is for, as the id's first digit. Two spaces so that the agent stream's ids
/// and the account stream's can never meet, though both derive from `(epoch, head, ordinal)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdSpace {
    Agent,
    Account,
}

/// Deterministic event identity (ADR-0001 ES-06, ES-21): the id is a pure function of
/// `(space, epoch, head, ordinal)`, so a retry re-derives the same id and the append answers
/// `AlreadyCommitted` (journal spec §5.1), and a new epoch derives new ids.
///
/// The id is the decimal digits of its parts, zero-padded to one, 5, 14 and 6 places: 26 characters
/// of the ULID alphabet whose first is `0` or `1`, which is what journal spec §3 checks. A part too
/// large for its places makes the id longer than 26, which the journal refuses as `Invalid` — a
/// refusal, never a silent truncation into another id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ids {
    pub space: IdSpace,
}

impl Ids {
    pub fn derive(self, epoch: u64, head: u64, ordinal: u32) -> String {
        let space = match self.space {
            IdSpace::Agent => '0',
            IdSpace::Account => '1',
        };
        format!("{space}{epoch:05}{head:014}{ordinal:06}")
    }
}

impl mandate_runtime::IdGen for Ids {
    fn event_id(
        &self,
        epoch: mandate_runtime::WriterEpoch,
        head: mandate_runtime::Seq,
        ordinal: u32,
    ) -> mandate_runtime::EventId {
        mandate_runtime::EventId(self.derive(epoch.0, head.0, ordinal))
    }
}

impl mandate_executor::IdGen for Ids {
    fn event_id(
        &self,
        epoch: mandate_executor::WriterEpoch,
        head: mandate_executor::Seq,
        ordinal: u32,
    ) -> mandate_executor::EventId {
        mandate_executor::EventId(self.derive(epoch.0, head.0, ordinal))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use mandate_canon::{Key, Value, parse};
    use mandate_journal::{
        AppendOutcome, Draft, InvalidReason, MemoryJournal, StreamId, TrustedStart, verify_events,
    };
    use mandate_time::UtcNanos;

    use super::{DraftFields, Envelope, IdSpace, Ids, Writer, draft_bytes, object, text};

    fn envelope_of(bytes: &[u8]) -> Result<Value, String> {
        parse(bytes).map_err(|e| format!("{e:?}"))
    }

    #[test]
    fn an_id_is_twenty_six_characters_of_the_ulid_alphabet_and_differs_by_space() {
        let agent = Ids {
            space: IdSpace::Agent,
        }
        .derive(3, 41, 2);
        let account = Ids {
            space: IdSpace::Account,
        }
        .derive(3, 41, 2);
        assert_eq!(agent, "00000300000000000041000002");
        assert_eq!(account, "10000300000000000041000002");
        assert_eq!(agent.len(), 26);
    }

    #[test]
    fn an_id_too_large_for_its_places_is_longer_than_a_ulid_rather_than_truncated() {
        let id = Ids {
            space: IdSpace::Agent,
        }
        .derive(100_000, 0, 0);
        assert_eq!(id.len(), 27, "{id}");
    }

    #[test]
    fn every_draft_says_paper_and_names_its_stream_writer_and_causation() -> Result<(), String> {
        let at = UtcNanos::parse("2026-09-25T20:00:00.000000000Z").map_err(|e| e.to_string())?;
        let payload = Value::Object(mandate_canon::Object::new());
        let config_refs = mandate_canon::Object::new();
        let bytes = draft_bytes(
            &Envelope {
                stream: "acct:ws1:acc1",
                writer: Writer::Executor,
                actor_id: "executor",
                event_time: at,
            },
            &DraftFields {
                event_id: "10000100000000000001000000",
                event_type: "OrderSubmitted",
                schema_version: 2,
                causation_id: Some("00000100000000000003000001"),
                config_refs: &config_refs,
                payload: &payload,
            },
        )
        .map_err(|e| e.to_string())?;
        let envelope = envelope_of(&bytes)?;
        let field = |name: &str| {
            envelope
                .get(name)
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        assert_eq!(field("environment").as_deref(), Some("paper"));
        assert_eq!(field("stream_id").as_deref(), Some("acct:ws1:acc1"));
        assert_eq!(field("event_type").as_deref(), Some("OrderSubmitted"));
        assert_eq!(
            envelope.get("schema_version").and_then(Value::as_int),
            Some(2),
            "the executor's OrderSubmitted schema version reaches the envelope"
        );
        assert_eq!(
            field("causation_id").as_deref(),
            Some("00000100000000000003000001")
        );
        assert_eq!(
            field("event_time").as_deref(),
            Some("2026-09-25T20:00:00.000000000Z")
        );
        let actor = |name: &str| {
            envelope
                .get("actor")
                .and_then(|actor| actor.get(name))
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        assert_eq!(actor("kind").as_deref(), Some("system"));
        assert_eq!(actor("id").as_deref(), Some("executor"));
        assert_eq!(actor("version").as_deref(), Some(env!("CARGO_PKG_VERSION")));
        let build = mandate_canon::Digest::of(
            format!("mandate-shell/{}", env!("CARGO_PKG_VERSION")).as_bytes(),
        );
        assert_eq!(actor("build"), Some(format!("sha256:{}", build.to_hex())));
        Ok(())
    }

    #[test]
    fn an_agent_draft_is_written_by_the_agent_with_no_causation() -> Result<(), String> {
        let at = UtcNanos::parse("2026-09-25T20:00:00.000000000Z").map_err(|e| e.to_string())?;
        let payload = Value::Null;
        let config_refs = mandate_canon::Object::new();
        let bytes = draft_bytes(
            &Envelope {
                stream: "agent:ws1:ag1",
                writer: Writer::Agent,
                actor_id: "ag1",
                event_time: at,
            },
            &DraftFields {
                event_id: "00000100000000000000000000",
                event_type: "AgentModeChanged",
                schema_version: 1,
                causation_id: None,
                config_refs: &config_refs,
                payload: &payload,
            },
        )
        .map_err(|e| e.to_string())?;
        let envelope = envelope_of(&bytes)?;
        assert_eq!(envelope.get("causation_id"), Some(&Value::Null));
        let kind = envelope
            .get("actor")
            .and_then(|actor| actor.get("kind"))
            .and_then(Value::as_str);
        assert_eq!(kind, Some("agent"));
        Ok(())
    }

    #[test]
    fn a_caller_selected_unregistered_version_is_refused() -> Result<(), String> {
        let at = UtcNanos::parse("2026-09-25T20:00:00.000000000Z").map_err(|e| e.to_string())?;
        let payload = object(vec![
            ("stream_type", text("account")),
            ("workspace_id", text("ws1")),
            ("broker", text("alpaca")),
            ("account_ref", text("acc1")),
        ])
        .map_err(|e| e.to_string())?;
        let config_refs = mandate_canon::Object::new();
        let bytes = draft_bytes(
            &Envelope {
                stream: "acct:ws1:acc1",
                writer: Writer::Executor,
                actor_id: "executor",
                event_time: at,
            },
            &DraftFields {
                event_id: "10000100000000000000000000",
                event_type: "StreamOpened",
                schema_version: 2,
                causation_id: None,
                config_refs: &config_refs,
                payload: &payload,
            },
        )
        .map_err(|e| e.to_string())?;
        let refusal = Draft::parse(&bytes).expect_err("StreamOpened has no version 2");
        assert_eq!(refusal.reason, InvalidReason::UnknownSchema);
        assert_eq!(refusal.path, "payload");
        Ok(())
    }

    #[test]
    fn intent_received_risk_clock_is_refused_at_version_one() -> Result<(), String> {
        let at = UtcNanos::parse("2026-09-25T20:00:00.000000000Z").map_err(|e| e.to_string())?;
        let payload = object(vec![
            ("intent_id", text("01JABCDEFGHJKMNPQRSTVWXYZ1")),
            ("agent_id", text("agent-a")),
            ("instrument_id", text("instrument-a")),
            ("side", text("buy")),
            ("type", text("limit")),
            ("tif", text("day")),
            ("qty", text("1")),
            ("limit_price", text("100")),
            ("purpose", text("open")),
            ("risk_clock", text("2026-09-25T20:00:00.000000000Z")),
        ])
        .map_err(|e| e.to_string())?;
        let mut config_refs = mandate_canon::Object::new();
        config_refs.insert(
            Key::new("mandate_version").map_err(|e| e.to_string())?,
            text(&format!("sha256:{}", "0".repeat(64))),
        );
        let bytes = draft_bytes(
            &Envelope {
                stream: "acct:ws1:acc1",
                writer: Writer::Executor,
                actor_id: "executor",
                event_time: at,
            },
            &DraftFields {
                event_id: "10000100000000000001000000",
                event_type: "IntentReceived",
                schema_version: 1,
                causation_id: None,
                config_refs: &config_refs,
                payload: &payload,
            },
        )
        .map_err(|e| e.to_string())?;
        let refusal = Draft::parse(&bytes).expect_err("version 1 excludes risk_clock");
        assert_eq!(refusal.reason, InvalidReason::Schema);
        assert_eq!(refusal.path, "payload.risk_clock");
        Ok(())
    }

    #[test]
    fn verification_checks_the_content_behind_an_executor_config_reference() -> Result<(), String> {
        let at = UtcNanos::parse("2026-09-25T20:00:00.000000000Z").map_err(|e| e.to_string())?;
        let stream_text = "acct:ws1:acc1";
        let stream = StreamId::parse(stream_text).ok_or("stream")?;
        let config_bytes = b"validated mandate fixture".to_vec();
        let digest = mandate_canon::Digest::of(&config_bytes);
        let reference = format!("sha256:{}", digest.to_hex());
        let mut config_refs = mandate_canon::Object::new();
        config_refs.insert(
            Key::new("mandate_version").map_err(|e| e.to_string())?,
            text(&reference),
        );
        let opened_payload = object(vec![
            ("stream_type", text("account")),
            ("workspace_id", text("ws1")),
            ("broker", text("alpaca")),
            ("account_ref", text("acc1")),
        ])
        .map_err(|e| e.to_string())?;
        let intent_payload = object(vec![
            ("intent_id", text("01JABCDEFGHJKMNPQRSTVWXYZ1")),
            ("agent_id", text("agent-a")),
            ("instrument_id", text("instrument-a")),
            ("side", text("buy")),
            ("type", text("limit")),
            ("tif", text("day")),
            ("qty", text("1")),
            ("limit_price", text("100")),
            ("purpose", text("open")),
            ("risk_clock", text("2026-09-25T20:00:00.000000000Z")),
        ])
        .map_err(|e| e.to_string())?;
        let envelope = Envelope {
            stream: stream_text,
            writer: Writer::Executor,
            actor_id: "executor",
            event_time: at,
        };
        let empty = mandate_canon::Object::new();
        let opened = draft_bytes(
            &envelope,
            &DraftFields {
                event_id: "10000100000000000000000000",
                event_type: "StreamOpened",
                schema_version: 1,
                causation_id: None,
                config_refs: &empty,
                payload: &opened_payload,
            },
        )
        .map_err(|e| e.to_string())?;
        let intent = draft_bytes(
            &envelope,
            &DraftFields {
                event_id: "10000100000000000001000000",
                event_type: "IntentReceived",
                schema_version: 2,
                causation_id: None,
                config_refs: &config_refs,
                payload: &intent_payload,
            },
        )
        .map_err(|e| e.to_string())?;
        let mut journal = MemoryJournal::new();
        let epoch = journal.take_ownership(&stream);
        let drafts = [opened.as_slice(), intent.as_slice()];
        let outcome = journal.append(&stream, 0, epoch, at, &drafts);
        assert!(
            matches!(outcome, AppendOutcome::Committed(_)),
            "{outcome:?}"
        );
        let artifacts = BTreeMap::from([(digest, config_bytes)]);
        verify_events(journal.rows(&stream), TrustedStart::GENESIS, &artifacts)
            .map_err(|error| format!("{error:?}"))?;
        let wrong = BTreeMap::from([(digest, b"wrong mandate".to_vec())]);
        assert!(
            verify_events(journal.rows(&stream), TrustedStart::GENESIS, &wrong).is_err(),
            "verification accepts bytes whose digest does not match the config reference"
        );
        Ok(())
    }

    #[test]
    fn artifact_refs_are_the_sorted_digest_references_in_the_payload() -> Result<(), String> {
        let at = UtcNanos::parse("2026-09-25T20:00:00.000000000Z").map_err(|e| e.to_string())?;
        let mut payload = mandate_canon::Object::new();
        payload.insert(
            mandate_canon::Key::new("content_hash").map_err(|_| "content_hash")?,
            Value::Str(format!("sha256:{}", "5".repeat(64))),
        );
        payload.insert(
            mandate_canon::Key::new("thesis_ref").map_err(|_| "thesis_ref")?,
            Value::Str(format!("sha256:{}", "2".repeat(64))),
        );
        let config_refs = mandate_canon::Object::new();
        let bytes = draft_bytes(
            &Envelope {
                stream: "agent:ws1:agent-a",
                writer: Writer::Agent,
                actor_id: "agent-a",
                event_time: at,
            },
            &DraftFields {
                event_id: "00000100000000000000000000",
                event_type: "ModelOutputRecorded",
                schema_version: 1,
                causation_id: None,
                config_refs: &config_refs,
                payload: &Value::Object(payload),
            },
        )
        .map_err(|e| e.to_string())?;
        let envelope = envelope_of(&bytes)?;
        let listed_values = envelope
            .get("artifact_refs")
            .and_then(Value::as_array)
            .ok_or("the envelope has no artifact_refs array")?;
        let listed: Vec<&str> = listed_values.iter().filter_map(Value::as_str).collect();
        let expected = [
            format!("sha256:{}", "2".repeat(64)),
            format!("sha256:{}", "5".repeat(64)),
        ];
        assert_eq!(
            listed,
            expected.iter().map(String::as_str).collect::<Vec<_>>()
        );
        Ok(())
    }
}
