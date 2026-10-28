//! The cell's demo bundle, `deploy/cell/demo/research/` (DEC-2210 item 1, DEC-2246): it loads,
//! records only the demo's one instrument, and runs through the recorded worker's agentic loop,
//! its scripted turns and then the recorded reply, whose thesis names that instrument and cites
//! only items retrieval returns from allowlisted sources. Its feed (DEC-2275) brings one item on
//! that instrument, from an allowlisted source, and its monitor reply invalidates the thesis the run
//! was shown, citing that item. The fixture rule: the bundle passes every rule it is not meant to
//! fail.

mod cell_research;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use cell_research::Shelf;
use mandate_canon::{Digest, encode_ulid, parse, to_canonical};
use mandate_daemon::recorded::Bundle;
use mandate_daemon::worker::{RecordedWorker, WorkerFor};
use mandate_daemon::{ResearchParts, ResearchWorker};
use mandate_gateway::money::Usd6;
use mandate_gateway::{CallRecord, Outcome as CallOutcome};
use mandate_num::Usd;
use mandate_research::{
    AdmissionFacts, AssetClass, AssetId, AutonomyDecision, LineageState, MandateEnvelope,
    PolicyOverlay, ResearchEnvelope, ValidatedMandate, WorkingUniverse,
};
use mandate_research_run::agentic::{RunKind, Tool, TurnPlan};
use mandate_research_run::{CallPlan, Symbol};
use mandate_runtime::{
    ActiveThesis, JournalCatalog, Preconditions, ResearchInput, ResearchRequest, RiskClock,
    TurnRef, TurnRequest,
};
use mandate_time::UtcNanos;
use serde_json::Value;

const DEMO: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../deploy/cell/demo/research"
);
const DEMO_INSTRUMENT: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";
const CUT_OFF: i64 = 1_792_180_800;
const RUN: &str = "01RUNDEMOBUNDLE";
/// The thesis a monitor run is shown, as the runtime would name it at admission.
const SHOWN: &str = "01SHOWNDEMOTHESIS";

fn bundle() -> Bundle {
    Bundle::load(Path::new(DEMO)).unwrap_or_else(|error| panic!("the demo bundle: {error:?}"))
}

fn json(file: &str) -> Value {
    let text = std::fs::read_to_string(Path::new(DEMO).join(file)).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// A pass's parts for the demo's researching mandate: one instrument, the bundle's allowlist.
fn parts(bundle: &Bundle) -> ResearchParts {
    let envelope = MandateEnvelope {
        universe_pinned: false,
        max_instruments: 1,
        asset_classes: BTreeSet::from([AssetClass::UsEquity]),
        leveraged_etps_enabled: false,
        leveraged_etp_disclosure_version: None,
        admits_instruments: true,
        research: Some(ResearchEnvelope {
            interval_s: 3_600,
            cost_cap_usd_per_day: Usd::parse("5").unwrap(),
            max_revisions_per_lineage: 3,
        }),
        admission: AutonomyDecision::Ask,
    };
    ResearchParts {
        preconditions: Preconditions::read(&JournalCatalog::JOURNAL),
        mandate: ValidatedMandate::from_validated_envelope(envelope),
        overlay: PolicyOverlay::internal_research_profile(),
        universe: WorkingUniverse::Known {
            instruments: BTreeSet::new(),
            pinned: false,
        },
        facts: AdmissionFacts {
            allowlist: bundle.allowlist.clone(),
            eligibility_failures: BTreeSet::new(),
            instrument_groups: BTreeMap::new(),
            claimed_by_other_agents: BTreeSet::new(),
            halted_instruments: BTreeSet::new(),
            disclosures_accepted: BTreeSet::new(),
            data_universe: Some(BTreeSet::from([AssetId::new(DEMO_INSTRUMENT).unwrap()])),
            research_spend_usd_today: Usd::parse("0").unwrap(),
        },
        lineages: LineageState::default(),
        active: Vec::new(),
        last_reservation: None,
        data_symbols: BTreeSet::from([Symbol::new("SPY").unwrap()]),
        event_triggers: None,
        events: Vec::new(),
    }
}

#[test]
fn the_demo_bundle_records_only_the_demo_instrument() {
    let bundle = bundle();
    let instruments: Vec<_> = bundle
        .plane
        .instruments
        .iter()
        .map(|fact| fact.asset_id.as_str().to_owned())
        .collect();
    assert_eq!(instruments, [DEMO_INSTRUMENT]);
    assert!(
        bundle.plane.filings.is_empty(),
        "the demo records no filings"
    );
    assert!(bundle.halted.is_empty(), "the demo records no halts");
    let instrument = json("../instrument.json");
    assert_eq!(instrument["instrument_id"], DEMO_INSTRUMENT);
    assert_eq!(instrument["symbol"], "SPY");
}

#[test]
fn the_demo_reply_cites_only_allowlisted_items_on_the_demo_instrument() {
    let research = json("research.json");
    let allowed: BTreeSet<_> = research["allowlist"]["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_owned())
        .collect();
    let cited: BTreeSet<_> = json("news.json")
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| allowed.contains(item["source"].as_str().unwrap()))
        .map(|item| item["item_id"].as_str().unwrap().to_owned())
        .collect();
    let reply: Value = serde_json::from_str(research["reply"]["output"].as_str().unwrap()).unwrap();
    let theses = reply["theses"].as_array().unwrap();
    assert_eq!(theses.len(), 1);
    assert_eq!(theses[0]["instrument"], "SPY");
    for evidence in theses[0]["evidence"].as_array().unwrap() {
        assert!(
            cited.contains(evidence.as_str().unwrap()),
            "{evidence} is an allowlisted item"
        );
    }
}

fn who() -> WorkerFor {
    WorkerFor {
        workspace: "ws1".to_owned(),
        agent: "demo-research".to_owned(),
        description: "The demo's researching agent, on recorded data.".to_owned(),
    }
}

/// The worker over the demo bundle with a run started under `parts`, and the run's item count.
fn started(bundle: &Bundle, shelf: &Shelf, parts: &ResearchParts) -> (RecordedWorker, usize) {
    let now = || UtcNanos::from_parts(CUT_OFF + 5, 0).unwrap();
    let mut worker =
        RecordedWorker::new(bundle, who(), Box::new(shelf.clone()), Box::new(now)).unwrap();
    worker.hand(
        &ResearchRequest::Start {
            run_id: RUN.to_owned(),
            cut_off: RiskClock::from_secs(CUT_OFF),
        },
        parts,
    );
    let retrieved = worker.answers(RiskClock::from_secs(CUT_OFF + 1));
    let Some(ResearchInput::Retrieved { items, .. }) = retrieved.first() else {
        panic!("the run retrieved: {retrieved:?}");
    };
    assert!(!items.is_empty(), "the demo retrieves something");
    (worker, items.len())
}

/// Turn `n` of a `kind` run over `items` retrieved items, offering `tools`, after `transcript`.
fn turn(
    bundle: &Bundle,
    kind: RunKind,
    tools: &[Tool],
    n: u32,
    items: usize,
    transcript: Vec<TurnRef>,
) -> TurnRequest {
    TurnRequest {
        call: CallPlan {
            run_id: RUN.to_owned(),
            call_id: encode_ulid((1 << 100) + u128::from(n)),
            workspace_id: "ws1".to_owned(),
            agent_id: "demo-research".to_owned(),
            model: bundle.entry.content().model_ref(),
            as_of: UtcNanos::from_parts(CUT_OFF, 0).unwrap(),
            deadline_ms: 120_000,
            max_output_tokens: 4_096,
            observations: (0..items)
                .map(|n| encode_ulid((1 << 101) + u128::try_from(n).unwrap()))
                .collect(),
        },
        kind,
        plan: TurnPlan {
            turn: n,
            tools: tools.iter().copied().collect(),
            spend_ceiling: Usd6::from_micros(1_500_000),
            time_left_ms: 900_000,
            turns_left: 9 - n,
            tool_calls_left: 24,
        },
        transcript,
    }
}

/// The one record `request` is answered with.
fn answered(worker: &mut RecordedWorker, request: &TurnRequest, at: i64) -> CallRecord {
    worker
        .turn(request)
        .unwrap_or_else(|e| panic!("turn {}: {e:?}", request.plan.turn));
    let answers = worker.answers(RiskClock::from_secs(at));
    match answers.as_slice() {
        [ResearchInput::Called { record, .. }] => (**record).clone(),
        other => panic!("turn {} answered one record: {other:?}", request.plan.turn),
    }
}

/// The earlier turn `record` makes, its response as stored and one stored result per call.
fn earlier(shelf: &Shelf, record: &CallRecord) -> TurnRef {
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

/// The JSON text `text` names, canonical, as the gateway's record holds an output.
fn canonical(text: &str) -> Vec<u8> {
    to_canonical(&parse(text.as_bytes()).unwrap())
}

#[test]
fn the_demo_bundle_runs_its_scripted_turns_through_the_recorded_fake_to_its_reply() {
    let bundle = bundle();
    assert!(bundle.entry.agentic.is_some(), "the demo entry is agentic");
    let research = json("research.json");
    let scripted = research["turns"].as_array().unwrap();
    assert_eq!(bundle.turns.len(), scripted.len());
    let shelf = Shelf::default();
    let parts = parts(&bundle);
    let (mut worker, items) = started(&bundle, &shelf, &parts);
    let tools = [Tool::MarketBars, Tool::NewsSearch, Tool::Note];
    let mut transcript = Vec::new();
    for (n, script) in (1..).zip(scripted) {
        let request = turn(
            &bundle,
            RunKind::Ideate,
            &tools,
            n,
            items,
            transcript.clone(),
        );
        let record = answered(&mut worker, &request, CUT_OFF + 30);
        assert_eq!(record.outcome, CallOutcome::Ok, "turn {n}");
        assert_eq!(record.endpoint.as_str(), "recorded:demo");
        let asked: Vec<String> = record.tool_calls.iter().map(|c| c.name.clone()).collect();
        let written: Vec<String> = script["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|call| call["name"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(asked, written, "turn {n} asks for its scripted calls");
        transcript.push(earlier(&shelf, &record));
    }
    let last = u32::try_from(scripted.len()).unwrap() + 1;
    let request = turn(&bundle, RunKind::Ideate, &tools, last, items, transcript);
    let record = answered(&mut worker, &request, CUT_OFF + 40);
    assert_eq!(record.outcome, CallOutcome::Ok);
    assert!(
        record.tool_calls.is_empty(),
        "past the script, the final reply"
    );
    assert_eq!(
        record.output.as_ref().map(to_canonical),
        Some(canonical(research["reply"]["output"].as_str().unwrap()))
    );
}

/// DEC-2392, DEC-2275: the demo's feed brings one item on the demo instrument, from an allowlisted
/// source, after the agent is deployed; and the monitor reply cites that item alone.
#[test]
fn the_demo_feed_brings_one_allowlisted_item_on_the_demo_instrument_that_the_monitor_cites() {
    let bundle = bundle();
    let [item] = bundle.feed.as_slice() else {
        panic!("one feed item: {:?}", bundle.feed)
    };
    assert_eq!(
        item.symbols.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        ["SPY"]
    );
    let research = json("research.json");
    let allowed: Vec<&str> = research["allowlist"]["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    assert!(
        allowed.contains(&item.source.as_str()),
        "{}",
        item.source.as_str()
    );
    let mandate: Value = serde_json::from_str(
        &std::fs::read_to_string(Path::new(DEMO).join("../research-mandate.json")).unwrap(),
    )
    .unwrap();
    let research_envelope = &mandate["behavior"]["research"];
    let triggers = &research_envelope["event_triggers"];
    assert_eq!(triggers["sources"], serde_json::json!(["news"]));
    let debounce = triggers["debounce_s"].as_u64().unwrap();
    let interval = research_envelope["interval_s"].as_u64().unwrap();
    let arrives = u64::from(item.arrives_s);
    assert!(
        debounce <= arrives && arrives < interval,
        "the item arrives once the debounce from the first run has passed, and before the next \
         scheduled research call would make it no newer than the last ({debounce} <= {arrives} \
         < {interval})"
    );
    let monitor: Value =
        serde_json::from_str(research["monitor_reply"]["output"].as_str().unwrap()).unwrap();
    let invalidations = monitor["invalidations"].as_array().unwrap();
    assert_eq!(invalidations.len(), 1);
    assert_eq!(
        monitor.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["invalidations"],
        "a monitor output is invalidations alone"
    );
    assert_eq!(
        invalidations[0]["evidence"],
        serde_json::json!([item.item_id.as_str()])
    );
}

/// Agent harness spec §6.9: a monitor run over the demo bundle is answered at its first turn with
/// the monitor reply, naming the thesis the run was shown.
#[test]
fn a_monitor_run_over_the_demo_bundle_invalidates_the_thesis_it_was_shown() {
    let bundle = bundle();
    let mut shown = parts(&bundle);
    shown.active = vec![ActiveThesis {
        thesis_id: SHOWN.to_owned(),
        lineage_id: SHOWN.to_owned(),
        revision: 0,
        instrument: AssetId::new(DEMO_INSTRUMENT).unwrap(),
        symbol: Symbol::new("SPY").unwrap(),
        expires_at: RiskClock::from_secs(CUT_OFF + 86_400),
    }];
    let shelf = Shelf::default();
    let (mut worker, items) = started(&bundle, &shelf, &shown);
    let tools = [Tool::NewsSearch, Tool::NewsGet, Tool::Note];
    let request = turn(&bundle, RunKind::Monitor, &tools, 1, items, Vec::new());
    let record = answered(&mut worker, &request, CUT_OFF + 30);
    assert_eq!(record.outcome, CallOutcome::Ok, "{:?}", record.outcome);
    assert!(record.tool_calls.is_empty(), "no scripted turn");
    let research = json("research.json");
    let expected = research["monitor_reply"]["output"]
        .as_str()
        .unwrap()
        .replace("$THESIS_ID", SHOWN);
    assert_eq!(
        record.output.as_ref().map(to_canonical),
        Some(canonical(&expected))
    );
}
