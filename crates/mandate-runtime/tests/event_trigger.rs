//! E19-12's event trigger in the core (agent harness spec §5.3, mandate spec v0.11 §8.4, journal
//! spec rule 271; DEC-2392): an allowlisted item of a listed kind, about an instrument with an
//! active thesis, newer than the last research call, brings a `monitor` run forward, once per item,
//! at least `debounce_s` after the last call or event run, and at most `max_runs_per_day` times a
//! risk day. Its batch journals the item first, then `RunStarted` naming it. Each hand case pairs
//! the refusal with the same offer accepted. `fuzz` drives random offers and cancels, and an oracle
//! that keeps its own clock, count, and item set from the offers alone predicts each start.

mod common;

use std::collections::BTreeSet;

use common::research::{AAPL, CUT_OFF, Loop, RecordedPlane, World, config};
use common::{
    ACCOUNT_STREAM, AllowGate, FixedPlan, TestIds, clock, event, ports, text, universe, with_clock,
};
use mandate_canon::{Value, to_canonical};
use mandate_gateway::money::Usd6;
use mandate_journal::{Draft, check_batch, check_stream};
use mandate_research::AssetId;
use mandate_research_run::agentic::{LoopBounds, OutputMode, Tool};
use mandate_research_run::{ItemId, ItemKind, RetrievedItem, Symbol};
use mandate_runtime::{
    ActiveThesis, AgenticConfig, EventTriggers, FoldedEvent, Input, Mode, Ports, ResearchInput,
    ResearchRequest, ResearchStep, RetrievedObservation, RiskClock, WriterEpoch,
};
use proptest::prelude::*;

const DEBOUNCE_S: u32 = 1_800;

fn agentic() -> AgenticConfig {
    AgenticConfig {
        output: OutputMode::FinalOutput,
        bounds: LoopBounds::v0_2(),
        ideate_menu: BTreeSet::from([Tool::NewsSearch, Tool::NewsGet, Tool::Note]),
        monitor_menu: BTreeSet::from([Tool::NewsSearch, Tool::NewsGet, Tool::Note]),
        cost_ceiling: Usd6::from_micros(500_000),
    }
}

fn triggers(kinds: &[ItemKind], max_runs_per_day: u32) -> EventTriggers {
    EventTriggers {
        kinds: kinds.iter().copied().collect(),
        debounce_s: DEBOUNCE_S,
        max_runs_per_day,
    }
}

/// An agentic entry, an active thesis on AAPL, and news triggers.
fn world() -> World {
    let mut world = World::open();
    let mut entry = config();
    entry.agentic = Some(agentic());
    world.config = Some(entry);
    world.active = vec![ActiveThesis {
        thesis_id: "01ACTIVE".to_owned(),
        lineage_id: "01ACTIVE".to_owned(),
        revision: 0,
        instrument: AssetId::new(AAPL).unwrap_or_else(|e| panic!("{e}")),
        symbol: Symbol::new("AAPL").unwrap_or_else(|e| panic!("{e}")),
        expires_at: clock(CUT_OFF + 86_400 * 30),
    }];
    world.event_triggers = Some(triggers(&[ItemKind::News], 24));
    world
}

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

/// The recorded plane's news item `id`, renamed `rename` and known at `known` when given, as the
/// shell captures it for the core.
fn item(id: &str, rename: Option<&str>, known: Option<i64>) -> RetrievedObservation {
    let plane = RecordedPlane::load();
    let mut news = plane
        .news
        .iter()
        .find(|n| n.item_id.as_str() == id)
        .cloned()
        .unwrap_or_else(|| panic!("no item {id}"));
    if let Some(name) = rename {
        news.item_id = ItemId::new(name).unwrap_or_else(|e| panic!("{e}"));
        news.headline = format!("{} ({name})", news.headline);
    }
    if let Some(secs) = known {
        news.knowledge_at = common::research::instant(secs);
    }
    let (captured, _) = RetrievedObservation::capture(&RetrievedItem::News(news), clock(CUT_OFF))
        .unwrap_or_else(|e| panic!("{e}"));
    captured
}

const AAPL_NEWS: &str = "nw-20261016-aapl-services";
const MSFT_NEWS: &str = "nw-20261016-msft-inject";
const OFF_LIST: &str = "bl-20261016-aapl-rumor";

fn offer(
    lp: &mut Loop,
    ports: &Ports<'_>,
    world: &World,
    at: i64,
    item: RetrievedObservation,
) -> ResearchStep {
    lp.step(
        ResearchInput::Event {
            at: clock(at),
            item,
        },
        ports,
        world,
    )
    .unwrap_or_else(|e| panic!("{e:?}"))
}

/// Every artifact reference a payload holds, sorted and unique, as the envelope lists them.
fn refs(value: &Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut stack = vec![value];
    while let Some(v) = stack.pop() {
        match v {
            Value::Str(s) if s.starts_with("sha256:") => {
                out.insert(s.clone());
            }
            Value::Array(items) => stack.extend(items.iter()),
            Value::Object(members) => stack.extend(members.values()),
            _ => {}
        }
    }
    out
}

/// `draft` in an agent-stream envelope of this file's own making, with the run's `correlation_id`
/// and the payload's artifact references listed, parsed by the journal's registered schemas.
fn journal_draft(draft: &FoldedEvent, run: &str) -> Result<Draft, mandate_journal::Invalid> {
    let body = format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{}","stream_id":"agent:ws1:agent-a","event_type":"{}","schema_version":1,"event_time":"2026-10-16T14:00:00.000000000Z","clock_source":"scheduler","causation_id":null,"correlation_id":"{run}","actor":{{"kind":"agent","id":"agent-a","version":"0.1.0","build":"sha256:{}"}},"config_refs":{{"model_version":"sha256:{}"}},"payload":{},"artifact_refs":[{}],"pii_refs":[]}}"#,
        draft.event_id.0,
        draft.event_type,
        "c".repeat(64),
        "a".repeat(64),
        String::from_utf8_lossy(&to_canonical(&draft.payload)),
        refs(&draft.payload)
            .iter()
            .map(|r| format!(r#""{r}""#))
            .collect::<Vec<_>>()
            .join(",")
    );
    Draft::parse(body.as_bytes())
}

fn started(step: &ResearchStep) -> bool {
    step.requests
        .iter()
        .any(|r| matches!(r, ResearchRequest::Start { .. }))
}

fn last_two(lp: &Loop) -> Vec<FoldedEvent> {
    let journal = &lp.shell.agent_journal;
    journal[journal.len().saturating_sub(2)..].to_vec()
}

#[test]
fn a_matching_item_starts_a_monitor_run_journaling_the_item_first() {
    let (ids, plan, view) = (TestIds, FixedPlan::silent(), universe(&[]));
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let step = offer(
        &mut lp,
        &ports,
        &world,
        CUT_OFF,
        item(AAPL_NEWS, None, None),
    );
    assert!(started(&step), "{step:?}");
    let [seen, start] =
        <[FoldedEvent; 2]>::try_from(last_two(&lp)).unwrap_or_else(|_| panic!("two"));
    assert_eq!(seen.event_type, "ObservationRecorded");
    assert_eq!(start.event_type, "RunStarted");
    let p = |e: &FoldedEvent, m: &str| e.payload.get(m).and_then(Value::as_str).map(str::to_owned);
    assert_eq!(p(&seen, "instrument_id").as_deref(), Some("AAPL"));
    assert_eq!(p(&seen, "source").as_deref(), Some("src.newswire"));
    assert_eq!(p(&start, "kind").as_deref(), Some("monitor"));
    let trigger = start
        .payload
        .get("trigger")
        .unwrap_or_else(|| panic!("a trigger"));
    assert_eq!(trigger.get("kind").and_then(Value::as_str), Some("event"));
    assert_eq!(
        trigger.get("cause").and_then(Value::as_str),
        Some(seen.event_id.0.as_str())
    );
    assert!(
        step.correlation_id.is_some(),
        "both drafts carry the run's id"
    );
    let batch: Vec<Draft> = [&seen, &start]
        .iter()
        .map(|draft| {
            journal_draft(draft, step.correlation_id.as_deref().unwrap_or_default())
                .unwrap_or_else(|e| panic!("the journal refuses the {}: {e:?}", draft.event_type))
        })
        .collect();
    let earlier: Vec<Draft> = Vec::new();
    if let Err(refused) = check_batch(&batch).and_then(|()| check_stream(&earlier, &batch)) {
        panic!("the journal's batch and stream rules refuse the start: {refused:?}");
    }
}

/// Each condition of §5.3 on its own: the same offer, with one fact changed, starts nothing.
#[test]
fn every_condition_must_hold_or_nothing_is_journaled() {
    let (ids, plan, view) = (TestIds, FixedPlan::silent(), universe(&[]));
    let ports = ports(&ids, &AllowGate, &plan, &view);
    type Edit = fn(&mut World) -> RetrievedObservation;
    let cases: [(&str, Edit); 6] = [
        ("no event triggers in the version", |w| {
            w.event_triggers = None;
            item(AAPL_NEWS, None, None)
        }),
        ("news not a listed kind", |w| {
            w.event_triggers = Some(triggers(&[ItemKind::Filing], 24));
            item(AAPL_NEWS, None, None)
        }),
        ("a source off the allowlist", |_| item(OFF_LIST, None, None)),
        ("no active thesis on its instrument", |_| {
            item(MSFT_NEWS, None, None)
        }),
        ("a single_call entry", |w| {
            if let Some(entry) = w.config.as_mut() {
                entry.agentic = None;
            }
            item(AAPL_NEWS, None, None)
        }),
        ("a reservation inside the debounce", |w| {
            w.last_reservation = Some(clock(CUT_OFF - i64::from(DEBOUNCE_S) + 1));
            item(AAPL_NEWS, None, None)
        }),
    ];
    for (what, edit) in cases {
        let mut world = world();
        let offered = edit(&mut world);
        let mut lp = Loop::new(1);
        boot(&mut lp, &ports);
        let before = lp.shell.agent_journal.len();
        let step = offer(&mut lp, &ports, &world, CUT_OFF, offered);
        assert!(!started(&step), "{what}: {step:?}");
        assert_eq!(
            lp.shell.agent_journal.len(),
            before,
            "{what}: nothing journaled"
        );
    }
    let mut world = world();
    world.last_reservation = Some(clock(CUT_OFF - i64::from(DEBOUNCE_S)));
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    assert!(
        started(&offer(
            &mut lp,
            &ports,
            &world,
            CUT_OFF,
            item(AAPL_NEWS, None, None)
        )),
        "a reservation exactly the debounce ago"
    );
}

#[test]
fn a_run_in_flight_an_older_item_or_a_paused_agent_starts_nothing() {
    let (ids, plan, view) = (TestIds, FixedPlan::silent(), universe(&[]));
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    assert!(started(&offer(
        &mut lp,
        &ports,
        &world,
        CUT_OFF,
        item(AAPL_NEWS, None, None)
    )));
    let other = item(AAPL_NEWS, Some("nw-other-1"), None);
    assert!(
        !started(&offer(&mut lp, &ports, &world, CUT_OFF + 10_000, other)),
        "one run in flight"
    );
    let mut paused = Loop::new(1);
    boot(&mut paused, &ports);
    paused
        .shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            2,
            "AgentModeApplied",
            with_clock(
                &[("to", text("paused")), ("restriction", text("daily_loss"))],
                CUT_OFF - 30,
            ),
        ))
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!started(&offer(
        &mut paused,
        &ports,
        &world,
        CUT_OFF,
        item(AAPL_NEWS, None, None)
    )));
    let mut called = Loop::new(1);
    boot(&mut called, &ports);
    called
        .research
        .fold(&event(
            "agent:ws1:agent-a",
            1,
            "ModelInvocationRecorded",
            mandate_canon::parse(
                br#"{"purpose":"research","completed_at":"2026-10-16T14:00:00.000000000Z"}"#,
            )
            .unwrap_or_else(|e| panic!("{e:?}")),
        ))
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(
        !started(&offer(
            &mut called,
            &ports,
            &world,
            CUT_OFF + 7_200,
            item(AAPL_NEWS, None, None)
        )),
        "the item was known before the last call"
    );
    assert!(
        started(&offer(
            &mut called,
            &ports,
            &world,
            CUT_OFF + 7_200,
            item(AAPL_NEWS, Some("nw-newer-1"), Some(CUT_OFF + 3_600))
        )),
        "a newer item"
    );
}

/// One offer or a cancel of the run in flight at a second after the last.
#[derive(Debug, Clone)]
enum Step {
    /// An offer of item `n` (several offers may repeat one) after `gap` seconds.
    Offer { n: u8, gap: u32 },
    /// The run in flight, if any, cancelled past its deadline.
    Cancel,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        3 => (0u8..6, 0u32..40_000).prop_map(|(n, gap)| Step::Offer { n, gap }),
        2 => Just(Step::Cancel),
    ]
}

/// The America/New_York date of `secs`, the oracle's own: EDT is UTC−4 for every second the fuzz
/// reaches (2026-10-16 to 2026-10-29, before the 2026-11-01 change).
fn ny_day(secs: i64) -> i64 {
    (secs - 4 * 3_600).div_euclid(86_400)
}

/// The fuzz: the offers and cancels of `steps`, with the core's triggers at `cap` runs a day and
/// `debounce_s`, against the oracle's own count at 2 a day and [`DEBOUNCE_S`]. The first offer the
/// two disagree on, or `None`. When `forgets`, the core sees every offer under a name of its own,
/// as a core that never remembers an item would.
fn disagreement(steps: &[Step], cap: u32, debounce_s: u32, forgets: bool) -> Option<String> {
    let (ids, plan, view) = (TestIds, FixedPlan::silent(), universe(&[]));
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let mut world = world();
    world.event_triggers = Some(EventTriggers {
        kinds: BTreeSet::from([ItemKind::News]),
        debounce_s,
        max_runs_per_day: cap,
    });
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let mut now = CUT_OFF;
    let mut in_flight: Option<i64> = None;
    let mut starts: Vec<i64> = Vec::new();
    let mut used: BTreeSet<u8> = BTreeSet::new();
    for s in steps {
        match s {
            Step::Offer { n, gap } => {
                now += i64::from(*gap);
                let name = if forgets {
                    format!("nw-fuzz-{n}-{now}")
                } else {
                    format!("nw-fuzz-{n}")
                };
                let expected = in_flight.is_none()
                    && !used.contains(n)
                    && starts
                        .last()
                        .is_none_or(|last| now >= last + i64::from(DEBOUNCE_S))
                    && starts.iter().filter(|s| ny_day(**s) == ny_day(now)).count() < 2;
                let step = offer(
                    &mut lp,
                    &ports,
                    &world,
                    now,
                    item(AAPL_NEWS, Some(&name), Some(CUT_OFF - 3_600)),
                );
                if started(&step) != expected {
                    return Some(format!("offer {n} at {now}: oracle {expected}"));
                }
                if expected {
                    in_flight = Some(now);
                    starts.push(now);
                    used.insert(*n);
                }
            }
            Step::Cancel => {
                if let Some(at) = in_flight.take() {
                    now = now.max(at + 901);
                    lp.run(
                        ResearchInput::Poll(RiskClock::from_secs(now)),
                        &ports,
                        &world,
                    );
                    if lp.research.in_flight().is_some() {
                        return Some("a run outlived its deadline".to_owned());
                    }
                }
            }
        }
    }
    let event_starts = lp
        .events("RunStarted")
        .iter()
        .filter(|e| {
            e.payload
                .get("trigger")
                .and_then(|t| t.get("kind"))
                .and_then(Value::as_str)
                == Some("event")
        })
        .count();
    (event_starts != starts.len())
        .then(|| format!("{event_starts} starts, oracle {}", starts.len()))
}

#[test]
fn the_oracle_catches_seeded_breaches() {
    let three_in_a_day = [
        Step::Offer { n: 0, gap: 0 },
        Step::Cancel,
        Step::Offer { n: 1, gap: 1_000 },
        Step::Cancel,
        Step::Offer { n: 2, gap: 1_000 },
    ];
    assert_eq!(disagreement(&three_in_a_day, 2, DEBOUNCE_S, false), None);
    assert!(
        disagreement(&three_in_a_day, 3, DEBOUNCE_S, false).is_some(),
        "a core that allows a third run a day"
    );
    let soon = [
        Step::Offer { n: 0, gap: 0 },
        Step::Cancel,
        Step::Offer { n: 1, gap: 0 },
    ];
    assert_eq!(disagreement(&soon, 2, DEBOUNCE_S, false), None);
    assert!(
        disagreement(&soon, 2, 60, false).is_some(),
        "a core with a shorter debounce"
    );
    let again = [
        Step::Offer { n: 0, gap: 0 },
        Step::Cancel,
        Step::Offer { n: 0, gap: 3_600 },
    ];
    assert_eq!(disagreement(&again, 2, DEBOUNCE_S, false), None);
    assert!(
        disagreement(&again, 2, DEBOUNCE_S, true).is_some(),
        "a core that starts a run on an item it already ran on"
    );
}

/// One item starts one run (rule 271): the same item, offered again after its run ended and the
/// debounce passed, starts nothing; another item does.
#[test]
fn an_item_that_started_a_run_never_starts_another() {
    let (ids, plan, view) = (TestIds, FixedPlan::silent(), universe(&[]));
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let once = || item(AAPL_NEWS, Some("nw-once-1"), Some(CUT_OFF - 3_600));
    assert!(started(&offer(&mut lp, &ports, &world, CUT_OFF, once())));
    lp.run(
        ResearchInput::Poll(RiskClock::from_secs(CUT_OFF + 3_600)),
        &ports,
        &world,
    );
    assert!(!started(&offer(
        &mut lp,
        &ports,
        &world,
        CUT_OFF + 7_200,
        once()
    )));
    assert!(started(&offer(
        &mut lp,
        &ports,
        &world,
        CUT_OFF + 7_200,
        item(AAPL_NEWS, Some("nw-twice-2"), Some(CUT_OFF - 3_600))
    )));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, failure_persistence: None, ..ProptestConfig::default() })]

    /// Every offer starts a run exactly when the oracle says: nothing in flight, the item new, at
    /// least the debounce after the last start, and under the day's cap (2); the journal holds one
    /// event `RunStarted` per start.
    #[test]
    fn fuzz(steps in prop::collection::vec(step(), 1..18)) {
        prop_assert_eq!(disagreement(&steps, 2, DEBOUNCE_S, false), None);
    }
}

/// The cost cap (§5.3, admission check 7): at the day's cap nothing starts; a cent under, it does.
#[test]
fn the_day_s_spend_at_the_cost_cap_starts_nothing() {
    let (ids, plan, view) = (TestIds, FixedPlan::silent(), universe(&[]));
    let ports = ports(&ids, &AllowGate, &plan, &view);
    for (spend, starts) in [("5", false), ("4.99", true)] {
        let mut world = world();
        world.facts.research_spend_usd_today =
            mandate_num::Usd::parse(spend).unwrap_or_else(|e| panic!("{e}"));
        let mut lp = Loop::new(1);
        boot(&mut lp, &ports);
        assert_eq!(
            started(&offer(
                &mut lp,
                &ports,
                &world,
                CUT_OFF,
                item(AAPL_NEWS, None, None)
            )),
            starts,
            "spend {spend} against the 5 USD cap"
        );
    }
}

/// A `monitor` run may start in `exits_only` (§6.9): an event run is one.
#[test]
fn an_agent_in_exits_only_still_starts_an_event_run() {
    let (ids, plan, view) = (TestIds, FixedPlan::silent(), universe(&[]));
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    lp.shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            2,
            "AgentModeApplied",
            with_clock(
                &[
                    ("to", text("exits_only")),
                    ("restriction", text("daily_loss")),
                ],
                CUT_OFF - 30,
            ),
        ))
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(started(&offer(
        &mut lp,
        &ports,
        &world(),
        CUT_OFF,
        item(AAPL_NEWS, None, None)
    )));
}

/// A restart: a fresh research state folded from the journal alone refuses another item inside the
/// debounce, refuses the same item again after it, and starts the other item, exactly as the
/// process that wrote the journal would.
#[test]
fn a_restart_folds_the_debounce_the_day_and_the_items_back_from_the_journal() {
    let (ids, plan, view) = (TestIds, FixedPlan::silent(), universe(&[]));
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let once = || item(AAPL_NEWS, Some("nw-once-1"), Some(CUT_OFF - 3_600));
    assert!(started(&offer(&mut lp, &ports, &world, CUT_OFF, once())));
    lp.run(
        ResearchInput::Poll(RiskClock::from_secs(CUT_OFF + 901)),
        &ports,
        &world,
    );
    let (shell, _) = lp.shell.restart(&ports);
    let mut research = mandate_runtime::ResearchState::new();
    for one in &shell.agent_journal {
        research.fold(one).unwrap_or_else(|e| panic!("{e}"));
    }
    let mut back = Loop {
        shell,
        research,
        steps: Vec::new(),
    };
    let early = || item(AAPL_NEWS, Some("nw-early-2"), Some(CUT_OFF - 3_600));
    assert!(
        !started(&offer(
            &mut back,
            &ports,
            &world,
            CUT_OFF + i64::from(DEBOUNCE_S) - 1,
            early()
        )),
        "inside the debounce"
    );
    assert!(
        !started(&offer(
            &mut back,
            &ports,
            &world,
            CUT_OFF + i64::from(DEBOUNCE_S),
            once()
        )),
        "the same item, past the debounce"
    );
    assert!(started(&offer(
        &mut back,
        &ports,
        &world,
        CUT_OFF + i64::from(DEBOUNCE_S),
        early()
    )));
}

/// The fold finds an event run's cause by its id among the observations since the last start, so
/// another observation between them never loses the item, and a cause it cannot find is refused
/// rather than dropped.
#[test]
fn the_fold_finds_the_cause_by_id_and_refuses_one_it_cannot() {
    let digest = |c: char| format!("sha256:{}", c.to_string().repeat(64));
    let observed = |seq: u64, data: char| {
        event(
            "agent:ws1:agent-a",
            seq,
            "ObservationRecorded",
            mandate_canon::parse(
                format!(
                    r#"{{"source":"src.newswire","instrument_id":"AAPL","as_of":"2026-10-16T14:00:00.000000000Z","data_ref":"{}"}}"#,
                    digest(data)
                )
                .as_bytes(),
            )
            .unwrap_or_else(|e| panic!("{e:?}")),
        )
    };
    let start = |seq: u64, cause: &str| {
        event(
            "agent:ws1:agent-a",
            seq,
            "RunStarted",
            mandate_canon::parse(
                format!(
                    r#"{{"kind":"monitor","trigger":{{"kind":"event","cause":"{cause}"}},"cut_off":"2026-10-16T20:00:00.000000000Z"}}"#
                )
                .as_bytes(),
            )
            .unwrap_or_else(|e| panic!("{e:?}")),
        )
    };
    let mut research = mandate_runtime::ResearchState::new();
    let cause = observed(1, 'a');
    research.fold(&cause).unwrap_or_else(|e| panic!("{e}"));
    research
        .fold(&observed(2, 'b'))
        .unwrap_or_else(|e| panic!("{e}"));
    research
        .fold(&start(3, &cause.event_id.0))
        .unwrap_or_else(|e| panic!("{e}"));
    let mut lost = mandate_runtime::ResearchState::new();
    assert!(lost.fold(&start(1, "agent:ws1:agent-a-9")).is_err());
}

const STORED_ROW: &[u8] = include_bytes!("../../../fixtures/news/licensed.row.json");
const STORED_RECORD: &[u8] = include_bytes!("../../../fixtures/news/licensed.record.json");
/// The stored item's `knowledge_at`, 2026-10-26T15:00:00Z.
const STORED_KNOWN: i64 = 1_793_026_800;

/// DEC-2288: a news item the news reader stored, about SPY, on which the agent holds a thesis,
/// starts one `monitor` run, whose `ObservationRecorded` names the row's `item_ref` as its
/// `data_ref`, and offered again starts nothing.
#[test]
fn a_stored_news_item_starts_one_monitor_run_naming_its_item_ref() {
    let (ids, plan, view) = (TestIds, FixedPlan::silent(), universe(&[]));
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let mut world = world();
    world.active[0].symbol = Symbol::new("SPY").unwrap_or_else(|e| panic!("{e}"));
    world
        .facts
        .allowlist
        .sources
        .insert(mandate_research::SourceId::new("alpaca.news").unwrap_or_else(|e| panic!("{e}")));
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let stored = || {
        RetrievedObservation::stored(STORED_ROW, STORED_RECORD).unwrap_or_else(|e| panic!("{e:?}"))
    };
    let step = offer(&mut lp, &ports, &world, STORED_KNOWN + 60, stored());
    assert!(started(&step), "{step:?}");
    let [seen, start] =
        <[FoldedEvent; 2]>::try_from(last_two(&lp)).unwrap_or_else(|_| panic!("two"));
    assert_eq!(seen.event_type, "ObservationRecorded");
    assert_eq!(start.event_type, "RunStarted");
    let p = |e: &FoldedEvent, m: &str| e.payload.get(m).and_then(Value::as_str).map(str::to_owned);
    assert_eq!(
        p(&seen, "data_ref").as_deref(),
        Some("sha256:bbf39bbd6017fb65b0b82df4d4c25ec382d7072bcc2ea2b6f30ed7eb7792f90b"),
        "the row's item_ref, as it is"
    );
    assert_eq!(p(&seen, "instrument_id").as_deref(), Some("SPY"));
    assert_eq!(p(&seen, "source").as_deref(), Some("alpaca.news"));
    assert_eq!(p(&start, "kind").as_deref(), Some("monitor"));
    lp.run(
        ResearchInput::Poll(RiskClock::from_secs(STORED_KNOWN + 3_600)),
        &ports,
        &world,
    );
    assert!(
        !started(&offer(
            &mut lp,
            &ports,
            &world,
            STORED_KNOWN + 7_200,
            stored()
        )),
        "one stored item starts one run"
    );
}
