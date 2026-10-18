//! The evaluation's input files (DEC-2244 item 3): the worked example decodes to the values the
//! hand-calculated cases use, and every member missing, unknown, or of the wrong type refuses
//! `input_invalid` naming its path, walked over every member of every file rather than listed.

mod common;

use common::*;
use mandate_canon::{Key, Value};
use mandate_evaluation::{EvaluationError, INPUT_FILE_NAMES, InputFiles, decode};

const DECISION: &str = include_str!("fixtures/inputs/decision.json");
const CLOSES: &str = include_str!("fixtures/inputs/closes.json");
const COST: &str = include_str!("fixtures/inputs/cost.json");
const BASELINES: &str = include_str!("fixtures/inputs/baselines.json");

fn example() -> [Value; 4] {
    [DECISION, CLOSES, COST, BASELINES].map(|text| {
        mandate_canon::parse(text.as_bytes()).unwrap_or_else(|e| panic!("the fixture parses: {e}"))
    })
}

/// Decodes the four documents as their canonical bytes.
fn decode_values(
    values: &[Value; 4],
) -> Result<mandate_evaluation::EvaluationInputs, EvaluationError> {
    let [decision, closes, cost, baselines] = values.each_ref().map(mandate_canon::to_canonical);
    decode(&InputFiles {
        decision: &decision,
        closes: &closes,
        cost: &cost,
        baselines: &baselines,
    })
}

fn refusal(values: &[Value; 4]) -> String {
    match decode_values(values) {
        Err(EvaluationError::Input(path)) => path,
        Err(other) => panic!("refused as an input: {other:?}"),
        Ok(_) => panic!("refused"),
    }
}

/// Every path in `value` that names an object member or a list element, each with its own path,
/// parents before children.
fn paths(value: &Value, at: &str, found: &mut Vec<String>) {
    match value {
        Value::Object(members) => {
            for (key, member) in members {
                let path = format!("{at}/{}", key.as_str());
                found.push(path.clone());
                paths(member, &path, found);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let path = format!("{at}/{index}");
                found.push(path.clone());
                paths(item, &path, found);
            }
        }
        _ => {}
    }
}

/// The value at `path`'s parent (`/`-separated, file name first stripped by the caller) and its
/// last step.
fn parent<'v>(value: &'v mut Value, steps: &[&str]) -> &'v mut Value {
    steps.iter().fold(value, |at, step| match at {
        Value::Object(members) => members
            .get_mut(&Key::new(step).unwrap_or_else(|e| panic!("{e}")))
            .unwrap_or_else(|| panic!("{step}")),
        Value::Array(items) => {
            let index: usize = step.parse().unwrap_or_else(|e| panic!("{e}"));
            items.get_mut(index).unwrap_or_else(|| panic!("{step}"))
        }
        other => panic!("{step} under {other:?}"),
    })
}

/// Every path of every file, as `<file name>/<member>/…`, with the file's index.
fn every_path() -> Vec<(usize, String)> {
    example()
        .iter()
        .zip(INPUT_FILE_NAMES)
        .enumerate()
        .flat_map(|(file, (value, name))| {
            let mut found = Vec::new();
            paths(value, name, &mut found);
            found.into_iter().map(move |path| (file, path))
        })
        .collect()
}

fn steps(path: &str) -> Vec<&str> {
    path.split('/').skip(1).collect()
}

/// DEC-2244 item 3, hand-written: the example is the hand-calculated cases' decision, closes, cost
/// and baselines.
#[test]
fn the_example_decodes_to_the_hand_calculated_inputs() {
    let inputs = decode_values(&example()).unwrap_or_else(|e| panic!("decodes: {e:?}"));
    assert_eq!(inputs.decision_id, "DEC-122");
    assert_eq!(inputs.decision, decision(0));
    assert_eq!(inputs.closes, closes());
    let expected = cost();
    assert_eq!(inputs.cost.slippage_per_leg, expected.slippage_per_leg);
    assert_eq!(inputs.cost.fees, expected.fees);
    assert_eq!(inputs.cost.reference_notional, expected.reference_notional);
    assert_eq!(inputs.cost.increment, expected.increment);
    assert_eq!(inputs.basket, [asset(BASKET_ASSET)]);
    assert_eq!(inputs.index, asset(INDEX_ASSET));
}

/// Every object member is required: removing any one, at any depth of any file, refuses, naming
/// it.
#[test]
fn a_missing_member_refuses_naming_it() {
    for (file, path) in every_path() {
        let all = steps(&path);
        let Some((last, above)) = all.split_last() else {
            continue;
        };
        let mut values = example();
        let holder = parent(&mut values[file], above);
        match holder {
            Value::Object(members) => {
                members.remove(&Key::new(last).unwrap_or_else(|e| panic!("{e}")));
                assert_eq!(refusal(&values), path, "removing {path}");
            }
            Value::Array(_) => {}
            other => panic!("{path} under {other:?}"),
        }
    }
}

/// A member no reader names refuses, naming it, in every object of every file.
#[test]
fn an_unknown_member_refuses_naming_it() {
    let objects = INPUT_FILE_NAMES
        .iter()
        .enumerate()
        .map(|(file, name)| (file, (*name).to_owned()))
        .chain(every_path())
        .collect::<Vec<_>>();
    for (file, path) in objects {
        let mut values = example();
        let holder = parent(&mut values[file], &steps(&path));
        let Value::Object(members) = holder else {
            continue;
        };
        members.insert(
            Key::new("unexpected").unwrap_or_else(|e| panic!("{e}")),
            Value::Null,
        );
        assert_eq!(refusal(&values), format!("{path}/unexpected"));
    }
}

/// Every leaf of the wrong type refuses, naming it: text (a decimal, an instant, a date, an id or
/// a word) as `null`, and the count as text.
#[test]
fn a_leaf_of_the_wrong_type_refuses_naming_it() {
    for (file, path) in every_path() {
        let mut values = example();
        let leaf = parent(&mut values[file], &steps(&path));
        let wrong = match leaf {
            Value::Str(_) => Value::Null,
            Value::Int(_) => Value::Str("0".to_owned()),
            _ => continue,
        };
        *leaf = wrong;
        assert_eq!(refusal(&values), path, "a wrong type at {path}");
    }
}

/// A text leaf that is the right type but not its value's form refuses, naming it.
#[test]
fn a_malformed_value_refuses_naming_it() {
    for (file, path, bad) in [
        (0, "decision.json/window/from", "2026-09-01"),
        (0, "decision.json/z", "one"),
        (1, "closes.json/series/0/closes/1/price", "ten"),
        (1, "closes.json/series/2/instrument", ""),
        (2, "cost.json/increment", "half"),
        (2, "cost.json/fees/equities/fee_rounding", "per_trade"),
        (
            2,
            "cost.json/fees/calendar/valid_to",
            "2026-12-31T00:00:00Z",
        ),
        (3, "baselines.json/index", ""),
    ] {
        let mut values = example();
        *parent(&mut values[file], &steps(path)) = Value::Str(bad.to_owned());
        assert_eq!(refusal(&values), path, "{bad} at {path}");
    }
}

/// One instrument with two series, or a window that ends before it starts, refuses; a window of
/// one instant is a window.
#[test]
fn a_repeated_series_or_a_reversed_window_refuses() {
    let mut values = example();
    let first = parent(&mut values[1], &steps("closes.json/series/0/instrument")).clone();
    *parent(&mut values[1], &steps("closes.json/series/1/instrument")) = first;
    assert_eq!(refusal(&values), "closes.json/series/1/instrument");

    let mut values = example();
    *parent(&mut values[0], &steps("decision.json/window/to")) =
        Value::Str("2026-08-31T00:00:00Z".to_owned());
    assert_eq!(refusal(&values), "decision.json/window");

    let mut values = example();
    *parent(&mut values[0], &steps("decision.json/window/to")) =
        Value::Str("2026-09-01T00:00:00Z".to_owned());
    let one_instant =
        decode_values(&values).unwrap_or_else(|e| panic!("a one-instant window: {e:?}"));
    assert_eq!(
        one_instant.decision.window.from,
        one_instant.decision.window.to
    );
}

/// A file that is not canonical JSON refuses, naming the file.
#[test]
fn an_unreadable_file_refuses_naming_it() {
    let [decision, closes, cost, baselines] = example().each_ref().map(mandate_canon::to_canonical);
    let files = [&decision, &closes, &cost, &baselines];
    for (index, name) in INPUT_FILE_NAMES.iter().enumerate() {
        let mut bytes = files.map(|bytes| bytes.as_slice());
        bytes[index] = b"{not json";
        let decoded = decode(&InputFiles {
            decision: bytes[0],
            closes: bytes[1],
            cost: bytes[2],
            baselines: bytes[3],
        });
        assert!(
            matches!(&decoded, Err(EvaluationError::Input(path)) if path == name),
            "{name}: {decoded:?}"
        );
        assert_eq!(
            decoded.err().map(|e| e.code()),
            Some("input_invalid"),
            "{name}"
        );
    }
}
