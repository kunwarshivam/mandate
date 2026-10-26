//! An agentic bundle's recorded multi-turn script (DEC-2266; agent harness spec v0.2 §6.9 and
//! §6.10; backlog E19-13 slice 3e): `research.json`'s `turns`, the scripted tool-call replies of
//! a run's turns before its final one, each a reply the entry's gateway would accept. A script the
//! entry could not serve, or on a bundle with no agentic loop, refuses the load. No scripted call
//! names an order, a quantity, a size, or a price. The worker answers turn n with scripted turn
//! n, and every turn past the script with the final reply.

mod cell_research;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use cell_research::{CUT_OFF, FIXTURE, RUN, Shelf, parts, who};
use mandate_canon::{Digest, Value, encode_ulid, parse, to_canonical};
use mandate_daemon::ResearchWorker;
use mandate_daemon::recorded::{Bundle, BundleError};
use mandate_daemon::worker::RecordedWorker;
use mandate_gateway::Outcome;
use mandate_gateway::money::Usd6;
use mandate_research_run::CallPlan;
use mandate_research_run::agentic::{RunKind, Tool, TurnPlan};
use mandate_runtime::{ResearchInput, ResearchRequest, RiskClock, TurnRef, TurnRequest};
use mandate_time::UtcNanos;
use serde_json::{Value as Json, json};

const AGENTIC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/research-agentic"
);

/// Member names an order, a quantity, a size, or a price to trade at would be written under
/// (`AGENTS.md` rule 4), the list DEC-2262's schema test walks.
const TRADING: [&str; 16] = [
    "amount",
    "limit_price",
    "notional",
    "order",
    "order_type",
    "price",
    "qty",
    "quantity",
    "shares",
    "side",
    "size",
    "stop_price",
    "target_price",
    "time_in_force",
    "units",
    "weight",
];

fn agentic() -> Bundle {
    Bundle::load(Path::new(AGENTIC)).unwrap_or_else(|e| panic!("{e:?}"))
}

/// The fixture's `turns`, read here from the file, not through the loader: each turn's calls as
/// (name, canonical arguments).
fn scripted_in_file() -> Vec<Vec<(String, String)>> {
    let file: Json =
        serde_json::from_slice(&fs::read(Path::new(AGENTIC).join("research.json")).unwrap())
            .unwrap();
    file["turns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|turn| {
            turn["calls"]
                .as_array()
                .unwrap()
                .iter()
                .map(|call| {
                    let arguments = parse(call["arguments"].to_string().as_bytes()).unwrap();
                    (
                        call["name"].as_str().unwrap().to_owned(),
                        String::from_utf8(to_canonical(&arguments)).unwrap(),
                    )
                })
                .collect()
        })
        .collect()
}

/// A copy of `from` with `edit` applied to its `research.json`, loaded.
fn load_edited(
    name: &str,
    from: &str,
    edit: impl FnOnce(&mut Json),
) -> Result<Bundle, BundleError> {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("daemon-{}-turns-{name}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        fs::copy(&path, dir.join(path.file_name().unwrap())).unwrap();
    }
    let file = dir.join("research.json");
    let mut value: Json = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    edit(&mut value);
    fs::write(&file, value.to_string()).unwrap();
    Bundle::load(&dir)
}

fn refused_turns() -> Result<Bundle, BundleError> {
    Err(BundleError::Malformed {
        file: "research.json",
        member: "turns",
    })
}

fn one_call(name: &str, arguments: Json) -> Json {
    json!([{
        "calls": [{"name": name, "arguments": arguments}],
        "latency_ms": 0,
        "input_tokens": 1000,
        "output_tokens": 40,
        "cached_tokens": 0
    }])
}

/// Every member name in `value`, at any depth.
fn member_names(value: &Value, names: &mut BTreeSet<String>) {
    match value {
        Value::Object(members) => {
            for (name, inner) in members {
                names.insert(name.as_str().to_owned());
                member_names(inner, names);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| member_names(item, names)),
        _ => {}
    }
}

fn turn(n: u32, observations: usize, transcript: Vec<TurnRef>) -> TurnRequest {
    TurnRequest {
        call: CallPlan {
            run_id: RUN.to_owned(),
            call_id: encode_ulid((1 << 100) + u128::from(n)),
            workspace_id: "ws1".to_owned(),
            agent_id: "agent-a".to_owned(),
            model: agentic().entry.content().model_ref(),
            as_of: UtcNanos::from_parts(CUT_OFF, 0).unwrap(),
            deadline_ms: 120_000,
            max_output_tokens: 4_096,
            observations: (0..observations)
                .map(|n| encode_ulid((1 << 101) + u128::try_from(n).unwrap()))
                .collect(),
        },
        kind: RunKind::Ideate,
        plan: TurnPlan {
            turn: n,
            tools: [Tool::MarketBars, Tool::NewsSearch, Tool::Note].into(),
            spend_ceiling: Usd6::from_micros(1_500_000),
            time_left_ms: 900_000,
            turns_left: 9 - n,
            tool_calls_left: 24,
        },
        transcript,
    }
}

/// The worker over the agentic fixture with the run started, and the seed's item count.
fn started(shelf: &Shelf) -> (RecordedWorker, usize) {
    let now = || UtcNanos::from_parts(CUT_OFF + 5, 0).unwrap();
    let mut worker =
        RecordedWorker::new(&agentic(), who(), Box::new(shelf.clone()), Box::new(now)).unwrap();
    worker.hand(
        &ResearchRequest::Start {
            run_id: RUN.to_owned(),
            cut_off: RiskClock::from_secs(CUT_OFF),
        },
        &parts(),
    );
    let answers = worker.answers(RiskClock::from_secs(CUT_OFF + 1));
    let Some(ResearchInput::Retrieved { items, .. }) = answers.first() else {
        panic!("the run retrieved: {answers:?}");
    };
    (worker, items.len())
}

/// Turn `n`'s record, with `transcript` the earlier turns.
fn answered(
    worker: &mut RecordedWorker,
    n: u32,
    observations: usize,
    transcript: Vec<TurnRef>,
) -> mandate_gateway::CallRecord {
    worker
        .turn(&turn(n, observations, transcript))
        .unwrap_or_else(|e| panic!("turn {n}: {e:?}"));
    let answers = worker.answers(RiskClock::from_secs(CUT_OFF + 30));
    match answers.as_slice() {
        [ResearchInput::Called { record, .. }] => (**record).clone(),
        other => panic!("turn {n} answered one record: {other:?}"),
    }
}

/// The earlier turn `record` makes, its response as stored and one stored result per call.
fn earlier(shelf: &Shelf, record: &mandate_gateway::CallRecord) -> TurnRef {
    let results = record
        .tool_calls
        .iter()
        .map(|call| {
            let content = format!("{{\"items\":[],\"call\":{}}}", call.index).into_bytes();
            let digest = Digest::of(&content);
            shelf.put.borrow_mut().push(content);
            digest
        })
        .collect();
    TurnRef {
        response: record.response_ref.unwrap(),
        results,
    }
}

#[test]
fn a_bundle_without_turns_scripts_none() {
    let single = Bundle::load(Path::new(FIXTURE)).unwrap();
    assert!(single.turns.is_empty());
}

#[test]
fn the_agentic_fixture_reads_its_scripted_turns_in_order() {
    let loaded: Vec<Vec<(String, String)>> = agentic()
        .turns
        .iter()
        .map(|turn| turn.calls.clone())
        .collect();
    assert_eq!(loaded, scripted_in_file());
    assert_eq!(loaded.len(), 2);
}

#[test]
fn turns_on_a_bundle_with_no_agentic_loop_refuse_the_load() {
    let loaded = load_edited("single", FIXTURE, |file| {
        file["turns"] = one_call("note", json!({"text": "a note"}));
    });
    assert_eq!(loaded.map(|_| ()), refused_turns().map(|_| ()));
}

#[test]
fn a_script_the_entry_could_not_serve_refuses_the_load() {
    let max_per_turn: Vec<Json> = (0..5)
        .map(|_| json!({"name": "note", "arguments": {"text": "x"}}))
        .collect();
    let eight: Vec<Json> = (0..8)
        .map(|_| one_call("note", json!({"text": "x"}))[0].clone())
        .collect();
    let cases: Vec<(&str, Json)> = vec![
        ("undeclared", one_call("memory.runs", json!({"limit": 1}))),
        (
            "action",
            one_call("propose_thesis", json!({"instrument": "AAPL"})),
        ),
        (
            "unfit",
            one_call("market.bars", json!({"count": 261, "symbol": "AAPL"})),
        ),
        (
            "extra",
            one_call(
                "market.bars",
                json!({"count": 5, "symbol": "AAPL", "quantity": 10}),
            ),
        ),
        (
            "empty",
            json!([{"calls": [], "latency_ms": 0, "input_tokens": 1, "output_tokens": 1, "cached_tokens": 0}]),
        ),
        (
            "over_per_turn",
            json!([{"calls": max_per_turn, "latency_ms": 0, "input_tokens": 1, "output_tokens": 1, "cached_tokens": 0}]),
        ),
        ("no_final_turn", Json::Array(eight)),
        (
            "no_usage",
            json!([{"calls": [{"name": "note", "arguments": {"text": "x"}}]}]),
        ),
        ("not_a_list", json!({"calls": []})),
    ];
    for (name, turns) in cases {
        let loaded = load_edited(name, AGENTIC, |file| file["turns"] = turns);
        assert_eq!(loaded.map(|_| ()), refused_turns().map(|_| ()), "{name}");
    }
}

#[test]
fn no_scripted_call_names_an_order_a_quantity_a_size_or_a_price() {
    let mut names = BTreeSet::new();
    for turn in &agentic().turns {
        for (_, arguments) in &turn.calls {
            member_names(&parse(arguments.as_bytes()).unwrap(), &mut names);
        }
    }
    for expected in ["count", "symbol", "symbols", "text"] {
        assert!(
            names.contains(expected),
            "the walk reads {expected}: {names:?}"
        );
    }
    let found: Vec<&str> = TRADING
        .iter()
        .copied()
        .filter(|name| names.contains(*name))
        .collect();
    assert!(found.is_empty(), "trading members: {found:?}");
}

#[test]
fn each_turn_is_answered_with_its_scripted_calls_then_the_final_reply() {
    let shelf = Shelf::default();
    let (mut worker, items) = started(&shelf);
    let script = scripted_in_file();
    let mut transcript = Vec::new();
    for (n, calls) in (1..).zip(&script) {
        let record = answered(&mut worker, n, items, transcript.clone());
        assert_eq!(record.outcome, Outcome::Ok, "turn {n}");
        assert_eq!(record.output, None, "turn {n} is a tool-call reply");
        let asked: Vec<(String, String)> = record
            .tool_calls
            .iter()
            .map(|call| {
                (
                    call.name.clone(),
                    String::from_utf8(to_canonical(&call.arguments)).unwrap(),
                )
            })
            .collect();
        assert_eq!(&asked, calls, "turn {n} asks for its scripted calls");
        transcript.push(earlier(&shelf, &record));
    }
    let last = u32::try_from(script.len()).unwrap() + 1;
    let record = answered(&mut worker, last, items, transcript);
    assert_eq!(record.outcome, Outcome::Ok);
    assert!(record.tool_calls.is_empty());
    assert!(record.output.is_some(), "past the script, the final reply");
}

/// Agent harness spec §6.9 end to end through the worker's gateway: a bundle's `monitor_reply`
/// answers a `monitor` run's first turn, with no scripted turn, and the gateway takes it under the
/// monitor schema, `invalidations` alone, so the record carries it as the run's final output. The
/// same bundle's ideate run still takes its script.
#[test]
fn a_monitor_turn_is_answered_with_the_bundle_s_monitor_reply() {
    let invalidation = json!({"invalidations": [{"thesis_id": "01ACTIVE", "evidence": []}]});
    let output = invalidation.to_string();
    let bundle = load_edited("monitor-reply", AGENTIC, |research| {
        research["monitor_reply"] = json!({
            "output": output,
            "latency_ms": 0,
            "input_tokens": 900,
            "output_tokens": 40,
            "cached_tokens": 0
        });
    })
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(bundle.monitor_reply.is_some());
    let shelf = Shelf::default();
    let now = || UtcNanos::from_parts(CUT_OFF + 5, 0).unwrap();
    let mut worker =
        RecordedWorker::new(&bundle, who(), Box::new(shelf.clone()), Box::new(now)).unwrap();
    worker.hand(
        &ResearchRequest::Start {
            run_id: RUN.to_owned(),
            cut_off: RiskClock::from_secs(CUT_OFF),
        },
        &parts(),
    );
    let answers = worker.answers(RiskClock::from_secs(CUT_OFF + 1));
    let Some(ResearchInput::Retrieved { items, .. }) = answers.first() else {
        panic!("the run retrieved: {answers:?}");
    };
    let mut monitor = turn(1, items.len(), Vec::new());
    monitor.kind = RunKind::Monitor;
    monitor.plan.tools = [Tool::NewsSearch, Tool::NewsGet, Tool::Note].into();
    worker
        .turn(&monitor)
        .unwrap_or_else(|e| panic!("monitor turn: {e:?}"));
    let answers = worker.answers(RiskClock::from_secs(CUT_OFF + 30));
    let [ResearchInput::Called { record, .. }] = answers.as_slice() else {
        panic!("the monitor turn answered one record: {answers:?}");
    };
    assert_eq!(record.outcome, Outcome::Ok, "{:?}", record.outcome);
    assert!(record.tool_calls.is_empty(), "no scripted turn");
    assert_eq!(
        record.output.as_ref().map(to_canonical),
        Some(to_canonical(
            &parse(invalidation.to_string().as_bytes()).unwrap()
        ))
    );
}
