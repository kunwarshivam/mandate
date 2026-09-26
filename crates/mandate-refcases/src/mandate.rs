//! The `mandate` suite: `fixtures/refcases/mandate.json`, the JSON form of
//! [the mandate reference cases](../../../docs/specs/reference-cases/mandate.yaml) (spec §11). One
//! named test per case id (`mandate::MC-S01`), 298 of them.
//!
//! Stream F owns 202: the families `schema` (S), `semantic` (V), `policy` (P), `change` (C),
//! `risk_state` (R), `risk_day` (T), and `goal` (L). The rest belong to other streams and **fail**
//! with "not interpreted until `<story>`" rather than passing quietly, the DEC-85 rule: `gate` and
//! `agent_flatten` to E6-3, `builder` and `autonomy` to E6-2, and `admission`, `lineage`,
//! `thesis_expiry`, and `stagger` to E17-3.
//!
//! The same rule holds inside an owned family. Every key of every owned case is read, and a case that
//! carries a key this harness does not know fails naming it, so no case can pass while part of it is
//! ignored.

use std::collections::BTreeSet;
use std::sync::Arc;

use mandate_canon::Value;
use mandate_spec::{Mandate, MandateVersion};

use crate::{Case, Json, at, ensure, expect_eq, list_at, str_at, to_canon, u64_at};

const SUITE: &str = "mandate";
/// The fixture version this harness reads (`version: 4`, spec v0.6).
const FIXTURE_VERSION: u64 = 4;

/// Case kinds another stream owns, with the story that will interpret them.
const PENDING_KINDS: &[(&str, &str)] = &[
    ("gate", "E6-3"),
    ("agent_flatten", "E6-3"),
    ("builder", "E6-2"),
    ("autonomy", "E6-2"),
    ("admission", "E17-3"),
    ("lineage", "E17-3"),
    ("thesis_expiry", "E17-3"),
    ("stagger", "E17-3"),
];

/// Every key an owned case may carry at its top level.
const CASE_KEYS: &[&str] = &[
    "id",
    "kind",
    "title",
    "note",
    "base",
    "patch",
    "context",
    "provenance",
    "policies",
    "initial",
    "steps",
    "state",
    "input",
    "at",
    "expect",
];

pub fn cases(fixture: &Arc<Json>) -> Vec<Case> {
    let f = Arc::clone(fixture);
    let mut out = vec![Case::new(format!("{SUITE}::version"), move || {
        expect_eq("version", u64_at(&f, "version")?, FIXTURE_VERSION)
    })];
    let f = Arc::clone(fixture);
    out.push(Case::new(format!("{SUITE}::version_vector"), move || {
        version_vector(&f)
    }));
    let listed = match list_at(fixture, "cases") {
        Ok(listed) => listed,
        Err(e) => {
            out.push(Case::new(format!("{SUITE}::cases"), move || Err(e)));
            return out;
        }
    };
    for (index, case) in listed.iter().enumerate() {
        let id = case
            .get("id")
            .and_then(Json::as_str)
            .map_or_else(|| format!("case_{index}"), str::to_owned);
        let f = Arc::clone(fixture);
        out.push(Case::new(format!("{SUITE}::{id}"), move || {
            run_listed(&f, index)
        }));
    }
    out
}

/// The canonical-form vector of spec §9.1: the base mandate's canonical bytes and its version hash.
fn version_vector(fixture: &Json) -> Result<(), String> {
    let vector = at(fixture, "version_vector")?;
    let base = str_at(vector, "base")?;
    let mandate = Mandate::parse(&base_value(fixture, base)?).map_err(|e| e.code().to_owned())?;
    let bytes = mandate.canonical_bytes().map_err(|e| e.code().to_owned())?;
    expect_eq(
        "canonical",
        String::from_utf8(bytes).map_err(|e| e.to_string())?,
        str_at(vector, "canonical")?.to_owned(),
    )?;
    let version: MandateVersion = mandate.version().map_err(|e| e.code().to_owned())?;
    expect_eq(
        "mandate_version",
        format!("sha256:{}", version.digest()),
        str_at(vector, "mandate_version")?.to_owned(),
    )
}

fn run_listed(fixture: &Json, index: usize) -> Result<(), String> {
    let case = list_at(fixture, "cases")?
        .get(index)
        .ok_or("case index out of range")?;
    let kind = str_at(case, "kind")?;
    if let Some((_, story)) = PENDING_KINDS.iter().find(|(k, _)| *k == kind) {
        return Err(format!("`{kind}` is not interpreted until {story}"));
    }
    unread_keys(case)?;
    match kind {
        "schema" => schema_case(fixture, case),
        "semantic" => semantic_case(fixture, case),
        "policy" => policy_case(fixture, case),
        "change" => change_case(fixture, case),
        "risk_state" => risk_state_case(fixture, case),
        "risk_day" => risk_day_case(case),
        "goal" => goal_case(fixture, case),
        other => Err(format!("unknown case kind `{other}`")),
    }
}

/// Fails the case for any top-level key the harness does not know, so a key added to the fixture
/// cannot be silently ignored (DEC-85).
fn unread_keys(case: &Json) -> Result<(), String> {
    let known: BTreeSet<&str> = CASE_KEYS.iter().copied().collect();
    let members = case
        .as_object()
        .ok_or_else(|| "a case must be an object".to_owned())?;
    let unknown: Vec<&str> = members
        .keys()
        .map(String::as_str)
        .filter(|k| !known.contains(k))
        .collect();
    ensure(unknown.is_empty(), || {
        format!("case keys not interpreted: {}", unknown.join(", "))
    })
}

/// The base mandate a case names, as a canonical value.
fn base_value(fixture: &Json, base: &str) -> Result<Value, String> {
    let mandate = at(at(fixture, "bases")?, base).and_then(|b| at(b, "mandate"))?;
    to_canon(mandate)
}

/// The base a case names, with its RFC 6902 patch applied.
fn patched(fixture: &Json, case: &Json) -> Result<Value, String> {
    let base = str_at(case, "base")?;
    let mut document = at(at(fixture, "bases")?, base)
        .and_then(|b| at(b, "mandate"))?
        .clone();
    for op in case
        .get("patch")
        .and_then(Json::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        apply(&mut document, op)?;
    }
    to_canon(&document)
}

/// The RFC 6902 subset the fixture uses: `replace`, `add`, and `remove`, on object members and array
/// elements. Anything else fails the case rather than being skipped.
fn apply(document: &mut Json, op: &Json) -> Result<(), String> {
    let path = str_at(op, "path")?;
    let tokens: Vec<String> = path
        .strip_prefix('/')
        .unwrap_or_default()
        .split('/')
        .map(|t| t.replace("~1", "/").replace("~0", "~"))
        .collect();
    let (last, parents) = tokens
        .split_last()
        .ok_or_else(|| format!("patch path `{path}` names nothing"))?;
    let mut node = document;
    for token in parents {
        node = child_mut(node, token)
            .ok_or_else(|| format!("patch path `{path}` does not resolve"))?;
    }
    let value = op.get("value").cloned();
    match str_at(op, "op")? {
        "replace" => replace(node, last, value.ok_or("replace needs a value")?),
        "add" => add(node, last, value.ok_or("add needs a value")?),
        "remove" => remove(node, last),
        other => Err(format!("patch op `{other}` is not one the harness applies")),
    }
    .map_err(|e| format!("{e} (at `{path}`)"))
}

fn child_mut<'a>(node: &'a mut Json, token: &str) -> Option<&'a mut Json> {
    match node {
        Json::Array(items) => token.parse::<usize>().ok().and_then(|i| items.get_mut(i)),
        other => other.get_mut(token),
    }
}

fn replace(node: &mut Json, token: &str, value: Json) -> Result<(), String> {
    match node {
        Json::Array(items) => {
            let index = token.parse::<usize>().map_err(|e| e.to_string())?;
            let slot = items.get_mut(index).ok_or("index out of range")?;
            *slot = value;
            Ok(())
        }
        Json::Object(members) => {
            ensure(members.contains_key(token), || {
                format!("`{token}` is absent, so it cannot be replaced")
            })?;
            members.insert(token.to_owned(), value);
            Ok(())
        }
        _ => Err("not a container".to_owned()),
    }
}

fn add(node: &mut Json, token: &str, value: Json) -> Result<(), String> {
    match node {
        Json::Array(items) => {
            if token == "-" {
                items.push(value);
            } else {
                let index = token.parse::<usize>().map_err(|e| e.to_string())?;
                ensure(index <= items.len(), || "index out of range".to_owned())?;
                items.insert(index, value);
            }
            Ok(())
        }
        Json::Object(members) => {
            members.insert(token.to_owned(), value);
            Ok(())
        }
        _ => Err("not a container".to_owned()),
    }
}

fn remove(node: &mut Json, token: &str) -> Result<(), String> {
    match node {
        Json::Array(items) => {
            let index = token.parse::<usize>().map_err(|e| e.to_string())?;
            ensure(index < items.len(), || "index out of range".to_owned())?;
            items.remove(index);
            Ok(())
        }
        Json::Object(members) => {
            ensure(members.remove(token).is_some(), || {
                format!("`{token}` is absent, so it cannot be removed")
            })?;
            Ok(())
        }
        _ => Err("not a container".to_owned()),
    }
}

/// `kind: schema` — the strict parse is what "passes the JSON Schema" means in Rust (ES-22).
///
/// A rejection only counts when it names a reason. Thirty of the thirty-one cases expect
/// `schema_valid: false`, so a parse that refuses everything would satisfy them **for the wrong
/// reason** — the failure mode AGENTS.md calls "tests that pass while checking nothing". An
/// `unimplemented` error therefore fails the case, which is also what makes every one of them fail on
/// this story's stubs, as a DEC-77 tests PR requires.
fn schema_case(fixture: &Json, case: &Json) -> Result<(), String> {
    let document = patched(fixture, case)?;
    let expected = at(at(case, "expect")?, "schema_valid")?
        .as_bool()
        .ok_or("`expect.schema_valid` is not a boolean")?;
    match Mandate::parse(&document) {
        Ok(_) if expected => Ok(()),
        Ok(_) => Err("the parse accepted a document the schema rejects".to_owned()),
        Err(e) if e.code() == "unimplemented" => Err(not_implemented("the mandate parser")),
        Err(e) if expected => Err(format!(
            "the parse rejected a document the schema accepts: {}",
            e.code()
        )),
        Err(_) => Ok(()),
    }
}

/// The message every owned family shares while its rule is a stub: a case must never pass because
/// nothing has been written yet (DEC-77, DEC-83).
fn not_implemented(what: &str) -> String {
    format!("{what} is not implemented yet, so this case cannot be said to pass")
}

/// `kind: semantic` — every V-code and W-code, and the four worst-case figures (§4.1, §4.2).
fn semantic_case(fixture: &Json, case: &Json) -> Result<(), String> {
    let _ = patched(fixture, case)?;
    Err(not_implemented("`mandate_spec::validate`"))
}

/// `kind: policy` — the nearest broken ancestor, per key kind (§4.3).
fn policy_case(fixture: &Json, case: &Json) -> Result<(), String> {
    let _ = patched(fixture, case)?;
    Err(not_implemented("`mandate_spec::policy::check`"))
}

/// `kind: change` — the classification, the changed paths, both version hashes, and step-up (§9.2).
fn change_case(fixture: &Json, case: &Json) -> Result<(), String> {
    let _ = (
        base_value(fixture, str_at(case, "base")?)?,
        patched(fixture, case)?,
    );
    Err(not_implemented("`mandate_spec::change::classify`"))
}

/// `kind: risk_state` — the fold of §5.2's inputs, each step's snapshot, its journal in order, and the
/// limits with breach time accumulating.
fn risk_state_case(fixture: &Json, case: &Json) -> Result<(), String> {
    let _ = patched(fixture, case)?;
    Err(not_implemented("`mandate_spec::risk::RiskState`"))
}

/// `kind: risk_day` — the risk day containing an instant and its bounds (§5.4).
fn risk_day_case(case: &Json) -> Result<(), String> {
    let _ = str_at(case, "at")?;
    Err(not_implemented("`mandate_spec::risk::risk_day`"))
}

/// `kind: goal` — goal completion for a state (§3.1).
fn goal_case(fixture: &Json, case: &Json) -> Result<(), String> {
    let _ = patched(fixture, case)?;
    Err(not_implemented("`mandate_spec::goal::status`"))
}
