//! Family E (`escalation`) of the `mandate` harness reads every key of every case it interprets,
//! compares every expectation, and never lets a case pass for the wrong reason (DEC-85, DEC-292).
//!
//! The arm lives in `src/mandate/escalation.rs`. Its `ask_permit` and `deliver_now` ops drive
//! `mandate_approval::ask_permit` and `mandate_approval::deliver_now`; its `lifecycle` op is not
//! interpreted yet and fails naming itself. Each test has two halves that need each other: a case
//! passes as the fixture states it, which an arm that failed everything could not do, and fails,
//! naming the member, when that member is edited, dropped, or added, which an arm that compared
//! nothing could not do.

use std::path::Path;
use std::sync::Arc;

use mandate_refcases::{Json, mandate, read_fixture};
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

fn case_mut<'a>(fixture: &'a mut Json, id: &str) -> &'a mut Json {
    fixture["cases"]
        .as_array_mut()
        .expect("a case list")
        .iter_mut()
        .find(|c| c["id"] == id)
        .unwrap_or_else(|| panic!("no case {id}"))
}

/// The family's cases with `op`, in fixture order.
fn ids_with_op(fixture: &Json, op: &str) -> Vec<String> {
    fixture["cases"]
        .as_array()
        .expect("a case list")
        .iter()
        .filter(|c| c["kind"] == "escalation" && c["op"] == op)
        .map(|c| c["id"].as_str().expect("an id").to_owned())
        .collect()
}

/// The cases the arm interprets: the four `ask_permit` and the two `deliver_now`.
fn interpreted(fixture: &Json) -> Vec<String> {
    let mut ids = ids_with_op(fixture, "ask_permit");
    ids.extend(ids_with_op(fixture, "deliver_now"));
    ids
}

/// Spec §11 lists family E as MC-E01 to MC-E32, every one `kind: escalation`: twenty-six
/// `lifecycle`, four `ask_permit` (MC-E25 to MC-E28) and two `deliver_now` (MC-E30, MC-E32).
#[test]
fn the_fixture_holds_family_e_as_spec_11_lists_it() {
    let fixture = fixture();
    let mut family: Vec<String> = fixture["cases"]
        .as_array()
        .expect("a case list")
        .iter()
        .map(|c| c["id"].as_str().expect("an id").to_owned())
        .filter(|id| id.starts_with("MC-E"))
        .collect();
    family.sort();
    let listed: Vec<String> = (1..=32).map(|n| format!("MC-E{n:02}")).collect();
    assert_eq!(family, listed);
    assert_eq!(ids_with_op(&fixture, "lifecycle").len(), 26);
    assert_eq!(
        ids_with_op(&fixture, "ask_permit"),
        ["MC-E25", "MC-E26", "MC-E27", "MC-E28"]
    );
    let mut deliver = ids_with_op(&fixture, "deliver_now");
    deliver.sort();
    assert_eq!(deliver, ["MC-E30", "MC-E32"]);
}

/// The arm reproduces each interpreted case as the fixture states it.
#[test]
fn every_interpreted_case_passes_as_stated() {
    let fixture = fixture();
    let ids = interpreted(&fixture);
    assert_eq!(ids.len(), 6);
    for id in &ids {
        run(fixture.clone(), id).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
}

/// DEC-292: a `lifecycle` case fails naming its op until an arm drives the runtime, so none can
/// pass on an arm that never ran it.
#[test]
fn a_lifecycle_case_fails_naming_its_op() {
    let fixture = fixture();
    let ids = ids_with_op(&fixture, "lifecycle");
    assert_eq!(ids.len(), 26);
    for id in &ids {
        let failure = run(fixture.clone(), id).expect_err("a lifecycle case cannot pass yet");
        assert!(failure.contains("`lifecycle`"), "{id}: {failure}");
    }
}

/// Every expectation is compared: flipping each query's expected answer to another one the op can
/// give fails the case, naming the query.
#[test]
fn every_expectation_is_compared() {
    let fixture = fixture();
    let mut flipped = 0;
    for id in interpreted(&fixture) {
        let count = fixture["cases"]
            .as_array()
            .expect("a case list")
            .iter()
            .find(|c| c["id"] == id.as_str())
            .and_then(|c| c["expect"].as_array())
            .expect("an expect list")
            .len();
        for index in 0..count {
            let mut doctored = fixture.clone();
            let expect = &mut case_mut(&mut doctored, &id)["expect"][index];
            if let Some(suppressed) = expect.get_mut("suppressed") {
                *suppressed = match suppressed.as_str() {
                    None => json!("budget"),
                    Some("budget") => json!("skipped_today"),
                    Some(_) => Json::Null,
                };
            } else {
                let status = &mut expect["status"];
                *status = if *status == "delivered" {
                    json!("suppressed_quiet_hours")
                } else {
                    json!("delivered")
                };
            }
            let failure = run(doctored, &id).expect_err("a changed expectation must fail");
            assert!(
                failure.contains(&format!("query {}", index + 1)),
                "{id}: {failure}"
            );
            flipped += 1;
        }
    }
    assert_eq!(flipped, 18, "every query of the six cases");
}

/// No query goes unjudged: an expectation dropped from the list fails the case.
#[test]
fn an_expectation_missing_for_a_query_fails() {
    let fixture = fixture();
    for id in interpreted(&fixture) {
        let mut doctored = fixture.clone();
        case_mut(&mut doctored, &id)["expect"]
            .as_array_mut()
            .expect("an expect list")
            .pop();
        let failure = run(doctored, &id).expect_err("a missing expectation must fail");
        assert!(failure.contains("expectations"), "{id}: {failure}");
    }
}

/// Every key is read (DEC-85): a member the arm does not know, added to the case, to a ledger
/// entry, to a query, to an expectation, or to the quiet hours, fails the case naming it.
#[test]
fn every_member_of_an_interpreted_case_is_read() {
    let fixture = fixture();
    const UNKNOWN: &str = "a_member_the_harness_does_not_know";
    let mut checked = 0;
    for id in interpreted(&fixture) {
        let case = fixture["cases"]
            .as_array()
            .expect("a case list")
            .iter()
            .find(|c| c["id"] == id.as_str())
            .expect("the case")
            .clone();
        let mut places: Vec<Vec<Json>> = vec![vec![]];
        for list in ["ledger", "queries", "expect"] {
            if case
                .get(list)
                .and_then(Json::as_array)
                .is_some_and(|l| !l.is_empty())
            {
                places.push(vec![json!(list), json!(0)]);
            }
        }
        if case.get("quiet_hours").is_some_and(Json::is_object) {
            places.push(vec![json!("quiet_hours")]);
        }
        for path in places {
            let mut doctored = fixture.clone();
            let mut node = case_mut(&mut doctored, &id);
            for step in &path {
                node = match step {
                    Json::String(key) => &mut node[key.as_str()],
                    other => &mut node[other.as_u64().expect("an index") as usize],
                };
            }
            node.as_object_mut()
                .expect("an object")
                .insert(UNKNOWN.to_owned(), Json::Null);
            let failure = run(doctored, &id).expect_err("an unknown member must fail");
            assert!(failure.contains(UNKNOWN), "{id} at {path:?}: {failure}");
            checked += 1;
        }
    }
    assert_eq!(
        checked,
        6 + 4 * 3 + 2 * 3,
        "each case, each list, each quiet-hours window"
    );
}

/// An instant with a fraction of a second is refused rather than truncated: the risk clock is whole
/// seconds (mandate spec §5.2).
#[test]
fn a_fractional_instant_fails_the_case() {
    let fixture = fixture();
    for id in interpreted(&fixture) {
        let mut doctored = fixture.clone();
        let query = &mut case_mut(&mut doctored, &id)["queries"][0]["at"];
        let text = query
            .as_str()
            .expect("an instant")
            .replace(".000000000Z", ".500000000Z");
        *query = json!(text);
        let failure = run(doctored, &id).expect_err("a fractional second must fail");
        assert!(failure.contains("whole second"), "{id}: {failure}");
    }
}

/// An op the arm does not know fails naming it, rather than passing.
#[test]
fn an_unknown_op_fails_naming_it() {
    let mut fixture = fixture();
    case_mut(&mut fixture, "MC-E25")["op"] = json!("ask_later");
    let failure = run(fixture, "MC-E25").expect_err("an unknown op must fail");
    assert!(failure.contains("ask_later"), "{failure}");
}

/// The ledger of MC-E27 with a `version_applied` added at `at` and placed `after_skip` or before
/// the owner's skip, and its first query's expectation set to `suppressed`.
fn with_version_applied(fixture: &Json, after_skip: bool, suppressed: Json) -> Json {
    let mut doctored = fixture.clone();
    let case = case_mut(&mut doctored, "MC-E27");
    let applied = json!({"event": "version_applied", "at": "2026-09-22T14:00:05.000000000Z"});
    let ledger = case["ledger"].as_array_mut().expect("a ledger");
    let skip = ledger
        .iter()
        .position(|e| e["event"] == "owner_skipped")
        .expect("MC-E27 records an owner skip");
    ledger.insert(if after_skip { skip + 1 } else { skip }, applied);
    case["expect"][0]["suppressed"] = suppressed;
    doctored
}

/// The #416 review, minor 1; DEC-292 item 1: the ledger folds in its order. A `version_applied`
/// after the owner's skip lifts it, so the query asks; the same entry before the skip lifts nothing,
/// so the query stays `skipped_today`. A fold in any other order answers one of the two wrongly.
#[test]
fn the_ledger_folds_in_its_order() {
    let fixture = fixture();
    run(with_version_applied(&fixture, true, Json::Null), "MC-E27")
        .unwrap_or_else(|e| panic!("a version after the skip lifts it: {e}"));
    run(
        with_version_applied(&fixture, false, json!("skipped_today")),
        "MC-E27",
    )
    .unwrap_or_else(|e| panic!("a version before the skip lifts nothing: {e}"));
    let failure = run(
        with_version_applied(&fixture, true, json!("skipped_today")),
        "MC-E27",
    )
    .expect_err("the lifted skip is not suppressed");
    assert!(failure.contains("query 1"), "{failure}");
}

/// The #416 review, minor 2: an expectation without its `suppressed` member fails loudly rather
/// than leaving its query unjudged.
#[test]
fn an_expectation_without_its_member_fails() {
    let fixture = fixture();
    for (id, member) in [("MC-E25", "suppressed"), ("MC-E30", "status")] {
        let mut doctored = fixture.clone();
        case_mut(&mut doctored, id)["expect"][0]
            .as_object_mut()
            .expect("an expectation")
            .remove(member);
        let failure = run(doctored, id).expect_err("an expectation without its member must fail");
        assert!(failure.contains(member), "{id}: {failure}");
    }
}

/// The #416 review, minor 2: an ask-ledger `event` the arm does not know fails, naming it, rather
/// than being read as another event.
#[test]
fn an_unknown_ledger_event_fails_naming_it() {
    let mut fixture = fixture();
    case_mut(&mut fixture, "MC-E25")["ledger"][0] =
        json!({"event": "ask_withdrawn", "at": "2026-09-22T14:00:00.000000000Z"});
    let failure = run(fixture, "MC-E25").expect_err("an unknown event must fail");
    assert!(failure.contains("ask_withdrawn"), "{failure}");
}

/// The #416 review, minor 2: a `channel` the arm does not know fails, naming it, rather than being
/// read as a push.
#[test]
fn an_unknown_channel_fails_naming_it() {
    let mut fixture = fixture();
    case_mut(&mut fixture, "MC-E30")["queries"][0]["channel"] = json!("pager");
    let failure = run(fixture, "MC-E30").expect_err("an unknown channel must fail");
    assert!(failure.contains("pager"), "{failure}");
}
