//! The vectors' `agent_stream` section (journal spec v0.6 §9.1 and §11, DEC-177, DEC-252; E7-9).
//! Every chain event is appended and reproduces its canonical body and hash; each invalid draft,
//! the base chain event with its changes, is refused by `append` with its reason and path; each
//! valid draft and valid batch is committed; each invalid batch is refused at its `draft_index`;
//! and each `range_verification` case, re-chained from the genesis hash, passes every per-event
//! check and fails `verify_agent_stream` with its code at its seq. A draft is appended to the chain
//! up to the event before its base, so the stream is open and every earlier event is stored.

use std::collections::BTreeMap;
use std::sync::Arc;

use mandate_canon::{Digest, to_canonical};
use mandate_journal::{
    AppendOutcome, MemoryJournal, StoredEvent, StreamId, TrustedStart, Verified,
    verify_agent_stream, verify_events,
};

use super::{APPEND_TIME, Check, chain_entry, digest, draft_json, timestamp};
use crate::{Case, Json, at, ensure, expect_eq, list_at, str_at, to_canon, u64_at};

const SECTION: &str = "agent_stream";

/// The section's cases. A case about one draft is named by its base event's type, so a family of
/// §9.1 can be run on its own.
pub(super) fn cases(fixture: &Arc<Json>) -> Vec<Case> {
    let mut out = Vec::new();
    let mut add = |id: String, run: Check, arg: String| {
        let fx = Arc::clone(fixture);
        out.push(Case::new(id, move || run(&fx, &arg)));
    };
    let section = fixture.get(SECTION).unwrap_or(&Json::Null);
    let type_of =
        |seq: u64| event_type(section, seq).map_or_else(|_| "<unknown>".to_owned(), str::to_owned);
    let prefix = format!("journal::{SECTION}");
    add(format!("{prefix}::artifacts"), artifacts, String::new());
    for (i, entry) in list_at(section, "chain")
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let seq = entry.get("seq").and_then(Json::as_u64).unwrap_or_default();
        add(
            format!("{prefix}::{}::chain::seq_{seq}", type_of(seq)),
            chain_seq,
            i.to_string(),
        );
    }
    add(
        format!("{prefix}::chain::append"),
        chain_append,
        String::new(),
    );
    let drafts: [(&str, Check); 2] = [("invalid_drafts", invalid), ("valid_drafts", valid)];
    for (list, run) in drafts {
        let kind = list.trim_end_matches("_drafts");
        for case in list_at(section, list).unwrap_or_default() {
            let base = case
                .get("base_seq")
                .and_then(Json::as_u64)
                .unwrap_or_default();
            let name = name_of(case);
            add(
                format!("{prefix}::{}::{kind}::{name}", type_of(base)),
                run,
                name.to_owned(),
            );
        }
    }
    let named: [(&str, &str, Check); 3] = [
        ("valid_batches", "batch", batch),
        ("invalid_batches", "batch", batch),
        ("range_verification", "range", range),
    ];
    for (list, kind, run) in named {
        for case in list_at(section, list).unwrap_or_default() {
            let name = name_of(case);
            add(format!("{prefix}::{kind}::{name}"), run, name.to_owned());
        }
    }
    out
}

fn name_of(case: &Json) -> &str {
    case.get("name")
        .and_then(Json::as_str)
        .unwrap_or("<unnamed>")
}

fn chain_entry_at(section: &Json, seq: u64) -> Result<&Json, String> {
    list_at(section, "chain")?
        .iter()
        .find(|e| e.get("seq").and_then(Json::as_u64) == Some(seq))
        .ok_or_else(|| format!("the chain has no seq {seq}"))
}

fn event_type(section: &Json, seq: u64) -> Result<&str, String> {
    str_at(chain_entry_at(section, seq)?, "event_type")
}

fn named<'a>(section: &'a Json, lists: &[&str], name: &str) -> Result<&'a Json, String> {
    lists
        .iter()
        .flat_map(|list| list_at(section, list).unwrap_or_default())
        .find(|case| case.get("name").and_then(Json::as_str) == Some(name))
        .ok_or_else(|| format!("no case `{name}` in {lists:?}"))
}

/// Every artifact the chain references, keyed by its digest, from its canonical bytes.
fn artifact_store(fx: &Json) -> Result<BTreeMap<Digest, Vec<u8>>, String> {
    let section = at(fx, SECTION)?;
    let mut store = BTreeMap::new();
    for artifact in list_at(fx, "artifacts")?
        .iter()
        .chain(list_at(section, "artifacts")?)
    {
        let name = str_at(artifact, "name")?;
        let canonical = str_at(artifact, "canonical")?;
        let reference = str_at(artifact, "ref")?;
        let calculated = Digest::of(canonical.as_bytes());
        let calculated_reference = format!("sha256:{}", calculated.to_hex());
        expect_eq(
            &format!("{name}: ref"),
            reference,
            calculated_reference.as_str(),
        )?;
        let bytes = canonical.as_bytes().to_vec();
        if let Some(previous) = store.insert(calculated, bytes.clone()) {
            ensure(previous == bytes, || {
                format!("{name}: duplicate digest has different canonical bytes")
            })?;
        }
    }
    Ok(store)
}

fn artifacts(fx: &Json, _: &str) -> Result<(), String> {
    let section = at(fx, SECTION)?;
    let listed = list_at(section, "artifacts")?;
    ensure(!listed.is_empty(), || "`artifacts` is empty".to_owned())?;
    for artifact in list_at(fx, "artifacts")?.iter().chain(listed) {
        let name = str_at(artifact, "name")?;
        let canonical = str_at(artifact, "canonical")?;
        let written = String::from_utf8(to_canonical(&to_canon(at(artifact, "object")?)?))
            .map_err(|e| e.to_string())?;
        expect_eq(&format!("{name}: canonical"), written.as_str(), canonical)?;
        let reference = format!("sha256:{}", Digest::of(canonical.as_bytes()).to_hex());
        expect_eq(
            &format!("{name}: ref"),
            str_at(artifact, "ref")?,
            reference.as_str(),
        )?;
    }
    Ok(())
}

fn chain_seq(fx: &Json, index: &str) -> Result<(), String> {
    chain_entry(at(fx, SECTION)?, index)
}

/// A journal holding the chain's events before `seq`, each appended alone, and the writer epoch.
fn appended_before(section: &Json, seq: u64) -> Result<(MemoryJournal, StreamId, u64), String> {
    let first = list_at(section, "chain")?
        .first()
        .ok_or("the chain is empty")?;
    let stream = StreamId::parse(str_at(first, "body.stream_id")?).ok_or("bad stream_id")?;
    let mut journal = MemoryJournal::new();
    let epoch = journal.take_ownership(&stream);
    for entry in list_at(section, "chain")? {
        let at_seq = u64_at(entry, "seq")?;
        if at_seq >= seq {
            break;
        }
        let body = at(entry, "body")?;
        let head = at_seq.saturating_sub(1);
        let outcome = journal.append(
            &stream,
            head,
            epoch,
            timestamp(str_at(body, "recorded_at")?)?,
            &[&draft_json(body)?],
        );
        let AppendOutcome::Committed(rows) = outcome else {
            return Err(format!("append of chain seq {at_seq} gave {outcome:?}"));
        };
        let [row] = rows.as_slice() else {
            return Err(format!("expected one committed row, got {}", rows.len()));
        };
        expect_eq(
            &format!("chain seq {at_seq}: stored hash"),
            row.hash.to_hex().as_str(),
            str_at(entry, "hash")?,
        )?;
    }
    Ok((journal, stream, epoch))
}

fn chain_append(fx: &Json, _: &str) -> Result<(), String> {
    let section = at(fx, SECTION)?;
    let last = list_at(section, "chain")?
        .last()
        .ok_or("the chain is empty")?;
    let last_seq = u64_at(last, "seq")?;
    let (journal, stream, _) = appended_before(section, last_seq.saturating_add(1))?;
    let rows = journal.rows(&stream);
    let expected = Verified {
        next_seq: last_seq.saturating_add(1),
        last_hash: digest(str_at(last, "hash")?)?,
    };
    expect_eq(
        "per-event verification of the appended chain",
        verify_events(rows, TrustedStart::GENESIS, &artifact_store(fx)?),
        Ok(expected),
    )?;
    expect_eq(
        "agent-stream range checks of the appended chain",
        verify_agent_stream(rows, TrustedStart::GENESIS),
        Ok(()),
    )
}

/// Applies one `{path, value}` or `{path, delete: true}` change to a body; the path is dotted from
/// the envelope, and a deleted member must exist.
fn apply_change(body: &mut Json, change: &Json) -> Result<(), String> {
    let path = str_at(change, "path")?;
    let (parents, last) = match path.rsplit_once('.') {
        Some((parents, last)) => (Some(parents), last),
        None => (None, path),
    };
    let mut node = body;
    for name in parents.into_iter().flat_map(|p| p.split('.')) {
        node = node
            .get_mut(name)
            .ok_or_else(|| format!("change path `{path}` has no `{name}`"))?;
    }
    let object = node
        .as_object_mut()
        .ok_or_else(|| format!("change path `{path}` does not end in an object"))?;
    if change.get("delete").and_then(Json::as_bool) == Some(true) {
        object
            .remove(last)
            .map(|_| ())
            .ok_or_else(|| format!("change deletes `{path}`, which the base does not have"))
    } else {
        object.insert(last.to_owned(), at(change, "value")?.clone());
        Ok(())
    }
}

/// The draft a case describes: its base chain event's body with its changes, as a writer sends it.
fn changed_draft(section: &Json, case: &Json) -> Result<Vec<u8>, String> {
    let base = u64_at(case, "base_seq")?;
    let mut body = at(chain_entry_at(section, base)?, "body")?.clone();
    for change in list_at(case, "changes")? {
        apply_change(&mut body, change)?;
    }
    draft_json(&body)
}

/// Appends `drafts`, built from `members`, as one batch onto the chain before the first base.
fn append_members(section: &Json, members: &[&Json]) -> Result<AppendOutcome, String> {
    let first = members
        .iter()
        .map(|m| u64_at(m, "base_seq"))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .min()
        .ok_or("a batch with no drafts")?;
    let (mut journal, stream, epoch) = appended_before(section, first)?;
    let drafts = members
        .iter()
        .map(|m| changed_draft(section, m))
        .collect::<Result<Vec<_>, _>>()?;
    let slices: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
    Ok(journal.append(
        &stream,
        first.saturating_sub(1),
        epoch,
        timestamp(APPEND_TIME)?,
        &slices,
    ))
}

/// The outcome matches `expect`: `Valid` commits every draft; `Invalid` refuses the draft at
/// `draft_index` (0 for a single draft) with the reason code and path.
fn check_outcome(outcome: &AppendOutcome, expect: &Json, drafts: usize) -> Result<(), String> {
    match str_at(expect, "outcome")? {
        "Valid" => match outcome {
            AppendOutcome::Committed(rows) => expect_eq("rows committed", rows.len(), drafts),
            other => Err(format!("expected Valid, got {other:?}")),
        },
        "Invalid" => {
            let index = match expect.get("draft_index") {
                Some(i) => i.as_u64().ok_or("`draft_index` is not an integer")?,
                None => 0,
            };
            let want = (
                index,
                str_at(expect, "reason")?.to_owned(),
                str_at(expect, "path")?.to_owned(),
            );
            match outcome {
                AppendOutcome::Invalid { draft, error } => expect_eq(
                    "refusal (draft, reason, path)",
                    (
                        u64::try_from(*draft).map_err(|e| e.to_string())?,
                        error.reason.code().to_owned(),
                        error.path.clone(),
                    ),
                    want,
                ),
                other => Err(format!("expected Invalid {want:?}, got {other:?}")),
            }
        }
        other => Err(format!("unknown expected outcome `{other}`")),
    }
}

fn single(fx: &Json, list: &str, name: &str, outcome: &str) -> Result<(), String> {
    let section = at(fx, SECTION)?;
    let case = named(section, &[list], name)?;
    let expect = at(case, "expect")?;
    expect_eq("expected outcome", str_at(expect, "outcome")?, outcome)?;
    let got = append_members(section, &[case])?;
    check_outcome(&got, expect, 1)
}

fn invalid(fx: &Json, name: &str) -> Result<(), String> {
    single(fx, "invalid_drafts", name, "Invalid")
}

fn valid(fx: &Json, name: &str) -> Result<(), String> {
    single(fx, "valid_drafts", name, "Valid")
}

fn batch(fx: &Json, name: &str) -> Result<(), String> {
    let section = at(fx, SECTION)?;
    let case = named(section, &["valid_batches", "invalid_batches"], name)?;
    let members: Vec<&Json> = list_at(case, "drafts")?.iter().collect();
    ensure(members.len() > 1, || {
        format!("batch `{name}` has one draft")
    })?;
    let got = append_members(section, &members)?;
    check_outcome(&got, at(case, "expect")?, members.len())
}

/// The chain with the case's changes applied, re-chained from the genesis hash as stored rows.
fn tampered_rows(section: &Json, case: &Json) -> Result<Vec<StoredEvent>, String> {
    let mut bodies = list_at(section, "chain")?
        .iter()
        .map(|e| at(e, "body").cloned())
        .collect::<Result<Vec<_>, _>>()?;
    for change in list_at(case, "changes")? {
        let seq = u64_at(change, "seq")?;
        let body = bodies
            .iter_mut()
            .find(|b| b.get("seq").and_then(Json::as_u64) == Some(seq))
            .ok_or_else(|| format!("a change names seq {seq}, which the chain lacks"))?;
        apply_change(body, change)?;
    }
    let mut prev_hash = Digest::ZERO;
    let mut rows = Vec::with_capacity(bodies.len());
    for mut body in bodies {
        let object = body
            .as_object_mut()
            .ok_or("a chain body is not an object")?;
        object.insert("prev_hash".to_owned(), Json::String(prev_hash.to_hex()));
        let canonical = to_canonical(&to_canon(&body)?);
        let hash = Digest::of(&canonical);
        rows.push(StoredEvent {
            stream_id: str_at(&body, "stream_id")?.to_owned(),
            seq: u64_at(&body, "seq")?,
            event_id: str_at(&body, "event_id")?.to_owned(),
            event_type: str_at(&body, "event_type")?.to_owned(),
            schema_version: u64_at(&body, "schema_version")?,
            environment: str_at(&body, "environment")?.to_owned(),
            recorded_at: str_at(&body, "recorded_at")?.to_owned(),
            prev_hash,
            hash,
            body: canonical,
        });
        prev_hash = hash;
    }
    Ok(rows)
}

fn range(fx: &Json, name: &str) -> Result<(), String> {
    let section = at(fx, SECTION)?;
    let case = named(section, &["range_verification"], name)?;
    let rows = tampered_rows(section, case)?;
    let from_seq = u64_at(case, "from_seq")?;
    let prev_hash = match from_seq.checked_sub(1) {
        None | Some(0) => Digest::ZERO,
        Some(before) => {
            rows.iter()
                .find(|r| r.seq == before)
                .ok_or_else(|| format!("no seq {before} before the range"))?
                .hash
        }
    };
    let start = TrustedStart {
        from_seq,
        prev_hash,
    };
    let range: Vec<StoredEvent> = rows.into_iter().filter(|r| r.seq >= from_seq).collect();
    let checked = verify_events(&range, start, &artifact_store(fx)?);
    ensure(checked.is_ok(), || {
        format!("the tampered range fails a per-event check: {checked:?}")
    })?;
    let expect = at(case, "expect")?;
    let want = (u64_at(expect, "seq")?, str_at(expect, "code")?.to_owned());
    match verify_agent_stream(&range, start) {
        Err(failure) => expect_eq(
            "range failure (seq, code)",
            (failure.seq, failure.check.code().to_owned()),
            want,
        ),
        Ok(()) => Err(format!("expected {want:?}, but the range verified")),
    }
}

/// The harness's own mechanics, live while the agent stream's checks are stubs (E7-9): driven
/// through the version-3 account chain as a stand-in section, which `mandate-journal` already
/// appends and verifies, and through the real section where no journal check is involved. Every
/// assertion holds as well once `verify_agent_stream` is implemented, since a stand-in range only
/// asserts that the per-event checks passed.
#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use mandate_canon::Digest;
    use serde_json::json;

    use super::{
        appended_before, artifact_store, artifacts, batch, cases, chain_append, chain_seq,
        event_type, invalid, named, range, tampered_rows, valid,
    };
    use crate::{Json, read_fixture};

    fn fixture() -> Json {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases");
        read_fixture(&dir, "journal.json")
            .map(|f| (*f).clone())
            .unwrap_or_default()
    }

    fn get<'a>(value: &'a Json, pointer: &str) -> &'a Json {
        value.pointer(pointer).unwrap_or(&Json::Null)
    }

    /// Replaces the member at `pointer`, which must exist.
    fn set(value: &mut Json, pointer: &str, to: Json) {
        let slot = value.pointer_mut(pointer);
        assert!(slot.is_some(), "no {pointer}");
        if let Some(slot) = slot {
            *slot = to;
        }
    }

    /// The fixture with `agent_stream` replaced by the account chain and these case lists.
    fn stand_in(mut lists: Json) -> Json {
        let mut fx = fixture();
        let chain = get(&fx, "/chain").clone();
        let artifacts = get(&fx, "/agent_stream/artifacts").clone();
        if let Some(section) = lists.as_object_mut() {
            section.insert("chain".to_owned(), chain);
            section.insert("artifacts".to_owned(), artifacts);
        }
        set(&mut fx, "/agent_stream", lists);
        fx
    }

    fn draft(name: &str, changes: Json, expect: Json) -> Json {
        json!({"name": name, "base_seq": 2, "changes": changes, "expect": expect})
    }

    #[test]
    fn every_case_of_the_section_is_registered_under_its_family() {
        let ids: Vec<String> = cases(&Arc::new(fixture()))
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(ids.len(), 106);
        for id in [
            "journal::agent_stream::artifacts",
            "journal::agent_stream::StreamOpened::chain::seq_1",
            "journal::agent_stream::DecisionMade::chain::seq_8",
            "journal::agent_stream::chain::append",
            "journal::agent_stream::OwnerExitRequested::invalid::owner_exit_floor_absent",
            "journal::agent_stream::AgentModeChanged::valid::mode_owner_pause",
            "journal::agent_stream::batch::decision_and_its_intent",
            "journal::agent_stream::range::intent_repeats_another_action",
        ] {
            assert!(ids.iter().any(|i| i == id), "{id}");
        }
        let fx = fixture();
        let section = get(&fx, "/agent_stream");
        assert_eq!(event_type(section, 4), Ok("DecisionMade"));
        assert_eq!(
            event_type(section, 99),
            Err("the chain has no seq 99".to_owned())
        );
    }

    #[test]
    fn a_case_is_found_in_any_list_it_names_and_only_there() {
        let fx = fixture();
        let section = get(&fx, "/agent_stream");
        let name = "intent_limit_differs_from_its_decision";
        let found = named(section, &["valid_batches", "invalid_batches"], name);
        assert_eq!(found.map(|c| get(c, "/name").clone()), Ok(json!(name)));
        assert_eq!(
            named(section, &["valid_batches"], name).err(),
            Some(format!("no case `{name}` in [\"valid_batches\"]"))
        );
    }

    #[test]
    fn the_artifacts_are_their_canonical_bytes_and_their_references() {
        let fx = fixture();
        assert_eq!(artifacts(&fx, ""), Ok(()));
        let store = artifact_store(&fx).unwrap_or_default();
        let refs: Vec<String> = store
            .keys()
            .map(|d| format!("sha256:{}", d.to_hex()))
            .collect();
        let root_refs: Vec<String> = get(&fx, "/artifacts")
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| get(a, "/ref").as_str().map(str::to_owned))
            .collect();
        let mut listed: Vec<String> = get(&fx, "/agent_stream/artifacts")
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| get(a, "/ref").as_str().map(str::to_owned))
            .collect();
        listed.extend(root_refs);
        let first = listed.first().cloned().unwrap_or_default();
        listed.sort();
        assert_eq!(refs, listed);
        assert!(!store.is_empty());
        assert!(store.iter().all(|(d, bytes)| Digest::of(bytes) == *d));

        let mut wrong_root_bytes = fx.clone();
        set(&mut wrong_root_bytes, "/artifacts/3/canonical", json!("{}"));
        let wrong_root_error = artifact_store(&wrong_root_bytes).err().unwrap_or_default();
        assert!(
            wrong_root_error.starts_with("mandate_version: ref: expected"),
            "{wrong_root_error}"
        );

        let zeros = format!("sha256:{}", "0".repeat(64));
        let mut edited = fx.clone();
        set(&mut edited, "/agent_stream/artifacts/0/ref", json!(zeros));
        assert_eq!(
            artifacts(&edited, ""),
            Err(format!(
                "quote_snapshot: ref: expected {first:?}, got {zeros:?}"
            ))
        );
        set(&mut edited, "/agent_stream/artifacts", json!([]));
        assert_eq!(
            artifacts(&edited, ""),
            Err("`artifacts` is empty".to_owned())
        );
    }

    #[test]
    fn a_chain_event_reproduces_its_bytes_and_a_wrong_hash_fails() {
        let fx = stand_in(json!({}));
        assert_eq!(chain_seq(&fx, "1"), Ok(()));
        let mut edited = fx.clone();
        set(
            &mut edited,
            "/agent_stream/chain/1/hash",
            json!("0".repeat(64)),
        );
        assert!(chain_seq(&edited, "1").is_err());

        let rows = appended_before(get(&fx, "/agent_stream"), 3)
            .map(|(journal, stream, _)| journal.rows(&stream).len());
        assert_eq!(rows, Ok(2));

        let mut broken = fx.clone();
        set(
            &mut broken,
            "/agent_stream/chain/4/hash",
            json!("0".repeat(64)),
        );
        let failed = chain_append(&broken, "").err().unwrap_or_default();
        assert!(failed.contains("chain seq 5: stored hash"), "{failed}");
    }

    #[test]
    fn a_draft_passes_only_with_the_outcome_reason_and_path_it_gets() {
        let bad_qty = json!([{"path": "payload.qty", "value": "1.5.0"}]);
        let fx = stand_in(json!({
            "invalid_drafts": [
                draft("bad_qty", bad_qty.clone(),
                    json!({"outcome": "Invalid", "reason": "non_canonical", "path": "payload.qty"})),
                draft("bad_qty_wrong_path", bad_qty.clone(),
                    json!({"outcome": "Invalid", "reason": "non_canonical", "path": "payload.side"})),
                draft("qty_deleted", json!([{"path": "payload.qty", "delete": true}]),
                    json!({"outcome": "Invalid", "reason": "schema", "path": "payload.qty"})),
            ],
            "valid_drafts": [
                draft("as_written", json!([]), json!({"outcome": "Valid"})),
                draft("refused", bad_qty, json!({"outcome": "Valid"})),
            ],
        }));
        assert_eq!(invalid(&fx, "bad_qty"), Ok(()));
        assert_eq!(invalid(&fx, "qty_deleted"), Ok(()));
        assert_eq!(
            invalid(&fx, "bad_qty_wrong_path"),
            Err(
                "refusal (draft, reason, path): expected (0, \"non_canonical\", \"payload.side\"), \
                 got (0, \"non_canonical\", \"payload.qty\")"
                    .to_owned()
            )
        );
        assert_eq!(valid(&fx, "as_written"), Ok(()));
        let refused = valid(&fx, "refused").err().unwrap_or_default();
        assert!(
            refused.starts_with("expected Valid, got Invalid"),
            "{refused}"
        );
        assert_eq!(
            valid(&fx, "bad_qty"),
            Err("no case `bad_qty` in [\"valid_drafts\"]".to_owned())
        );
    }

    #[test]
    fn a_batch_commits_or_is_refused_at_its_draft_index() {
        let member = |base: u64, changes: Json| json!({"base_seq": base, "changes": changes});
        let no_verdict = json!([{"path": "payload.verdict", "delete": true}]);
        let refused_at = |index: u64| json!({"outcome": "Invalid", "reason": "schema", "path": "payload.verdict", "draft_index": index});
        let fx = stand_in(json!({
            "valid_batches": [
                {"name": "two", "drafts": [member(2, json!([])), member(3, json!([]))],
                 "expect": {"outcome": "Valid"}},
                {"name": "one", "drafts": [member(2, json!([]))], "expect": {"outcome": "Valid"}},
            ],
            "invalid_batches": [
                {"name": "second_refused", "drafts": [member(2, json!([])), member(3, no_verdict.clone())],
                 "expect": refused_at(1)},
                {"name": "wrong_index", "drafts": [member(2, json!([])), member(3, no_verdict)],
                 "expect": refused_at(0)},
            ],
        }));
        assert_eq!(batch(&fx, "two"), Ok(()));
        assert_eq!(batch(&fx, "second_refused"), Ok(()));
        assert_eq!(
            batch(&fx, "one"),
            Err("batch `one` has one draft".to_owned())
        );
        assert_eq!(
            batch(&fx, "wrong_index"),
            Err(
                "refusal (draft, reason, path): expected (0, \"schema\", \"payload.verdict\"), \
                 got (1, \"schema\", \"payload.verdict\")"
                    .to_owned()
            )
        );
    }

    #[test]
    fn a_tampered_range_is_rechained_and_passes_every_per_event_check() {
        let case = |from_seq: u64| {
            json!({"name": format!("from_{from_seq}"), "from_seq": from_seq,
                   "changes": [{"seq": 2, "path": "payload.qty", "value": "3"}],
                   "expect": {"code": "intent_action_mismatch", "seq": 4}})
        };
        let fx = stand_in(json!({"range_verification": [case(1), case(3)]}));
        let rows = tampered_rows(get(&fx, "/agent_stream"), &case(1)).unwrap_or_default();
        let bodies: Vec<String> = rows
            .iter()
            .map(|r| String::from_utf8_lossy(&r.body).into_owned())
            .collect();
        assert_eq!(bodies.len(), 5);
        assert!(bodies.get(1).is_some_and(|b| b.contains("\"qty\":\"3\"")));
        assert!(bodies.first().is_some_and(|b| !b.contains("\"qty\"")));
        assert!(
            rows.windows(2)
                .all(|w| matches!(w, [a, b] if b.prev_hash == a.hash))
        );
        let mut missing_configs = fx.clone();
        set(&mut missing_configs, "/artifacts", json!([]));
        let missing_error = range(&missing_configs, "from_1").err().unwrap_or_default();
        assert!(missing_error.contains("ArtifactMissing"), "{missing_error}");
        for name in ["from_1", "from_3"] {
            let failed = range(&fx, name).err().unwrap_or_default();
            assert!(
                !failed.is_empty() && !failed.contains("per-event"),
                "{name}: {failed}"
            );
        }
    }
}
