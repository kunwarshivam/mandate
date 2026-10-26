//! The model half of a recorded `research.json` (DEC-2210 items 1 and 2, DEC-2266, DEC-2326 item
//! 3), read once for the cell's bundle loader and the model-gateway service's recorded mode: the
//! entry, its one endpoint, what the endpoint reports and the recorded reply, each member required
//! and nothing defaulted; an agentic recording's turns read only when every call is one the entry
//! declares; and the registry offering the entry on its endpoint. The oracle is each recording's
//! own text, read here with no help from the reader.

use mandate_canon::{Key, Value, parse, to_canonical};
use mandate_gateway::money::Usage;
use mandate_gateway::registry::{Locality, Reported, Status};
use mandate_research_run::recorded::{
    RecordedError, RecordedModel, SHOWN_THESIS, memory_theses, shown_thesis,
};

const DEMO: &str = include_str!("../../../deploy/cell/demo/research/research.json");
const AGENTIC: &str =
    include_str!("../../mandate-daemon/tests/fixtures/research-agentic/research.json");

fn demo() -> Value {
    parse(DEMO.as_bytes()).unwrap()
}

fn agentic() -> Value {
    parse(AGENTIC.as_bytes()).unwrap()
}

/// The demo recording as a single-call one: its entry names no loop, and it scripts no turn and no
/// monitor reply.
fn single_call() -> Value {
    let demo = edited(demo(), &["entry", "agentic"], None);
    edited(edited(demo, &["turns"], None), &["monitor_reply"], None)
}

fn text<'a>(value: &'a Value, path: &[&str]) -> &'a str {
    path.iter()
        .fold(value, |at, member| at.get(member).unwrap())
        .as_str()
        .unwrap()
}

fn int(value: &Value, path: &[&str]) -> u64 {
    path.iter()
        .fold(value, |at, member| at.get(member).unwrap())
        .as_int()
        .unwrap()
}

/// `value` with the member at `path` set to `to`, or removed when `to` is `None`.
fn edited(mut value: Value, path: &[&str], to: Option<Value>) -> Value {
    let (last, parents) = path.split_last().unwrap();
    let mut at = &mut value;
    for member in parents {
        let Value::Object(members) = at else {
            panic!("{member} is inside an object")
        };
        at = members.get_mut(&Key::new(member).unwrap()).unwrap();
    }
    let Value::Object(members) = at else {
        panic!("{last} is inside an object")
    };
    let key = Key::new(last).unwrap();
    match to {
        Some(to) => drop(members.insert(key, to)),
        None => drop(members.remove(&key)),
    }
    value
}

/// DEC-2210 item 1: the demo recording reads as the model its own text names.
#[test]
fn the_demo_recording_reads_as_the_model_its_text_names() {
    let file = demo();
    let model = RecordedModel::read(&file).unwrap();
    assert_eq!(model.entry.model_id, text(&file, &["entry", "model_id"]));
    assert_eq!(model.endpoint.id, text(&file, &["endpoint", "id"]));
    assert_eq!(model.endpoint.host, text(&file, &["endpoint", "host"]));
    assert_eq!(model.endpoint.region, text(&file, &["endpoint", "region"]));
    assert_eq!(model.endpoint.locality, Locality::Hosted);
    assert_eq!(
        model.endpoint.route.upstream,
        text(&file, &["endpoint", "upstream"])
    );
    assert_eq!(
        model.probe,
        Reported {
            model: Some(text(&file, &["entry", "model_name"]).to_owned()),
            provider: Some(text(&file, &["endpoint", "reported_provider"]).to_owned()),
        }
    );
    assert_eq!(model.reply.output, text(&file, &["reply", "output"]));
    assert_eq!(model.reply.latency_ms, int(&file, &["reply", "latency_ms"]));
    assert_eq!(
        model.reply.usage,
        Usage {
            input: int(&file, &["reply", "input_tokens"]),
            output: int(&file, &["reply", "output_tokens"]),
            cached: int(&file, &["reply", "cached_tokens"]),
        }
    );
    assert_eq!(
        model.turns.len(),
        file.get("turns").unwrap().as_array().unwrap().len(),
        "each of the demo recording's turns is read"
    );
    assert!(
        model.monitor_reply.is_some(),
        "the demo recording answers a monitor run"
    );
}

/// DEC-2210 item 2: the registry offers the entry on its one endpoint, probed with what the
/// endpoint reports.
#[test]
fn the_registry_offers_the_entry_on_its_one_endpoint() {
    let model = RecordedModel::read(&demo()).unwrap();
    let registry = model.registry().unwrap();
    let pin = model.entry.content().model_ref();
    let entry = registry.entry(&pin).unwrap();
    assert_eq!(entry.status(), Status::Offered);
    let endpoints: Vec<_> = entry.endpoints().collect();
    assert_eq!(
        endpoints,
        [(&model.endpoint, true)],
        "one endpoint, healthy"
    );
}

/// Every member is required, none is defaulted, and an empty text is as missing as an absent one:
/// each refuses with its own name.
#[test]
fn each_member_missing_or_empty_refuses_with_its_name() {
    let paths: [&[&str]; 14] = [
        &["entry"],
        &["endpoint"],
        &["reply"],
        &["endpoint", "id"],
        &["endpoint", "host"],
        &["endpoint", "region"],
        &["endpoint", "upstream"],
        &["endpoint", "reported_provider"],
        &["endpoint", "price_table"],
        &["reply", "output"],
        &["reply", "latency_ms"],
        &["reply", "input_tokens"],
        &["reply", "output_tokens"],
        &["reply", "cached_tokens"],
    ];
    for path in paths {
        let name = *path.last().unwrap();
        assert_eq!(
            RecordedModel::read(&edited(demo(), path, None)),
            Err(RecordedError::Malformed(name)),
            "{path:?} removed"
        );
        if path.len() == 2 && name != "latency_ms" && !name.ends_with("tokens") {
            assert_eq!(
                RecordedModel::read(&edited(demo(), path, Some(Value::Str(String::new())))),
                Err(RecordedError::Malformed(name)),
                "{path:?} empty"
            );
        }
    }
    for price in [
        "input_per_million",
        "output_per_million",
        "cached_per_million",
    ] {
        assert_eq!(
            RecordedModel::read(&edited(
                demo(),
                &["endpoint", price],
                Some(Value::Str("three".to_owned()))
            )),
            Err(RecordedError::Malformed(price)),
            "{price} not an amount"
        );
    }
    assert_eq!(
        RecordedModel::read(&edited(demo(), &["entry", "model_id"], None)),
        Err(RecordedError::Malformed("model_id")),
        "the entry's own members name themselves"
    );
}

/// DEC-2266: an agentic recording's turns read in order, each call's arguments canonical, and its
/// counts its own.
#[test]
fn an_agentic_recording_reads_its_turns_in_order() {
    let file = agentic();
    let model = RecordedModel::read(&file).unwrap();
    let listed = file.get("turns").unwrap().as_array().unwrap();
    assert_eq!(model.turns.len(), listed.len());
    for (turn, written) in model.turns.iter().zip(listed) {
        let calls: Vec<(String, String)> = written
            .get("calls")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|call| {
                (
                    call.get("name").unwrap().as_str().unwrap().to_owned(),
                    String::from_utf8(to_canonical(call.get("arguments").unwrap())).unwrap(),
                )
            })
            .collect();
        assert_eq!(turn.calls, calls);
        assert_eq!(
            turn.latency_ms,
            written.get("latency_ms").unwrap().as_int().unwrap()
        );
        assert_eq!(
            turn.usage,
            Usage {
                input: written.get("input_tokens").unwrap().as_int().unwrap(),
                output: written.get("output_tokens").unwrap().as_int().unwrap(),
                cached: written.get("cached_tokens").unwrap().as_int().unwrap(),
            }
        );
    }
}

/// DEC-2266: turns refuse whole on an entry with no agentic loop, and on a call the entry does not
/// declare, so no recording scripts a call its gateway would refuse.
#[test]
fn turns_the_entry_could_not_serve_refuse_whole() {
    let turns = agentic().get("turns").unwrap().clone();
    assert_eq!(
        RecordedModel::read(&edited(single_call(), &["turns"], Some(turns.clone()))),
        Err(RecordedError::Malformed("turns")),
        "a single-call entry"
    );
    let undeclared = parse(
        br#"[{"cached_tokens":0,"calls":[{"arguments":{"quantity":5},"name":"order.place"}],"input_tokens":1,"latency_ms":0,"output_tokens":1}]"#,
    )
    .unwrap();
    assert_eq!(
        RecordedModel::read(&edited(agentic(), &["turns"], Some(undeclared))),
        Err(RecordedError::Malformed("turns")),
        "a tool the entry does not declare"
    );
    let bound = RecordedModel::read(&agentic())
        .unwrap()
        .entry
        .content()
        .agentic()
        .unwrap()
        .loop_bounds
        .max_tool_calls_per_turn;
    let first_call = turns.as_array().unwrap()[0]
        .get("calls")
        .unwrap()
        .as_array()
        .unwrap()[0]
        .clone();
    for (count, why) in [
        (0, "a turn with no calls"),
        (bound + 1, "a turn with more calls than the loop's bound"),
    ] {
        let calls = vec![first_call.clone(); usize::try_from(count).unwrap()];
        let turn = edited(
            turns.as_array().unwrap()[0].clone(),
            &["calls"],
            Some(Value::Array(calls)),
        );
        assert_eq!(
            RecordedModel::read(&edited(
                agentic(),
                &["turns"],
                Some(Value::Array(vec![turn]))
            )),
            Err(RecordedError::Malformed("turns")),
            "{why}"
        );
    }
    let at_bound = edited(
        turns.as_array().unwrap()[0].clone(),
        &["calls"],
        Some(Value::Array(vec![
            first_call;
            usize::try_from(bound).unwrap()
        ])),
    );
    assert!(
        RecordedModel::read(&edited(
            agentic(),
            &["turns"],
            Some(Value::Array(vec![at_bound]))
        ))
        .is_ok(),
        "exactly the loop's bound"
    );
    assert_eq!(
        RecordedModel::read(&edited(agentic(), &["turns"], Some(Value::Array(vec![])))),
        Ok(RecordedModel {
            turns: Vec::new(),
            ..RecordedModel::read(&agentic()).unwrap()
        }),
        "no turns is no turns"
    );
}

/// A recorded monitor reply names the thesis the call showed (agent harness spec §6.9): the least
/// id the call's memory listed replaces every placeholder, and with none shown the output is left
/// as recorded, so the invalidation names no thesis and ends nothing.
#[test]
fn a_monitor_reply_names_the_least_thesis_the_call_showed() {
    let output = format!(r#"{{"invalidations":[{{"thesis_id":"{SHOWN_THESIS}","evidence":[]}}]}}"#);
    assert_eq!(
        shown_thesis(&output, &["01B", "01A"]),
        r#"{"invalidations":[{"thesis_id":"01A","evidence":[]}]}"#
    );
    assert_eq!(shown_thesis(&output, &[]), output);
    let inputs = std::collections::BTreeMap::from([(
        "memory".to_owned(),
        r#"[{"expires_at":"2026-10-20T00:00:00.000000000Z","symbol":"SPY","thesis_id":"01T2"},{"expires_at":"2026-10-20T00:00:00.000000000Z","symbol":"AAPL","thesis_id":"01T1"}]"#
            .to_owned(),
    )]);
    assert_eq!(memory_theses(&inputs), ["01T2", "01T1"]);
    assert!(memory_theses(&std::collections::BTreeMap::new()).is_empty());
    let garbled = std::collections::BTreeMap::from([("memory".to_owned(), "{}".to_owned())]);
    assert!(memory_theses(&garbled).is_empty());
}
