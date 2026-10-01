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
fn artifact_store(section: &Json) -> Result<BTreeMap<Digest, Vec<u8>>, String> {
    list_at(section, "artifacts")?
        .iter()
        .map(|a| {
            let canonical = str_at(a, "canonical")?;
            Ok((
                Digest::of(canonical.as_bytes()),
                canonical.as_bytes().to_vec(),
            ))
        })
        .collect()
}

fn artifacts(fx: &Json, _: &str) -> Result<(), String> {
    let section = at(fx, SECTION)?;
    let listed = list_at(section, "artifacts")?;
    ensure(!listed.is_empty(), || "`artifacts` is empty".to_owned())?;
    for artifact in listed {
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
        verify_events(rows, TrustedStart::GENESIS, &artifact_store(section)?),
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
    let prev_hash = match from_seq.checked_sub(1).filter(|s| *s > 0) {
        None => Digest::ZERO,
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
    let checked = verify_events(&range, start, &artifact_store(section)?);
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
