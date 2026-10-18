//! The input objects a report digests (DEC-2244 item 6): each is the object a caller stores under
//! the reference the record names, so each is checked against a shape written out here from the
//! request, and against the digest `evaluate`'s report holds.

mod common;

use common::*;
use mandate_canon::{Digest, Value};
use mandate_evaluation::{
    Baselines, EvaluationReport, EvaluationRequest, InputObjects, ThesisRecord,
};

const AS_OF: &str = "2026-09-21T14:00:00Z";
const HORIZON: &str = "2026-09-23T21:00:00Z";
const AFTER: &str = "2026-09-24T00:00:00Z";

/// The input objects and the report of one request over `records` and the common closes.
fn run(records: &[ThesisRecord]) -> (InputObjects, EvaluationReport) {
    let (version, document) = stored_research();
    let documents = move |digest: &Digest| (*digest == version).then(|| document.clone());
    let decision = decision(0);
    let closes = closes();
    let basket = [asset(BASKET_ASSET)];
    let index = asset(INDEX_ASSET);
    let cost = cost();
    let request = EvaluationRequest {
        decision_id: "DEC-122",
        decision: &decision,
        cut_off: at(AFTER),
        records,
        documents: &documents,
        closes: &closes,
        baselines: Baselines {
            basket: &basket,
            index: &index,
        },
        cost: &cost,
    };
    let objects = mandate_evaluation::input_objects(&request)
        .unwrap_or_else(|e| panic!("the input objects: {e:?}"));
    let report =
        mandate_evaluation::evaluate(&request).unwrap_or_else(|e| panic!("the report: {e:?}"));
    (objects, report)
}

fn canonical(json: &str) -> Value {
    mandate_canon::parse(json.as_bytes()).unwrap_or_else(|e| panic!("the oracle parses: {e}"))
}

fn digest(value: &Value) -> Digest {
    Digest::of(&mandate_canon::to_canonical(value))
}

fn two_records() -> Vec<ThesisRecord> {
    let (version, _) = stored_research();
    records(
        version,
        &[
            thesis("th_1", AS_OF, HORIZON),
            thesis("th_2", AS_OF, "2026-09-30T21:00:00Z"),
        ],
    )
}

/// Each object hashes to the digest the report's `inputs` name, so the record's references resolve
/// to what the report was computed from.
#[test]
#[ignore = "pending E17-8"]
fn each_object_is_the_one_the_report_digests() {
    let (objects, report) = run(&two_records());
    assert_eq!(digest(&objects.theses), report.inputs.theses);
    assert_eq!(digest(&objects.closes), report.inputs.closes);
    assert_eq!(digest(&objects.cost_model), report.inputs.cost_model);
}

/// The thesis records read, in the request's order: each record's position, hash, version and
/// payload, the digests in the journal's `ref` form.
#[test]
#[ignore = "pending E17-8"]
fn the_theses_object_lists_every_record_read() {
    let records = two_records();
    let (objects, _) = run(&records);
    let Value::Array(rows) = &objects.theses else {
        panic!("the theses object is an array: {:?}", objects.theses);
    };
    assert_eq!(rows.len(), 2);
    for (row, record) in rows.iter().zip(&records) {
        let Value::Object(members) = row else {
            panic!("each row is an object: {row:?}");
        };
        let names: Vec<&str> = members.keys().map(|key| key.as_str()).collect();
        assert_eq!(
            names,
            ["hash", "mandate_version", "payload", "seq", "stream_id"]
        );
        let member = |name: &str| {
            members
                .iter()
                .find(|(key, _)| key.as_str() == name)
                .map(|(_, value)| value.clone())
                .unwrap_or_else(|| panic!("{name}"))
        };
        assert_eq!(
            member("stream_id"),
            Value::Str("agent:ws_internal:agt_research".to_owned())
        );
        assert_eq!(member("seq"), canonical(&record.seq.to_string()));
        assert_eq!(
            member("hash"),
            Value::Str(format!("sha256:{}", record.hash.to_hex()))
        );
        assert_eq!(
            member("mandate_version"),
            Value::Str(format!("sha256:{}", record.mandate_version.to_hex()))
        );
        assert_eq!(member("payload"), record.payload);
    }
}

/// The close series, one per instrument in id order, each close's instant at nanosecond precision
/// and its price as the store answered it.
#[test]
#[ignore = "pending E17-8"]
fn the_closes_object_is_every_series_in_id_order() {
    let (objects, _) = run(&two_records());
    let series = |instrument: &str, prices: [&str; 3]| {
        format!(
            r#"{{"closes":[{{"at":"2026-09-21T20:00:00.000000000Z","price":"{}"}},{{"at":"2026-09-23T20:00:00.000000000Z","price":"{}"}},{{"at":"2026-09-24T20:00:00.000000000Z","price":"{}"}}],"instrument":"{instrument}"}}"#,
            prices[0], prices[1], prices[2]
        )
    };
    let expected = canonical(&format!(
        "[{},{},{}]",
        series(THESIS_ASSET, ["100", "110", "111"]),
        series(BASKET_ASSET, ["100", "105", "106"]),
        series(INDEX_ASSET, ["100", "102", "103"]),
    ));
    assert_eq!(objects.closes, expected);
}

/// The cost model: the slippage a leg, the reference notional and its increment, and the equity
/// fee schedule the round trip is charged.
#[test]
#[ignore = "pending E17-8"]
fn the_cost_model_object_is_the_configured_cost() {
    let (objects, _) = run(&two_records());
    let expected = canonical(
        r#"{"cat_per_share":"0.00001","fee_rounding":"daily_total","increment":"whole","reference_notional":"10000","sec_rate":"0.00003","slippage_per_leg_bps":"3","taf_cap":"9.79","taf_cap_basis":"per_execution","taf_per_share":"0.0002"}"#,
    );
    assert_eq!(objects.cost_model, expected);
}

/// The objects depend on the records read: one more record changes the theses object and its
/// digest, and leaves the closes and the cost model as they were.
#[test]
#[ignore = "pending E17-8"]
fn another_record_changes_only_the_theses_object() {
    let (version, _) = stored_research();
    let (one, _) = run(&records(version, &[thesis("th_1", AS_OF, HORIZON)]));
    let (two, report) = run(&two_records());
    assert_ne!(one.theses, two.theses);
    assert_eq!(digest(&two.theses), report.inputs.theses);
    assert_eq!(one.closes, two.closes);
    assert_eq!(one.cost_model, two.cost_model);
}
