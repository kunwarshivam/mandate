//! The `mandate` harness reads every key of every case it owns, fails any kind it does not
//! interpret, and never lets a case pass for the wrong reason (DEC-85, DEC-128).
//!
//! Added in review round 1 of the tests PR, which found the three tests the brief names absent. They
//! matter because the harness is the thing that decides whether the reference cases are being checked
//! or merely being run: the first run of this suite had 31 cases "passing", thirty of them because a
//! parse that rejects everything satisfies a `schema_valid: false`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use mandate_num::Usd;
use mandate_refcases::{CaseStatus, Json, mandate, parse_status, read_fixture};
use mandate_spec::document::LadderAction;
use mandate_spec::risk::{LimitKey, RiskEvent, TriggerReason};
use serde_json::json;

fn fixture() -> Json {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases");
    Arc::unwrap_or_clone(read_fixture(&dir, "mandate.json").expect("the mandate fixture"))
}

fn run(fixture: Json, id: &str) -> Result<(), String> {
    let wanted = format!("mandate::{id}");
    let case = mandate::cases(&Arc::new(fixture))
        .into_iter()
        .find(|c| c.id == wanted)
        .unwrap_or_else(|| panic!("no case {wanted}"));
    (case.run)()
}

/// The families this harness owns, by case-ID prefix, each with its kind. Each one's size is the number
/// of cases spec §11 lists for it (`MC-S01` to `MC-S31`, and so on), read by `listed_size`.
///
/// Every count in this file is over these families alone. A family another harness owns, or one the
/// fixture gains before any arm interprets it, changes none of them; a case added to or dropped from
/// one of these families without §11 saying so fails `the_fixture_holds_the_families_this_stream_expects`,
/// naming the family.
const OWNED: [(&str, &str); 7] = [
    ("MC-S", "schema"),
    ("MC-V", "semantic"),
    ("MC-P", "policy"),
    ("MC-C", "change"),
    ("MC-R", "risk_state"),
    ("MC-T", "risk_day"),
    ("MC-L", "goal"),
];

/// The number of cases spec §11's table lists for the family `prefix`, from its `<prefix>01 to
/// <prefix>NN` range (DEC-276).
///
/// The spec and the fixture are both protected paths, so they change together in one reference PR
/// (ES-22), while this file is code and cannot ship with them. Reading the size here rather than
/// pinning it lets a reference PR add a case without turning `main` red, and still fails any case the
/// fixture gains or drops that §11 does not list.
fn listed_size(prefix: &str) -> usize {
    let range = format!("{prefix}01 to {prefix}");
    let sizes: Vec<usize> = section_11()
        .lines()
        .filter(|line| line.starts_with('|'))
        .filter_map(|line| line.split(&range).nth(1))
        .map(|rest| {
            rest.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .expect("a family's last case number")
        })
        .collect();
    assert_eq!(
        sizes.len(),
        1,
        "spec §11 lists family {prefix} as one `{range}NN` range"
    );
    sizes.into_iter().next().expect("the family's size")
}

/// Spec §11, read once.
fn section_11() -> &'static str {
    static SECTION: OnceLock<String> = OnceLock::new();
    SECTION.get_or_init(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/specs/mandate.md");
        let spec = std::fs::read_to_string(&path).expect("the mandate spec");
        spec.split("\n## 11.")
            .nth(1)
            .and_then(|rest| rest.split("\n## ").next())
            .expect("spec §11")
            .to_owned()
    })
}

/// `status.toml`, parsed once.
fn status() -> &'static BTreeMap<String, CaseStatus> {
    static STATUS: OnceLock<BTreeMap<String, CaseStatus>> = OnceLock::new();
    STATUS.get_or_init(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("status.toml");
        parse_status(&std::fs::read_to_string(&path).expect("status.toml"))
            .expect("a well-formed status.toml")
    })
}

/// Whether `status.toml` marks the case `passing`. A case it lists as `pending`, or does not list, is
/// pending (DEC-77 item 1), and need not pass yet.
fn passing(id: &str) -> bool {
    status().get(&format!("mandate::{id}")) == Some(&CaseStatus::Passing)
}

/// Every kind an arm of `mandate::cases` interprets, this harness's and the other streams'.
const INTERPRETED: [&str; 15] = [
    "schema",
    "semantic",
    "policy",
    "change",
    "risk_state",
    "risk_day",
    "goal",
    "gate",
    "agent_flatten",
    "admission",
    "lineage",
    "thesis_expiry",
    "stagger",
    "autonomy",
    "builder",
];

/// A case ID's family: `MC-V` for `MC-V07`.
fn family(id: &str) -> &str {
    id.trim_end_matches(|c: char| c.is_ascii_digit())
}

/// The ids of the cases whose family `owned` accepts, in fixture order.
fn ids_where(fixture: &Json, owned: impl Fn(&str) -> bool) -> Vec<String> {
    fixture["cases"]
        .as_array()
        .expect("a case list")
        .iter()
        .map(|c| c["id"].as_str().expect("an id"))
        .filter(|id| owned(family(id)))
        .map(str::to_owned)
        .collect()
}

/// The ids of every case of the seven owned families.
fn owned_ids(fixture: &Json) -> Vec<String> {
    ids_where(fixture, |f| OWNED.iter().any(|(prefix, _)| *prefix == f))
}

/// The ids of the owned family whose kind is `kind`.
fn ids_of(fixture: &Json, kind: &str) -> Vec<String> {
    let (prefix, _) = OWNED
        .iter()
        .find(|(_, owned)| *owned == kind)
        .unwrap_or_else(|| panic!("`{kind}` is not an owned family's kind"));
    ids_where(fixture, |f| f == *prefix)
}

/// Every key of every owned case is one the harness knows. A key the fixture gains and the harness
/// does not read fails its case, naming the key, rather than being skipped.
#[test]
fn every_owned_case_key_is_read() {
    let fixture = fixture();
    let mut checked = 0;
    for id in &owned_ids(&fixture) {
        checked += 1;
        let id = id.as_str();
        let mut doctored = fixture.clone();
        let slot = doctored["cases"]
            .as_array_mut()
            .expect("a case list")
            .iter_mut()
            .find(|c| c["id"] == id)
            .expect("the case");
        slot.as_object_mut()
            .expect("a case object")
            .insert("a_key_the_harness_does_not_know".to_owned(), Json::Null);
        let failure = run(doctored, id).expect_err("an unknown key must fail the case");
        assert!(
            failure.contains("a_key_the_harness_does_not_know"),
            "{id}: the failure must name the key it did not read, got: {failure}"
        );
    }
    let listed: usize = OWNED.iter().map(|(prefix, _)| listed_size(prefix)).sum();
    assert_eq!(
        checked, listed,
        "the seven owned families are the cases spec §11 lists for them"
    );
}

/// A case of a kind no arm interprets fails naming the kind, so nobody can mistake it for covered; and
/// every kind the fixture holds reaches an arm.
///
/// This was `a_family_another_stream_owns_fails_with_its_story` while some families waited on their
/// stories. Family N left that list with stream J's harness (E17-3), family A with stream H's (E6-2,
/// DEC-162), families G and F with stream G's (DEC-178), and family B, the last, with stream H's
/// (E6-2, DEC-250), so no story is left to name: the rule that remains is that an uninterpreted kind
/// fails rather than passes (DEC-85).
///
/// Each of the fifteen interpreted kinds is in the fixture and reaches its arm. A kind outside them,
/// such as a family the fixture gains before its arm lands, is allowed, and its cases fail until an arm
/// interprets it and joins `INTERPRETED`.
#[test]
fn a_kind_no_arm_interprets_fails_naming_it() {
    let fixture = fixture();
    let cases = fixture["cases"].as_array().expect("a case list");
    let mut kinds: Vec<(&str, &str)> = Vec::new();
    for case in cases {
        let kind = case["kind"].as_str().expect("a kind");
        if !kinds.iter().any(|(k, _)| *k == kind) {
            kinds.push((kind, case["id"].as_str().expect("an id")));
        }
    }
    for kind in INTERPRETED {
        assert!(
            kinds.iter().any(|(k, _)| *k == kind),
            "the fixture holds no `{kind}` case"
        );
    }
    for (kind, id) in &kinds {
        let outcome = run(fixture.clone(), id);
        if !INTERPRETED.contains(kind) {
            assert!(
                outcome.is_err(),
                "{id}: no arm is listed for `{kind}`, so its case must fail rather than pass; an arm \
                 that interprets it belongs in `INTERPRETED`"
            );
        } else if let Err(failure) = outcome {
            assert!(
                !failure.contains(&format!("kind `{kind}`")),
                "{id}: `{kind}` must reach an arm, got: {failure}"
            );
        }
    }

    let (_, id) = kinds.first().expect("a kind");
    let mut doctored = fixture.clone();
    doctored["cases"]
        .as_array_mut()
        .expect("a case list")
        .iter_mut()
        .find(|c| c["id"] == *id)
        .expect("the case")["kind"] = json!("a_kind_no_arm_interprets");
    let failure = run(doctored, id).expect_err("a kind no arm interprets must fail");
    assert!(
        failure.contains("`a_kind_no_arm_interprets`"),
        "{id}: the failure must name the kind, got: {failure}"
    );
}

/// A wrong expected value fails its case, and a right one passes: MC-S01 and the first rejection
/// case pass as the fixture states them and fail with their `schema_valid` flipped, and an
/// expectation the harness cannot read fails loudly.
///
/// Pending on E10-1 because the first half needs a working parse. The version this replaces
/// asserted that the stub fails MC-S01, which any correct parse turns red (#225, the coordinator's
/// round 1 ruling there).
#[test]
fn a_wrong_expected_value_fails_the_case() {
    let fixture = fixture();
    let valid = fixture["cases"]
        .as_array()
        .expect("a case list")
        .iter()
        .find(|c| c["kind"] == "schema" && c["expect"]["schema_valid"] == true)
        .and_then(|c| c["id"].as_str())
        .expect("MC-S01 expects a valid document")
        .to_owned();
    if let Err(failure) = run(fixture.clone(), &valid) {
        panic!("{valid} expects the base to parse: {failure}");
    }

    let rejected = fixture["cases"]
        .as_array()
        .expect("a case list")
        .iter()
        .find(|c| c["kind"] == "schema" && c["expect"]["schema_valid"] == false)
        .and_then(|c| c["id"].as_str())
        .expect("a rejection case")
        .to_owned();
    if let Err(failure) = run(fixture.clone(), &rejected) {
        panic!("{rejected} expects the parse to reject its document: {failure}");
    }

    for (id, flipped) in [(&valid, false), (&rejected, true)] {
        let mut wrong = fixture.clone();
        let slot = wrong["cases"]
            .as_array_mut()
            .expect("a case list")
            .iter_mut()
            .find(|c| c["id"] == id.as_str())
            .expect("the case");
        slot["expect"]["schema_valid"] = Json::Bool(flipped);
        assert!(
            run(wrong, id).is_err(),
            "{id}: a wrong expected value must fail the case"
        );
    }

    let mut doctored = fixture.clone();
    let slot = doctored["cases"]
        .as_array_mut()
        .expect("a case list")
        .iter_mut()
        .find(|c| c["id"] == rejected.as_str())
        .expect("the case");
    slot["expect"]["schema_valid"] = Json::String("maybe".to_owned());
    let failure = run(doctored, &rejected).expect_err("a non-boolean expectation must fail");
    assert!(
        failure.contains("schema_valid"),
        "{rejected}: an expectation the harness cannot read must fail loudly, got: {failure}"
    );
}

/// The owned families' shape, so a regenerated file that renames, renumbers, or drops one of their
/// cases is caught here, naming the family, rather than by a run of individually confusing failures;
/// and one test per fixture case, so no case of any family is dropped between the fixture and the run.
///
/// Families this harness does not own are counted by their own harnesses, and a family the fixture
/// gains changes nothing here.
#[test]
fn the_fixture_holds_the_families_this_stream_expects() {
    let fixture = fixture();
    let cases = fixture["cases"].as_array().expect("a case list");
    for (prefix, kind) in OWNED {
        let count = listed_size(prefix);
        let mut ids = ids_where(&fixture, |f| f == prefix);
        ids.sort();
        let listed: Vec<String> = (1..=count).map(|n| format!("{prefix}{n:02}")).collect();
        assert_eq!(
            ids, listed,
            "family {prefix} is {prefix}01 to {prefix}{count:02}, as spec §11 lists it"
        );
        for id in &ids {
            let case = cases
                .iter()
                .find(|c| c["id"] == id.as_str())
                .expect("the case");
            assert_eq!(case["kind"], kind, "{id}: family {prefix} is `{kind}`");
        }
    }
    let mut expected = vec![
        "mandate::version".to_owned(),
        "mandate::version_vector".to_owned(),
    ];
    expected.extend(
        cases
            .iter()
            .map(|c| format!("mandate::{}", c["id"].as_str().expect("an id"))),
    );
    let run: Vec<String> = mandate::cases(&Arc::new(fixture.clone()))
        .into_iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(
        run, expected,
        "one test per fixture case, plus `version` and `version_vector`"
    );
}

/// Every member of every owned case's `expect` is one the harness reads, at the top level and inside
/// a `risk_state` step.
///
/// This is planted bug 26's oracle, and review round 2 found it missing: sweeping only the top-level
/// keys let an `expect` grow a member that nothing compared, so the case would pass while checking
/// less than it claims. The bug is the silence, not the wrong value, so the planted key is inserted
/// beside the real expectations rather than replacing one.
#[test]
fn every_owned_expectation_member_is_read() {
    let fixture = fixture();
    let mut top_level = 0;
    let mut in_steps = 0;
    for id in &owned_ids(&fixture) {
        let id = id.as_str();
        let mut doctored = fixture.clone();
        let slot = doctored["cases"]
            .as_array_mut()
            .expect("a case list")
            .iter_mut()
            .find(|c| c["id"] == id)
            .expect("the case");
        let planted = "an_expectation_the_harness_does_not_read".to_owned();
        let mut planted_somewhere = false;
        if let Some(expect) = slot.get_mut("expect").and_then(Json::as_object_mut) {
            expect.insert(planted.clone(), Json::Null);
            planted_somewhere = true;
            top_level += 1;
        }
        for step in slot
            .get_mut("steps")
            .and_then(Json::as_array_mut)
            .map(Vec::as_mut_slice)
            .unwrap_or_default()
        {
            if let Some(expect) = step.get_mut("expect").and_then(Json::as_object_mut) {
                expect.insert(planted.clone(), Json::Null);
                planted_somewhere = true;
                in_steps += 1;
            }
        }
        assert!(
            planted_somewhere,
            "{id}: an owned case with no expectation at all would be checking nothing"
        );
        let failure = run(doctored, id).expect_err("an unread expectation must fail the case");
        assert!(
            failure.contains(&planted),
            "{id}: the failure must name the expectation it did not read, got: {failure}"
        );
    }
    let risk_cases = ids_of(&fixture, "risk_state");
    let risk_steps: usize = fixture["cases"]
        .as_array()
        .expect("a case list")
        .iter()
        .filter(|c| risk_cases.iter().any(|id| c["id"] == id.as_str()))
        .map(|c| c["steps"].as_array().map_or(0, Vec::len))
        .sum();
    assert_eq!(
        (top_level, in_steps),
        (
            owned_ids(&fixture)
                .len()
                .saturating_sub(listed_size("MC-R")),
            risk_steps
        ),
        "both sweeps must have been exercised over every expectation the owned cases carry: one \
         top-level expectation per case outside family R, and one per step of family R"
    );
    assert!(
        top_level > 0 && in_steps > 0,
        "neither sweep may be vacuous"
    );
}

/// A value no expectation of these families can hold, in the shape of the one it replaces: a string
/// becomes a name no rule writes, a boolean flips, a number moves, and a list's first member becomes one
/// no case lists, keeping its size so a comparison by size alone cannot pass it (an empty list gains that
/// member instead). The edited case can therefore only fail on the member that was edited.
fn edited(value: &Json) -> Json {
    match value {
        Json::Bool(flag) => Json::Bool(!flag),
        Json::Number(number) => json!(number.as_u64().expect("a count").saturating_add(3_600)),
        Json::Array(items) => {
            let stranger = if items.first().is_some_and(Json::is_object) {
                json!({"type": "NotAnEvent"})
            } else {
                json!("not_a_name")
            };
            let mut items = items.clone();
            match items.first_mut() {
                Some(first) => *first = stranger,
                None => items.push(stranger),
            }
            Json::Array(items)
        }
        _ => json!("0.123456789"),
    }
}

/// Every MC-L case passes as the fixture states it, and fails, naming the member, when any one of its
/// expectations is edited.
///
/// This is the goal arm's read-every-key half, which the stub test it replaces could not state: every
/// MC-L case failed at a stub whatever its expectations said. A stub is still never a pass: `cargo xtask
/// ci pending` requires this test to fail at one until E6-4's `goal::status` lands (DEC-137), which is
/// the check the removed test made, and which that test could not keep making once E10-1's slice P moved
/// the stub these cases stop at (DEC-77).
#[test]
fn every_goal_case_passes_and_fails_on_each_edited_expectation() {
    let fixture = fixture();
    let ids = ids_of(&fixture, "goal");
    assert_eq!(ids.len(), 5, "family L is five cases");
    let mut edits = 0;
    for id in &ids {
        if let Err(failure) = run(fixture.clone(), id) {
            panic!("{id} must pass as the fixture states it: {failure}");
        }
        let members: Vec<String> = fixture["cases"]
            .as_array()
            .expect("a case list")
            .iter()
            .find(|c| c["id"] == id.as_str())
            .and_then(|c| c["expect"].as_object())
            .expect("an expectation")
            .keys()
            .cloned()
            .collect();
        for member in members {
            let mut doctored = fixture.clone();
            let expect = &mut doctored["cases"]
                .as_array_mut()
                .expect("a case list")
                .iter_mut()
                .find(|c| c["id"] == id.as_str())
                .expect("the case")["expect"];
            expect[&member] = edited(&expect[&member]);
            let failure = run(doctored, id).expect_err("an edited expectation must fail the case");
            assert!(
                failure.contains(&member),
                "{id}: an edited `{member}` must fail on `{member}`, got: {failure}"
            );
            edits += 1;
        }
    }
    assert_eq!(
        edits, 17,
        "four members of each done case and one of the running one"
    );
}

/// Every MC-C case passes as the fixture states it, and fails, naming the member, when any one of its
/// expectations is edited or dropped; and the one invalid case, which states no `step_up_required`,
/// fails when it is given one saying step-up is needed.
///
/// The change arm's read-every-key half (DEC-172 item 4). Editing shows each member is compared, and
/// dropping shows none is optional: the arm reads `step_up_required` as absent only where the case
/// itself says `invalid`, so leaving it out elsewhere cannot pass.
#[test]
fn every_change_case_passes_and_fails_on_each_edited_expectation() {
    let fixture = fixture();
    let ids = ids_of(&fixture, "change");
    assert_eq!(ids.len(), 48, "family C is 48 cases");
    let expectation = |doctored: &mut Json, id: &str| -> Json {
        doctored["cases"]
            .as_array_mut()
            .expect("a case list")
            .iter_mut()
            .find(|c| c["id"] == id)
            .map(|c| c["expect"].take())
            .expect("the case")
    };
    let put = |doctored: &mut Json, id: &str, expect: Json| {
        if let Some(case) = doctored["cases"]
            .as_array_mut()
            .expect("a case list")
            .iter_mut()
            .find(|c| c["id"] == id)
        {
            case["expect"] = expect;
        }
    };
    let mut edits = 0;
    for id in &ids {
        if let Err(failure) = run(fixture.clone(), id) {
            panic!("{id} must pass as the fixture states it: {failure}");
        }
        let expect = expectation(&mut fixture.clone(), id);
        let members: Vec<String> = expect
            .as_object()
            .expect("an expectation")
            .keys()
            .cloned()
            .collect();
        for member in members {
            let mut edited_expect = expect.clone();
            edited_expect[&member] = edited(&expect[&member]);
            let mut dropped_expect = expect.clone();
            dropped_expect
                .as_object_mut()
                .expect("an expectation")
                .remove(&member);
            for (how, changed) in [("edited", edited_expect), ("dropped", dropped_expect)] {
                let mut doctored = fixture.clone();
                put(&mut doctored, id, changed);
                let failure = run(doctored, id)
                    .expect_err("an edited or dropped expectation must fail the case");
                assert!(
                    failure.contains(&member),
                    "{id}: a {how} `{member}` must fail on `{member}`, got: {failure}"
                );
                edits += 1;
            }
        }
    }
    assert_eq!(
        edits,
        2 * (47 * 5 + 4),
        "five members of each case, four of the invalid one, each edited and dropped"
    );
    let invalid = ids
        .iter()
        .find(|id| {
            let mut copy = fixture.clone();
            expectation(&mut copy, id)["classification"] == "invalid"
        })
        .expect("an invalid case");
    let mut doctored = fixture.clone();
    let mut expect = expectation(&mut fixture.clone(), invalid);
    expect["step_up_required"] = Json::Bool(true);
    put(&mut doctored, invalid, expect);
    let failure = run(doctored, invalid).expect_err("an invalid change is refused, not stepped up");
    assert!(
        failure.contains("step_up_required"),
        "{invalid}: got {failure}"
    );
}

/// The edits `every_risk_state_case_passes_and_fails_on_each_edited_expectation` makes on one case,
/// counted from the fixture alone: one per member of each step, except that a non-empty journal is
/// edited once per member of each of its events, and a non-empty name set once per name and twice more
/// (a name dropped, a stranger added).
fn risk_state_edits(case: &Json) -> usize {
    case["steps"]
        .as_array()
        .expect("a step list")
        .iter()
        .flat_map(|step| step["expect"].as_object().expect("a step expectation"))
        .map(|(member, value)| match value.as_array() {
            Some(events) if member == "journal" && !events.is_empty() => events
                .iter()
                .map(|e| e.as_object().expect("a journal event").len())
                .sum(),
            Some(names) if !names.is_empty() => names.len() + 2,
            _ => 1,
        })
        .sum()
}

/// Every MC-R case `status.toml` marks passing passes as the fixture states it, and fails, naming the
/// member and its step, when any one expectation of any one step is edited.
///
/// A pending case (DEC-77 item 1) is left to `refcases.rs`, which runs it only with
/// `--include-ignored`: a reference PR can move a case's expectation ahead of the code that meets it,
/// as DEC-270 moved MC-R17's journal, and the code cannot ship with that PR (ES-22, DEC-276). The
/// other sweeps in this file still read every key and member of a pending case.
///
/// The risk-state arm's read-every-key half, for the reason the goal test above gives. The member sweep
/// in `every_owned_expectation_member_is_read` shows a member the harness does not know is refused; this
/// shows each member it knows is compared, which an arm that read a member and ignored its value would
/// pass the sweep and fail here.
///
/// A non-empty name set (`restrictions`, `instrument_restrictions`, `pending`) is edited name by name,
/// for the same reason as the journal below: `edited` replaces a list's first element only, so a second
/// name was never swept (#276 review, and the coordinator's ruling there). Each such set is also edited
/// once with its first name dropped and once with a stranger added, because substitution alone keeps
/// the set's size and so cannot tell an equality from a subset or a superset comparison, and a fold
/// that drops a restriction is the loosening direction (#283 review, and the coordinator's ruling there).
///
/// A non-empty journal is edited member by member in every event, not by replacing its first event:
/// replacing the first event left every later event's members unswept, so a fold that journalled the
/// right first event and a wrong second one could not be told from a right one here (#271 review, note
/// 2). An empty journal still gains one stranger event.
#[test]
fn every_risk_state_case_passes_and_fails_on_each_edited_expectation() {
    let fixture = fixture();
    let family = ids_of(&fixture, "risk_state");
    assert_eq!(
        family.len(),
        listed_size("MC-R"),
        "family R is the cases spec §11 lists"
    );
    let ids: Vec<String> = family.into_iter().filter(|id| passing(id)).collect();
    assert!(!ids.is_empty(), "at least one MC-R case is passing");
    let listed_passing: Vec<String> = status()
        .iter()
        .filter(|(key, state)| key.starts_with("mandate::MC-R") && **state == CaseStatus::Passing)
        .map(|(key, _)| key.trim_start_matches("mandate::").to_owned())
        .collect();
    let mut swept = ids.clone();
    swept.sort();
    assert_eq!(
        swept, listed_passing,
        "the sweep covers exactly the MC-R cases status.toml marks passing"
    );
    let mut edits = 0;
    for id in &ids {
        if let Err(failure) = run(fixture.clone(), id) {
            panic!("{id} must pass as the fixture states it: {failure}");
        }
        let steps: Vec<Vec<String>> = fixture["cases"]
            .as_array()
            .expect("a case list")
            .iter()
            .find(|c| c["id"] == id.as_str())
            .and_then(|c| c["steps"].as_array())
            .expect("a step list")
            .iter()
            .map(|step| {
                step["expect"]
                    .as_object()
                    .expect("a step expectation")
                    .keys()
                    .cloned()
                    .collect()
            })
            .collect();
        for (index, members) in steps.iter().enumerate() {
            for member in members {
                let step = format!("step {}:", index.saturating_add(1));
                let listed = fixture["cases"]
                    .as_array()
                    .expect("a case list")
                    .iter()
                    .find(|c| c["id"] == id.as_str())
                    .expect("the case")["steps"][index]["expect"][member]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let events: &[Json] = if member == "journal" { &listed } else { &[] };
                for (position, event) in events.iter().enumerate() {
                    for key in event.as_object().expect("a journal event").keys() {
                        let mut doctored = fixture.clone();
                        let value = &mut doctored["cases"]
                            .as_array_mut()
                            .expect("a case list")
                            .iter_mut()
                            .find(|c| c["id"] == id.as_str())
                            .expect("the case")["steps"][index]["expect"]["journal"][position][key];
                        *value = edited(value);
                        let failure = run(doctored, id)
                            .expect_err("an edited journal member must fail the case");
                        let named = format!("journal event {}:", position.saturating_add(1));
                        assert!(
                            failure.contains(&step) && failure.contains(&named),
                            "{id}: an edited `{key}` of {named} at {step} must fail there, naming the event, got: {failure}"
                        );
                        edits += 1;
                    }
                }
                let names: &[Json] = if member == "journal" { &[] } else { &listed };
                for (position, name) in names.iter().enumerate() {
                    let mut doctored = fixture.clone();
                    let value = &mut doctored["cases"]
                        .as_array_mut()
                        .expect("a case list")
                        .iter_mut()
                        .find(|c| c["id"] == id.as_str())
                        .expect("the case")["steps"][index]["expect"][member][position];
                    *value = edited(name);
                    let failure =
                        run(doctored, id).expect_err("an edited listed name must fail the case");
                    assert!(
                        failure.contains(&step) && failure.contains(member.as_str()),
                        "{id}: an edited name {} of `{member}` at {step} must fail there, naming it, got: {failure}",
                        position.saturating_add(1)
                    );
                    edits += 1;
                }
                if !names.is_empty() {
                    let shorter: Vec<Json> = names.iter().skip(1).cloned().collect();
                    let mut longer = names.to_vec();
                    longer.push(json!("not_a_name"));
                    for (edit, list) in
                        [("one name dropped", shorter), ("a stranger added", longer)]
                    {
                        let mut doctored = fixture.clone();
                        doctored["cases"]
                            .as_array_mut()
                            .expect("a case list")
                            .iter_mut()
                            .find(|c| c["id"] == id.as_str())
                            .expect("the case")["steps"][index]["expect"][member] =
                            Json::Array(list);
                        let failure = run(doctored, id).expect_err(
                            "a name set with a name dropped or added must fail the case",
                        );
                        assert!(
                            failure.contains(&step) && failure.contains(member.as_str()),
                            "{id}: `{member}` with {edit} at {step} must fail there, naming it, got: {failure}"
                        );
                        edits += 1;
                    }
                }
                if !listed.is_empty() {
                    continue;
                }
                let mut doctored = fixture.clone();
                let expect = &mut doctored["cases"]
                    .as_array_mut()
                    .expect("a case list")
                    .iter_mut()
                    .find(|c| c["id"] == id.as_str())
                    .expect("the case")["steps"][index]["expect"];
                expect[member] = edited(&expect[member]);
                let failure =
                    run(doctored, id).expect_err("an edited expectation must fail the case");
                assert!(
                    failure.contains(&step) && failure.contains(member.as_str()),
                    "{id}: an edited `{member}` at {step} must fail there, naming it, got: {failure}"
                );
                edits += 1;
            }
        }
    }
    let expected: usize = fixture["cases"]
        .as_array()
        .expect("a case list")
        .iter()
        .filter(|c| ids.iter().any(|id| c["id"] == id.as_str()))
        .map(risk_state_edits)
        .sum();
    assert_eq!(
        edits, expected,
        "every member of every step of the passing cases was edited once, every member of every \
         journal event, every name the three name sets list, each in place of its list's first element \
         alone, and each non-empty name set once with a name dropped and once with a stranger added"
    );
}

/// Every MC-T case passes as the fixture states it, and fails, naming the member, when any one of its
/// four expectations is edited.
///
/// This is the read-every-key half the stub test above promised for the risk-day arm. The two halves
/// need each other: the pass alone would be satisfied by an arm that compared nothing, and the edits
/// alone by an arm that failed everything, which is what the `risk_day` stub does. Each edit moves its
/// member to a value no MC-T case can produce — a date in 2000, an instant at that date's UTC midnight,
/// a length one hour longer — so the edited case can only fail on the member that was edited.
#[test]
fn every_risk_day_case_passes_and_fails_on_each_edited_expectation() {
    let fixture = fixture();
    let ids = ids_of(&fixture, "risk_day");
    assert_eq!(ids.len(), 5, "family T is five cases");
    let mut edited = 0;
    for id in &ids {
        if let Err(failure) = run(fixture.clone(), id) {
            panic!("{id} must pass as the fixture states it: {failure}");
        }
        for member in ["risk_day", "starts_at", "ends_at", "length_s"] {
            let mut doctored = fixture.clone();
            let expect = &mut doctored["cases"]
                .as_array_mut()
                .expect("a case list")
                .iter_mut()
                .find(|c| c["id"] == id.as_str())
                .expect("the case")["expect"];
            expect[member] = match member {
                "risk_day" => json!("2000-01-01"),
                "length_s" => json!(
                    expect["length_s"]
                        .as_u64()
                        .and_then(|seconds| seconds.checked_add(3_600))
                        .expect("a length in seconds")
                ),
                _ => json!("2000-01-01T00:00:00.000000000Z"),
            };
            let failure = run(doctored, id).expect_err("an edited expectation must fail the case");
            assert!(
                failure.starts_with(&format!("{member}: expected")),
                "{id}: an edited `{member}` must fail on `{member}`, got: {failure}"
            );
            edited += 1;
        }
    }
    assert_eq!(
        edited, 20,
        "four members of each of the five cases were edited"
    );
}

/// Every field of every `risk_state` step is one the harness reads, and so is every field of `initial`.
///
/// The `expect` sweep is one level down from the case; a step's own fields are another, and a `mark` that
/// grew an `ask` or a `fill` that grew a `fee` would otherwise change nothing and be believed. The same
/// holds for `initial`, which is what `RiskState::open` is built from.
#[test]
fn every_risk_state_step_field_and_initial_field_is_read() {
    let fixture = fixture();
    let ids = ids_of(&fixture, "risk_state");
    assert_eq!(
        ids.len(),
        listed_size("MC-R"),
        "family R is the cases spec §11 lists"
    );
    let planted = "a_field_the_harness_does_not_read".to_owned();
    let mut steps_planted = 0;
    for id in &ids {
        for (where_, plant) in [("step", true), ("initial", false)] {
            let mut doctored = fixture.clone();
            let slot = doctored["cases"]
                .as_array_mut()
                .expect("a case list")
                .iter_mut()
                .find(|c| c["id"] == id.as_str())
                .expect("the case");
            if plant {
                let first = slot["steps"]
                    .as_array_mut()
                    .expect("a step list")
                    .first_mut()
                    .expect("at least one step");
                first
                    .as_object_mut()
                    .expect("a step object")
                    .insert(planted.clone(), Json::Null);
                steps_planted += 1;
            } else {
                slot["initial"]
                    .as_object_mut()
                    .expect("an initial object")
                    .insert(planted.clone(), Json::Null);
            }
            let failure = run(doctored, id).expect_err("an unread field must fail the case");
            assert!(
                failure.contains(&planted),
                "{id}: the {where_} failure must name the field it did not read, got: {failure}"
            );
        }
    }
    assert_eq!(
        steps_planted,
        ids.len(),
        "every case's first step was doctored"
    );
}

/// A step whose `event` the harness does not apply fails its case, naming the kind.
///
/// Ten input kinds is the whole of §5.2's list; an eleventh would be a rule nothing folds, and a harness
/// that quietly skipped it would report a pass over a shorter walk than the case states.
#[test]
fn a_step_whose_event_the_harness_does_not_apply_fails_the_case() {
    let fixture = fixture();
    let id = ids_of(&fixture, "risk_state")
        .into_iter()
        .next()
        .expect("a risk_state case");
    let mut doctored = fixture.clone();
    let slot = doctored["cases"]
        .as_array_mut()
        .expect("a case list")
        .iter_mut()
        .find(|c| c["id"] == id.as_str())
        .expect("the case");
    slot["steps"]
        .as_array_mut()
        .expect("a step list")
        .first_mut()
        .expect("at least one step")["event"] = Json::String("teleported".to_owned());
    let failure = run(doctored, &id).expect_err("an unknown input kind must fail the case");
    assert!(
        failure.contains("teleported"),
        "{id}: the failure must name the kind it could not apply, got: {failure}"
    );
}

/// One way of spoiling a case's journal, with the name the failure message uses.
type Perturbation = (&'static str, fn(&mut Json));

/// The journal comparison fails on any difference: a missing member, an extra one, a wrong value, or a
/// different number of events.
///
/// Exercised directly rather than through a case, because every `risk_state` case stops at the mandate
/// parser while that is a stub — a test that went through one could not tell this comparison from the stub
/// it never reached. It is the function the two shapes under this change's Decisions needed rest on:
/// A member the fixture states and no `RiskEvent` carries must **fail** rather than be skipped: that is
/// what turned up the two shapes DEC-128 item 29 added to the API, and it is what would turn up the next
/// one.
#[test]
fn the_journal_comparison_fails_on_any_difference() {
    let events = vec![
        RiskEvent::RiskDayStarted {
            day_start_equity: Usd::parse("9800").expect("a dollar amount"),
        },
        RiskEvent::RiskLimitTriggered {
            limit: LimitKey::MaxDailyLoss,
            action: LadderAction::ExitsOnly,
            reason: Some(TriggerReason::ResolvedAtRollover),
        },
    ];
    let matching = json!({"journal": [
        {"type": "RiskDayStarted", "day_start_equity": "9800"},
        {"type": "RiskLimitTriggered", "limit": "max_daily_loss", "action": "exits_only",
         "reason": "resolved_at_rollover"},
    ]});
    assert_eq!(
        mandate::expect_journal(&matching, &events),
        Ok(()),
        "the events the case states are the events it got"
    );

    let doctored: [Perturbation; 5] = [
        ("a member no event carries", |j: &mut Json| {
            j["journal"][0]["a_member_no_event_carries"] = Json::String("9800".to_owned());
        }),
        ("a wrong value", |j: &mut Json| {
            j["journal"][0]["day_start_equity"] = Json::String("9801".to_owned());
        }),
        ("a member the event does carry, dropped", |j: &mut Json| {
            j["journal"][1]
                .as_object_mut()
                .expect("an event object")
                .remove("reason");
        }),
        ("one event too few", |j: &mut Json| {
            j["journal"].as_array_mut().expect("a journal").pop();
        }),
        ("one event too many", |j: &mut Json| {
            let extra = json!({"type": "RiskDayStarted", "day_start_equity": "9800"});
            j["journal"].as_array_mut().expect("a journal").push(extra);
        }),
    ];
    for (what, doctor) in doctored {
        let mut case = matching.clone();
        doctor(&mut case);
        let failure = mandate::expect_journal(&case, &events);
        assert!(
            failure.is_err(),
            "{what} must fail the comparison, and it reported {failure:?}"
        );
    }

    let named = mandate::expect_journal(
        &json!({"journal": [
            {"type": "RiskDayStarted", "day_start_equity": "9800",
             "max_loss_from_allocation": "0.2"},
        ]}),
        &events[..1],
    )
    .expect_err("a member `RiskEvent` has no field for must fail");
    assert!(
        named.contains("max_loss_from_allocation"),
        "the failure must name the member the harness could not supply, got: {named}"
    );
}
