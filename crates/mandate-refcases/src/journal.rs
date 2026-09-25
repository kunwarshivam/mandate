//! Journal test vectors (`docs/specs/reference-cases/journal.yaml`, version 2). Tamper and append
//! cases are written in prose; each has one interpretation below, and a case fails if its prose
//! changes, so a changed vector is re-read rather than silently passed.

use std::collections::BTreeMap;
use std::sync::Arc;

use mandate_canon::{DecStr, Digest, to_canonical};
use mandate_journal::{
    Anchor, AnchorLeaf, AppendOutcome, Draft, MemoryJournal, StoredEvent, StreamId, TrustedStart,
    Verified, export_line, merkle_root, seal, tsa_imprint, verify_anchor, verify_events,
};
use mandate_time::UtcNanos;
use serde_json::json;

use crate::{Case, Json, at, ensure, expect_eq, list_at, str_at, to_canon, u64_at};

const FIXTURE_VERSION: u64 = 2;
/// `recorded_at` for appends the vectors do not time.
const APPEND_TIME: &str = "2026-09-21T14:00:02.000000000Z";
/// Event IDs for the new drafts the append cases call F and "new GateDecided draft".
const DRAFT_F_ID: &str = "01J8Z3M4F0000000000000000F";
const NEW_GATE_ID: &str = "01J8Z3M4G0000000000000000G";
/// A hash that is not in the chain, for rewritten `prev_hash` values.
const FOREIGN_HASH: &str = "abababababababababababababababababababababababababababababababab";

/// A case check: the fixture and the case's argument (an index or a name).
type Check = fn(&Json, &str) -> Result<(), String>;

pub fn cases(fixture: &Arc<Json>) -> Vec<Case> {
    let mut out = Vec::new();
    let mut add = |id: String, run: Check, arg: String| {
        let fx = Arc::clone(fixture);
        out.push(Case::new(id, move || run(&fx, &arg)));
    };
    add("journal::version".into(), version, String::new());
    let decimals: [(&str, Check); 2] = [("accept", decimal_accept), ("reject", decimal_reject)];
    for (section, run) in decimals {
        let path = format!("decimal_normalization.{section}");
        for (i, item) in list_at(fixture, &path)
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let input = item.get("input").unwrap_or(item);
            let id = match input.as_str() {
                Some(text) => format!("journal::decimal::{section}::{text:?}"),
                None => format!("journal::decimal::{section}::#{i}"),
            };
            add(id, run, i.to_string());
        }
    }
    add(
        "journal::string_escaping".into(),
        string_escaping,
        String::new(),
    );
    for (i, entry) in list_at(fixture, "chain")
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let seq = entry.get("seq").and_then(Json::as_u64).unwrap_or_default();
        add(
            format!("journal::chain::seq_{seq}"),
            chain_entry,
            i.to_string(),
        );
    }
    add("journal::chain::append".into(), chain_append, String::new());
    add(
        "journal::export_line_seq_1".into(),
        export_line_seq_1,
        String::new(),
    );
    add("journal::merkle".into(), merkle, String::new());
    let named: [(&str, &str, Check); 2] = [
        ("tamper_cases", "tamper", tamper),
        ("append_cases.cases", "append", append),
    ];
    for (section, prefix, run) in named {
        for case in list_at(fixture, section).unwrap_or_default() {
            let name = case
                .get("name")
                .and_then(Json::as_str)
                .unwrap_or("<unnamed>");
            add(format!("journal::{prefix}::{name}"), run, name.to_owned());
        }
    }
    out
}

fn version(fx: &Json, _: &str) -> Result<(), String> {
    expect_eq("fixture version", u64_at(fx, "version")?, FIXTURE_VERSION)?;
    expect_eq("hash algorithm", str_at(fx, "hash_algorithm")?, "sha256")?;
    expect_eq(
        "genesis prev_hash",
        str_at(fx, "genesis_prev_hash")?,
        Digest::ZERO.to_hex().as_str(),
    )?;
    for section in [
        "decimal_normalization.accept",
        "decimal_normalization.reject",
        "chain",
        "tamper_cases",
        "append_cases.cases",
        "merkle.leaves",
    ] {
        ensure(!list_at(fx, section)?.is_empty(), || {
            format!("`{section}` is empty")
        })?;
    }
    Ok(())
}

fn nth<'a>(fx: &'a Json, path: &str, index: &str) -> Result<&'a Json, String> {
    let i: usize = index.parse().map_err(|_| format!("bad index {index}"))?;
    list_at(fx, path)?
        .get(i)
        .ok_or_else(|| format!("`{path}` has no item {i}"))
}

fn decimal_accept(fx: &Json, index: &str) -> Result<(), String> {
    let item = nth(fx, "decimal_normalization.accept", index)?;
    let input = str_at(item, "input")?;
    let canonical = str_at(item, "canonical")?;
    let got = DecStr::parse(input).map(|d| d.as_str().to_owned());
    expect_eq(
        &format!("normalize({input:?})"),
        got,
        Ok(canonical.to_owned()),
    )
}

fn decimal_reject(fx: &Json, index: &str) -> Result<(), String> {
    let item = nth(fx, "decimal_normalization.reject", index)?;
    let input = item.as_str().ok_or("reject item is not a string")?;
    let got = DecStr::parse(input);
    ensure(got.is_err(), || {
        format!("normalize({input:?}) accepted as {got:?}")
    })
}

fn string_escaping(fx: &Json, _: &str) -> Result<(), String> {
    let bytes = to_canonical(&to_canon(at(fx, "string_escaping.object")?)?);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    expect_eq(
        "canonical bytes",
        hex.as_str(),
        str_at(fx, "string_escaping.canonical_utf8_hex")?,
    )?;
    expect_eq(
        "hash",
        Digest::of(&bytes).to_hex().as_str(),
        str_at(fx, "string_escaping.hash")?,
    )
}

fn digest(hex: &str) -> Result<Digest, String> {
    Digest::from_hex(hex).ok_or_else(|| format!("`{hex}` is not a digest"))
}

fn timestamp(text: &str) -> Result<UtcNanos, String> {
    UtcNanos::parse(text).map_err(|e| format!("`{text}`: {e}"))
}

/// A draft as a writer would send it: the body without the journal-assigned fields, serialized by
/// `serde_json` (not canonical), so the journal's own parsing and normalization are exercised.
fn draft_json(body: &Json) -> Result<Vec<u8>, String> {
    let mut draft = body.as_object().cloned().ok_or("body is not an object")?;
    for field in ["seq", "prev_hash", "recorded_at"] {
        draft
            .remove(field)
            .ok_or_else(|| format!("body has no `{field}`"))?;
    }
    serde_json::to_vec(&draft).map_err(|e| e.to_string())
}

fn chain_entry(fx: &Json, index: &str) -> Result<(), String> {
    let entry = nth(fx, "chain", index)?;
    let body = at(entry, "body")?;
    let canonical = str_at(entry, "canonical")?;
    let hash = str_at(entry, "hash")?;
    let seq = u64_at(entry, "seq")?;
    expect_eq("body seq", u64_at(body, "seq")?, seq)?;
    expect_eq(
        "event_type",
        str_at(body, "event_type")?,
        str_at(entry, "event_type")?,
    )?;

    let written = String::from_utf8(to_canonical(&to_canon(body)?)).map_err(|e| e.to_string())?;
    expect_eq("canonical body", written.as_str(), canonical)?;
    expect_eq(
        "hash",
        Digest::of(canonical.as_bytes()).to_hex().as_str(),
        hash,
    )?;

    let prev = match index.parse::<usize>().ok().and_then(|i| i.checked_sub(1)) {
        None => Digest::ZERO.to_hex(),
        Some(p) => str_at(nth(fx, "chain", &p.to_string())?, "hash")?.to_owned(),
    };
    expect_eq(
        "prev_hash chains",
        str_at(body, "prev_hash")?,
        prev.as_str(),
    )?;

    let draft = Draft::parse(&draft_json(body)?).map_err(|e| format!("draft rejected: {e}"))?;
    let row = seal(
        &draft,
        seq,
        digest(&prev)?,
        timestamp(str_at(body, "recorded_at")?)?,
    )
    .map_err(|e| format!("seal failed: {e}"))?;
    expect_eq(
        "sealed body",
        String::from_utf8_lossy(&row.body).as_ref(),
        canonical,
    )?;
    expect_eq("sealed hash", row.hash.to_hex().as_str(), hash)
}

/// Appends the chain through the append protocol, one draft per append, starting from an empty
/// stream with writer epoch 1.
fn journal_with_chain(fx: &Json) -> Result<(MemoryJournal, StreamId), String> {
    let chain = list_at(fx, "chain")?;
    let first = chain.first().ok_or("chain is empty")?;
    let stream = StreamId::parse(str_at(first, "body.stream_id")?).ok_or("bad stream_id")?;
    let mut journal = MemoryJournal::new();
    let epoch = journal.take_ownership(&stream);
    for (head, entry) in (0u64..).zip(chain) {
        let body = at(entry, "body")?;
        let outcome = journal.append(
            &stream,
            head,
            epoch,
            timestamp(str_at(body, "recorded_at")?)?,
            &[&draft_json(body)?],
        );
        let AppendOutcome::Committed(rows) = outcome else {
            return Err(format!(
                "append of seq {} gave {outcome:?}",
                head.saturating_add(1)
            ));
        };
        let [row] = rows.as_slice() else {
            return Err(format!("expected one committed row, got {}", rows.len()));
        };
        expect_eq(
            "stored body",
            String::from_utf8_lossy(&row.body).as_ref(),
            str_at(entry, "canonical")?,
        )?;
        expect_eq(
            "stored hash",
            row.hash.to_hex().as_str(),
            str_at(entry, "hash")?,
        )?;
    }
    Ok((journal, stream))
}

fn chain_append(fx: &Json, _: &str) -> Result<(), String> {
    let (journal, stream) = journal_with_chain(fx)?;
    let chain = list_at(fx, "chain")?;
    let last = chain.last().ok_or("chain is empty")?;
    let expected = Verified {
        next_seq: u64_at(last, "seq")?.saturating_add(1),
        last_hash: digest(str_at(last, "hash")?)?,
    };
    let verified = verify_events(
        journal.rows(&stream),
        TrustedStart::GENESIS,
        &BTreeMap::new(),
    );
    expect_eq("verification of the appended chain", verified, Ok(expected))
}

/// Rows as the Postgres store holds them, built from the vectors alone.
fn rows_from_chain(fx: &Json) -> Result<Vec<StoredEvent>, String> {
    list_at(fx, "chain")?
        .iter()
        .map(|entry| {
            let body = at(entry, "body")?;
            Ok(StoredEvent {
                stream_id: str_at(body, "stream_id")?.to_owned(),
                seq: u64_at(body, "seq")?,
                event_id: str_at(body, "event_id")?.to_owned(),
                event_type: str_at(body, "event_type")?.to_owned(),
                schema_version: u64_at(body, "schema_version")?,
                environment: str_at(body, "environment")?.to_owned(),
                recorded_at: str_at(body, "recorded_at")?.to_owned(),
                prev_hash: digest(str_at(body, "prev_hash")?)?,
                hash: digest(str_at(entry, "hash")?)?,
                body: str_at(entry, "canonical")?.as_bytes().to_vec(),
            })
        })
        .collect()
}

fn export_line_seq_1(fx: &Json, _: &str) -> Result<(), String> {
    let rows = rows_from_chain(fx)?;
    let first = rows.first().ok_or("chain is empty")?;
    expect_eq(
        "export line",
        String::from_utf8_lossy(&export_line(first)).as_ref(),
        str_at(fx, "export_line_seq_1")?,
    )
}

fn anchor_leaves(fx: &Json) -> Result<Vec<AnchorLeaf>, String> {
    list_at(fx, "merkle.leaves")?
        .iter()
        .map(|leaf| {
            Ok(AnchorLeaf {
                stream_id: str_at(leaf, "stream_id")?.to_owned(),
                seq: u64_at(leaf, "seq")?,
                hash: digest(str_at(leaf, "hash")?)?,
            })
        })
        .collect()
}

fn merkle(fx: &Json, _: &str) -> Result<(), String> {
    let leaves = anchor_leaves(fx)?;
    let expected: Vec<&str> = list_at(fx, "merkle.leaf_hashes")?
        .iter()
        .map(|h| h.as_str().unwrap_or_default())
        .collect();
    let got: Vec<String> = leaves
        .iter()
        .map(|l| l.leaf_hash().map(|d| d.to_hex()).unwrap_or_default())
        .collect();
    expect_eq(
        "leaf hashes",
        got.iter().map(String::as_str).collect::<Vec<_>>(),
        expected,
    )?;
    let root = digest(str_at(fx, "merkle.root")?)?;
    expect_eq("root", merkle_root(&leaves), Some(root))?;
    expect_eq(
        "tsa imprint",
        tsa_imprint(&root).to_hex().as_str(),
        str_at(fx, "merkle.tsa_imprint")?,
    )?;
    let mut shuffled = leaves.clone();
    shuffled.reverse();
    expect_eq(
        "anchor from unsorted heads",
        Anchor::compute(shuffled),
        Some(Anchor { leaves, root }),
    )
}

type Tamper = fn(&mut Vec<StoredEvent>) -> Result<(), String>;

/// (name, the vector's `change` text, the interpretation).
const TAMPERS: &[(&str, &str, Tamper)] = &[
    (
        "payload_modified",
        "seq 3 body: payload.verdict 'allow' -> 'deny'; stored hash unchanged",
        |rows| replace_in_body(row(rows, 3)?, r#""verdict":"allow""#, r#""verdict":"deny""#),
    ),
    ("event_deleted", "row seq 3 deleted", |rows| delete(rows, 3)),
    (
        "seq_values_swapped",
        "the seq members inside bodies 2 and 3 are swapped; rows and columns stay in seq order",
        |rows| {
            replace_in_body(row(rows, 2)?, r#""seq":2,"#, r#""seq":3,"#)?;
            replace_in_body(row(rows, 3)?, r#""seq":3,"#, r#""seq":2,"#)
        },
    ),
    (
        "prev_hash_changed_without_rehash",
        "seq 4: prev_hash replaced in body and column; stored hash unchanged",
        |rows| replace_prev_hash(row(rows, 4)?, FOREIGN_HASH),
    ),
    (
        "prev_hash_rewritten_and_rehashed",
        "seq 4: prev_hash replaced in body and column; hash recomputed",
        |rows| {
            let r = row(rows, 4)?;
            replace_prev_hash(r, FOREIGN_HASH)?;
            rehash(r);
            Ok(())
        },
    ),
    (
        "whitespace_inserted_and_rehashed",
        "seq 2: a space inserted after the opening brace of the body; hash recomputed",
        |rows| {
            let r = row(rows, 2)?;
            ensure(r.body.first() == Some(&b'{'), || {
                "body does not start with {".into()
            })?;
            r.body.insert(1, b' ');
            rehash(r);
            Ok(())
        },
    ),
    (
        "column_altered",
        "seq 5: event_type column changed to 'FillReversed'",
        |rows| {
            row(rows, 5)?.event_type = "FillReversed".into();
            Ok(())
        },
    ),
    (
        "tail_truncated_after_anchor",
        "anchor covers the account stream at seq 5 (merkle leaf 0); row seq 5 deleted and head reset to 4",
        |rows| delete(rows, 5),
    ),
    (
        "chain_rewritten_from_seq_3",
        "events 3-5 rewritten with recomputed hashes and columns",
        |rows| {
            let r3 = row(rows, 3)?;
            replace_in_body(r3, r#""verdict":"allow""#, r#""verdict":"deny""#)?;
            rehash(r3);
            let mut prev = r3.hash;
            for seq in [4, 5] {
                let r = row(rows, seq)?;
                replace_prev_hash(r, &prev.to_hex())?;
                rehash(r);
                prev = r.hash;
            }
            Ok(())
        },
    ),
];

fn row(rows: &mut [StoredEvent], seq: u64) -> Result<&mut StoredEvent, String> {
    rows.iter_mut()
        .find(|r| r.seq == seq)
        .ok_or_else(|| format!("no row with seq {seq}"))
}

fn delete(rows: &mut Vec<StoredEvent>, seq: u64) -> Result<(), String> {
    let before = rows.len();
    rows.retain(|r| r.seq != seq);
    ensure(rows.len() < before, || format!("no row with seq {seq}"))
}

fn replace_in_body(row: &mut StoredEvent, from: &str, to: &str) -> Result<(), String> {
    let text = String::from_utf8(row.body.clone()).map_err(|e| e.to_string())?;
    ensure(text.matches(from).count() == 1, || {
        format!("seq {}: `{from}` does not occur exactly once", row.seq)
    })?;
    row.body = text.replacen(from, to, 1).into_bytes();
    Ok(())
}

fn replace_prev_hash(row: &mut StoredEvent, new_hex: &str) -> Result<(), String> {
    let old = format!(r#""prev_hash":"{}""#, row.prev_hash.to_hex());
    replace_in_body(row, &old, &format!(r#""prev_hash":"{new_hex}""#))?;
    row.prev_hash = digest(new_hex)?;
    Ok(())
}

fn rehash(row: &mut StoredEvent) {
    row.hash = Digest::of(&row.body);
}

fn tamper(fx: &Json, name: &str) -> Result<(), String> {
    let case = list_at(fx, "tamper_cases")?
        .iter()
        .find(|c| c.get("name").and_then(Json::as_str) == Some(name))
        .ok_or_else(|| format!("no tamper case {name}"))?;
    let (_, change, apply) = TAMPERS.iter().find(|(n, _, _)| *n == name).ok_or_else(|| {
        format!("no interpretation for tamper case `{name}`; add one to mandate-refcases")
    })?;
    expect_eq(
        "change (the vector changed; re-read it)",
        str_at(case, "change")?,
        change,
    )?;
    let mut rows = rows_from_chain(fx)?;
    apply(&mut rows)?;
    let per_event = verify_events(&rows, TrustedStart::GENESIS, &BTreeMap::new());

    let expect = at(case, "expect")?;
    match expect.get("range_check").and_then(Json::as_str) {
        Some(range_check) => {
            expect_eq("expect.per_event", str_at(expect, "per_event")?, "pass")?;
            ensure(per_event.is_ok(), || {
                format!("per-event checks failed: {per_event:?}")
            })?;
            let anchor = Anchor {
                leaves: anchor_leaves(fx)?,
                root: digest(str_at(fx, "merkle.root")?)?,
            };
            let first = rows.first().ok_or("no rows")?;
            let stream = StreamId::parse(&first.stream_id).ok_or("bad stream_id")?;
            let got = verify_anchor(&anchor, &stream, &rows)
                .err()
                .map(|c| c.code());
            expect_eq("range check", got, Some(range_check))
        }
        None => {
            let got = per_event.err().map(|f| (f.seq, f.check.code()));
            expect_eq(
                "first failure",
                got,
                Some((u64_at(expect, "seq")?, str_at(expect, "check")?)),
            )
        }
    }
}

type Drafts = fn(&Json) -> Result<Vec<Vec<u8>>, String>;

/// (name, the vector's draft descriptions, the drafts they describe).
const APPENDS: &[(&str, &[&str], Drafts)] = &[
    (
        "identical_retry_with_stale_head",
        &["draft of seq 5 (event_id 01J8Z3M3T0000000000000000E), unchanged"],
        |fx| Ok(vec![draft_json(&seq_5_body(fx)?)?]),
    ),
    (
        "retry_with_equivalent_decimal",
        &["draft of seq 5 with payload.price '150.00'"],
        |fx| {
            Ok(vec![draft_json(&with_payload(
                seq_5_body(fx)?,
                "price",
                json!("150.00"),
            )?)?])
        },
    ),
    (
        "same_event_id_different_content",
        &["draft of seq 5 with payload.qty_gross '11'"],
        |fx| {
            Ok(vec![draft_json(&with_payload(
                seq_5_body(fx)?,
                "qty_gross",
                json!("11"),
            )?)?])
        },
    ),
    (
        "partial_overlap_batch",
        &["draft of seq 5, unchanged", "new draft F"],
        |fx| {
            Ok(vec![
                draft_json(&seq_5_body(fx)?)?,
                draft_json(&draft_f(fx)?)?,
            ])
        },
    ),
    ("stale_head", &["new draft F"], |fx| {
        Ok(vec![draft_json(&draft_f(fx)?)?])
    }),
    ("fenced_writer", &["new draft F"], |fx| {
        Ok(vec![draft_json(&draft_f(fx)?)?])
    }),
    ("committed", &["new draft F"], |fx| {
        Ok(vec![draft_json(&draft_f(fx)?)?])
    }),
    (
        "float_rejected",
        &["new draft with payload.price as the JSON number 150.0"],
        |fx| {
            let price = serde_json::Number::from_f64(150.0).ok_or("150.0 is not finite")?;
            Ok(vec![draft_json(&with_payload(
                draft_f(fx)?,
                "price",
                Json::Number(price),
            )?)?])
        },
    ),
    (
        "missing_required_config_ref",
        &["new GateDecided draft without config_refs.rule_set"],
        |fx| {
            let mut body = nth(fx, "chain", "2")?
                .get("body")
                .cloned()
                .ok_or("no body")?;
            set(&mut body, "event_id", json!(NEW_GATE_ID))?;
            body.get_mut("config_refs")
                .and_then(Json::as_object_mut)
                .and_then(|refs| refs.remove("rule_set"))
                .ok_or("seq 3 has no config_refs.rule_set")?;
            Ok(vec![draft_json(&body)?])
        },
    ),
];

fn seq_5_body(fx: &Json) -> Result<Json, String> {
    nth(fx, "chain", "4")?
        .get("body")
        .cloned()
        .ok_or_else(|| "no body".into())
}

fn set(body: &mut Json, field: &str, value: Json) -> Result<(), String> {
    body.as_object_mut()
        .ok_or("body is not an object")?
        .insert(field.to_owned(), value);
    Ok(())
}

fn with_payload(mut body: Json, field: &str, value: Json) -> Result<Json, String> {
    let payload = body.get_mut("payload").ok_or("body has no payload")?;
    ensure(payload.get(field).is_some(), || {
        format!("payload has no `{field}`")
    })?;
    set(payload, field, value)?;
    Ok(body)
}

/// Draft F: a new `MarkUpdated` in the same stream, caused by the seq 5 fill.
fn draft_f(fx: &Json) -> Result<Json, String> {
    let fill = seq_5_body(fx)?;
    let opened = nth(fx, "chain", "0")?
        .get("body")
        .cloned()
        .ok_or("no body")?;
    let mut body = fill.clone();
    for (field, value) in [
        ("event_id", json!(DRAFT_F_ID)),
        ("event_type", json!("MarkUpdated")),
        ("schema_version", json!(1)),
        ("event_time", json!(APPEND_TIME)),
        ("clock_source", json!("local")),
        (
            "causation_id",
            fill.get("event_id").cloned().ok_or("no event_id")?,
        ),
        ("actor", opened.get("actor").cloned().ok_or("no actor")?),
        ("config_refs", json!({})),
        (
            "payload",
            json!({
                "instrument_id": at(&fill, "payload.instrument_id")?,
                "price": "150.01",
                "source": "quote",
                "feed": "iex",
            }),
        ),
    ] {
        set(&mut body, field, value)?;
    }
    Ok(body)
}

fn append(fx: &Json, name: &str) -> Result<(), String> {
    let case = list_at(fx, "append_cases.cases")?
        .iter()
        .find(|c| c.get("name").and_then(Json::as_str) == Some(name))
        .ok_or_else(|| format!("no append case {name}"))?;
    let (_, descriptions, build) =
        APPENDS.iter().find(|(n, _, _)| *n == name).ok_or_else(|| {
            format!("no interpretation for append case `{name}`; add one to mandate-refcases")
        })?;
    let described: Vec<&str> = list_at(case, "request.drafts")?
        .iter()
        .map(|d| d.as_str().unwrap_or_default())
        .collect();
    expect_eq(
        "drafts (the vector changed; re-read it)",
        described.as_slice(),
        *descriptions,
    )?;

    let (mut journal, stream) = journal_with_chain(fx)?;
    let before = journal.head(&stream);
    expect_eq(
        "stream_state.head_seq",
        before.seq,
        u64_at(fx, "append_cases.stream_state.head_seq")?,
    )?;
    expect_eq(
        "stream_state.head_hash",
        before.hash.to_hex().as_str(),
        str_at(fx, "append_cases.stream_state.head_hash")?,
    )?;
    expect_eq(
        "stream_state.writer_epoch",
        before.writer_epoch,
        u64_at(fx, "append_cases.stream_state.writer_epoch")?,
    )?;

    let drafts = build(fx)?;
    let refs: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
    let outcome = journal.append(
        &stream,
        u64_at(case, "request.expected_head")?,
        u64_at(case, "request.writer_epoch")?,
        timestamp(APPEND_TIME)?,
        &refs,
    );

    let expect = at(case, "expect")?
        .as_object()
        .ok_or("expect is not an object")?;
    for (key, value) in expect {
        let want_u64 = || {
            value
                .as_u64()
                .ok_or_else(|| format!("expect.{key} is not an integer"))
        };
        let want_str = || {
            value
                .as_str()
                .ok_or_else(|| format!("expect.{key} is not a string"))
        };
        match (key.as_str(), &outcome) {
            ("outcome", _) => expect_eq("outcome", outcome.name(), want_str()?)?,
            ("returns_seq", AppendOutcome::AlreadyCommitted(rows)) => {
                expect_eq(
                    "returned seqs",
                    rows.iter().map(|r| r.seq).collect::<Vec<_>>(),
                    vec![want_u64()?],
                )?;
            }
            ("stored_seq", AppendOutcome::IdempotencyConflict { stored_seq }) => {
                expect_eq("stored_seq", *stored_seq, want_u64()?)?;
            }
            ("actual_seq", AppendOutcome::HeadMismatch { actual_seq, .. }) => {
                expect_eq("actual_seq", *actual_seq, want_u64()?)?;
            }
            ("actual_hash", AppendOutcome::HeadMismatch { actual_hash, .. }) => {
                expect_eq("actual_hash", actual_hash.to_hex().as_str(), want_str()?)?;
            }
            ("current_epoch", AppendOutcome::Fenced { current_epoch }) => {
                expect_eq("current_epoch", *current_epoch, want_u64()?)?;
            }
            ("seq", AppendOutcome::Committed(rows)) => {
                expect_eq(
                    "committed seqs",
                    rows.iter().map(|r| r.seq).collect::<Vec<_>>(),
                    vec![want_u64()?],
                )?;
            }
            ("prev_hash", AppendOutcome::Committed(rows)) => {
                let first = rows.first().ok_or("nothing committed")?;
                expect_eq("prev_hash", first.prev_hash.to_hex().as_str(), want_str()?)?;
            }
            ("reason", AppendOutcome::Invalid { error, .. }) => {
                expect_eq("reason", error.reason.code(), want_str()?)?;
            }
            _ => return Err(format!("cannot check expect.{key} against {outcome:?}")),
        }
    }

    if !matches!(outcome, AppendOutcome::Committed(_)) {
        expect_eq(
            "head after a non-committing append",
            journal.head(&stream),
            before,
        )?;
        expect_eq(
            "rows after a non-committing append",
            journal.rows(&stream).len(),
            5,
        )?;
    }
    let verified = verify_events(
        journal.rows(&stream),
        TrustedStart::GENESIS,
        &BTreeMap::new(),
    );
    ensure(verified.is_ok(), || {
        format!("journal fails verification after the append: {verified:?}")
    })
}
