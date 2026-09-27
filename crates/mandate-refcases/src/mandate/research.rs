//! Family N of the `mandate` suite, run through `mandate-research` (DEC-132, the brief's
//! "case-loading design"). This slice interprets `thesis_expiry` (§8.6, DEC-118) and `stagger`
//! (§8.4, DEC-100); `admission` and `lineage` still fail naming E17-3 until the stacked slices that
//! interpret them.
//!
//! **Every key is read (DEC-85).** Each object a case carries — `input`, an expiry entry, and a
//! lineage's state — is swept against the members this module reads, and a member it does not know
//! fails the case naming it. A journal row is compared whole, as a JSON object built from the
//! crate's event, so a row that grew a member fails as a difference.

use std::collections::{BTreeMap, BTreeSet};

use mandate_research::{
    AssetId, InstrumentRestriction, Lineage, LineageId, LineageState, ResearchEvent, StaggerWindow,
    ThesisId, UniverseChange, UniverseEntry, WorkingUniverse, WorkspaceId, expire_theses,
    stagger_offset,
};
use mandate_time::UtcNanos;
use serde_json::json;

use super::{u32_of, unknown_members};
use crate::{Json, at, ensure, expect_eq, list_at, str_at};

const ENTRY_KEYS: &str = "instrument thesis_id lineage_id revision expires_at invalidated";

/// `kind: thesis_expiry` — removals at the horizon, on invalidation, and on retirement (DEC-118).
pub(super) fn thesis_expiry_case(case: &Json) -> Result<(), String> {
    let (input, expect) = (at(case, "input")?, at(case, "expect")?);
    swept(input, "now entries lineages", "input")?;
    let entries = list_at(input, "entries")?
        .iter()
        .map(|e| {
            swept(e, ENTRY_KEYS, "entry")?;
            Ok(UniverseEntry {
                instrument: asset(str_at(e, "instrument")?)?,
                thesis_id: thesis_id(str_at(e, "thesis_id")?)?,
                lineage_id: lineage_id(str_at(e, "lineage_id")?)?,
                revision: u32_of(e, "revision")?,
                expires_at: instant(str_at(e, "expires_at")?)?,
                invalidated: e
                    .get("invalidated")
                    .map_or(Ok(false), |_| bool_of(e, "invalidated"))?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let now = instant(str_at(input, "now")?)?;
    let e = expire_theses(now, &entries, &lineages(input)?)
        .map_err(|e| format!("expire_theses: {}", e.code()))?;
    same_universe(&e.universe, expect)?;
    let removed: Vec<&str> = e.removed.iter().map(AssetId::as_str).collect();
    same("removed", removed.into(), expect)?;
    let restrictions: serde_json::Map<_, _> = e
        .instrument_restrictions
        .iter()
        .map(|(a, r)| {
            let code = match r {
                InstrumentRestriction::RemovedInstrument => "removed_instrument",
                InstrumentRestriction::StaleMark => "stale_mark",
            };
            (a.as_str().to_owned(), code.into())
        })
        .collect();
    same("instrument_restrictions", restrictions.into(), expect)?;
    journal(&e.journal, list_at(expect, "journal")?)
}

/// `kind: stagger` — §8.4's deterministic offset for each (workspace, thesis) pair (DEC-100).
pub(super) fn stagger_case(case: &Json) -> Result<(), String> {
    let (input, expect) = (at(case, "input")?, at(case, "expect")?);
    swept(input, "window_s pairs", "input")?;
    let window = u32_of(input, "window_s")?;
    let offsets = list_at(input, "pairs")?
        .iter()
        .map(|pair| {
            let (Some(ws), Some(th), None) = (
                pair.get(0).and_then(Json::as_str),
                pair.get(1).and_then(Json::as_str),
                pair.get(2),
            ) else {
                return Err(format!("a pair is [workspace, thesis], got {pair}"));
            };
            let ws = WorkspaceId::new(ws).map_err(|e| e.code().to_owned())?;
            stagger_offset(&ws, &thesis_id(th)?, StaggerWindow(window))
                .map_err(|e| format!("stagger_offset: {}", e.code()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    same("offsets", offsets.into(), expect)?;
    same("window_s", window.into(), expect)
}

/// `got` against the expectation's `key`, compared as JSON.
fn same(key: &str, got: Json, expect: &Json) -> Result<(), String> {
    expect_eq(key, got, at(expect, key)?.clone())
}

/// The member-set sweep over space-separated names, with the object named in the failure.
fn swept(value: &Json, known: &str, what: &str) -> Result<(), String> {
    let known: Vec<&str> = known.split_whitespace().collect();
    unknown_members(value, &known)
        .map_err(|unknown| format!("`{what}` members not interpreted: {unknown}"))
}

/// The folded lineage state an input supplies. `ref.py`'s fold also reads an input
/// `lineage_instruments`, but no case states one, so the sweep rejects it rather than this module
/// carrying a reading nothing exercises; every lineage therefore starts holding nothing.
fn lineages(input: &Json) -> Result<LineageState, String> {
    let mut folded = BTreeMap::new();
    let listed = at(input, "lineages")?.as_object();
    for (id, state) in listed.ok_or("`lineages` is an object")? {
        swept(state, "revisions admitted retired", id)?;
        let lineage = Lineage {
            revisions: u32_of(state, "revisions")?,
            admitted: u32_of(state, "admitted")?,
            retired: bool_of(state, "retired")?,
        };
        folded.insert(lineage_id(id)?, lineage);
    }
    Ok(LineageState::from_parts(folded, BTreeMap::new()))
}

fn members(universe: &WorkingUniverse) -> Result<&BTreeSet<AssetId>, String> {
    match universe {
        WorkingUniverse::Known { instruments, .. } => Ok(instruments),
        WorkingUniverse::Unavailable => Err("the crate returned an unavailable universe".into()),
    }
}

/// The working universe compared as a set: the crate holds a `BTreeSet`, and for a refusal `ref.py`
/// hands back the input list in its authored order, so the array's order carries no meaning. A
/// repeated member in the expectation fails rather than being absorbed. Returns the size held.
fn same_universe(got: &WorkingUniverse, expect: &Json) -> Result<usize, String> {
    let want = strings(expect, "working_universe")?;
    let unique: BTreeSet<&str> = want.iter().map(String::as_str).collect();
    ensure(unique.len() == want.len(), || {
        "the expected universe repeats an instrument (MI-15)".to_owned()
    })?;
    let held = members(got)?;
    let mine: BTreeSet<&str> = held.iter().map(AssetId::as_str).collect();
    expect_eq("working_universe", mine, unique)?;
    Ok(held.len())
}

/// The journal, each event rendered as the fixture writes it and compared whole.
fn journal(events: &[ResearchEvent], expect: &[Json]) -> Result<(), String> {
    let rows = events.iter().map(row);
    let rows = rows.collect::<Result<Vec<_>, _>>()?;
    expect_eq("journal", Json::Array(rows), Json::Array(expect.to_vec()))
}

fn row(event: &ResearchEvent) -> Result<Json, String> {
    let ResearchEvent::UniverseChanged(c) = event else {
        return Err("a thesis entry where only universe changes are interpreted".to_owned());
    };
    let change = match c.change {
        UniverseChange::Admitted => "admitted",
        UniverseChange::Removed => "removed",
    };
    Ok(json!({
        "type": "UniverseChanged", "instrument": c.instrument.as_str(), "change": change,
        "reason": c.reason.code(), "thesis_id": c.thesis_id.as_str(),
        "universe_size_after": c.universe_size_after,
    }))
}

fn strings(value: &Json, key: &str) -> Result<Vec<String>, String> {
    let listed = list_at(value, key)?
        .iter()
        .map(|s| s.as_str().map(str::to_owned));
    listed
        .collect::<Option<_>>()
        .ok_or_else(|| format!("`{key}` holds a non-string"))
}

fn asset(id: &str) -> Result<AssetId, String> {
    AssetId::new(id).map_err(|e| e.to_string())
}

fn thesis_id(id: &str) -> Result<ThesisId, String> {
    ThesisId::new(id).map_err(|e| e.code().to_owned())
}

fn lineage_id(id: &str) -> Result<LineageId, String> {
    LineageId::new(id).map_err(|e| e.code().to_owned())
}

fn instant(text: &str) -> Result<UtcNanos, String> {
    UtcNanos::parse(text).map_err(|e| format!("`{text}`: {e}"))
}

fn bool_of(value: &Json, key: &str) -> Result<bool, String> {
    at(value, key)?
        .as_bool()
        .ok_or_else(|| format!("`{key}` is not a boolean"))
}

/// The harness's own oracle: a family-N case passes only because every expectation was compared.
/// Each test doctors one member of the real fixture and requires the case to fail, so a comparison
/// dropped from this module turns a doctored case green and fails here.
#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use crate::{Json, mandate, read_fixture};

    /// Input members that are data maps, whose keys are ids rather than field names.
    const DATA_MAPS: [&str; 2] = ["instrument_groups", "lineages"];
    const PLANTED: &str = "zz_planted";

    fn fixture() -> Result<Json, String> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases");
        read_fixture(&dir, "mandate.json").map(Arc::unwrap_or_clone)
    }

    /// The family-N cases whose kind this module interprets now; the rest still name their story.
    fn family_n(fixture: &Json) -> Result<Vec<String>, String> {
        let all: Vec<&Json> = crate::list_at(fixture, "cases")?
            .iter()
            .filter(|c| c["id"].as_str().is_some_and(|id| id.starts_with("MC-N")))
            .collect();
        let pending = |c: &&Json| {
            super::super::PENDING_KINDS
                .iter()
                .any(|(k, _)| c["kind"] == *k)
        };
        let ids: Vec<String> = all
            .iter()
            .filter(|c| !pending(c))
            .filter_map(|c| c["id"].as_str().map(str::to_owned))
            .collect();
        crate::ensure(all.len() == 28 && !ids.is_empty(), || {
            format!("family N holds 28 cases, {} interpreted here", ids.len())
        })?;
        Ok(ids)
    }

    fn run(fixture: Json, id: &str) -> Result<(), String> {
        let wanted = format!("mandate::{id}");
        let case = mandate::cases(&Arc::new(fixture))
            .into_iter()
            .find(|c| c.id == wanted)
            .ok_or_else(|| format!("no case {wanted}"))?;
        (case.run)()
    }

    /// The fixture with one case's member at `pointer` rewritten by `doctor`.
    fn doctored(
        fixture: &Json,
        id: &str,
        pointer: &str,
        doctor: impl FnOnce(&mut Json),
    ) -> Result<Json, String> {
        let mut copy = fixture.clone();
        let case = copy
            .get_mut("cases")
            .and_then(Json::as_array_mut)
            .and_then(|cases| cases.iter_mut().find(|c| c["id"] == id))
            .ok_or_else(|| format!("no case {id}"))?;
        doctor(
            case.pointer_mut(pointer)
                .ok_or_else(|| format!("{id} has no {pointer}"))?,
        );
        Ok(copy)
    }

    /// Every JSON pointer under `root`, with the value found there.
    fn pointers(root: &Json, at: &str, out: &mut Vec<(String, Json)>) {
        out.push((at.to_owned(), root.clone()));
        match root {
            Json::Object(members) => members
                .iter()
                .for_each(|(k, v)| pointers(v, &format!("{at}/{k}"), out)),
            Json::Array(items) => items
                .iter()
                .enumerate()
                .for_each(|(i, v)| pointers(v, &format!("{at}/{i}"), out)),
            _ => {}
        }
    }

    /// A value that differs from `value` and keeps its JSON type where it has one.
    fn changed(value: &Json) -> Json {
        match value {
            Json::Bool(b) => Json::Bool(!b),
            Json::Number(n) => Json::from(n.as_u64().unwrap_or_default().saturating_add(1)),
            Json::String(s) => Json::String(format!("{s}x")),
            Json::Null => Json::String(PLANTED.to_owned()),
            Json::Array(items) if items.is_empty() => {
                Json::Array(vec![Json::String(PLANTED.to_owned())])
            }
            Json::Array(items) => Json::Array(items.iter().skip(1).cloned().collect()),
            Json::Object(_) => Json::Null,
        }
    }

    fn must_fail(result: Result<(), String>, id: &str, what: &str) -> Result<(), String> {
        match result {
            Ok(()) => Err(format!(
                "{id}: {what} was doctored and the case still passed"
            )),
            Err(_) => Ok(()),
        }
    }

    #[test]
    fn every_interpreted_case_passes() -> Result<(), String> {
        let fixture = fixture()?;
        for id in family_n(&fixture)? {
            run(fixture.clone(), &id).map_err(|e| format!("{id}: {e}"))?;
        }
        Ok(())
    }

    /// Every member of every expectation, at every depth, is compared: changing a leaf, shortening
    /// a list, emptying an object, dropping a member, or adding one fails the case.
    #[test]
    fn every_expected_member_is_compared() -> Result<(), String> {
        let fixture = fixture()?;
        let mut doctorings = 0_usize;
        let ids = family_n(&fixture)?;
        let cases = ids.len();
        for id in ids {
            let expect = run_expect(&fixture, &id)?;
            let mut all = Vec::new();
            pointers(&expect, "/expect", &mut all);
            for (pointer, value) in all.iter().skip(1) {
                let replaced = doctored(&fixture, &id, pointer, |v| *v = changed(value))?;
                must_fail(run(replaced, &id), &id, pointer)?;
                let (parent, key) = pointer.rsplit_once('/').ok_or("a pointer has a parent")?;
                let removed = doctored(&fixture, &id, parent, |p| {
                    if let Some(members) = p.as_object_mut() {
                        members.remove(key);
                    } else if let Some(items) = p.as_array_mut() {
                        key.parse::<usize>()
                            .ok()
                            .filter(|i| *i < items.len())
                            .map(|i| items.remove(i));
                    }
                })?;
                must_fail(run(removed, &id), &id, &format!("{pointer} (removed)"))?;
                if value.is_object() {
                    let grown = doctored(&fixture, &id, pointer, |v| {
                        if let Some(members) = v.as_object_mut() {
                            members.insert(PLANTED.to_owned(), Json::Null);
                        }
                    })?;
                    must_fail(run(grown, &id), &id, &format!("{pointer}/{PLANTED}"))?;
                }
                doctorings = doctorings.saturating_add(2);
            }
        }
        crate::ensure(doctorings > cases, || {
            format!("{doctorings} doctorings for {cases} cases")
        })
    }

    /// Every field object a case's `input` holds rejects a member the harness does not read, and the
    /// failure names it (DEC-85). Data maps are keyed by ids, so their members are data, not fields;
    /// the objects inside them are swept.
    #[test]
    fn every_input_object_rejects_an_unknown_member() -> Result<(), String> {
        let fixture = fixture()?;
        let mut swept = 0_usize;
        let ids = family_n(&fixture)?;
        let cases = ids.len();
        for id in ids {
            let case = crate::list_at(&fixture, "cases")?
                .iter()
                .find(|c| c["id"] == id.as_str())
                .ok_or("the case")?;
            let mut all = Vec::new();
            pointers(crate::at(case, "input")?, "/input", &mut all);
            for (pointer, _) in all.iter().filter(|(p, v)| {
                v.is_object() && !DATA_MAPS.iter().any(|m| p.ends_with(&format!("/{m}")))
            }) {
                let planted = doctored(&fixture, &id, pointer, |v| {
                    if let Some(members) = v.as_object_mut() {
                        members.insert(PLANTED.to_owned(), Json::Null);
                    }
                })?;
                match run(planted, &id) {
                    Err(e) if e.contains(PLANTED) => swept = swept.saturating_add(1),
                    other => return Err(format!("{id}{pointer}: an unread member gave {other:?}")),
                }
            }
        }
        crate::ensure(swept >= cases, || {
            format!("{swept} input objects swept for {cases} cases")
        })
    }

    fn run_expect(fixture: &Json, id: &str) -> Result<Json, String> {
        crate::list_at(fixture, "cases")?
            .iter()
            .find(|c| c["id"] == id)
            .and_then(|c| c.get("expect"))
            .cloned()
            .ok_or_else(|| format!("{id} has no expect"))
    }

    /// A repeated instrument fails on either side rather than collapsing into the crate's set
    /// (MI-15), so a duplicate can never make two lists compare equal.
    #[test]
    fn a_repeated_instrument_fails_on_either_side() -> Result<(), String> {
        let fixture = fixture()?;
        let pairs = [("MC-N22", "/expect/working_universe")];
        for (id, pointer) in pairs {
            let repeated = doctored(&fixture, id, pointer, |v| {
                if let Some(items) = v.as_array_mut() {
                    let first = items.first().cloned().unwrap_or_default();
                    items.push(first);
                }
            })?;
            match run(repeated, id) {
                Err(e) if e.contains("MI-15") => {}
                other => return Err(format!("{id}{pointer}: a repeat gave {other:?}")),
            }
        }
        Ok(())
    }
}
