//! E19-5's fuzz (agent harness spec HI-19, mandate spec MI-19, journal spec §9.21, DEC-2202): random
//! active theses, random invalidations, and random theses on the same instruments, each judged in
//! one research run. An oracle computed from the inputs alone, never from the judge, asserts:
//! exactly the invalidations that name an active thesis are accepted, each journaled once as
//! `ThesisInvalidated` after the call record and before every thesis record; no thesis record ever
//! continues an invalidated thesis's lineage (a thesis on its instrument is a new admission, never a
//! renewal); and an invalidation never adds a model output (rule 2: removal adds no risk).
//! `the_oracle_catches_seeded_breaches` shows each check failing on a journal broken by hand.

mod common;

use std::collections::BTreeSet;

use common::research::{AAPL, CUT_OFF, Loop, MSFT, RecordedPlane, World, full_run, recorded};
use common::{
    ACCOUNT_STREAM, AllowGate, FixedPlan, TestIds, clock, event, ports, text, universe, with_clock,
};
use mandate_canon::Value;
use mandate_research::{AssetId, WorkingUniverse};
use mandate_runtime::{ActiveThesis, FoldedEvent, Input, Mode, Ports, WriterEpoch};
use proptest::prelude::*;

const SERVICES: &str = "nw-20261016-aapl-services";
const SUPPLY: &str = "w2-20261016-aapl-supply";

/// The two instruments the recorded plane has items for, as (asset id, symbol).
const INSTRUMENTS: [(&str, &str); 2] = [(AAPL, "AAPL"), (MSFT, "MSFT")];

fn boot(lp: &mut Loop, ports: &Ports<'_>) {
    lp.shell.run(Input::Started(WriterEpoch(1)), ports);
    lp.shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            1,
            "ReconciliationRun",
            with_clock(&[("result", text("clean"))], CUT_OFF - 120),
        ))
        .unwrap_or_else(|e| panic!("{e}"));
    lp.shell.run(Input::Tick(clock(CUT_OFF - 60)), ports);
    assert_eq!(lp.shell.state.effective_mode(), Mode::Normal);
}

/// One case: which instruments hold an active thesis, which thesis ids the output invalidates (in
/// order, no id twice, some naming no active thesis), and which instruments it proposes theses on.
#[derive(Debug, Clone)]
struct Case {
    active: Vec<usize>,
    invalidate: Vec<String>,
    propose_aapl: bool,
}

fn active_id(index: usize) -> String {
    format!("01ACTIVE{index}")
}

fn case() -> impl Strategy<Value = Case> {
    (
        proptest::sample::subsequence(vec![0usize, 1], 0..=2),
        proptest::sample::subsequence(vec![active_id(0), active_id(1), "01NOPE".to_owned()], 0..=3)
            .prop_shuffle(),
        any::<bool>(),
    )
        .prop_map(|(active, invalidate, propose_aapl)| Case {
            active,
            invalidate,
            propose_aapl,
        })
}

fn world_of(case: &Case) -> World {
    let mut world = World::open();
    world.active = case
        .active
        .iter()
        .map(|&i| {
            let (instrument, symbol) = INSTRUMENTS[i];
            ActiveThesis {
                thesis_id: active_id(i),
                lineage_id: active_id(i),
                revision: 0,
                instrument: AssetId::new(instrument).unwrap_or_else(|e| panic!("{e}")),
                symbol: mandate_research_run::Symbol::new(symbol).unwrap_or_else(|e| panic!("{e}")),
                expires_at: clock(CUT_OFF + 86_400),
            }
        })
        .collect();
    world.universe = WorkingUniverse::Known {
        instruments: case
            .active
            .iter()
            .map(|&i| AssetId::new(INSTRUMENTS[i].0).unwrap_or_else(|e| panic!("{e}")))
            .collect(),
        pinned: false,
    };
    world
}

fn output_of(case: &Case) -> String {
    let theses = if case.propose_aapl {
        format!(
            r#"{{"instrument":"AAPL","direction":"long","conviction":"0.6","confidence":"0.6","horizon_s":86400,"thesis":"x","invalidation":"It stops.","evidence":["{SERVICES}","{SUPPLY}"]}}"#
        )
    } else {
        String::new()
    };
    let invalidations: Vec<String> = case
        .invalidate
        .iter()
        .map(|id| format!(r#"{{"thesis_id":"{id}","evidence":[]}}"#))
        .collect();
    format!(
        r#"{{"theses":[{theses}],"invalidations":[{}]}}"#,
        invalidations.join(",")
    )
}

fn judged(case: &Case) -> Vec<FoldedEvent> {
    let (ids, plan, view) = (TestIds, FixedPlan::silent(), universe(&[]));
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let plane = RecordedPlane::load();
    full_run(
        &mut lp,
        clock(CUT_OFF),
        &ports,
        &world_of(case),
        &plane,
        &recorded(&output_of(case)),
    );
    lp.shell.agent_journal.clone()
}

fn str_of<'a>(event: &'a FoldedEvent, member: &str) -> Option<&'a str> {
    event.payload.get(member).and_then(Value::as_str)
}

/// The oracle: what the inputs alone say must hold of the journal a run wrote. Returns the first
/// breach found, if any.
fn breach(case: &Case, journal: &[FoldedEvent]) -> Option<String> {
    let accepted: Vec<String> = case
        .invalidate
        .iter()
        .filter(|id| case.active.iter().any(|&i| active_id(i) == **id))
        .cloned()
        .collect();
    let invalidated: Vec<&FoldedEvent> = journal
        .iter()
        .filter(|e| e.event_type == "ThesisInvalidated")
        .collect();
    let got: Vec<&str> = invalidated
        .iter()
        .filter_map(|e| str_of(e, "thesis_id"))
        .collect();
    if got != accepted.iter().map(String::as_str).collect::<Vec<_>>() {
        return Some(format!("accepted {accepted:?}, journaled {got:?}"));
    }
    let position = |pred: &dyn Fn(&FoldedEvent) -> bool| journal.iter().position(pred);
    if !invalidated.is_empty() {
        let call = position(&|e| e.event_type == "ModelInvocationRecorded");
        let first_invalidation = position(&|e| e.event_type == "ThesisInvalidated");
        let last_invalidation = journal
            .iter()
            .rposition(|e| e.event_type == "ThesisInvalidated");
        let first_thesis =
            position(&|e| matches!(e.event_type.as_str(), "ThesisProposed" | "ThesisRevised"));
        if !(call < first_invalidation
            && first_thesis.is_none_or(|t| last_invalidation.is_some_and(|l| l < t)))
        {
            return Some("the invalidations are not between the call record and the theses".into());
        }
        let cause = call.and_then(|i| journal.get(i)).map(|e| &e.event_id);
        if invalidated.iter().any(|e| e.causation_id.as_ref() != cause) {
            return Some("an invalidation does not name its call record as cause".into());
        }
    }
    let ended: BTreeSet<&str> = accepted.iter().map(String::as_str).collect();
    for record in journal
        .iter()
        .filter(|e| matches!(e.event_type.as_str(), "ThesisProposed" | "ThesisRevised"))
    {
        if str_of(record, "lineage_id").is_some_and(|l| ended.contains(l)) {
            return Some(format!(
                "{} continues the invalidated lineage {:?}",
                record.event_type,
                str_of(record, "lineage_id")
            ));
        }
    }
    let outputs = journal
        .iter()
        .filter(|e| e.event_type == "ModelOutputRecorded")
        .count();
    let theses = journal
        .iter()
        .filter(|e| matches!(e.event_type.as_str(), "ThesisProposed" | "ThesisRevised"))
        .count();
    if outputs > theses {
        return Some(format!("{outputs} outputs from {theses} thesis records"));
    }
    None
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    #[test]
    fn invalidations_remove_once_before_any_thesis_and_never_renew_or_add_an_output(case in case()) {
        let journal = judged(&case);
        prop_assert_eq!(breach(&case, &journal), None, "{:?}", case);
    }
}

/// Each of the oracle's checks fails on a journal broken by hand, so a pass means something.
#[test]
fn the_oracle_catches_seeded_breaches() {
    let case = Case {
        active: vec![0, 1],
        invalidate: vec![active_id(1)],
        propose_aapl: true,
    };
    let journal = judged(&case);
    assert_eq!(breach(&case, &journal), None, "the honest run passes");
    let at = |t: &str| {
        journal
            .iter()
            .position(|e| e.event_type == t)
            .unwrap_or_else(|| panic!("no {t}"))
    };

    let mut dropped = journal.clone();
    dropped.remove(at("ThesisInvalidated"));
    assert!(breach(&case, &dropped).is_some(), "a dropped invalidation");

    let mut doubled = journal.clone();
    doubled.insert(
        at("ThesisInvalidated"),
        journal[at("ThesisInvalidated")].clone(),
    );
    assert!(breach(&case, &doubled).is_some(), "an invalidation twice");

    let mut late = journal.clone();
    let record = late.remove(at("ThesisInvalidated"));
    late.push(record);
    assert!(
        breach(&case, &late).is_some(),
        "an invalidation after the theses"
    );

    let mut uncaused = journal.clone();
    uncaused[at("ThesisInvalidated")].causation_id = None;
    assert!(
        breach(&case, &uncaused).is_some(),
        "an invalidation with no cause"
    );

    let mut renewed = journal.clone();
    if let Value::Object(members) = &mut renewed[at("ThesisProposed")].payload {
        members.insert(
            mandate_canon::Key::new("lineage_id").unwrap_or_else(|e| panic!("{e}")),
            Value::Str(active_id(1)),
        );
    }
    assert!(
        breach(&case, &renewed).is_some(),
        "a renewed invalidated lineage"
    );

    let mut output = journal.clone();
    output.push(journal[at("ModelOutputRecorded")].clone());
    assert!(breach(&case, &output).is_some(), "an extra output");
}
