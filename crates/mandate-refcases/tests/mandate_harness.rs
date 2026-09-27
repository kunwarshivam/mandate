//! The `mandate` harness reads every key of every case it owns, dispatches every family it does not
//! own to the story that will, and never lets a case pass for the wrong reason (DEC-85, DEC-128).
//!
//! Added in review round 1 of the tests PR, which found the three tests the brief names absent. They
//! matter because the harness is the thing that decides whether 202 reference cases are being checked
//! or merely being run: the first run of this suite had 31 cases "passing", thirty of them because a
//! parse that rejects everything satisfies a `schema_valid: false`.

use std::path::Path;
use std::sync::Arc;

use mandate_refcases::{Json, mandate, read_fixture};

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
#[test]
fn a_family_another_stream_owns_fails_with_its_story() {
    let fixture = fixture();
    let expected = [
        ("gate", "E6-3"),
        ("agent_flatten", "E6-3"),
        ("builder", "E6-2"),
        ("autonomy", "E6-2"),
        ("admission", "E17-3"),
        ("lineage", "E17-3"),
        ("thesis_expiry", "E17-3"),
        ("stagger", "E17-3"),
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
    assert_eq!(seen, 8, "all eight unowned kinds are dispatched");
}

/// A wrong expected value fails its case. On the stubs only the schema family can show this, because
/// it is the one owned family whose expectation the harness already compares; the other six report
/// their rule as unimplemented, which is itself a failure and is what keeps them from passing early.
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
    assert!(
        run(fixture.clone(), &valid).is_err(),
        "{valid} expects the base to parse, which the stub cannot do, so it fails now"
    );

    let rejected = fixture["cases"]
        .as_array()
        .expect("a case list")
        .iter()
        .find(|c| c["kind"] == "schema" && c["expect"]["schema_valid"] == false)
        .and_then(|c| c["id"].as_str())
        .expect("a rejection case")
        .to_owned();
    let failure = run(fixture.clone(), &rejected).expect_err("a stubbed parse names no reason");
    assert!(
        failure.contains("not implemented"),
        "{rejected}: a rejection with no reason must not count as a rejection, got: {failure}"
    );

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

/// Every input member of every owned case is one the harness turns into a typed value: an `initial`
/// field, a step field, and a `goal` case's `state`.
///
/// Tests PR 1 swept the `expect` side and left this one open, which planted bug 26 also reaches from:
/// a step that grew a field the harness never read would fold as though the field were not there, and
/// the case would still compare the snapshot and pass. The three sites are counted exactly, because a
/// case that stops carrying an input stops being swept and a loose bound would not notice.
#[test]
fn every_owned_input_member_is_read() {
    let planted = "an_input_the_harness_does_not_read".to_owned();
    let fixture = fixture();
    let cases = fixture["cases"].as_array().expect("a case list");
    let (mut initials, mut steps, mut states) = (0, 0, 0);
    for case in cases {
        let kind = case["kind"].as_str().expect("a kind");
        if kind != "risk_state" && kind != "goal" {
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
        for block in ["initial", "state"] {
            if let Some(members) = slot.get_mut(block).and_then(Json::as_object_mut) {
                members.insert(planted.clone(), Json::Null);
                if block == "initial" {
                    initials += 1;
                } else {
                    states += 1;
                }
            }
        }
        for step in slot
            .get_mut("steps")
            .and_then(Json::as_array_mut)
            .map(Vec::as_mut_slice)
            .unwrap_or_default()
        {
            step.as_object_mut()
                .expect("a step object")
                .insert(planted.clone(), Json::Null);
            steps += 1;
        }
        let failure = run(doctored, id).expect_err("an unread input must fail the case");
        assert!(
            failure.contains(&planted),
            "{id}: the failure must name the input it did not read, got: {failure}"
        );
    }
    assert_eq!(
        (initials, steps, states),
        (24, 111, 5),
        "every `initial`, every step, and every goal `state` must have been swept"
    );
}

/// Every member of every expected journal event is one the harness reads off the event the risk state
/// returns, so a case cannot state a payload member that nothing compares.
#[test]
fn every_expected_journal_member_is_read() {
    let planted = "a_payload_member_the_harness_does_not_read".to_owned();
    let fixture = fixture();
    let cases = fixture["cases"].as_array().expect("a case list");
    let mut events = 0;
    for case in cases {
        if case["kind"] != "risk_state" {
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
        let mut planted_here = 0;
        for step in slot
            .get_mut("steps")
            .and_then(Json::as_array_mut)
            .map(Vec::as_mut_slice)
            .unwrap_or_default()
        {
            for event in step["expect"]
                .get_mut("journal")
                .and_then(Json::as_array_mut)
                .map(Vec::as_mut_slice)
                .unwrap_or_default()
            {
                event
                    .as_object_mut()
                    .expect("an event object")
                    .insert(planted.clone(), Json::Null);
                planted_here += 1;
            }
        }
        events += planted_here;
        if planted_here == 0 {
            continue;
        }
        let failure = run(doctored, id).expect_err("an unread payload member must fail the case");
        assert!(
            failure.contains(&planted),
            "{id}: the failure must name the payload member it did not read, got: {failure}"
        );
    }
    assert_eq!(
        events, 138,
        "every event the risk-state cases expect must have been swept"
    );
}

/// A step whose `event` is not one of the eleven §5.2 inputs fails naming it, rather than being read
/// as a clock tick that changes nothing.
#[test]
fn an_unknown_step_event_fails_naming_it() {
    let fixture = fixture();
    let id = "MC-R01";
    let mut doctored = fixture.clone();
    let slot = doctored["cases"]
        .as_array_mut()
        .expect("a case list")
        .iter_mut()
        .find(|c| c["id"] == id)
        .expect("the case");
    slot["steps"][0]["event"] = Json::String("teleport".to_owned());
    let failure = run(doctored, id).expect_err("an unknown input must fail the case");
    assert!(
        failure.contains("teleport") && failure.contains("step 1"),
        "the failure must name the input and the step, got: {failure}"
    );
}

/// The two places the merged public API cannot express what a case states, pinned so that neither can
/// start passing for the wrong reason before the coordinator rules (see this change's Decisions
/// needed). Both are harness-visible today and neither is a rule this stream may change on its own.
#[test]
fn the_two_shapes_the_api_cannot_express_fail_loudly() {
    let fixture = fixture();
    let goal_failure =
        run(fixture.clone(), "MC-L01").expect_err("the increment is not expressible");
    assert!(
        goal_failure.contains("qty_increment") && goal_failure.contains("ShareIncrement"),
        "a goal increment off the two grids must name itself, got: {goal_failure}"
    );
    let floor_failure = run(fixture.clone(), "MC-R21").expect_err("the member is not expressible");
    assert!(
        floor_failure.contains("max_loss_from_allocation"),
        "a payload member no `ApplyResult` carries must name itself, got: {floor_failure}"
    );

    let mut doctored = fixture.clone();
    let slot = doctored["cases"]
        .as_array_mut()
        .expect("a case list")
        .iter_mut()
        .find(|c| c["id"] == "MC-L01")
        .expect("the case");
    slot["state"]["qty_increment"] = Json::String("1".to_owned());
    let whole_shares = run(doctored, "MC-L01").expect_err("the rules are stubs");
    assert!(
        whole_shares.contains("not implemented") || whole_shares.contains("unimplemented"),
        "a grid the type does express must get past the reader to the rule, got: {whole_shares}"
    );
}
