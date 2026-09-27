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

use mandate_canon::Digest;
use mandate_canon::Value;
use mandate_domain::{AgentMode, AssetClass, AssetId, MarketSession, Side};
use mandate_num::{Price, Qty, Usd};
use mandate_spec::document::{ModelId, ProvenanceMap};
use mandate_spec::goal::{self, GoalInputs, GoalStatus};
use mandate_spec::risk::LiftReason;
use mandate_spec::risk::{
    self, ApplyResult, Input, KillScope, Latch, Opening, RemovalReason, RestrictionReason,
    RiskEvent, StopReason, TriggerReason, UniverseChange,
};
use mandate_spec::validate::{RegisteredModel, ValidatedMandate, ValidationContext};
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

/// The members every `risk_state` step carries, whatever its kind.
const STEP_COMMON_KEYS: &[&str] = &["event", "at", "session", "expect"];

/// The ten §5.2 input kinds and the fields the harness reads for each. A field the fixture adds and
/// `risk_step` would ignore fails its case, the DEC-85 rule one level below `expect`: a `mark` that
/// grew an `ask`, or a `fill` that grew a `fee`, would otherwise change nothing and be believed.
const STEP_KEYS: &[(&str, &[&str])] = &[
    ("mark", &["bid", "sane"]),
    ("fill", &["side", "qty", "price"]),
    ("risk_day_started", &[]),
    ("owner_acknowledged", &["restriction"]),
    ("allocation_change", &["delta_usd"]),
    ("clock", &[]),
    ("universe_changed", &["instrument", "change", "reason"]),
    (
        "floor_loosened",
        &[
            "new_max_loss_from_allocation",
            "confirmed_at",
            "independent_approval",
        ],
    ),
    ("agent_stopped", &["reason"]),
    ("goal_complete", &[]),
];

/// What the agent held when a `risk_state` case opened (§5.2). `mark_max_age_s` is the data profile's
/// staleness limit, which every case takes from `harness_defaults` instead of stating.
const INITIAL_KEYS: &[&str] = &[
    "position_qty",
    "avg_cost",
    "asset_class",
    "at",
    "inherited_loss_usd",
    "mark_max_age_s",
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
    unread_expect_keys(case)
}

/// The `expect` sweep, one level down from [`unread_keys`].
fn unread_expect_keys(case: &Json) -> Result<(), String> {
    let kind = str_at(case, "kind")?;
    if kind == "risk_state" {
        if let Some(initial) = case.get("initial") {
            unknown_members(initial, INITIAL_KEYS)
                .map_err(|unknown| format!("`initial` fields not interpreted: {unknown}"))?;
        }
        for (index, step) in case
            .get("steps")
            .and_then(Json::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let number = index.saturating_add(1);
            unread_step_fields(step)
                .map_err(|unknown| format!("step {number}: fields not interpreted: {unknown}"))?;
            if let Some(expect) = step.get("expect") {
                unknown_members(expect, STEP_EXPECT_KEYS).map_err(|unknown| {
                    format!("step {number}: expectations not interpreted: {unknown}")
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

/// One step's own fields: the four every step carries plus the ones its `event` kind declares. An
/// unknown kind fails here rather than at [`risk_step`], so the message names the kind once.
fn unread_step_fields(step: &Json) -> Result<(), String> {
    let kind = str_at(step, "event").map_err(|_| "an `event` name".to_owned())?;
    let (_, own) = STEP_KEYS
        .iter()
        .find(|(name, _)| *name == kind)
        .ok_or_else(|| format!("`{kind}` is not a risk input the harness applies"))?;
    let known: Vec<&str> = STEP_COMMON_KEYS.iter().chain(own.iter()).copied().collect();
    unknown_members(step, &known)
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
/// Every step is applied to one state, in order, and every member of every step's `expect` is compared:
/// the ten reported figures, the three name sets, the journal list position by position and member by
/// member, and the refusal a rejected input carries. A step that states no `error` requires
/// `rejection: None`, so a refusal cannot slip past by being ignored.
fn risk_state_case(fixture: &Json, case: &Json) -> Result<(), String> {
    let validated = validated_mandate(fixture, case)?;
    let initial = at_of(case, "initial")?;
    let asset_class = AssetClass::parse(str_at(initial, "asset_class")?)
        .map_err(|e| format!("`initial.asset_class`: {}", e.code()))?;
    let opening = Opening {
        position_qty: num(Qty::parse(str_at(initial, "position_qty")?), "position_qty")?,
        avg_cost: num(Price::parse(str_at(initial, "avg_cost")?), "avg_cost")?,
        asset_class,
        at: instant(at(initial, "at")?, "initial.at")?,
        inherited_loss_usd: num(
            Usd::parse(str_at(initial, "inherited_loss_usd")?),
            "inherited_loss_usd",
        )?,
        mark_max_age_s: match initial.get("mark_max_age_s") {
            Some(_) => u32_of(initial, "mark_max_age_s")?,
            None => u32_of(at_of(fixture, "harness_defaults")?, "mark_max_age_s")?,
        },
    };
    let clock = HarnessSessionClock::for_class(asset_class)?;
    let mut state = spec(
        risk::RiskState::open(&validated, &opening, &clock),
        "RiskState::open",
    )?;
    for (index, step) in list_at(case, "steps")?.iter().enumerate() {
        let number = index.saturating_add(1);
        let outcome = spec(state.step(&risk_step(step)?), "RiskState::step")
            .map_err(|e| format!("step {number}: {e}"))?;
        check_step(at_of(step, "expect")?, &outcome).map_err(|e| format!("step {number}: {e}"))?;
    }
    Ok(())
}

/// The case's patched document, parsed and validated. Every `risk_state` case's mandate must be a
/// [`ValidatedMandate`], because that is the only thing [`risk::RiskState::open`] takes: a risk state is
/// a state *of a mandate nobody may bypass* (DEC-128 item 8).
fn validated_mandate(fixture: &Json, case: &Json) -> Result<ValidatedMandate, String> {
    let mandate = must_parse(&patched(fixture, case)?)?;
    ValidatedMandate::new(mandate, &context_defaults(fixture)?, &[])
        .map_err(|e| format!("the mandate is not valid here: {e}"))
}

/// The strict parse, with the DEC-77 message when the parser is still a stub.
fn must_parse(document: &Value) -> Result<Mandate, String> {
    Mandate::parse(document).map_err(|e| {
        if e.code() == "unimplemented" {
            not_implemented("the mandate parser")
        } else {
            format!("the parse rejected this case's document: {}", e.code())
        }
    })
}

/// One step as a [`risk::Step`]: the risk clock, the session it arrived in, and the input itself.
///
/// The event names are the fixture's, and every field each event kind carries is read — [`STEP_KEYS`]
/// is the same list, swept for keys this function would ignore.
///
/// An `agent_stopped` step that names no reason is an owner stop: the one such case states none and
/// journals `owner_stop`, and §5.10's other three stop reasons come from a goal or an end date rather
/// than from an input.
fn risk_step(step: &Json) -> Result<risk::Step, String> {
    let kind = str_at(step, "event")?;
    let input = match kind {
        "mark" => Input::Mark {
            bid: num(Price::parse(str_at(step, "bid")?), "bid")?,
            sane: at(step, "sane")?
                .as_bool()
                .ok_or("`sane` is not a boolean")?,
        },
        "fill" => Input::Fill {
            side: match str_at(step, "side")? {
                "buy" => Side::Buy,
                "sell" => Side::Sell,
                other => return Err(format!("`side` is not a side: `{other}`")),
            },
            qty: num(Qty::parse(str_at(step, "qty")?), "qty")?,
            price: num(Price::parse(str_at(step, "price")?), "price")?,
        },
        "risk_day_started" => Input::RiskDayStarted,
        "owner_acknowledged" => Input::OwnerAcknowledged {
            restriction: match str_at(step, "restriction")? {
                "daily_loss" => Latch::DailyLoss,
                "drawdown_ladder" => Latch::DrawdownLadder,
                "lifetime_floor" => Latch::LifetimeFloor,
                other => return Err(format!("`restriction` is not a latch: `{other}`")),
            },
        },
        "allocation_change" => Input::AllocationChange {
            delta_usd: num(Usd::parse(str_at(step, "delta_usd")?), "delta_usd")?,
        },
        "clock" => Input::Clock,
        "universe_changed" => Input::UniverseChanged {
            instrument: AssetId::parse(str_at(step, "instrument")?)
                .map_err(|e| format!("`instrument`: {}", e.code()))?,
            change: match str_at(step, "change")? {
                "admitted" => UniverseChange::Admitted,
                "removed" => UniverseChange::Removed,
                other => return Err(format!("`change` is not a universe change: `{other}`")),
            },
            reason: removal_reason(str_at(step, "reason")?)?,
        },
        "floor_loosened" => Input::FloorLoosened {
            new_max_loss_from_allocation: SchemaDec::parse(
                str_at(step, "new_max_loss_from_allocation")?,
                DecGrammar::OpenFraction,
            )
            .map_err(|e| format!("`new_max_loss_from_allocation`: {e}"))?,
            confirmed_at: instant(at(step, "confirmed_at")?, "confirmed_at")?,
            independent_approval: at(step, "independent_approval")?
                .as_bool()
                .ok_or("`independent_approval` is not a boolean")?,
        },
        "agent_stopped" => Input::AgentStopped {
            reason: match step.get("reason").and_then(Json::as_str) {
                None | Some("owner_stop") => StopReason::OwnerStop,
                Some("profit_stop_reached") => StopReason::ProfitStopReached,
                Some("end_date") => StopReason::EndDate,
                Some("goal_complete") => StopReason::GoalComplete,
                Some(other) => return Err(format!("`reason` is not a stop reason: `{other}`")),
            },
        },
        "goal_complete" => Input::GoalComplete,
        other => return Err(format!("`{other}` is not a risk input the harness applies")),
    };
    Ok(risk::Step {
        at: instant(at(step, "at")?, "at")?,
        session: MarketSession::parse_condition_form(str_at(step, "session")?)
            .map_err(|e| format!("`session`: {}", e.code()))?,
        input,
    })
}

fn removal_reason(text: &str) -> Result<RemovalReason, String> {
    match text {
        "thesis_admitted" => Ok(RemovalReason::ThesisAdmitted),
        "thesis_expired" => Ok(RemovalReason::ThesisExpired),
        "thesis_invalidated" => Ok(RemovalReason::ThesisInvalidated),
        "lineage_retired" => Ok(RemovalReason::LineageRetired),
        "eligibility_lost" => Ok(RemovalReason::EligibilityLost),
        "operator_halt" => Ok(RemovalReason::OperatorHalt),
        "version_applied" => Ok(RemovalReason::VersionApplied),
        other => Err(format!("`{other}` is not a §5.10 universe-change reason")),
    }
}

/// Every member of one step's `expect`, compared against the outcome.
fn check_step(expect: &Json, outcome: &risk::Outcome) -> Result<(), String> {
    let snapshot = &outcome.snapshot;
    for (key, actual) in [
        ("agent_equity", snapshot.agent_equity.to_string()),
        ("high_water_mark", snapshot.high_water_mark.to_string()),
        ("drawdown", snapshot.drawdown.to_string()),
        ("day_start_equity", snapshot.day_start_equity.to_string()),
        ("daily_pnl", snapshot.daily_pnl.to_string()),
        (
            "daily_pnl_fraction",
            snapshot.daily_pnl_fraction.to_string(),
        ),
        ("capital_base", snapshot.capital_base.to_string()),
        ("size_factor", snapshot.size_factor.to_string()),
        ("net_contributed", snapshot.net_contributed.to_string()),
        (
            "agent_mode",
            agent_mode_name(snapshot.agent_mode).to_owned(),
        ),
    ] {
        expect_eq(key, actual, str_at(expect, key)?.to_owned())?;
    }
    expect_set(
        expect,
        "restrictions",
        snapshot
            .restrictions
            .iter()
            .map(|r| r.as_str().to_owned())
            .collect(),
    )?;
    expect_set(
        expect,
        "instrument_restrictions",
        snapshot
            .instrument_restrictions
            .iter()
            .map(|r| r.as_str().to_owned())
            .collect(),
    )?;
    expect_set(
        expect,
        "pending",
        outcome
            .pending
            .iter()
            .map(|limit| limit.journal_name())
            .collect(),
    )?;
    expect_rejection(expect, outcome.rejection)?;
    expect_journal(expect, &outcome.journal)
}

/// A set of names, compared as a set: §5.6 and §5.9 fix which names are present, not the order a
/// reader lists them in. A name listed twice fails rather than collapsing into the set.
fn expect_set(expect: &Json, key: &str, actual: BTreeSet<String>) -> Result<(), String> {
    let listed = list_at(expect, key)?;
    let wanted = listed
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("`{key}` holds something that is not a name"))
        })
        .collect::<Result<BTreeSet<String>, String>>()?;
    ensure(wanted.len() == listed.len(), || {
        format!("`{key}` names the same thing twice")
    })?;
    expect_eq(key, actual, wanted)
}

/// The refusal, if the step states one. A step with no `error` requires no rejection, which is what
/// stops a refused input from passing as an applied one (DEC-128 item 16).
fn expect_rejection(expect: &Json, rejection: Option<risk::Rejection>) -> Result<(), String> {
    let wanted = match expect.get("error") {
        Some(value) => Some(
            value
                .as_str()
                .ok_or("`error` is not a reason code")?
                .to_owned(),
        ),
        None => None,
    };
    expect_eq("error", rejection.map(|r| r.code().to_owned()), wanted)
}

/// The journal, position by position and member by member.
///
/// Both sides are compared as complete member maps, so an event that carries a member the case does not
/// state fails as surely as one that is missing a member it does state: §5.10's shapes are the
/// expectation, and a `reason` appearing where plain confirmation is meant is a different event.
fn expect_journal(expect: &Json, journal: &[risk::RiskEvent]) -> Result<(), String> {
    let listed = list_at(expect, "journal")?;
    ensure(journal.len() == listed.len(), || {
        format!(
            "journal: expected {} event(s) {:?}, got {} {:?}",
            listed.len(),
            listed
                .iter()
                .map(|e| e.get("type").and_then(Json::as_str).unwrap_or("?"))
                .collect::<Vec<_>>(),
            journal.len(),
            journal
                .iter()
                .map(|e| event_members(e).get("type").cloned().unwrap_or_default())
                .collect::<Vec<_>>()
        )
    })?;
    for (index, (event, wanted)) in journal.iter().zip(listed).enumerate() {
        let members = wanted
            .as_object()
            .ok_or_else(|| format!("journal event {index} is not an object"))?
            .iter()
            .map(|(k, v)| Ok((k.clone(), scalar(v, k)?)))
            .collect::<Result<BTreeMap<String, String>, String>>()?;
        expect_eq(
            &format!("journal event {}", index.saturating_add(1)),
            event_members(event),
            members,
        )?;
    }
    Ok(())
}

/// A fixture scalar as the text the harness compares.
fn scalar(value: &Json, what: &str) -> Result<String, String> {
    match value {
        Json::String(text) => Ok(text.clone()),
        Json::Bool(flag) => Ok(flag.to_string()),
        Json::Number(number) => Ok(number.to_string()),
        _ => Err(format!("`{what}` is not a value this harness compares")),
    }
}

/// One event as the members §5.10 gives it, `type` included, with an absent optional member omitted
/// rather than written as a null — which is how the fixture states them.
fn event_members(event: &risk::RiskEvent) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    match event {
        RiskEvent::MandateVersionApplied { result } => {
            out.insert("type".to_owned(), "MandateVersionApplied".to_owned());
            match result {
                ApplyResult::Applied { allocation_change } => {
                    out.insert("result".to_owned(), "applied".to_owned());
                    if let Some(delta) = allocation_change {
                        out.insert("allocation_change".to_owned(), delta.to_string());
                    }
                }
                ApplyResult::Rejected { reason } => {
                    out.insert("result".to_owned(), "rejected".to_owned());
                    out.insert("reason".to_owned(), reason.code().to_owned());
                }
            }
        }
        RiskEvent::RiskDayStarted { day_start_equity } => {
            out.insert("type".to_owned(), "RiskDayStarted".to_owned());
            out.insert("day_start_equity".to_owned(), day_start_equity.to_string());
        }
        RiskEvent::RiskLimitTriggered {
            limit,
            action,
            reason,
        } => {
            out.insert("type".to_owned(), "RiskLimitTriggered".to_owned());
            out.insert("limit".to_owned(), limit.journal_name());
            out.insert("action".to_owned(), action.as_str().to_owned());
            if let Some(reason) = reason {
                out.insert("reason".to_owned(), trigger_reason_name(*reason).to_owned());
            }
        }
        RiskEvent::RiskLimitLifted {
            limit,
            action,
            reason,
        } => {
            out.insert("type".to_owned(), "RiskLimitLifted".to_owned());
            out.insert("limit".to_owned(), limit.journal_name());
            if let Some(action) = action {
                out.insert("action".to_owned(), action.as_str().to_owned());
            }
            if let Some(reason) = reason {
                out.insert("reason".to_owned(), lift_reason_name(*reason).to_owned());
            }
        }
        RiskEvent::HighWaterMarkReset { from, to } => {
            out.insert("type".to_owned(), "HighWaterMarkReset".to_owned());
            out.insert("from".to_owned(), from.to_string());
            out.insert("to".to_owned(), to.to_string());
        }
        RiskEvent::AgentModeApplied { from, to } => {
            out.insert("type".to_owned(), "AgentModeApplied".to_owned());
            out.insert("from".to_owned(), agent_mode_name(*from).to_owned());
            out.insert("to".to_owned(), agent_mode_name(*to).to_owned());
        }
        RiskEvent::KillSwitchActivated { scope, initiator } => {
            out.insert("type".to_owned(), "KillSwitchActivated".to_owned());
            out.insert("scope".to_owned(), kill_scope_name(*scope).to_owned());
            out.insert("initiator".to_owned(), initiator.journal_name());
        }
        RiskEvent::UniverseChanged {
            instrument,
            change,
            reason,
        } => {
            out.insert("type".to_owned(), "UniverseChanged".to_owned());
            out.insert("instrument".to_owned(), instrument.as_str().to_owned());
            out.insert(
                "change".to_owned(),
                universe_change_name(*change).to_owned(),
            );
            out.insert("reason".to_owned(), removal_reason_name(*reason).to_owned());
        }
        RiskEvent::InstrumentRestrictionChanged {
            restriction,
            reason,
            active,
        } => {
            out.insert("type".to_owned(), "InstrumentRestrictionChanged".to_owned());
            out.insert("restriction".to_owned(), restriction.as_str().to_owned());
            out.insert("reason".to_owned(), restriction_reason_name(*reason));
            out.insert("active".to_owned(), active.to_string());
        }
        RiskEvent::GoalCompleted {
            reason,
            then,
            on_complete,
        } => {
            out.insert("type".to_owned(), "GoalCompleted".to_owned());
            if let Some(reason) = reason {
                out.insert("reason".to_owned(), reason.as_str().to_owned());
            }
            if let Some(then) = then {
                out.insert("then".to_owned(), then.as_str().to_owned());
            }
            if let Some(on_complete) = on_complete {
                out.insert("on_complete".to_owned(), on_complete.as_str().to_owned());
            }
        }
        RiskEvent::PositionReleased { qty } => {
            out.insert("type".to_owned(), "PositionReleased".to_owned());
            out.insert("qty".to_owned(), qty.to_string());
        }
        RiskEvent::AgentStopped {
            reason,
            loss_carry_usd,
        } => {
            out.insert("type".to_owned(), "AgentStopped".to_owned());
            out.insert("reason".to_owned(), reason.as_str().to_owned());
            out.insert("loss_carry_usd".to_owned(), loss_carry_usd.to_string());
        }
    }
    out
}

/// The §5.9 spelling of a mode. Written here rather than taken from `mandate-domain`, which has none:
/// the fixture's word is the expectation, so the harness must know it independently.
fn agent_mode_name(mode: AgentMode) -> &'static str {
    match mode {
        AgentMode::Normal => "normal",
        AgentMode::ExitsOnly => "exits_only",
        AgentMode::Paused => "paused",
        AgentMode::Stopped => "stopped",
    }
}

fn trigger_reason_name(reason: TriggerReason) -> &'static str {
    match reason {
        TriggerReason::HardTrigger => "hard_trigger",
        TriggerReason::HardBreachPending => "hard_breach_pending",
        TriggerReason::ResolvedAtRollover => "resolved_at_rollover",
        TriggerReason::NewDayBreach => "new_day_breach",
        TriggerReason::AfterReset => "after_reset",
    }
}

fn lift_reason_name(reason: LiftReason) -> &'static str {
    match reason {
        LiftReason::OwnerAcknowledged => "owner_acknowledged",
        LiftReason::VersionLoosened => "version_loosened",
        LiftReason::HardBreachCleared => "hard_breach_cleared",
    }
}

fn restriction_reason_name(reason: RestrictionReason) -> String {
    match reason {
        RestrictionReason::NoSaneMark => "no_sane_mark".to_owned(),
        RestrictionReason::SaneMark => "sane_mark".to_owned(),
        RestrictionReason::Removal(removal) => removal_reason_name(removal).to_owned(),
    }
}

fn universe_change_name(change: UniverseChange) -> &'static str {
    match change {
        UniverseChange::Admitted => "admitted",
        UniverseChange::Removed => "removed",
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

fn kill_scope_name(scope: KillScope) -> &'static str {
    match scope {
        KillScope::Agent => "agent",
    }
}

/// The clock §5.2 and §5.5 count staleness and scale-lift delays on: regular-session seconds for an
/// equity, every second for crypto.
///
/// The calendar is `mandate-time`'s NYSE data, which is where the platform's own regular session comes
/// from; `mandate-spec` holds none, which is why [`risk::SessionClock`] is a parameter (DEC-128 item 23).
struct HarnessSessionClock {
    calendar: Option<ExchangeCalendar>,
}

impl HarnessSessionClock {
    fn for_class(asset_class: AssetClass) -> Result<Self, String> {
        Ok(Self {
            calendar: match asset_class {
                AssetClass::Crypto => None,
                AssetClass::UsEquity => Some(
                    ExchangeCalendar::us_equities()
                        .map_err(|e| format!("the US equities calendar: {}", e.code()))?,
                ),
            },
        })
    }
}

impl risk::SessionClock for HarnessSessionClock {
    fn seconds_between(&self, from: UtcNanos, to: UtcNanos) -> Result<u64, SpecError> {
        let Some(calendar) = self.calendar.as_ref() else {
            return whole_seconds(from, to);
        };
        let mut total: u64 = 0;
        let mut date = from.date();
        while date <= to.date() {
            for span in calendar.sessions(date)? {
                if span.session() != Session::Regular {
                    continue;
                }
                let start = span.start().max(from);
                let end = span.end().min(to);
                if start < end {
                    total = total.saturating_add(whole_seconds(start, end)?);
                }
            }
            date = date.next()?;
        }
        Ok(total)
    }
}

/// Whole seconds from `from` to `to`, and [`SpecError::ClockWentBackwards`] if that is negative —
/// the same answer the risk state gives a step that goes back in time.
fn whole_seconds(from: UtcNanos, to: UtcNanos) -> Result<u64, SpecError> {
    to.secs()
        .checked_sub(from.secs())
        .and_then(|seconds| u64::try_from(seconds).ok())
        .ok_or(SpecError::ClockWentBackwards)
}

/// `kind: risk_day` — the risk day containing an instant and its bounds (§5.4).
///
/// All four expectations are read. `length_s` is compared as the fixture states it **and** against the
/// distance between the two bounds, so a day whose `length_s` disagrees with its own bounds fails even
/// if the fixture were to state the wrong number.
fn risk_day_case(case: &Json) -> Result<(), String> {
    let instant_at = instant(at(case, "at")?, "at")?;
    let day = spec(risk::risk_day(instant_at), "risk_day")?;
    let expect = at_of(case, "expect")?;
    expect_eq(
        "risk_day",
        day.day.to_string(),
        str_at(expect, "risk_day")?.to_owned(),
    )?;
    expect_eq(
        "starts_at",
        day.starts_at,
        instant(at(expect, "starts_at")?, "starts_at")?,
    )?;
    expect_eq(
        "ends_at",
        day.ends_at,
        instant(at(expect, "ends_at")?, "ends_at")?,
    )?;
    expect_eq(
        "length_s",
        u64::from(day.length_s),
        u64_at(expect, "length_s")?,
    )?;
    let span = day
        .ends_at
        .secs()
        .checked_sub(day.starts_at.secs())
        .and_then(|d| u64::try_from(d).ok())
        .ok_or_else(|| "the day's bounds do not run forwards".to_owned())?;
    expect_eq(
        "length_s against its own bounds",
        u64::from(day.length_s),
        span,
    )
}

/// `kind: goal` — goal completion for a state (§3.1).
///
/// Every member of `state` is read and every member of `expect` is compared. `reason`, `then`, and
/// `stop_reason` are absent from a case that is not done (MC-L05), and a `Done` status where the case
/// expects none fails on `done` before any of them is looked for.
fn goal_case(fixture: &Json, case: &Json) -> Result<(), String> {
    let validated = validated_mandate(fixture, case)?;
    let state = at_of(case, "state")?;
    let inputs = GoalInputs {
        now: instant(at(state, "now")?, "now")?,
        position_qty: num(Qty::parse(str_at(state, "position_qty")?), "position_qty")?,
        goal_spent_usd: num(
            Usd::parse(str_at(state, "goal_spent_usd")?),
            "goal_spent_usd",
        )?,
        min_order_usd: num(Usd::parse(str_at(state, "min_order_usd")?), "min_order_usd")?,
        qty_increment: num(Qty::parse(str_at(state, "qty_increment")?), "qty_increment")?,
        ask: num(Price::parse(str_at(state, "ask")?), "ask")?,
    };
    let status = spec(goal::status(&validated, &inputs), "goal::status")?;
    let expect = at_of(case, "expect")?;
    let done = at(expect, "done")?
        .as_bool()
        .ok_or("`expect.done` is not a boolean")?;
    match (&status, done) {
        (GoalStatus::Done { .. }, false) => {
            return Err(format!(
                "the goal is not done here, but it reported {status:?}"
            ));
        }
        (GoalStatus::Running | GoalStatus::ConfirmedInRiskState, true) => {
            return Err(format!("the goal is done here, but it reported {status:?}"));
        }
        _ => {}
    }
    let GoalStatus::Done {
        reason,
        then,
        stop_reason,
    } = status
    else {
        return Ok(());
    };
    expect_eq(
        "reason",
        reason.as_str().to_owned(),
        str_at(expect, "reason")?.to_owned(),
    )?;
    expect_eq(
        "then",
        then.as_str().to_owned(),
        str_at(expect, "then")?.to_owned(),
    )?;
    expect_eq(
        "stop_reason",
        stop_reason.as_str().to_owned(),
        str_at(expect, "stop_reason")?.to_owned(),
    )
}

/// The validation context, read from the fixture's own `validation_context_defaults`.
///
/// Review round 1 found the first version of this hardcoding an equity, a date, and a `None` registry
/// beside the fixture's stated defaults while claiming to be the values the bases were written against.
/// It now reads all six: account equity, other allocations, the validation date, the signal-model
/// registry, and the workspace and approver user counts. Every field is owner-entered and confirmed,
/// which is what an absent [`ProvenanceMap`] entry means (§2.1), and the remaining fields are the "not
/// stated" of the cases — no group map, nothing claimed elsewhere, no disclosure, no previous version.
fn context_defaults(fixture: &Json) -> Result<ValidationContext, String> {
    let defaults = at_of(fixture, "validation_context_defaults")?;
    Ok(ValidationContext {
        account_equity_usd: num(
            Usd::parse(str_at(defaults, "account_equity_usd")?),
            "account_equity_usd",
        )?,
        other_allocations_usd: num(
            Usd::parse(str_at(defaults, "other_allocations_usd")?),
            "other_allocations_usd",
        )?,
        validation_date: Date::parse(str_at(defaults, "validation_date")?)
            .map_err(|e| format!("validation_date: {e}"))?,
        registry: Some(registry(at_of(defaults, "registry")?)?),
        provenance: ProvenanceMap::default(),
        workspace_users: u32_of(defaults, "workspace_users")?,
        approver_users: u32_of(defaults, "approver_users")?,
        disclosures_accepted: BTreeSet::new(),
        instrument_groups: BTreeMap::new(),
        claimed_by_other_agents: BTreeSet::new(),
        connection_environment: None,
        connection_loss_carry_usd: Usd::ZERO,
        eligibility_failures: BTreeSet::new(),
        previous_version: None,
    })
}

/// The fixture's `signal_model_registry` shape, which V-007 compares the document's models against.
fn registry(value: &Json) -> Result<BTreeMap<ModelId, RegisteredModel>, String> {
    let members = value
        .as_object()
        .ok_or_else(|| "`registry` is not an object".to_owned())?;
    members
        .iter()
        .map(|(id, model)| {
            let id =
                ModelId::parse(id).map_err(|e| format!("registry key `{id}`: {}", e.code()))?;
            let hash = str_at(model, "content_hash")?
                .strip_prefix("sha256:")
                .and_then(Digest::from_hex)
                .ok_or_else(|| format!("`{}`: content_hash is not sha256 hex", id.as_str()))?;
            Ok((
                id,
                RegisteredModel {
                    version: str_at(model, "version")?.to_owned(),
                    content_hash: hash,
                    params: list_at(model, "params")?
                        .iter()
                        .map(|p| {
                            p.as_str()
                                .map(str::to_owned)
                                .ok_or_else(|| "a param name is not a string".to_owned())
                        })
                        .collect::<Result<_, String>>()?,
                    admits_instruments: model
                        .get("admits_instruments")
                        .and_then(Json::as_bool)
                        .unwrap_or(false),
                },
            ))
        })
        .collect()
}

fn u32_of(value: &Json, key: &str) -> Result<u32, String> {
    u32::try_from(u64_at(value, key)?).map_err(|_| format!("`{key}` does not fit a u32"))
}

/// An instant in a case, in the RFC 3339 form the steps use.
fn instant(value: &Json, what: &str) -> Result<UtcNanos, String> {
    let text = value
        .as_str()
        .ok_or_else(|| format!("`{what}` is not an instant"))?;
    UtcNanos::parse_rfc3339(text).map_err(|e| format!("`{what}`: {e}"))
}

/// The member at `key`, which must be an object.
fn at_of<'a>(value: &'a Json, key: &str) -> Result<&'a Json, String> {
    let member = at(value, key)?;
    if member.is_object() {
        Ok(member)
    } else {
        Err(format!("`{key}` is not an object"))
    }
}

fn num<T>(parsed: Result<T, mandate_num::NumError>, what: &str) -> Result<T, String> {
    parsed.map_err(|e| format!("`{what}`: {e}"))
}

/// A `mandate-spec` result, with `unimplemented` turned into the message DEC-77 requires: a case must
/// never pass because the rule has not been written.
fn spec<T>(result: Result<T, SpecError>, what: &str) -> Result<T, String> {
    result.map_err(|e| {
        if e.code() == "unimplemented" {
            not_implemented(&format!("`{what}`"))
        } else {
            format!("`{what}`: {e} ({})", e.code())
        }
    })
}
