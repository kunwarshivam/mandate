//! E7-10 (DEC-168, DEC-261, DEC-302): the journal vectors' `control_stream` section (journal spec
//! v0.7 §9.2), family by family, through `mandate-journal`'s `append` and `mandate-spec`'s mapping
//! to `JournaledFact`. Each family holds a draft the journal must accept and one it must refuse, so
//! neither a journal that accepts every draft nor one that refuses every draft passes.
//! `AccountSnapshotRecorded`'s cases are their own family, registered once stream K's fee-step
//! writer conforms (DEC-261 item 7, DEC-399).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use mandate_canon::{Digest, Value, to_canonical};
use mandate_journal::{AppendOutcome, MemoryJournal, StreamId, TrustedStart, verify_events};
use mandate_refcases::{Json, read_fixture, to_canon};
use mandate_spec::context::JournaledFact;
use mandate_spec::document::ConnectionId;
use mandate_time::UtcNanos;
use serde_json::json;

const APPEND_TIME: &str = "2026-09-26T14:00:00.000000000Z";
const SNAPSHOT: &str = "AccountSnapshotRecorded";

fn fixture() -> Json {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases");
    Arc::unwrap_or_clone(read_fixture(&dir, "journal.json").expect("the journal fixture"))
}

fn section(fx: &Json) -> &Json {
    fx.get("control_stream")
        .expect("journal.json has a control_stream section")
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

/// Applies one `{path, value}` or `{path, delete: true}` change, the path dotted from the envelope.
fn apply(body: &mut Json, change: &Json) {
    let path = text(change, "path");
    let (parents, last) = path
        .rsplit_once('.')
        .map_or((None, path), |(p, l)| (Some(p), l));
    let mut node = body;
    for name in parents.into_iter().flat_map(|p| p.split('.')) {
        node = node
            .get_mut(name)
            .unwrap_or_else(|| panic!("`{path}` has no `{name}`"));
    }
    let object = node.as_object_mut().expect("a change ends in an object");
    if change.get("delete").and_then(Json::as_bool) == Some(true) {
        assert!(object.remove(last).is_some(), "`{path}` deletes nothing");
    } else {
        object.insert(
            last.to_owned(),
            change.get("value").cloned().expect("a value"),
        );
    }
}

/// The base of a case: a chain event's body, or one of the section's named drafts.
fn base_of(section: &Json, case: &Json) -> Json {
    match case.get("base_seq").and_then(Json::as_u64) {
        Some(seq) => list(section, "chain")
            .iter()
            .find(|e| e.get("seq").and_then(Json::as_u64) == Some(seq))
            .and_then(|e| e.get("body"))
            .cloned()
            .unwrap_or_else(|| panic!("the chain has no seq {seq}")),
        None => section
            .get("drafts")
            .and_then(|d| d.get(text(case, "base_draft")))
            .cloned()
            .expect("a named base draft"),
    }
}

/// The event that opens `stream_id` in the vectors: the control chain's, the agent stream's, or
/// the version-3 account chain's.
fn opener(fx: &Json, stream_id: &str) -> Json {
    let chains = [
        list(section(fx), "chain"),
        list(
            fx.get("agent_stream").expect("an agent_stream section"),
            "chain",
        ),
        list(fx, "chain"),
    ];
    chains
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

/// Appends a case's draft onto its stream: after the control chain's events before its base, or
/// after the `StreamOpened` of the agent or account stream its named draft is on.
fn append_case(fx: &Json, case: &Json) -> AppendOutcome {
    let section = section(fx);
    let mut body = base_of(section, case);
    for change in list(case, "changes") {
        apply(&mut body, change);
    }
    let base_stream = text(&base_of(section, case), "stream_id").to_owned();
    let stream = StreamId::parse(&base_stream).expect("a stream id");
    let before: Vec<Json> = match case.get("base_seq").and_then(Json::as_u64) {
        Some(seq) => list(section, "chain")
            .iter()
            .filter(|e| e.get("seq").and_then(Json::as_u64) < Some(seq))
            .filter_map(|e| e.get("body").cloned())
            .collect(),
        None => vec![opener(fx, &base_stream)],
    };
    let mut journal = MemoryJournal::new();
    let epoch = journal.take_ownership(&stream);
    for (head, earlier) in (0u64..).zip(&before) {
        let outcome = journal.append(
            &stream,
            head,
            epoch,
            at(text(earlier, "recorded_at")),
            &[&draft_bytes(earlier)],
        );
        assert!(
            matches!(outcome, AppendOutcome::Committed(_)),
            "the events before the case append: {outcome:?}"
        );
    }
    let head = u64::try_from(before.len()).expect("a small chain");
    journal.append(
        &stream,
        head,
        epoch,
        at(APPEND_TIME),
        &[&draft_bytes(&body)],
    )
}

fn event_type_of(fx: &Json, case: &Json) -> String {
    text(&base_of(section(fx), case), "event_type").to_owned()
}

/// Runs every invalid and valid draft whose base is one of `event_types` and returns each failure,
/// after checking the family holds a draft to accept and one to refuse.
fn family_failures(fx: &Json, event_types: &[&str]) -> Vec<String> {
    let section = section(fx);
    let mut failures = Vec::new();
    let (mut accepting, mut refusing) = (0, 0);
    let chain_cases: Vec<Json> = list(section, "chain")
        .iter()
        .map(|e| {
            json!({"name": format!("chain seq {}", e["seq"]), "base_seq": e["seq"],
            "changes": [], "expect": {"outcome": "Valid"}})
        })
        .collect();
    for (kind, want_valid, cases) in [
        ("chain", true, chain_cases.as_slice()),
        ("valid_drafts", true, list(section, "valid_drafts")),
        ("invalid_drafts", false, list(section, "invalid_drafts")),
    ] {
        for case in cases {
            if !event_types.contains(&event_type_of(fx, case).as_str()) {
                continue;
            }
            let name = text(case, "name");
            let expect = case.get("expect").expect("an expect");
            let got = append_case(fx, case);
            let ok = if want_valid {
                accepting += 1;
                matches!(&got, AppendOutcome::Committed(rows) if rows.len() == 1)
            } else {
                refusing += 1;
                matches!(&got, AppendOutcome::Invalid { draft: 0, error }
                    if error.reason.code() == text(expect, "reason") && error.path == text(expect, "path"))
            };
            if !ok {
                failures.push(format!("{kind} {name}: expected {expect}, got {got:?}"));
            }
        }
    }
    assert!(accepting > 0, "{event_types:?} has no draft to accept");
    assert!(refusing > 0, "{event_types:?} has no draft to refuse");
    failures
}

fn assert_family(event_types: &[&str]) {
    let failed = family_failures(&fixture(), event_types);
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

#[test]
fn the_control_chain_appends_and_verifies_with_its_artifacts() {
    let fx = fixture();
    let section = section(&fx);
    let chain = list(section, "chain");
    assert!(chain.len() > 1, "the chain holds more than its opening");
    let stream = StreamId::parse(text(section, "stream_id")).expect("a stream id");
    let mut journal = MemoryJournal::new();
    let epoch = journal.take_ownership(&stream);
    for (head, entry) in (0u64..).zip(chain) {
        let body = entry.get("body").expect("a body");
        let mut widened = body.clone();
        widened["payload"]["unlisted"] = json!(true);
        let refused = journal.append(
            &stream,
            head,
            epoch,
            at(text(body, "recorded_at")),
            &[&draft_bytes(&widened)],
        );
        assert!(
            matches!(&refused, AppendOutcome::Invalid { draft: 0, error }
                if error.reason.code() == "schema" && error.path == "payload.unlisted"),
            "seq {}: a member §9.2 does not list is refused there: {refused:?}",
            head + 1
        );
        let outcome = journal.append(
            &stream,
            head,
            epoch,
            at(text(body, "recorded_at")),
            &[&draft_bytes(body)],
        );
        let AppendOutcome::Committed(rows) = outcome else {
            panic!("seq {}: {outcome:?}", head + 1);
        };
        assert_eq!(
            rows.iter().map(|r| r.hash.to_hex()).collect::<Vec<_>>(),
            vec![text(entry, "hash").to_owned()],
            "seq {}: the stored hash is the vector's",
            head + 1
        );
    }
    let artifacts: BTreeMap<Digest, Vec<u8>> = list(section, "artifacts")
        .iter()
        .map(|a| {
            let canonical = text(a, "canonical").as_bytes().to_vec();
            (Digest::of(&canonical), canonical)
        })
        .collect();
    let verified = verify_events(journal.rows(&stream), TrustedStart::GENESIS, &artifacts);
    assert!(verified.is_ok(), "{verified:?}");
}

#[test]
fn the_control_stream_opens_with_its_own_workspace() {
    assert_family(&["StreamOpened"]);
}

#[test]
fn a_connection_is_named_by_id_and_scopes_and_never_by_a_key() {
    assert_family(&["ConnectionEstablished", "ConnectionRevoked"]);
}

#[test]
fn a_disclosure_is_accepted_with_step_up_for_one_stored_version() {
    assert_family(&["DisclosureAccepted"]);
}

#[test]
fn a_configuration_registers_under_its_hash_and_a_model_with_its_terms() {
    assert_family(&["ConfigSnapshotRegistered"]);
}

#[test]
fn a_mandate_version_binds_its_document_provenance_and_confirmation() {
    assert_family(&["MandateVersionCreated", "MandateConfirmed"]);
}

#[test]
fn a_deployment_and_a_retirement_bind_their_version() {
    assert_family(&["AgentDeployed", "AgentStopped"]);
}

#[test]
fn a_refused_owner_command_is_on_its_own_stream_and_names_its_input() {
    assert_family(&["OwnerCommandRefused"]);
}

/// The account stream's snapshot is closed, with rule 24's cash members `null` together or
/// compared within the band, and every snapshot draft in the vectors gets its own `expect`: the
/// fee step's (`snapshot_fees`) and a cash comparison's (`snapshot_reconciled`) are accepted, and
/// each invalid draft is refused with its reason at its path (DEC-261 item 7, DEC-399).
#[test]
#[ignore = "pending E7-10"]
fn an_account_snapshot_is_closed_and_checked_by_rule_24() {
    let fx = fixture();
    let mut failed = family_failures(&fx, &[SNAPSHOT]);
    for name in ["snapshot_reconciled", "snapshot_fees"] {
        let base = json!({"name": name, "base_draft": name, "changes": []});
        let got = append_case(&fx, &base);
        if !matches!(&got, AppendOutcome::Committed(rows) if rows.len() == 1) {
            failed.push(format!("base draft {name}: expected Valid, got {got:?}"));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// The fact a record maps to, written in the vectors' form: decimals and dates as text, sets as
/// sorted lists, digests as `sha256:` references.
fn fact_json(fact: &JournaledFact) -> Json {
    let sha = |d: &Digest| format!("sha256:{}", d.to_hex());
    match fact {
        JournaledFact::ConnectionEstablished {
            connection_id,
            environment,
        } => json!({"kind": "ConnectionEstablished", "connection_id": connection_id.as_str(),
            "environment": format!("{environment:?}").to_lowercase()}),
        JournaledFact::ConnectionRevoked { connection_id } => {
            json!({"kind": "ConnectionRevoked", "connection_id": connection_id.as_str()})
        }
        JournaledFact::DisclosureAccepted { version } => {
            json!({"kind": "DisclosureAccepted", "version": sha(version)})
        }
        JournaledFact::ModelRegistered { id, model } => json!({"kind": "ModelRegistered",
            "id": id.as_str(), "version": model.version, "content_hash": sha(&model.content_hash),
            "params": model.params, "admits_instruments": model.admits_instruments}),
        JournaledFact::MandateVersionCreated { version, sources } => json!({
            "kind": "MandateVersionCreated", "version": sha(&version.digest()),
            "sources": sources.iter().map(|(path, source)|
                json!({"path": path.as_str(), "source": source.as_str()})).collect::<Vec<_>>()}),
        JournaledFact::MandateConfirmed {
            version,
            confirmed_paths,
        } => json!({"kind": "MandateConfirmed", "version": sha(&version.digest()),
            "confirmed_paths": confirmed_paths.iter().map(|p| p.as_str()).collect::<Vec<_>>()}),
        JournaledFact::AgentVersionActive {
            agent,
            connection_id,
            environment,
            allocation_usd,
            pinned,
        } => json!({"kind": "AgentVersionActive", "agent": agent.as_str(),
            "connection_id": connection_id.as_str(),
            "environment": format!("{environment:?}").to_lowercase(),
            "allocation_usd": allocation_usd.to_string(),
            "pinned": pinned.iter().map(|a| a.as_str()).collect::<Vec<_>>()}),
        JournaledFact::AgentStopped {
            agent,
            connection_id,
            retired_on,
            loss_added_usd,
        } => json!({"kind": "AgentStopped", "agent": agent.as_str(),
            "connection_id": connection_id.as_str(), "retired_on": retired_on.to_string(),
            "loss_added_usd": loss_added_usd.to_string()}),
        JournaledFact::AccountSnapshot {
            connection_id,
            equity_usd,
        } => json!({"kind": "AccountSnapshot", "connection_id": connection_id.as_str(),
            "equity_usd": equity_usd.to_string()}),
        other => json!({"kind": format!("{other:?}")}),
    }
}

/// Every record of the section, chain event or named draft, mapped with the stored artifacts and
/// the account stream's connection the derivation gives; the facts must be exactly the vectors'
/// `journaled_facts`, and every other record must map to none.
#[test]
fn each_record_maps_to_its_journaled_fact() {
    let fx = fixture();
    let section = section(&fx);
    let stored: BTreeMap<Digest, Value> = list(section, "artifacts")
        .iter()
        .map(|a| {
            let object = to_canon(a.get("object").expect("an object")).expect("canonical");
            (Digest::of(&to_canonical(&object)), object)
        })
        .collect();
    let documents = |digest: &Digest| stored.get(digest).cloned();
    let binding = section
        .get("derivation")
        .and_then(|d| d.get("account_connection"))
        .expect("the account stream's connection");
    let connection = ConnectionId::parse(text(binding, "connection_id")).expect("a connection");
    let mut records: Vec<(Json, Json)> = list(section, "chain")
        .iter()
        .map(|e| {
            (
                json!({"seq": e.get("seq")}),
                e.get("body").cloned().expect("a body"),
            )
        })
        .collect();
    if let Some(drafts) = section.get("drafts").and_then(Json::as_object) {
        records.extend(
            drafts
                .iter()
                .map(|(name, d)| (json!({"draft": name}), d.clone())),
        );
    }
    let mut mapped = Vec::new();
    for (key, body) in &records {
        let payload = to_canon(body.get("payload").expect("a payload")).expect("canonical");
        let on_binding = text(body, "stream_id") == text(binding, "stream_id");
        let fact = JournaledFact::from_record(
            text(body, "event_type"),
            &payload,
            &documents,
            on_binding.then_some(&connection),
        )
        .unwrap_or_else(|e| panic!("{key}: {e:?}"));
        if let Some(fact) = fact {
            let mut entry = key.clone();
            entry["fact"] = fact_json(&fact);
            mapped.push(entry);
        }
    }
    let mut expected = list(section, "journaled_facts").to_vec();
    assert!(expected.len() > 5, "the vectors list the facts");
    let by_record = |entry: &Json| entry.to_string();
    mapped.sort_by_key(by_record);
    expected.sort_by_key(by_record);
    assert_eq!(mapped, expected);
}
