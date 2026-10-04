//! The account stream's risk-state records (journal spec v0.8 §9.3, DEC-403, DEC-404): the journal
//! vectors' `risk_state` section through `mandate-journal`'s `append` and `mandate-spec`'s mapping
//! to `JournaledFact`. `MandateVersionApplied` and `UniverseChanged` each hold drafts the journal
//! must accept and drafts it must refuse, so neither a journal that accepts every draft nor one
//! that refuses every draft passes.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use mandate_canon::{Digest, Value, to_canonical};
use mandate_journal::{AppendOutcome, MemoryJournal, StreamId};
use mandate_refcases::{Json, read_fixture, to_canon};
use mandate_spec::SpecError;
use mandate_spec::context::JournaledFact;
use mandate_time::UtcNanos;
use serde_json::json;

const APPEND_TIME: &str = "2026-09-26T14:00:00.000000000Z";
const VERSION_APPLIED: &str = "MandateVersionApplied";
const CLASSIFICATIONS: [&str; 3] = ["risk_increasing", "risk_reducing", "neutral"];

fn fixture() -> Json {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases");
    Arc::unwrap_or_clone(read_fixture(&dir, "journal.json").expect("the journal fixture"))
}

fn section(fx: &Json) -> &Json {
    fx.get("risk_state")
        .expect("journal.json has a risk_state section")
}

fn list<'a>(value: &'a Json, key: &str) -> &'a [Json] {
    value
        .get(key)
        .and_then(Json::as_array)
        .map(Vec::as_slice)
        .unwrap_or_else(|| panic!("`{key}` is a list"))
}

fn text<'a>(value: &'a Json, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(Json::as_str)
        .unwrap_or_else(|| panic!("`{key}` is text"))
}

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse(text).expect("a timestamp")
}

/// A body as a writer sends it: without the journal-assigned fields.
fn draft_bytes(body: &Json) -> Vec<u8> {
    let mut draft = body.as_object().cloned().expect("a body is an object");
    for field in ["seq", "prev_hash", "recorded_at"] {
        draft.remove(field);
    }
    serde_json::to_vec(&draft).expect("a draft serializes")
}

/// Applies a `{path, value}` or `{path, delete: true}` change, dotted from the envelope.
fn apply(body: &mut Json, change: &Json) {
    let path = text(change, "path");
    let mut names: Vec<&str> = path.split('.').collect();
    let last = names.pop().expect("a non-empty path");
    let mut node = body;
    for name in names {
        node = node
            .get_mut(name)
            .unwrap_or_else(|| panic!("`{path}` resolves"));
    }
    let members = node
        .as_object_mut()
        .unwrap_or_else(|| panic!("`{path}` is inside an object"));
    if change.get("delete").and_then(Json::as_bool) == Some(true) {
        members.remove(last);
    } else {
        let value = change.get("value").cloned().expect("a change has a value");
        members.insert(last.to_owned(), value);
    }
}

/// A case's draft: its base draft with its changes applied.
fn case_body(section: &Json, case: &Json) -> Json {
    let mut body = section
        .get("drafts")
        .and_then(|d| d.get(text(case, "base_draft")))
        .cloned()
        .expect("a named base draft");
    for change in list(case, "changes") {
        apply(&mut body, change);
    }
    body
}

/// The `StreamOpened` that opens `stream_id` in the vectors: the version-3 account chain's, or the
/// control chain's for a draft moved onto the control stream.
fn opener(fx: &Json, stream_id: &str) -> Json {
    [
        list(fx, "chain"),
        list(fx.get("control_stream").expect("a control_stream"), "chain"),
    ]
    .iter()
    .find_map(|chain| {
        chain
            .first()
            .and_then(|e| e.get("body"))
            .filter(|b| b.get("stream_id").and_then(Json::as_str) == Some(stream_id))
            .cloned()
    })
    .unwrap_or_else(|| panic!("no chain opens {stream_id}"))
}

/// Appends `body` after the `StreamOpened` of the stream it names.
fn append(fx: &Json, body: &Json) -> AppendOutcome {
    let stream_id = text(body, "stream_id");
    let stream = StreamId::parse(stream_id).expect("a stream id");
    let opened = opener(fx, stream_id);
    let mut journal = MemoryJournal::new();
    let epoch = journal.take_ownership(&stream);
    let outcome = journal.append(
        &stream,
        0,
        epoch,
        at(text(&opened, "recorded_at")),
        &[&draft_bytes(&opened)],
    );
    assert!(
        matches!(outcome, AppendOutcome::Committed(_)),
        "the stream opens: {outcome:?}"
    );
    journal.append(&stream, 1, epoch, at(APPEND_TIME), &[&draft_bytes(body)])
}

/// Every base and valid draft of both records appends, and every invalid one is refused with its
/// reason at its path, for each record type in turn (DEC-403, DEC-404).
#[test]
fn the_risk_state_records_append_as_their_vectors_say() {
    let fx = fixture();
    let section = section(&fx);
    let mut failures = Vec::new();
    let mut seen: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut accepting: Vec<(String, Json)> = section
        .get("drafts")
        .and_then(Json::as_object)
        .expect("named drafts")
        .iter()
        .map(|(name, body)| (format!("base {name}"), body.clone()))
        .collect();
    accepting.extend(list(section, "valid_drafts").iter().map(|case| {
        (
            format!("valid {}", text(case, "name")),
            case_body(section, case),
        )
    }));
    for (name, body) in &accepting {
        seen.entry(text(body, "event_type").to_owned())
            .or_default()
            .0 += 1;
        let got = append(&fx, body);
        if !matches!(&got, AppendOutcome::Committed(rows) if rows.len() == 1) {
            failures.push(format!("{name}: expected Valid, got {got:?}"));
        }
    }
    for case in list(section, "invalid_drafts") {
        let body = case_body(section, case);
        seen.entry(text(&body, "event_type").to_owned())
            .or_default()
            .1 += 1;
        let expect = case.get("expect").expect("an expect");
        let got = append(&fx, &body);
        let refused = matches!(&got, AppendOutcome::Invalid { draft: 0, error }
            if error.reason.code() == text(expect, "reason") && error.path == text(expect, "path"));
        if !refused {
            failures.push(format!(
                "invalid {}: expected {expect}, got {got:?}",
                text(case, "name")
            ));
        }
    }
    for event_type in [VERSION_APPLIED, "UniverseChanged"] {
        let (accepts, refuses) = seen.get(event_type).copied().unwrap_or_default();
        assert!(
            accepts > 0 && refuses > 0,
            "{event_type} has drafts to accept and to refuse"
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The stored mandate documents of the section, under their digests.
fn documents(section: &Json) -> BTreeMap<Digest, Value> {
    list(section, "artifacts")
        .iter()
        .map(|a| {
            let object = to_canon(a.get("object").expect("an object")).expect("canonical");
            (Digest::of(&to_canonical(&object)), object)
        })
        .collect()
}

/// A record's fact in the vectors' form, `null` for none.
fn fact_json(fact: Option<&JournaledFact>) -> Json {
    match fact {
        None => Json::Null,
        Some(JournaledFact::AgentVersionActive {
            agent,
            connection_id,
            environment,
            allocation_usd,
            pinned,
            version: _,
        }) => json!({"kind": "AgentVersionActive", "agent": agent.as_str(),
            "connection_id": connection_id.as_str(),
            "environment": format!("{environment:?}").to_lowercase(),
            "allocation_usd": allocation_usd.to_string(),
            "pinned": pinned.iter().map(|a| a.as_str()).collect::<Vec<_>>()}),
        Some(JournaledFact::UniverseChanged {
            agent,
            instrument,
            admitted,
        }) => json!({"kind": "UniverseChanged", "agent": agent.as_str(),
            "instrument": instrument.as_str(), "admitted": admitted}),
        Some(other) => json!({"kind": format!("{other:?}")}),
    }
}

fn map(section: &Json, body: &Json) -> Result<Option<JournaledFact>, SpecError> {
    let stored = documents(section);
    let payload = to_canon(body.get("payload").expect("a payload")).expect("canonical");
    JournaledFact::from_record(
        text(body, "event_type"),
        &payload,
        &|digest: &Digest| stored.get(digest).cloned(),
        None,
    )
}

/// Each base draft maps to the fact the vectors list for it: an applied version to the agent's
/// version in force, read from the stored document `new_version` names; a rejected one to none;
/// and a universe change to its instrument admitted or removed (journal spec §9.3).
#[test]
fn each_risk_state_record_maps_to_its_journaled_fact() {
    let fx = fixture();
    let section = section(&fx);
    let drafts = section
        .get("drafts")
        .and_then(Json::as_object)
        .expect("named drafts");
    let mut mapped: Vec<Json> = drafts
        .iter()
        .map(|(name, body)| {
            let fact = map(section, body).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            json!({"draft": name, "fact": fact_json(fact.as_ref())})
        })
        .collect();
    let mut expected = list(section, "journaled_facts").to_vec();
    assert_eq!(expected.len(), drafts.len(), "one listed fact per draft");
    let by_record = |entry: &Json| entry.to_string();
    mapped.sort_by_key(by_record);
    expected.sort_by_key(by_record);
    assert_eq!(mapped, expected);
}

/// An applied version whose `new_version` document is not stored is refused at `new_version`,
/// never skipped, with every other document stored.
#[test]
fn an_applied_version_whose_document_is_not_stored_is_refused() {
    let fx = fixture();
    let section = section(&fx);
    let body = section
        .get("drafts")
        .and_then(|d| d.get("version_applied"))
        .expect("the applied base draft");
    let payload = to_canon(body.get("payload").expect("a payload")).expect("canonical");
    let new = Digest::from_hex(
        text(&body["payload"], "new_version")
            .strip_prefix("sha256:")
            .expect("a sha256 reference"),
    )
    .expect("a digest");
    let mut stored = documents(section);
    assert!(stored.remove(&new).is_some(), "the new version is stored");
    let got = JournaledFact::from_record(
        VERSION_APPLIED,
        &payload,
        &|digest: &Digest| stored.get(digest).cloned(),
        None,
    );
    assert_eq!(
        got,
        Err(SpecError::InvalidInput {
            what: "new_version"
        })
    );
}

/// The registration re-derives mandate spec §9.2's classification from the two stored documents
/// and refuses a record that states another: every version record the vectors accept maps under
/// its own classification, and under each other one is refused at `classification`, applied or
/// rejected alike. Rule 33 sees only the allocation change, the floor, and three rejections, so
/// this is what keeps a risk-increasing version through any other §9.2 row from being journaled
/// as applied, labelled `neutral`, with no step-up (#470 round 2, minor 5; DEC-403 item 5).
#[test]
fn a_version_whose_classification_differs_from_its_documents_is_refused() {
    let fx = fixture();
    let section = section(&fx);
    let mut versions: Vec<(String, Json)> = section
        .get("drafts")
        .and_then(Json::as_object)
        .expect("named drafts")
        .iter()
        .map(|(name, body)| (format!("base {name}"), body.clone()))
        .collect();
    versions.extend(list(section, "valid_drafts").iter().map(|case| {
        (
            format!("valid {}", text(case, "name")),
            case_body(section, case),
        )
    }));
    versions.retain(|(_, body)| text(body, "event_type") == VERSION_APPLIED);
    assert!(
        versions.len() >= 6,
        "the vectors hold applied and rejected versions of each classification"
    );
    let mut failures = Vec::new();
    for (name, body) in &versions {
        let stated = text(&body["payload"], "classification").to_owned();
        if let Err(e) = map(section, body) {
            failures.push(format!("{name} as {stated}: {e:?}"));
        }
        for other in CLASSIFICATIONS.iter().filter(|c| **c != stated) {
            let mut relabelled = body.clone();
            relabelled["payload"]["classification"] = json!(other);
            let got = map(section, &relabelled);
            if got
                != Err(SpecError::InvalidInput {
                    what: "classification",
                })
            {
                failures.push(format!("{name} relabelled {other}: {got:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
