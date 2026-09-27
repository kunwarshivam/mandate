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

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use mandate_canon::Value;
use mandate_domain::{AgentMode, AssetClass, AssetId, MarketSession, Side};
use mandate_num::{Price, Qty, Ratio, ShareIncrement, Usd};
use mandate_spec::document::OnComplete;
use mandate_spec::goal::{GoalInputs, GoalStatus};
use mandate_spec::policy::platform_base;
use mandate_spec::risk::{
    ApplyResult, GoalReason, Input, InstrumentRestriction, Latch, LiftReason, LimitKey, Opening,
    Outcome, RemovalReason, Restriction, RestrictionReason, RiskEvent, RiskState, SessionClock,
    Snapshot, Step, StopReason, ThenAction, TriggerReason, UniverseChange, risk_day,
};
use mandate_spec::validate::{ValidatedMandate, ValidationContext};
use mandate_spec::{DecGrammar, Mandate, MandateVersion, SchemaDec, SpecError};
use mandate_time::{Date, ExchangeCalendar, Session, UtcNanos};

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

/// The `expect` members each owned family reads. A family's set is exhaustive: an `expect` key the
/// fixture grows and this harness does not read would otherwise be silently satisfied, which is
/// planted bug 26 and what review round 2 found still uncaught.
const EXPECT_KEYS: &[(&str, &[&str])] = &[
    ("schema", &["schema_valid"]),
    ("semantic", &["violations", "warnings", "worst_case"]),
    ("policy", &["valid", "violations"]),
    (
        "change",
        &[
            "classification",
            "changed_paths",
            "old_version",
            "new_version",
            "step_up_required",
        ],
    ),
    (
        "risk_day",
        &["risk_day", "starts_at", "ends_at", "length_s"],
    ),
    ("goal", &["done", "reason", "then", "stop_reason"]),
];

/// A `risk_state` case expects per step, not once, so its keys are swept on every step's `expect`.
const STEP_EXPECT_KEYS: &[&str] = &[
    "agent_equity",
    "high_water_mark",
    "drawdown",
    "day_start_equity",
    "daily_pnl",
    "daily_pnl_fraction",
    "capital_base",
    "net_contributed",
    "size_factor",
    "restrictions",
    "instrument_restrictions",
    "agent_mode",
    "journal",
    "pending",
    "error",
];

/// Fails the case for any key the harness does not know, at the top level and inside every `expect`,
/// so a key added to the fixture cannot be silently ignored (DEC-85).
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
    })?;
    unread_expect_keys(case)?;
    unread_input_keys(case)
}

/// Every member a `risk_state` case's `initial` block may carry. `mark_max_age_s` is not among them:
/// [`Opening::mark_max_age_s`] comes from `harness_defaults`, which every case shares.
const INITIAL_KEYS: &[&str] = &[
    "position_qty",
    "avg_cost",
    "asset_class",
    "at",
    "inherited_loss_usd",
];

/// The fields a `risk_state` step may carry, by its `event`. `at`, `session`, `event`, and `expect`
/// are common to all; the rest are the members of that event's [`Input`] variant.
///
/// A step whose `event` is not listed fails rather than being read as some other input, and a field
/// the harness does not turn into an `Input` member fails naming it (DEC-85).
const STEP_KEYS: &[(&str, &[&str])] = &[
    ("mark", &["bid", "sane"]),
    ("fill", &["side", "qty", "price"]),
    ("risk_day_started", &[]),
    ("clock", &[]),
    ("owner_acknowledged", &["restriction"]),
    ("allocation_change", &["delta_usd"]),
    ("goal_complete", &[]),
    (
        "floor_loosened",
        &[
            "new_max_loss_from_allocation",
            "confirmed_at",
            "independent_approval",
        ],
    ),
    ("agent_stopped", &["reason"]),
    ("universe_changed", &["instrument", "change", "reason"]),
];

/// The members common to every step, whatever its event.
const STEP_COMMON_KEYS: &[&str] = &["event", "at", "session", "expect"];

/// Every member a `goal` case's `state` block may carry: exactly [`GoalInputs`].
const GOAL_STATE_KEYS: &[&str] = &[
    "now",
    "position_qty",
    "goal_spent_usd",
    "min_order_usd",
    "qty_increment",
    "ask",
];

/// The members the harness reads from each journalled event, by its `type`.
///
/// This is the vocabulary [`rendered`] produces, in one place, so the sweep below and the comparison
/// itself cannot disagree about what is read. `MandateVersionApplied` has no
/// `max_loss_from_allocation`, which is why MC-R21 step 5 fails naming it: `ApplyResult::Applied`
/// carries an allocation change and nothing else (see this change's Decisions needed).
const JOURNAL_MEMBERS: &[(&str, &[&str])] = &[
    (
        "MandateVersionApplied",
        &["result", "allocation_change", "reason"],
    ),
    ("RiskDayStarted", &["day_start_equity"]),
    ("RiskLimitTriggered", &["limit", "action", "reason"]),
    ("RiskLimitLifted", &["limit", "action", "reason"]),
    ("HighWaterMarkReset", &["from", "to"]),
    ("AgentModeApplied", &["from", "to"]),
    ("KillSwitchActivated", &["scope", "initiator"]),
    ("UniverseChanged", &["instrument", "change", "reason"]),
    (
        "InstrumentRestrictionChanged",
        &["restriction", "reason", "active"],
    ),
    ("GoalCompleted", &["reason", "then", "on_complete"]),
    ("PositionReleased", &["qty"]),
    ("AgentStopped", &["reason", "loss_carry_usd"]),
];

/// The input side of the DEC-85 sweep: `initial`, every step, and a `goal` case's `state`.
///
/// The `expect` sweep alone would let a case grow an input the harness never turns into an [`Input`]
/// member — a step field that silently does nothing is exactly as quiet as an unread expectation, and
/// the case would still report whatever the rules produced without it.
fn unread_input_keys(case: &Json) -> Result<(), String> {
    match str_at(case, "kind")? {
        "risk_state" => {
            if let Some(initial) = case.get("initial") {
                unknown_members(initial, INITIAL_KEYS)
                    .map_err(|unknown| format!("`initial` fields not interpreted: {unknown}"))?;
            }
            for (index, step) in steps_of(case).iter().enumerate() {
                let n = index.saturating_add(1);
                let event = str_at(step, "event").map_err(|e| format!("step {n}: {e}"))?;
                let Some((_, fields)) = STEP_KEYS.iter().find(|(e, _)| *e == event) else {
                    return Err(format!(
                        "step {n}: `{event}` is not an input the harness reads"
                    ));
                };
                let known: Vec<&str> = STEP_COMMON_KEYS
                    .iter()
                    .chain(fields.iter())
                    .copied()
                    .collect();
                unknown_members(step, &known)
                    .map_err(|unknown| format!("step {n}: fields not interpreted: {unknown}"))?;
                unread_journal_members(step.get("expect")).map_err(|e| format!("step {n}: {e}"))?;
            }
            Ok(())
        }
        "goal" => match case.get("state") {
            Some(state) => unknown_members(state, GOAL_STATE_KEYS)
                .map_err(|unknown| format!("`state` fields not interpreted: {unknown}")),
            None => Err("a `goal` case states no `state`, so it would check nothing".to_owned()),
        },
        _ => Ok(()),
    }
}

/// The journal sweep, one level below a step's `expect`: every member of every expected event is one
/// [`rendered`] produces for that event type.
fn unread_journal_members(expect: Option<&Json>) -> Result<(), String> {
    let Some(journal) = expect.and_then(|e| e.get("journal")) else {
        return Ok(());
    };
    let events = journal
        .as_array()
        .ok_or_else(|| "`expect.journal` is not a list".to_owned())?;
    for (index, event) in events.iter().enumerate() {
        let kind = str_at(event, "type").map_err(|e| format!("journal[{index}]: {e}"))?;
        let Some((_, members)) = JOURNAL_MEMBERS.iter().find(|(t, _)| *t == kind) else {
            return Err(format!(
                "journal[{index}]: `{kind}` is not an event the harness reads"
            ));
        };
        let known: Vec<&str> = core::iter::once("type")
            .chain(members.iter().copied())
            .collect();
        unknown_members(event, &known).map_err(|unknown| {
            format!("journal[{index}] (`{kind}`): members not interpreted: {unknown}")
        })?;
    }
    Ok(())
}

/// A case's steps, or none.
fn steps_of(case: &Json) -> &[Json] {
    case.get("steps")
        .and_then(Json::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

/// The `expect` sweep, one level down from [`unread_keys`].
fn unread_expect_keys(case: &Json) -> Result<(), String> {
    let kind = str_at(case, "kind")?;
    if kind == "risk_state" {
        for (index, step) in case
            .get("steps")
            .and_then(Json::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            if let Some(expect) = step.get("expect") {
                unknown_members(expect, STEP_EXPECT_KEYS).map_err(|unknown| {
                    format!(
                        "step {}: expectations not interpreted: {unknown}",
                        index.saturating_add(1)
                    )
                })?;
            }
        }
        return Ok(());
    }
    let Some((_, keys)) = EXPECT_KEYS.iter().find(|(k, _)| *k == kind) else {
        return Err(format!("no expectation keys declared for kind `{kind}`"));
    };
    match case.get("expect") {
        Some(expect) => unknown_members(expect, keys)
            .map_err(|unknown| format!("expectations not interpreted: {unknown}")),
        None => Ok(()),
    }
}

/// The members of `value` that are not in `known`, joined, or `Ok` when there are none.
fn unknown_members(value: &Json, known: &[&str]) -> Result<(), String> {
    let known: BTreeSet<&str> = known.iter().copied().collect();
    let members = value
        .as_object()
        .ok_or_else(|| "not an object".to_owned())?;
    let unknown: Vec<&str> = members
        .keys()
        .map(String::as_str)
        .filter(|k| !known.contains(k))
        .collect();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(unknown.join(", "))
    }
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
///
/// The whole case is read into typed values **before** the first rule is called, so a step field or an
/// expected event member the harness cannot interpret fails the case now rather than after the rules
/// land (DEC-85). Only then does the mandate go through [`Mandate::parse`] and
/// [`ValidatedMandate::new`], which is the one way a risk state can be opened at all (DEC-128 item 8).
fn risk_state_case(fixture: &Json, case: &Json) -> Result<(), String> {
    let document = patched(fixture, case)?;
    let initial = at(case, "initial")?;
    let asset_class = AssetClass::parse(str_at(initial, "asset_class")?)
        .map_err(|e| format!("`initial.asset_class`: {}", e.code()))?;
    let opening = Opening {
        position_qty: qty_of(initial, "position_qty")?,
        avg_cost: price_of(initial, "avg_cost")?,
        asset_class,
        at: instant_of(initial, "at")?,
        inherited_loss_usd: usd_of(initial, "inherited_loss_usd")?,
        mark_max_age_s: small(u64_at(at(fixture, "harness_defaults")?, "mark_max_age_s")?)?,
    };
    let stated = steps_of(case);
    let inputs: Vec<Step> = stated
        .iter()
        .enumerate()
        .map(|(index, step)| {
            step_of(step).map_err(|e| format!("step {}: {e}", index.saturating_add(1)))
        })
        .collect::<Result<_, String>>()?;
    let clock = session_clock(asset_class);
    let validated = validated(fixture, &document)?;
    let mut state = RiskState::open(&validated, &opening, clock.as_ref())
        .map_err(|e| format!("`RiskState::open`: {}", e.code()))?;
    for (index, (input, step)) in inputs.iter().zip(stated).enumerate() {
        let n = index.saturating_add(1);
        let outcome = state
            .step(input)
            .map_err(|e| format!("step {n}: `RiskState::step`: {}", e.code()))?;
        check_step(at(step, "expect")?, &outcome).map_err(|e| format!("step {n}: {e}"))?;
    }
    Ok(())
}

/// `kind: risk_day` — the risk day containing an instant and its bounds (§5.4).
fn risk_day_case(case: &Json) -> Result<(), String> {
    let day = risk_day(instant_of(case, "at")?).map_err(|e| format!("`risk_day`: {}", e.code()))?;
    let expect = at(case, "expect")?;
    expect_eq("risk_day", day.day, date_of(expect, "risk_day")?)?;
    expect_eq("starts_at", day.starts_at, instant_of(expect, "starts_at")?)?;
    expect_eq("ends_at", day.ends_at, instant_of(expect, "ends_at")?)?;
    expect_eq(
        "length_s",
        u64::from(day.length_s),
        u64_at(expect, "length_s")?,
    )
}

/// `kind: goal` — goal completion for a state (§3.1).
fn goal_case(fixture: &Json, case: &Json) -> Result<(), String> {
    let document = patched(fixture, case)?;
    let state = at(case, "state")?;
    let inputs = GoalInputs {
        now: instant_of(state, "now")?,
        position_qty: qty_of(state, "position_qty")?,
        goal_spent_usd: usd_of(state, "goal_spent_usd")?,
        min_order_usd: usd_of(state, "min_order_usd")?,
        qty_increment: increment_of(state, "qty_increment")?,
        ask: price_of(state, "ask")?,
    };
    let validated = validated(fixture, &document)?;
    let status = mandate_spec::goal::status(&validated, &inputs)
        .map_err(|e| format!("`goal::status`: {}", e.code()))?;
    let expect = at(case, "expect")?;
    let done = at(expect, "done")?
        .as_bool()
        .ok_or("`expect.done` is not a boolean")?;
    match (&status, done) {
        (GoalStatus::Done { .. }, false) => Err(format!(
            "the goal is not done, but `status` returned {status:?}"
        )),
        (other, true) => match other {
            GoalStatus::Done {
                reason,
                then,
                stop_reason,
            } => {
                expect_eq("reason", goal_reason(*reason), str_at(expect, "reason")?)?;
                expect_eq("then", then_action(*then), str_at(expect, "then")?)?;
                expect_eq(
                    "stop_reason",
                    stop_reason_name(*stop_reason),
                    str_at(expect, "stop_reason")?,
                )
            }
            _ => Err(format!("the goal is done, but `status` returned {other:?}")),
        },
        _ => Ok(()),
    }
}

/// The only way to a [`ValidatedMandate`], which every risk-state and goal rule takes.
///
/// The risk-state and goal families state no `context` of their own, so the fixture's
/// `validation_context_defaults` supply it. The registry is `None` — the fixture's own spelling of
/// "not stated", which leaves V-007 unchecked — because `mandate_spec::document::ModelId` has no
/// public constructor to key one with; V-007 belongs to the semantic family, which states its own
/// context.
fn validated(fixture: &Json, document: &Value) -> Result<ValidatedMandate, String> {
    let mandate =
        Mandate::parse(document).map_err(|e| format!("`Mandate::parse`: {}", e.code()))?;
    let defaults = at(fixture, "validation_context_defaults")?;
    let context = ValidationContext {
        account_equity_usd: usd_of(defaults, "account_equity_usd")?,
        other_allocations_usd: usd_of(defaults, "other_allocations_usd")?,
        validation_date: date_of(defaults, "validation_date")?,
        registry: None,
        provenance: mandate_spec::document::ProvenanceMap::default(),
        workspace_users: small(u64_at(defaults, "workspace_users")?)?,
        approver_users: small(u64_at(defaults, "approver_users")?)?,
        disclosures_accepted: BTreeSet::new(),
        instrument_groups: BTreeMap::new(),
        claimed_by_other_agents: BTreeSet::new(),
        connection_environment: None,
        connection_loss_carry_usd: Usd::ZERO,
        eligibility_failures: BTreeSet::new(),
        previous_version: None,
    };
    let chain = [platform_base().map_err(|e| format!("`platform_base`: {}", e.code()))?];
    ValidatedMandate::new(mandate, &context, &chain)
        .map_err(|e| format!("`ValidatedMandate::new`: {e}"))
}

/// One step, as the typed input §5.2 folds.
fn step_of(step: &Json) -> Result<Step, String> {
    let session = MarketSession::parse_condition_form(str_at(step, "session")?)
        .map_err(|e| format!("`session`: {}", e.code()))?;
    let event = str_at(step, "event")?;
    let input = match event {
        "mark" => Input::Mark {
            bid: price_of(step, "bid")?,
            sane: at(step, "sane")?
                .as_bool()
                .ok_or("`sane` is not a boolean")?,
        },
        "fill" => Input::Fill {
            side: match str_at(step, "side")? {
                "buy" => Side::Buy,
                "sell" => Side::Sell,
                other => return Err(format!("`side` is `{other}`")),
            },
            qty: qty_of(step, "qty")?,
            price: price_of(step, "price")?,
        },
        "risk_day_started" => Input::RiskDayStarted,
        "clock" => Input::Clock,
        "owner_acknowledged" => Input::OwnerAcknowledged {
            restriction: latch(str_at(step, "restriction")?)?,
        },
        "allocation_change" => Input::AllocationChange {
            delta_usd: usd_of(step, "delta_usd")?,
        },
        "goal_complete" => Input::GoalComplete,
        "floor_loosened" => Input::FloorLoosened {
            new_max_loss_from_allocation: SchemaDec::parse(
                str_at(step, "new_max_loss_from_allocation")?,
                DecGrammar::OpenFraction,
            )
            .map_err(|_| {
                "`new_max_loss_from_allocation` is not in the `open_fraction` grammar".to_owned()
            })?,
            confirmed_at: instant_of(step, "confirmed_at")?,
            independent_approval: at(step, "independent_approval")?
                .as_bool()
                .ok_or("`independent_approval` is not a boolean")?,
        },
        "agent_stopped" => Input::AgentStopped {
            reason: match step.get("reason").and_then(Json::as_str) {
                Some(text) => stop_reason(text)?,
                None => StopReason::OwnerStop,
            },
        },
        "universe_changed" => Input::UniverseChanged {
            instrument: AssetId::parse(str_at(step, "instrument")?)
                .map_err(|e| format!("`instrument`: {}", e.code()))?,
            change: match str_at(step, "change")? {
                "admitted" => UniverseChange::Admitted,
                "removed" => UniverseChange::Removed,
                other => return Err(format!("`change` is `{other}`")),
            },
            reason: removal_reason(str_at(step, "reason")?)?,
        },
        other => return Err(format!("`{other}` is not an input the harness reads")),
    };
    Ok(Step {
        at: instant_of(step, "at")?,
        session,
        input,
    })
}

/// Every member of a step's `expect`, against the outcome that step produced.
fn check_step(expect: &Json, outcome: &Outcome) -> Result<(), String> {
    let s: &Snapshot = &outcome.snapshot;
    check_usd(expect, "agent_equity", s.agent_equity)?;
    check_usd(expect, "high_water_mark", s.high_water_mark)?;
    check_usd(expect, "day_start_equity", s.day_start_equity)?;
    check_usd(expect, "daily_pnl", s.daily_pnl)?;
    check_usd(expect, "capital_base", s.capital_base)?;
    check_usd(expect, "net_contributed", s.net_contributed)?;
    check_ratio(expect, "drawdown", s.drawdown)?;
    check_ratio(expect, "daily_pnl_fraction", s.daily_pnl_fraction)?;
    check_ratio(expect, "size_factor", s.size_factor)?;
    if let Some(stated) = expect.get("restrictions") {
        let wanted: BTreeSet<Restriction> = named(stated, restriction)?;
        expect_eq("restrictions", s.restrictions.clone(), wanted)?;
    }
    if let Some(stated) = expect.get("instrument_restrictions") {
        let wanted: BTreeSet<InstrumentRestriction> = named(stated, instrument_restriction)?;
        expect_eq(
            "instrument_restrictions",
            s.instrument_restrictions.clone(),
            wanted,
        )?;
    }
    if let Some(stated) = expect.get("agent_mode") {
        let wanted = stated.as_str().ok_or("`agent_mode` is not a string")?;
        expect_eq("agent_mode", mode_name(s.agent_mode), wanted)?;
    }
    if let Some(stated) = expect.get("pending") {
        let wanted: BTreeSet<LimitKey> = named(stated, limit_key)?;
        expect_eq("pending", outcome.pending.clone(), wanted)?;
    }
    check_rejection(expect, outcome)?;
    match expect.get("journal") {
        Some(journal) => check_journal(journal, &outcome.journal),
        None => Ok(()),
    }
}

/// A refused input still produces an outcome (DEC-128 item 16), so the case's `error` is compared
/// against the rejection the outcome carries, and an unexpected refusal fails rather than passing as
/// an unread member.
fn check_rejection(expect: &Json, outcome: &Outcome) -> Result<(), String> {
    match (
        expect.get("error").and_then(Json::as_str),
        outcome.rejection,
    ) {
        (Some(wanted), Some(got)) => expect_eq("error", got.code(), wanted),
        (Some(wanted), None) => Err(format!("the step expects `{wanted}` but was not refused")),
        (None, Some(got)) => Err(format!(
            "the step was refused (`{}`) but the case states no error",
            got.code()
        )),
        (None, None) => Ok(()),
    }
}

/// The journal in order, comparing each event's `type` and exactly the members the case states
/// (DEC-128 item 15): `GoalCompleted` and the two limit events carry different members in different
/// cases, and a member the event does not carry is a failure, never a pass.
fn check_journal(journal: &Json, actual: &[RiskEvent]) -> Result<(), String> {
    let expected = journal
        .as_array()
        .ok_or_else(|| "`expect.journal` is not a list".to_owned())?;
    let seen: Vec<&'static str> = actual.iter().map(|e| rendered(e).0).collect();
    ensure(expected.len() == actual.len(), || {
        format!(
            "journal: expected {} event(s), got {}: {seen:?}",
            expected.len(),
            actual.len()
        )
    })?;
    for (index, (want, got)) in expected.iter().zip(actual).enumerate() {
        let (kind, members) = rendered(got);
        expect_eq(
            &format!("journal[{index}].type"),
            kind,
            str_at(want, "type")?,
        )?;
        let stated = want
            .as_object()
            .ok_or_else(|| format!("journal[{index}] is not an object"))?;
        for (key, value) in stated {
            if key == "type" {
                continue;
            }
            let Some((_, found)) = members.iter().find(|(k, _)| k == key) else {
                return Err(format!(
                    "journal[{index}] (`{kind}`): the event carries no `{key}`"
                ));
            };
            compare(&format!("journal[{index}].{key}"), found, value)?;
        }
    }
    Ok(())
}

/// A journalled member as the harness reads it: text, or a number compared in its own type rather
/// than by spelling, so `849.999` and `849.9990` are one value.
enum Rendered {
    Text(String),
    Money(Usd),
    Quantity(Qty),
    Flag(bool),
}

fn compare(what: &str, found: &Rendered, stated: &Json) -> Result<(), String> {
    match found {
        Rendered::Text(text) => expect_eq(
            what,
            text.as_str(),
            stated
                .as_str()
                .ok_or_else(|| format!("{what} is not a string"))?,
        ),
        Rendered::Money(amount) => {
            let wanted = Usd::parse(
                stated
                    .as_str()
                    .ok_or_else(|| format!("{what} is not a string"))?,
            )
            .map_err(|e| format!("{what}: {}", e.code()))?;
            expect_eq(what, *amount, wanted)
        }
        Rendered::Quantity(qty) => {
            let wanted = Qty::parse(
                stated
                    .as_str()
                    .ok_or_else(|| format!("{what} is not a string"))?,
            )
            .map_err(|e| format!("{what}: {}", e.code()))?;
            expect_eq(what, *qty, wanted)
        }
        Rendered::Flag(flag) => expect_eq(
            what,
            *flag,
            stated
                .as_bool()
                .ok_or_else(|| format!("{what} is not a boolean"))?,
        ),
    }
}

/// One journalled event as its `type` and the members the harness reads, which is the vocabulary
/// [`JOURNAL_MEMBERS`] declares.
fn rendered(event: &RiskEvent) -> (&'static str, Vec<(&'static str, Rendered)>) {
    let text = |value: &str| Rendered::Text(value.to_owned());
    match event {
        RiskEvent::MandateVersionApplied { result } => (
            "MandateVersionApplied",
            match result {
                ApplyResult::Applied { allocation_change } => {
                    let mut members = vec![("result", text("applied"))];
                    if let Some(delta) = allocation_change {
                        members.push(("allocation_change", Rendered::Money(*delta)));
                    }
                    members
                }
                ApplyResult::Rejected { reason } => vec![
                    ("result", text("rejected")),
                    ("reason", text(reason.code())),
                ],
            },
        ),
        RiskEvent::RiskDayStarted { day_start_equity } => (
            "RiskDayStarted",
            vec![("day_start_equity", Rendered::Money(*day_start_equity))],
        ),
        RiskEvent::RiskLimitTriggered {
            limit,
            action,
            reason,
        } => {
            let mut members = vec![
                ("limit", Rendered::Text(limit.journal_name())),
                ("action", text(action.as_str())),
            ];
            if let Some(reason) = reason {
                members.push(("reason", text(trigger_reason(*reason))));
            }
            ("RiskLimitTriggered", members)
        }
        RiskEvent::RiskLimitLifted {
            limit,
            action,
            reason,
        } => {
            let mut members = vec![("limit", Rendered::Text(limit.journal_name()))];
            if let Some(action) = action {
                members.push(("action", text(action.as_str())));
            }
            if let Some(reason) = reason {
                members.push(("reason", text(lift_reason(*reason))));
            }
            ("RiskLimitLifted", members)
        }
        RiskEvent::HighWaterMarkReset { from, to } => (
            "HighWaterMarkReset",
            vec![
                ("from", Rendered::Money(*from)),
                ("to", Rendered::Money(*to)),
            ],
        ),
        RiskEvent::AgentModeApplied { from, to } => (
            "AgentModeApplied",
            vec![
                ("from", text(mode_name(*from))),
                ("to", text(mode_name(*to))),
            ],
        ),
        RiskEvent::KillSwitchActivated {
            scope: _,
            initiator,
        } => (
            "KillSwitchActivated",
            vec![
                ("scope", text("agent")),
                ("initiator", Rendered::Text(initiator.journal_name())),
            ],
        ),
        RiskEvent::UniverseChanged {
            instrument,
            change,
            reason,
        } => (
            "UniverseChanged",
            vec![
                ("instrument", Rendered::Text(instrument.as_str().to_owned())),
                (
                    "change",
                    text(match change {
                        UniverseChange::Admitted => "admitted",
                        UniverseChange::Removed => "removed",
                    }),
                ),
                ("reason", text(removal_reason_name(*reason))),
            ],
        ),
        RiskEvent::InstrumentRestrictionChanged {
            restriction,
            reason,
            active,
        } => (
            "InstrumentRestrictionChanged",
            vec![
                ("restriction", text(restriction.as_str())),
                ("reason", Rendered::Text(restriction_reason(*reason))),
                ("active", Rendered::Flag(*active)),
            ],
        ),
        RiskEvent::GoalCompleted {
            reason,
            then,
            on_complete,
        } => {
            let mut members = Vec::new();
            if let Some(reason) = reason {
                members.push(("reason", text(goal_reason(*reason))));
            }
            if let Some(then) = then {
                members.push(("then", text(then_action(*then))));
            }
            if let Some(on_complete) = on_complete {
                members.push(("on_complete", text(on_complete.as_str())));
            }
            ("GoalCompleted", members)
        }
        RiskEvent::PositionReleased { qty } => {
            ("PositionReleased", vec![("qty", Rendered::Quantity(*qty))])
        }
        RiskEvent::AgentStopped {
            reason,
            loss_carry_usd,
        } => (
            "AgentStopped",
            vec![
                ("reason", text(stop_reason_name(*reason))),
                ("loss_carry_usd", Rendered::Money(*loss_carry_usd)),
            ],
        ),
    }
}

fn check_usd(expect: &Json, key: &str, actual: Usd) -> Result<(), String> {
    match expect.get(key) {
        Some(_) => expect_eq(key, actual, usd_of(expect, key)?),
        None => Ok(()),
    }
}

fn check_ratio(expect: &Json, key: &str, actual: Ratio) -> Result<(), String> {
    match expect.get(key) {
        Some(_) => expect_eq(key, actual, ratio_of(expect, key)?),
        None => Ok(()),
    }
}

/// A stated list of names, as the set the snapshot holds.
fn named<T: Ord>(
    stated: &Json,
    parse: fn(&str) -> Result<T, String>,
) -> Result<BTreeSet<T>, String> {
    stated
        .as_array()
        .ok_or_else(|| "a set of names must be a list".to_owned())?
        .iter()
        .map(|name| parse(name.as_str().ok_or("a name must be a string")?))
        .collect()
}

fn restriction(name: &str) -> Result<Restriction, String> {
    [
        Restriction::DailyLoss,
        Restriction::DrawdownExitsOnly,
        Restriction::DrawdownFlatten,
        Restriction::LifetimeFloor,
        Restriction::HardBreach,
        Restriction::GoalComplete,
        Restriction::Retired,
    ]
    .into_iter()
    .find(|r| r.as_str() == name)
    .ok_or_else(|| format!("`{name}` is not a restriction §5.9 names"))
}

fn instrument_restriction(name: &str) -> Result<InstrumentRestriction, String> {
    [
        InstrumentRestriction::StaleMark,
        InstrumentRestriction::RemovedInstrument,
    ]
    .into_iter()
    .find(|r| r.as_str() == name)
    .ok_or_else(|| format!("`{name}` is not an instrument restriction §5.9 names"))
}

/// A limit as §5.10 journals it, `drawdown_ladder[i]` included, which is how `pending` names one.
fn limit_key(name: &str) -> Result<LimitKey, String> {
    for key in [
        LimitKey::MaxDailyLoss,
        LimitKey::LifetimeFloor,
        LimitKey::ProfitStop,
    ] {
        if key.journal_name() == name {
            return Ok(key);
        }
    }
    let index = name
        .strip_prefix("drawdown_ladder[")
        .and_then(|rest| rest.strip_suffix(']'))
        .and_then(|digits| digits.parse::<u8>().ok())
        .ok_or_else(|| format!("`{name}` is not a limit §5.10 names"))?;
    Ok(LimitKey::DrawdownRung(index))
}

fn latch(name: &str) -> Result<Latch, String> {
    match name {
        "daily_loss" => Ok(Latch::DailyLoss),
        "drawdown_ladder" => Ok(Latch::DrawdownLadder),
        "lifetime_floor" => Ok(Latch::LifetimeFloor),
        other => Err(format!("`{other}` is not a latch §5.8 names")),
    }
}

fn stop_reason(name: &str) -> Result<StopReason, String> {
    [
        StopReason::OwnerStop,
        StopReason::ProfitStopReached,
        StopReason::EndDate,
        StopReason::GoalComplete,
    ]
    .into_iter()
    .find(|r| stop_reason_name(*r) == name)
    .ok_or_else(|| format!("`{name}` is not a stop reason"))
}

fn removal_reason(name: &str) -> Result<RemovalReason, String> {
    [
        RemovalReason::ThesisAdmitted,
        RemovalReason::ThesisExpired,
        RemovalReason::ThesisInvalidated,
        RemovalReason::LineageRetired,
        RemovalReason::EligibilityLost,
        RemovalReason::OperatorHalt,
        RemovalReason::VersionApplied,
    ]
    .into_iter()
    .find(|r| removal_reason_name(*r) == name)
    .ok_or_else(|| format!("`{name}` is not a §5.10 universe-change reason"))
}

fn mode_name(mode: AgentMode) -> &'static str {
    match mode {
        AgentMode::Normal => "normal",
        AgentMode::ExitsOnly => "exits_only",
        AgentMode::Paused => "paused",
        AgentMode::Stopped => "stopped",
    }
}

fn trigger_reason(reason: TriggerReason) -> &'static str {
    match reason {
        TriggerReason::HardTrigger => "hard_trigger",
        TriggerReason::HardBreachPending => "hard_breach_pending",
        TriggerReason::ResolvedAtRollover => "resolved_at_rollover",
        TriggerReason::NewDayBreach => "new_day_breach",
        TriggerReason::AfterReset => "after_reset",
    }
}

fn lift_reason(reason: LiftReason) -> &'static str {
    match reason {
        LiftReason::OwnerAcknowledged => "owner_acknowledged",
        LiftReason::VersionLoosened => "version_loosened",
        LiftReason::HardBreachCleared => "hard_breach_cleared",
    }
}

fn restriction_reason(reason: RestrictionReason) -> String {
    match reason {
        RestrictionReason::NoSaneMark => "no_sane_mark".to_owned(),
        RestrictionReason::SaneMark => "sane_mark".to_owned(),
        RestrictionReason::Removal(removal) => removal_reason_name(removal).to_owned(),
    }
}

fn removal_reason_name(reason: RemovalReason) -> &'static str {
    match reason {
        RemovalReason::ThesisAdmitted => "thesis_admitted",
        RemovalReason::ThesisExpired => "thesis_expired",
        RemovalReason::ThesisInvalidated => "thesis_invalidated",
        RemovalReason::LineageRetired => "lineage_retired",
        RemovalReason::EligibilityLost => "eligibility_lost",
        RemovalReason::OperatorHalt => "operator_halt",
        RemovalReason::VersionApplied => "version_applied",
    }
}

fn goal_reason(reason: GoalReason) -> &'static str {
    match reason {
        GoalReason::ProfitStopReached => "profit_stop_reached",
        GoalReason::TargetQty => "target_qty",
        GoalReason::MaxSpend => "max_spend",
        GoalReason::EndDate => "end_date",
    }
}

fn then_action(then: ThenAction) -> &'static str {
    match then {
        ThenAction::Applied(OnComplete::HoldProtected) => "hold_protected",
        ThenAction::Applied(OnComplete::DisarmLadder) => "disarm_ladder",
        ThenAction::Applied(OnComplete::Release) => "release",
        ThenAction::DiscretionaryExitAllThenRetire => "discretionary_exit_all_then_retire",
    }
}

fn stop_reason_name(reason: StopReason) -> &'static str {
    match reason {
        StopReason::OwnerStop => "owner_stop",
        StopReason::ProfitStopReached => "profit_stop_reached",
        StopReason::EndDate => "end_date",
        StopReason::GoalComplete => "goal_complete",
    }
}

fn usd_of(value: &Json, key: &str) -> Result<Usd, String> {
    Usd::parse(str_at(value, key)?).map_err(|e| format!("`{key}`: {}", e.code()))
}

fn ratio_of(value: &Json, key: &str) -> Result<Ratio, String> {
    Ratio::parse(str_at(value, key)?).map_err(|e| format!("`{key}`: {}", e.code()))
}

fn qty_of(value: &Json, key: &str) -> Result<Qty, String> {
    Qty::parse(str_at(value, key)?).map_err(|e| format!("`{key}`: {}", e.code()))
}

fn price_of(value: &Json, key: &str) -> Result<Price, String> {
    Price::parse(str_at(value, key)?).map_err(|e| format!("`{key}`: {}", e.code()))
}

fn instant_of(value: &Json, key: &str) -> Result<UtcNanos, String> {
    UtcNanos::parse(str_at(value, key)?).map_err(|e| format!("`{key}`: {}", e.code()))
}

fn date_of(value: &Json, key: &str) -> Result<Date, String> {
    Date::parse(str_at(value, key)?).map_err(|e| format!("`{key}`: {}", e.code()))
}

fn small(value: u64) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| format!("{value} does not fit the field's width"))
}

/// §3.1's "below one increment", which [`GoalInputs::qty_increment`] states as a
/// [`ShareIncrement`] — a grid of 10^-9 or of whole shares, and nothing between.
///
/// Every `goal` case states `0.0001`, the increment its crypto instrument actually trades on, so all
/// five fail here rather than being read as some other grid: a harness that rounded `0.0001` to
/// `Fractional` would answer MC-L02 ("the remainder is below one increment") with the wrong grid and
/// call it a pass. See this change's Decisions needed.
fn increment_of(value: &Json, key: &str) -> Result<ShareIncrement, String> {
    match str_at(value, key)? {
        "0.000000001" => Ok(ShareIncrement::Fractional),
        "1" => Ok(ShareIncrement::Whole),
        other => Err(format!(
            "`{key}` is `{other}`, which `GoalInputs::qty_increment` cannot express: \
             `mandate_num::ShareIncrement` is `Fractional` (10^-9) or `Whole`"
        )),
    }
}

/// The clock §5.2 and §5.5 count an equity's staleness and scale-lift timers in: regular-session
/// seconds from the NYSE calendar for an equity, and every second for crypto, which trades
/// continuously.
fn session_clock(asset_class: AssetClass) -> Box<dyn SessionClock> {
    match asset_class {
        AssetClass::Crypto => Box::new(EverySecond),
        AssetClass::UsEquity => Box::new(RegularSessionSeconds),
    }
}

struct EverySecond;

impl SessionClock for EverySecond {
    fn seconds_between(&self, from: UtcNanos, to: UtcNanos) -> Result<u64, SpecError> {
        u64::try_from(to.secs().saturating_sub(from.secs()))
            .map_err(|_| SpecError::ClockWentBackwards)
    }
}

struct RegularSessionSeconds;

/// How many calendar days one interval of a reference case may span before the harness calls it a
/// mistake rather than walking the calendar for it.
const MAX_INTERVAL_DAYS: u32 = 400;

impl SessionClock for RegularSessionSeconds {
    fn seconds_between(&self, from: UtcNanos, to: UtcNanos) -> Result<u64, SpecError> {
        if to.secs() < from.secs() {
            return Err(SpecError::ClockWentBackwards);
        }
        let calendar = ExchangeCalendar::us_equities().map_err(|_| SpecError::Unimplemented)?;
        let mut day = from.date();
        let last = to.date();
        let mut total: u64 = 0;
        for _ in 0..MAX_INTERVAL_DAYS {
            for span in calendar.sessions(day)? {
                if span.session() != Session::Regular {
                    continue;
                }
                let start = span.start().secs().max(from.secs());
                let end = span.end().secs().min(to.secs());
                total = total
                    .saturating_add(u64::try_from(end.saturating_sub(start)).unwrap_or_default());
            }
            if day >= last {
                return Ok(total);
            }
            day = day.next()?;
        }
        Err(SpecError::ClockWentBackwards)
    }
}
