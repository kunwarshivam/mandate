//! The `mandate` harness reads every key of every case it owns, dispatches every family it does not
//! own to the story that will, and never lets a case pass for the wrong reason (DEC-85, DEC-128).
//!
//! Added in review round 1 of the tests PR, which found the three tests the brief names absent. They
//! matter because the harness is the thing that decides whether 202 reference cases are being checked
//! or merely being run: the first run of this suite had 31 cases "passing", thirty of them because a
//! parse that rejects everything satisfies a `schema_valid: false`.

use std::path::Path;
use std::sync::Arc;

use mandate_num::Usd;
use mandate_refcases::{Json, mandate, read_fixture};
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

/// Every key of every owned case is one the harness knows. A key the fixture gains and the harness
/// does not read fails its case, naming the key, rather than being skipped.
#[test]
fn every_owned_case_key_is_read() {
    let owned = [
        "schema",
        "semantic",
        "policy",
        "change",
        "risk_state",
        "risk_day",
        "goal",
    ];
    let fixture = fixture();
    let cases = fixture["cases"].as_array().expect("a case list");
    let mut checked = 0;
    for case in cases {
        let kind = case["kind"].as_str().expect("a kind");
        if !owned.contains(&kind) {
            continue;
        }
        checked += 1;
        let id = case["id"].as_str().expect("an id");
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
    assert_eq!(
        checked, 202,
        "the seven owned families are 202 of the fixture's 298 cases"
    );
}

/// A family another stream owns fails with that stream's story, so nobody can mistake it for covered.
/// Family N (`admission`, `lineage`, `thesis_expiry`, `stagger`) left this list when stream J's
/// harness interpreted it (E17-3, DEC-77 stage 4); its own oracle is in `src/mandate/research.rs`.
#[test]
fn a_family_another_stream_owns_fails_with_its_story() {
    let fixture = fixture();
    let expected = [
        ("gate", "E6-3"),
        ("agent_flatten", "E6-3"),
        ("builder", "E6-2"),
        ("autonomy", "E6-2"),
    ];
    let mut seen = 0;
    for (kind, story) in expected {
        let id = fixture["cases"]
            .as_array()
            .expect("a case list")
            .iter()
            .find(|c| c["kind"] == kind)
            .and_then(|c| c["id"].as_str())
            .unwrap_or_else(|| panic!("the fixture has no `{kind}` case"));
        seen += 1;
        let failure = run(fixture.clone(), id).expect_err("another stream's family must fail");
        assert!(
            failure.contains(story) && failure.contains("not interpreted until"),
            "{id} (`{kind}`) must name {story}, got: {failure}"
        );
    }
    assert_eq!(seen, 4, "all four unowned kinds are dispatched");
}

/// A wrong expected value fails its case, and a right one passes: MC-S01 and the first rejection
/// case pass as the fixture states them and fail with their `schema_valid` flipped, and an
/// expectation the harness cannot read fails loudly.
///
/// Pending on E10-1 because the first half needs a working parse. The version this replaces
/// asserted that the stub fails MC-S01, which any correct parse turns red (#225, the coordinator's
/// round 1 ruling there).
#[test]
#[ignore = "pending E10-1"]
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

/// The fixture's own shape, so a regenerated file that renames a family or drops a case is caught here
/// rather than by 298 individually confusing failures.
#[test]
fn the_fixture_holds_the_families_this_stream_expects() {
    let fixture = fixture();
    let cases = fixture["cases"].as_array().expect("a case list");
    assert_eq!(cases.len(), 298, "spec §11 states 298 cases");
    let count = |kind: &str| cases.iter().filter(|c| c["kind"] == kind).count();
    for (kind, expected) in [
        ("schema", 31),
        ("semantic", 67),
        ("policy", 22),
        ("change", 48),
        ("risk_state", 24),
        ("risk_day", 5),
        ("goal", 5),
    ] {
        assert_eq!(count(kind), expected, "`{kind}` count");
    }
    assert_eq!(
        mandate::cases(&Arc::new(fixture)).len(),
        300,
        "298 cases plus `version` and `version_vector`"
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
    let owned = [
        "schema",
        "semantic",
        "policy",
        "change",
        "risk_state",
        "risk_day",
        "goal",
    ];
    let fixture = fixture();
    let cases = fixture["cases"].as_array().expect("a case list");
    let mut top_level = 0;
    let mut in_steps = 0;
    for case in cases {
        let kind = case["kind"].as_str().expect("a kind");
        if !owned.contains(&kind) {
            continue;
        }
        let id = case["id"].as_str().expect("an id");
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
    assert_eq!(
        (top_level, in_steps),
        (178, 111),
        "both sweeps must have been exercised over every expectation the owned cases carry"
    );
}

/// The risk-day and goal arms fail with the DEC-77 message while their rules are stubs, and name the
/// stub rather than a comparison.
///
/// The `trading_domain` suite proves its arms read each key by editing an expectation and requiring the
/// case to fail. That test cannot be written for these two families yet: every MC-T and MC-L case fails
/// on the stub regardless of what its expectations say, so an edited value would fail for the same
/// reason as an unedited one and prove nothing. What is checkable now is the other half — that a stub
/// is never mistaken for a pass — and the read-every-key half arrives with the implementation PR, where
/// an edited `starts_at` or `then` must change the outcome.
///
/// A goal or risk-state case stops at the first stub on its path: the parser until E10-1's parse
/// lands, then validation's `rule` stub until E10-1's V-rules do (#225, round 1).
#[test]
fn the_risk_day_and_goal_arms_name_their_stub_rather_than_passing() {
    let fixture = fixture();
    for (kind, stubs) in [
        ("risk_day", &["risk_day"][..]),
        ("goal", &["parser", "rule"][..]),
        ("risk_state", &["parser", "rule"][..]),
    ] {
        let ids: Vec<String> = fixture["cases"]
            .as_array()
            .expect("a case list")
            .iter()
            .filter(|c| c["kind"] == kind)
            .map(|c| c["id"].as_str().expect("an id").to_owned())
            .collect();
        assert!(!ids.is_empty(), "the fixture carries {kind} cases");
        for id in ids {
            let failure = run(fixture.clone(), &id).expect_err("a stub is never a pass");
            assert!(
                failure.contains("not implemented yet")
                    && stubs.iter().any(|stub| failure.contains(stub)),
                "{id}: the failure must name the stub it stopped at, got: {failure}"
            );
        }
    }
}

/// Every field of every `risk_state` step is one the harness reads, and so is every field of `initial`.
///
/// The `expect` sweep is one level down from the case; a step's own fields are another, and a `mark` that
/// grew an `ask` or a `fill` that grew a `fee` would otherwise change nothing and be believed. The same
/// holds for `initial`, which is what `RiskState::open` is built from.
#[test]
fn every_risk_state_step_field_and_initial_field_is_read() {
    let fixture = fixture();
    let ids: Vec<String> = fixture["cases"]
        .as_array()
        .expect("a case list")
        .iter()
        .filter(|c| c["kind"] == "risk_state")
        .map(|c| c["id"].as_str().expect("an id").to_owned())
        .collect();
    assert_eq!(ids.len(), 24, "family R is 24 cases");
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
    assert_eq!(steps_planted, 24, "every case's first step was doctored");
}

/// A step whose `event` the harness does not apply fails its case, naming the kind.
///
/// Ten input kinds is the whole of §5.2's list; an eleventh would be a rule nothing folds, and a harness
/// that quietly skipped it would report a pass over a shorter walk than the case states.
#[test]
fn a_step_whose_event_the_harness_does_not_apply_fails_the_case() {
    let fixture = fixture();
    let id = fixture["cases"]
        .as_array()
        .expect("a case list")
        .iter()
        .find(|c| c["kind"] == "risk_state")
        .and_then(|c| c["id"].as_str())
        .expect("a risk_state case")
        .to_owned();
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
