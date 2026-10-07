//! E7-19 (DEC-484): versioned policy and model-registry references in production records.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use mandate_canon::{Digest, parse, to_canonical};
use mandate_journal::{AppendOutcome, ArtifactRef, MemoryJournal, StreamId};
use mandate_refcases::{Json, read_fixture};
use mandate_time::UtcNanos;

const SECTION: &str = "production_config_refs";

fn fixture() -> Json {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases");
    Arc::unwrap_or_clone(read_fixture(&dir, "journal.json").unwrap())
}

fn production_section(fixture: &Json) -> &Json {
    fixture
        .get(SECTION)
        .expect("production_config_refs section")
}

fn config_artifacts(section: &Json) -> BTreeMap<Digest, Vec<u8>> {
    section["artifacts"]
        .as_array()
        .expect("configuration artifacts")
        .iter()
        .map(|artifact| {
            let reference = ArtifactRef::parse(artifact["ref"].as_str().expect("artifact ref"))
                .expect("canonical artifact ref");
            let bytes = artifact["canonical"]
                .as_str()
                .expect("canonical artifact bytes")
                .as_bytes()
                .to_vec();
            let object_bytes = serde_json::to_vec(&artifact["object"]).expect("artifact object");
            let object = parse(&object_bytes).expect("canonicalizable artifact object");
            assert_eq!(
                to_canonical(&object),
                bytes,
                "{} canonical bytes",
                artifact["name"].as_str().expect("artifact name")
            );
            let digest = Digest::of(&bytes);
            assert_eq!(
                digest,
                reference.digest(),
                "artifact ref must hash its bytes"
            );
            (digest, bytes)
        })
        .collect()
}

fn draft_without_journal_fields(body: &Json) -> Json {
    let mut draft = body.clone();
    let object = draft.as_object_mut().expect("event body");
    for member in ["seq", "prev_hash", "recorded_at"] {
        object.remove(member);
    }
    draft
}

fn opened_journal(fixture: &Json, draft: &Json) -> (MemoryJournal, StreamId, u64) {
    let stream_text = draft["stream_id"].as_str().expect("stream id");
    let stream = StreamId::parse(stream_text).expect("canonical stream id");
    let source = if stream_text.starts_with("agent:") {
        &fixture["agent_stream"]["chain"][0]["body"]
    } else {
        &fixture["control_stream"]["chain"][0]["body"]
    };
    let opened = draft_without_journal_fields(source);
    let bytes = serde_json::to_vec(&opened).expect("opened draft");
    let mut journal = MemoryJournal::new();
    let epoch = journal.take_ownership(&stream);
    let at = UtcNanos::parse("2026-09-21T14:00:02.000000000Z").expect("append time");
    assert!(
        matches!(
            journal.append(&stream, 0, epoch, at, &[bytes.as_slice()]),
            AppendOutcome::Committed(_)
        ),
        "stream setup must commit"
    );
    (journal, stream, epoch)
}

fn append(
    fixture: &Json,
    draft: &Json,
    artifacts: &BTreeMap<Digest, Vec<u8>>,
) -> (AppendOutcome, MemoryJournal, StreamId) {
    let (mut journal, stream, epoch) = opened_journal(fixture, draft);
    let bytes = serde_json::to_vec(draft).expect("production draft");
    let at = UtcNanos::parse("2026-09-21T14:00:03.000000000Z").expect("append time");
    let outcome =
        journal.append_with_config_artifacts(&stream, 1, epoch, at, &[bytes.as_slice()], artifacts);
    (outcome, journal, stream)
}

fn assert_committed(fixture: &Json, name: &str, draft: &Json) {
    let artifacts = config_artifacts(production_section(fixture));
    let (outcome, journal, stream) = append(fixture, draft, &artifacts);
    let AppendOutcome::Committed(rows) = outcome else {
        panic!("{name} gave {outcome:?}");
    };
    let [row] = rows.as_slice() else {
        panic!("{name} committed {} rows", rows.len());
    };
    assert_eq!(journal.rows(&stream).len(), 2, "{name}: journal row count");
    assert_eq!(row.seq, 2, "{name}: sequence");
    assert_eq!(
        row.event_id,
        draft["event_id"].as_str().expect("draft event id"),
        "{name}: event id"
    );
    assert_eq!(
        row.event_type,
        draft["event_type"].as_str().expect("draft event type"),
        "{name}: event type"
    );
    let body: Json = serde_json::from_slice(&row.body).expect("stored event body");
    assert_eq!(
        body["config_refs"], draft["config_refs"],
        "{name}: config refs"
    );
    assert_eq!(
        body["artifact_refs"], draft["artifact_refs"],
        "{name}: artifact refs"
    );
}

fn changed(section: &Json, case: &Json) -> Json {
    let base = case["base"].as_str().expect("invalid draft base");
    let mut draft = section["valid_drafts"][base].clone();
    for change in case["changes"].as_array().expect("draft changes") {
        let path = change["path"].as_str().expect("change path");
        let mut node = &mut draft;
        let mut members = path.split('.').peekable();
        while let Some(member) = members.next() {
            if members.peek().is_none() {
                if change.get("delete").and_then(Json::as_bool) == Some(true) {
                    node.as_object_mut().expect("change parent").remove(member);
                } else {
                    node[member] = change["value"].clone();
                }
                break;
            }
            node = node.get_mut(member).expect("change parent member");
        }
    }
    draft
}

#[test]
fn version_two_agent_records_bind_the_complete_configuration() {
    let fixture = fixture();
    let section = production_section(&fixture);
    for name in ["model_output", "decision"] {
        assert_committed(&fixture, name, &section["valid_drafts"][name]);
    }
}

#[test]
fn version_two_registration_records_bind_their_stored_objects() {
    let fixture = fixture();
    let section = production_section(&fixture);
    for name in ["policy_registration", "model_registry_registration"] {
        assert_committed(&fixture, name, &section["valid_drafts"][name]);
    }
}

#[test]
fn every_invalid_production_reference_is_refused_as_specified() {
    let fixture = fixture();
    let section = production_section(&fixture);
    let cases = section["invalid_drafts"]
        .as_array()
        .expect("invalid production drafts");
    assert!(
        !cases.is_empty(),
        "invalid production drafts must be non-empty"
    );
    let artifacts = config_artifacts(section);
    for case in cases {
        let name = case["name"].as_str().expect("invalid draft name");
        let expected_reason = case["expect"]["reason"].as_str().expect("expected reason");
        let expected_path = case["expect"]["path"].as_str().expect("expected path");
        let (outcome, _, _) = append(&fixture, &changed(section, case), &artifacts);
        let AppendOutcome::Invalid { draft, error } = outcome else {
            panic!("{name} gave {outcome:?}");
        };
        assert_eq!(draft, 0, "{name}: draft index");
        assert_eq!(error.reason.code(), expected_reason, "{name}: reason");
        assert_eq!(error.path, expected_path, "{name}: path");
    }
}

#[test]
fn every_new_configuration_reference_requires_its_stored_object() {
    let fixture = fixture();
    let section = production_section(&fixture);
    for (name, reference_path, expected_path) in [
        (
            "model_output",
            "config_refs.model_registry",
            "config_refs.model_registry",
        ),
        (
            "decision",
            "config_refs.policy_set",
            "config_refs.policy_set",
        ),
        (
            "decision",
            "config_refs.model_registry",
            "config_refs.model_registry",
        ),
        (
            "policy_registration",
            "payload.content_hash",
            "payload.content_hash",
        ),
        (
            "model_registry_registration",
            "payload.content_hash",
            "payload.content_hash",
        ),
    ] {
        let draft = &section["valid_drafts"][name];
        let reference = reference_path
            .split('.')
            .fold(draft, |value, member| &value[member])
            .as_str()
            .and_then(ArtifactRef::parse)
            .expect("configuration artifact ref");
        let mut artifacts = config_artifacts(section);
        assert!(
            artifacts.remove(&reference.digest()).is_some(),
            "{name}: artifact is stored"
        );
        let (outcome, _, _) = append(&fixture, draft, &artifacts);
        let AppendOutcome::Invalid { draft, error } = outcome else {
            panic!("{name} gave {outcome:?}");
        };
        assert_eq!(draft, 0, "{name}: draft index");
        assert_eq!(error.reason.code(), "missing_artifact", "{name}: reason");
        assert_eq!(error.path, expected_path, "{name}: path");
    }
}
