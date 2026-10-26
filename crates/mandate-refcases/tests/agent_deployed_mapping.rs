//! Journal spec v0.54 §9.24 rule 200 (DEC-1171; E21-33): §9.2's mapping of `AgentDeployed` to
//! `AgentVersionActive`, judged where the document is read. The journal vectors' `agent_deployed`
//! section lists each mapping case: a draft, the stored documents, and the fact or the member it is
//! refused at. Version 2 maps to the same fact as version 1, from the same document; one whose
//! `connection_id` differs from the document's is refused at `connection_id`, and with no document
//! stored it is refused at `mandate_version` first. Before DEC-1171's code change the mismatch
//! mapped to the document's fact, the answer `another_connection` fails on.
//!
//! Journal spec v0.84 (DEC-2131 item 4): the `stepped_up_records` section's mappings hold each
//! version-3 record, `MandateConfirmed` and `AgentDeployed`, to its version 2's fact, rule 200
//! included, so the mapping reads a version 3 as a version 2 and ignores `step_up`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use mandate_canon::{Digest, Value, to_canonical};
use mandate_refcases::{Json, read_fixture, to_canon};
use mandate_spec::SpecError;
use mandate_spec::context::JournaledFact;
use serde_json::json;

fn section_named(name: &str) -> Json {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases");
    let fixture = Arc::unwrap_or_clone(read_fixture(&dir, "journal.json").expect("the fixture"));
    fixture
        .get(name)
        .cloned()
        .unwrap_or_else(|| panic!("journal.json has a {name} section"))
}

fn section() -> Json {
    section_named("agent_deployed")
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

/// A case's draft: its named base draft with its `{path, value}` changes applied.
fn draft(section: &Json, case: &Json) -> Json {
    let mut body = section["drafts"][text(case, "base_draft")].clone();
    for change in list(case, "changes") {
        let path = text(change, "path");
        let (parent, last) = path.rsplit_once('.').map_or(("", path), |(p, l)| (p, l));
        let node = parent
            .split('.')
            .filter(|name| !name.is_empty())
            .fold(&mut body, |node, name| &mut node[name]);
        node[last] = change["value"].clone();
    }
    body
}

/// The documents a case stores, by the digest of their canonical bytes.
fn stored(section: &Json, case: &Json) -> BTreeMap<Digest, Value> {
    let named: Vec<&str> = list(case, "stored")
        .iter()
        .filter_map(Json::as_str)
        .collect();
    list(section, "artifacts")
        .iter()
        .filter(|a| named.contains(&text(a, "name")))
        .map(|a| {
            let object = to_canon(&a["object"]).expect("canonical");
            (Digest::of(&to_canonical(&object)), object)
        })
        .collect()
}

/// The mapping's answer in the vectors' form: the fact, or the payload member it is refused at.
fn answer(got: Result<Option<JournaledFact>, SpecError>) -> Json {
    match got {
        Ok(Some(JournaledFact::AgentVersionActive {
            agent,
            connection_id,
            environment,
            allocation_usd,
            pinned,
            version: _,
        })) => json!({"refused": null, "fact": {"kind": "AgentVersionActive",
            "agent": agent.as_str(), "connection_id": connection_id.as_str(),
            "environment": format!("{environment:?}").to_lowercase(),
            "allocation_usd": allocation_usd.to_string(),
            "pinned": pinned.iter().map(|a| a.as_str()).collect::<Vec<_>>()}}),
        Ok(Some(JournaledFact::MandateConfirmed {
            version,
            confirmed_paths,
        })) => json!({"refused": null, "fact": {"kind": "MandateConfirmed",
            "version": format!("sha256:{}", version.digest().to_hex()),
            "confirmed_paths": confirmed_paths.iter().map(|p| p.as_str()).collect::<Vec<_>>()}}),
        Err(SpecError::InvalidInput { what }) => json!({"refused": format!("payload.{what}")}),
        other => json!({"unexpected": format!("{other:?}")}),
    }
}

/// Every mapping case of `section`, against its vector: the failures, one line each.
fn mapping_failures(section: &Json) -> Vec<String> {
    let mut failed = Vec::new();
    for case in list(section, "mappings") {
        let body = draft(section, case);
        let documents = stored(section, case);
        let payload = to_canon(&body["payload"]).expect("canonical");
        let got = answer(JournaledFact::from_record(
            text(&body, "event_type"),
            &payload,
            &|digest: &Digest| documents.get(digest).cloned(),
            None,
        ));
        if got != case["expect"] {
            failed.push(format!(
                "{} ({}): expected {}, got {got}",
                text(case, "name"),
                text(case, "clause"),
                case["expect"]
            ));
        }
    }
    failed
}

/// A version-3 `MandateConfirmed` and `AgentDeployed` map to their version 2's facts, and rule
/// 200 refuses a version 3 whose `connection_id` differs from the document's (DEC-2131 item 4).
#[test]
fn every_version_3_mapping_is_its_version_2s_fact_or_refusal() {
    let section = section_named("stepped_up_records");
    let failed = mapping_failures(&section);
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    let cases = list(&section, "mappings");
    let kinds: Vec<&Json> = cases.iter().map(|c| &c["expect"]["fact"]["kind"]).collect();
    assert!(
        kinds.contains(&&json!("MandateConfirmed"))
            && kinds.contains(&&json!("AgentVersionActive"))
            && cases
                .iter()
                .any(|c| c["expect"]["refused"] == "payload.connection_id"),
        "both records mapped and rule 200 refused at version 3"
    );
}

#[test]
fn every_agent_deployed_mapping_is_the_fact_or_the_refusal_its_vector_lists() {
    let section = section();
    let cases = list(&section, "mappings");
    let failed = mapping_failures(&section);
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    let refused = cases
        .iter()
        .filter(|c| c["expect"]["refused"] == "payload.connection_id")
        .count();
    assert!(
        cases.len() >= 7 && refused >= 1,
        "{} mappings, {refused} refused for rule 200; never fewer",
        cases.len()
    );
}
