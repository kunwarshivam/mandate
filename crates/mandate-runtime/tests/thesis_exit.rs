//! The thesis exit's fold (mandate spec v0.10 §8.3 step 2, §8.6, MI-34; DEC-2391): which
//! instruments' current thesis has ended, read from the runtime's own agent stream and the account
//! stream's removals, and handed to the order plan as `SignalInputs::ended_theses`.
//!
//! A thesis is current from the latest admitted thesis record naming its instrument, and ends at
//! the first of a `ThesisInvalidated` naming it, the risk clock reaching its `expires_at`, a
//! refused record with reason `lineage_retired` in its lineage, or an account-stream
//! `UniverseChanged` (`removed`, a thesis reason, this agent) naming it. The hand cases pin each
//! rule with the step beside it that does not end; `fuzz` folds random journals and compares the
//! fold, at every tick, with an oracle that keeps its own per-instrument history and never calls
//! `mandate_research`; `the_oracle_catches_seeded_breaches` shows the comparison failing when the
//! fold is handed a journal with one end removed or moved.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{
    ACCOUNT_STREAM, AGENT, AGENT_STREAM, AllowGate, Shell, TestIds, clock, event, int, object,
    ports, text, universe, with_clock,
};
use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_canon::{Value, to_canonical};
use mandate_journal::Draft;
use mandate_num::{Price, Qty};
use mandate_runtime::{
    Autonomy, Classified, EventDraft, ExitOrigin, FoldedEvent, Input, MandateView, OrderPlan,
    Proposal, Purpose, RuntimeError, RuntimeState, SignalInputs, WriterEpoch, fold,
};
use proptest::prelude::*;

const A: &str = "7b4a1c2e-1111-4a2b-9c3d-000000000001";
const B: &str = "7b4a1c2e-2222-4a2b-9c3d-000000000002";
const C: &str = "7b4a1c2e-3333-4a2b-9c3d-000000000003";

/// The risk clock every hand case starts at.
const T0: i64 = 1_792_180_800;

/// `secs` since the epoch as RFC 3339 UTC, by the civil-from-days algorithm, written here rather
/// than taken from `mandate-time` so the fixture does not share the parser it feeds.
fn rfc3339(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// What a test journal holds, before it is sequenced onto its stream.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    /// A thesis record: `Some(reason)` refuses it, `None` admits it.
    Thesis {
        thesis: String,
        lineage: String,
        instrument: &'static str,
        expires: i64,
        refused: Option<&'static str>,
    },
    Invalidated {
        thesis: String,
    },
    /// An account-stream `UniverseChanged`.
    Universe {
        agent: &'static str,
        change: &'static str,
        reason: &'static str,
        thesis: String,
        instrument: &'static str,
    },
    Tick(i64),
}

fn admitted(thesis: &str, lineage: &str, instrument: &'static str, expires: i64) -> Op {
    Op::Thesis {
        thesis: thesis.to_owned(),
        lineage: lineage.to_owned(),
        instrument,
        expires,
        refused: None,
    }
}

fn refused(thesis: &str, lineage: &str, instrument: &'static str, reason: &'static str) -> Op {
    Op::Thesis {
        thesis: thesis.to_owned(),
        lineage: lineage.to_owned(),
        instrument,
        expires: T0 + 86_400,
        refused: Some(reason),
    }
}

fn invalidated(thesis: &str) -> Op {
    Op::Invalidated {
        thesis: thesis.to_owned(),
    }
}

fn removed(reason: &'static str, thesis: &str, instrument: &'static str) -> Op {
    Op::Universe {
        agent: AGENT,
        change: "removed",
        reason,
        thesis: thesis.to_owned(),
        instrument,
    }
}

fn thesis_record(
    thesis: &str,
    lineage: &str,
    instrument: &str,
    expires: i64,
    refused: Option<&str>,
) -> Value {
    object(&[
        ("thesis_id", text(thesis)),
        ("lineage_id", text(lineage)),
        ("instrument_id", text(instrument)),
        ("revision", int(0)),
        ("as_of", text(&rfc3339(expires - 86_400))),
        ("expires_at", text(&rfc3339(expires))),
        ("admitted", Value::Bool(refused.is_none())),
        ("reason", refused.map_or(Value::Null, text)),
    ])
}

/// The events of `ops`, each on its stream in order: thesis records and invalidations on the
/// agent stream, removals on the account stream. A tick is no event.
fn events(ops: &[Op]) -> Vec<FoldedEvent> {
    let (mut agent, mut account) = (0, 0);
    let mut out = Vec::new();
    for op in ops {
        match op {
            Op::Thesis {
                thesis,
                lineage,
                instrument,
                expires,
                refused,
            } => {
                agent += 1;
                out.push(event(
                    AGENT_STREAM,
                    agent,
                    "ThesisProposed",
                    thesis_record(thesis, lineage, instrument, *expires, *refused),
                ));
            }
            Op::Invalidated { thesis } => {
                agent += 1;
                out.push(event(
                    AGENT_STREAM,
                    agent,
                    "ThesisInvalidated",
                    object(&[("thesis_id", text(thesis))]),
                ));
            }
            Op::Universe {
                agent: owner,
                change,
                reason,
                thesis,
                instrument,
            } => {
                account += 1;
                out.push(event(
                    ACCOUNT_STREAM,
                    account,
                    "UniverseChanged",
                    with_clock(
                        &[
                            ("agent_id", text(owner)),
                            ("instrument", text(instrument)),
                            ("change", text(change)),
                            ("reason", text(reason)),
                            ("thesis_id", text(thesis)),
                            ("lineage_id", text(thesis)),
                        ],
                        0,
                    ),
                ));
            }
            Op::Tick(_) => {}
        }
    }
    out
}

fn folded(ops: &[Op]) -> RuntimeState {
    let mut state = RuntimeState::new(common::deployment());
    for one in events(ops) {
        fold(&mut state, &one).unwrap_or_else(|e| panic!("{} folds: {e}", one.event_type));
    }
    state
}

fn ended(ops: &[Op], at: i64) -> BTreeSet<String> {
    folded(ops)
        .ended_theses(clock(at))
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect()
}

fn set(instruments: &[&str]) -> BTreeSet<String> {
    instruments.iter().map(|&i| i.to_owned()).collect()
}

#[test]
fn an_admitted_thesis_has_not_ended_before_its_horizon() {
    let ops = [admitted("th1", "th1", A, T0 + 600)];
    assert_eq!(ended(&ops, T0), set(&[]));
    assert_eq!(ended(&ops, T0 + 599), set(&[]));
}

#[test]
fn the_horizon_ends_a_thesis_at_its_expires_at_inclusive() {
    let ops = [admitted("th1", "th1", A, T0 + 600)];
    assert_eq!(
        ended(&ops, T0 + 600),
        set(&[A]),
        "now >= expires_at ends it"
    );
    assert_eq!(ended(&ops, T0 + 601), set(&[A]));
}

#[test]
fn an_invalidation_ends_the_current_thesis_at_once() {
    let ops = [
        admitted("th1", "th1", A, T0 + 86_400),
        admitted("th2", "th2", B, T0 + 86_400),
        invalidated("th1"),
    ];
    assert_eq!(ended(&ops, T0), set(&[A]), "only the thesis it names");
}

#[test]
fn an_invalidation_of_a_superseded_thesis_ends_nothing() {
    let ops = [
        admitted("th1", "lin", A, T0 + 86_400),
        admitted("th2", "lin", A, T0 + 86_400),
        invalidated("th1"),
    ];
    assert_eq!(
        ended(&ops, T0),
        set(&[]),
        "th2 is current, and nothing named it"
    );
    let named = [
        admitted("th1", "lin", A, T0 + 86_400),
        admitted("th2", "lin", A, T0 + 86_400),
        invalidated("th2"),
    ];
    assert_eq!(ended(&named, T0), set(&[A]));
}

#[test]
fn a_retired_lineage_ends_its_thesis_and_no_other_refusal_does() {
    let retired = [
        admitted("th1", "lin1", A, T0 + 86_400),
        admitted("th2", "lin2", B, T0 + 86_400),
        refused("th3", "lin1", A, "lineage_retired"),
    ];
    assert_eq!(ended(&retired, T0), set(&[A]), "lin1's thesis, not lin2's");
    let other = [
        admitted("th1", "lin1", A, T0 + 86_400),
        refused("th3", "lin1", A, "no_corroboration"),
    ];
    assert_eq!(ended(&other, T0), set(&[]));
    let elsewhere = [
        admitted("th1", "lin1", A, T0 + 86_400),
        refused("th3", "lin9", C, "lineage_retired"),
    ];
    assert_eq!(
        ended(&elsewhere, T0),
        set(&[]),
        "another lineage's retirement"
    );
}

#[test]
fn a_fresh_admission_makes_the_instrument_current_again() {
    let ops = [
        admitted("th1", "th1", A, T0 + 86_400),
        invalidated("th1"),
        admitted("th2", "th2", A, T0 + 86_400),
    ];
    assert_eq!(ended(&ops, T0), set(&[]));
    assert_eq!(ended(&ops[..2], T0), set(&[A]));
}

#[test]
fn an_account_removal_with_a_thesis_reason_ends_the_thesis_it_names() {
    for reason in ["thesis_invalidated", "thesis_expired", "lineage_retired"] {
        let ops = [
            admitted("th1", "th1", A, T0 + 86_400),
            removed(reason, "th1", A),
        ];
        assert_eq!(ended(&ops, T0), set(&[A]), "{reason}");
    }
}

#[test]
fn an_account_removal_ends_nothing_for_another_reason_agent_change_or_thesis() {
    let base = || admitted("th1", "th1", A, T0 + 86_400);
    for reason in ["eligibility_lost", "operator_halt", "version_applied"] {
        assert_eq!(
            ended(&[base(), removed(reason, "th1", A)], T0),
            set(&[]),
            "{reason} keeps §2.2's reading"
        );
    }
    let foreign = Op::Universe {
        agent: "agent-b",
        change: "removed",
        reason: "thesis_invalidated",
        thesis: "th1".to_owned(),
        instrument: A,
    };
    assert_eq!(ended(&[base(), foreign], T0), set(&[]), "another agent's");
    let admission = Op::Universe {
        agent: AGENT,
        change: "admitted",
        reason: "thesis_invalidated",
        thesis: "th1".to_owned(),
        instrument: A,
    };
    assert_eq!(ended(&[base(), admission], T0), set(&[]), "an admission");
    assert_eq!(
        ended(&[base(), removed("thesis_invalidated", "th0", A)], T0),
        set(&[]),
        "another thesis"
    );
}

#[test]
fn a_thesis_record_the_fold_cannot_read_is_refused_not_dropped() {
    let mut cases = Vec::new();
    for member in [
        "thesis_id",
        "lineage_id",
        "instrument_id",
        "revision",
        "expires_at",
        "admitted",
    ] {
        let Value::Object(mut body) = thesis_record("th1", "th1", A, T0, None) else {
            unreachable!("an object");
        };
        body.remove(&common::key(member));
        cases.push((member, Value::Object(body)));
    }
    cases.push((
        "reason",
        thesis_record("th1", "th1", A, T0, Some("not_a_reason")),
    ));
    let Value::Object(mut admitted_with_reason) = thesis_record("th1", "th1", A, T0, None) else {
        unreachable!("an object");
    };
    admitted_with_reason.insert(common::key("reason"), text("no_corroboration"));
    cases.push(("reason", Value::Object(admitted_with_reason)));
    for (member, body) in cases {
        let mut state = RuntimeState::new(common::deployment());
        let result = fold(&mut state, &event(AGENT_STREAM, 1, "ThesisProposed", body));
        assert!(
            matches!(&result, Err(RuntimeError::NonCanonicalPayload { field }) if field == member),
            "{member}: {result:?}"
        );
    }
    let mut state = RuntimeState::new(common::deployment());
    let result = fold(
        &mut state,
        &event(AGENT_STREAM, 1, "ThesisInvalidated", object(&[])),
    );
    assert!(matches!(
        result,
        Err(RuntimeError::NonCanonicalPayload { .. })
    ));
}

/// A plan that proposes the §8.3 step 2 exit for every ended instrument the test says the agent
/// holds and has no exit working in, and records what it was handed.
struct ThesisExitPlan {
    held: BTreeSet<InstrumentId>,
    seen: std::cell::RefCell<Vec<BTreeSet<InstrumentId>>>,
}

impl OrderPlan for ThesisExitPlan {
    fn plan(&self, _view: &MandateView, inputs: &SignalInputs) -> Option<Proposal> {
        self.seen.borrow_mut().push(inputs.ended_theses.clone());
        let instrument = inputs.ended_theses.iter().find(|instrument| {
            self.held.contains(*instrument) && !inputs.working_exits.contains_key(*instrument)
        })?;
        Some(Proposal {
            instrument: instrument.clone(),
            asset_class: AssetClass::UsEquity,
            side: Side::Sell,
            qty: Qty::parse("10").unwrap_or_else(|e| panic!("{e}")),
            limit: Price::parse("99.5").unwrap_or_else(|e| panic!("{e}")),
            purpose: Purpose::DiscretionaryExit,
            exit_origin: Some(ExitOrigin::RemovedInstrument),
            exit_conviction: None,
            buy_conviction: None,
            combined_score: Value::Null,
            outputs_used: BTreeSet::new(),
            model_weights: BTreeMap::new(),
            clips_applied: Vec::new(),
            execution: None,
        })
    }

    fn classify(&self, _view: &MandateView, _proposal: &Proposal) -> Classified {
        Classified {
            autonomy: Autonomy::Auto,
            decided_by: Some("builtin_risk_reducing".to_owned()),
        }
    }
}

/// The draft in an agent-stream envelope of this file's own making, parsed by the journal's
/// registered schemas and consistency rules, rule 8 among them, which share no code with the
/// runtime's writer.
fn journal_parses(draft: &EventDraft) -> Result<Draft, mandate_journal::Invalid> {
    let causation = draft
        .causation_id
        .as_ref()
        .map_or_else(|| "null".to_owned(), |id| format!("\"{}\"", id.0));
    let body = format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{}",
        "stream_id":"{AGENT_STREAM}","event_type":"{}","schema_version":1,
        "event_time":"2026-09-21T13:59:58.000000000Z","clock_source":"scheduler",
        "causation_id":{causation},"correlation_id":null,
        "actor":{{"kind":"agent","id":"agent-a","version":"0.1.0","build":"sha256:{}"}},
        "config_refs":{{"mandate_version":"sha256:{}"}},
        "payload":{},"artifact_refs":[],"pii_refs":[]}}"#,
        draft.event_id.0,
        draft.event_type,
        "c".repeat(64),
        "d".repeat(64),
        String::from_utf8_lossy(&to_canonical(&draft.payload))
    );
    Draft::parse(body.as_bytes())
}

fn booted(shell: &mut Shell, ports: &mandate_runtime::Ports<'_>) {
    shell.run(Input::Started(WriterEpoch(1)), ports);
    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            1,
            "ReconciliationRun",
            with_clock(&[("result", text("clean"))], T0 - 60),
        ))
        .unwrap_or_else(|e| panic!("{e}"));
}

/// Appends `body` to the shell's agent stream as the next event.
fn journal(shell: &mut Shell, event_type: &str, body: Value) {
    let seq = shell.head().0 + 1;
    let one = event(AGENT_STREAM, seq, event_type, body);
    shell.agent_journal.push(one.clone());
    fold(&mut shell.state, &one).unwrap_or_else(|e| panic!("{event_type} folds: {e}"));
}

#[test]
fn a_tick_hands_the_plan_the_ended_theses_and_journals_their_exit() {
    let held = BTreeSet::from([InstrumentId::new(A).unwrap_or_else(|e| panic!("{e}"))]);
    let plan = ThesisExitPlan {
        held,
        seen: Default::default(),
    };
    let (ids, view) = (TestIds, universe(&[]));
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let mut shell = Shell::new(1);
    booted(&mut shell, &ports);
    journal(
        &mut shell,
        "ThesisProposed",
        thesis_record("th1", "th1", A, T0 + 86_400, None),
    );
    let quiet = shell.run(Input::Tick(clock(T0)), &ports);
    assert!(
        !quiet.draft_types().contains(&"IntentProposed"),
        "a current thesis sells nothing: {:?}",
        quiet.draft_types()
    );
    journal(
        &mut shell,
        "ThesisInvalidated",
        object(&[("thesis_id", text("th1"))]),
    );
    let ran = shell.run(Input::Tick(clock(T0 + 1)), &ports);
    let seen = plan.seen.borrow();
    assert_eq!(
        seen.last()
            .map(|s| s.iter().map(|i| i.as_str()).collect::<Vec<_>>()),
        Some(vec![A])
    );
    let decision = ran
        .drafts
        .iter()
        .find(|d| d.event_type == "DecisionMade")
        .unwrap_or_else(|| panic!("decided: {:?}", ran.draft_types()));
    assert_eq!(
        decision.payload.get("exit_origin").and_then(Value::as_str),
        Some("removed_instrument")
    );
    assert_eq!(
        decision.payload.get("purpose").and_then(Value::as_str),
        Some("discretionary_exit")
    );
    assert!(ran.draft_types().contains(&"IntentProposed"));
    assert_eq!(ran.handed.len(), 1, "handed to the executor");
    for draft in &ran.drafts {
        if let Err(refused) = journal_parses(draft) {
            panic!("the journal refuses the {}: {refused:?}", draft.event_type);
        }
    }
}

#[test]
fn the_exit_is_proposed_in_exits_only_and_not_while_paused() {
    let held = BTreeSet::from([InstrumentId::new(A).unwrap_or_else(|e| panic!("{e}"))]);
    for (mode, proposes) in [("exits_only", true), ("paused", false), ("stopped", false)] {
        let plan = ThesisExitPlan {
            held: held.clone(),
            seen: Default::default(),
        };
        let (ids, view) = (TestIds, universe(&[]));
        let ports = ports(&ids, &AllowGate, &plan, &view);
        let mut shell = Shell::new(1);
        booted(&mut shell, &ports);
        shell
            .fold_one(&event(
                ACCOUNT_STREAM,
                2,
                "AgentModeApplied",
                with_clock(&[("agent", text(AGENT)), ("to", text(mode))], T0),
            ))
            .unwrap_or_else(|e| panic!("{e}"));
        journal(
            &mut shell,
            "ThesisProposed",
            thesis_record("th1", "th1", A, T0 + 86_400, None),
        );
        journal(
            &mut shell,
            "ThesisInvalidated",
            object(&[("thesis_id", text("th1"))]),
        );
        let ran = shell.run(Input::Tick(clock(T0 + 1)), &ports);
        assert_eq!(
            ran.draft_types().contains(&"IntentProposed"),
            proposes,
            "{mode}: {:?}",
            ran.draft_types()
        );
    }
}

/// The oracle: each instrument's history kept as plain records, read backwards from `at`'s
/// position, with no lineage state and no call into `mandate_research`.
fn oracle(ops: &[Op], at: i64) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for instrument in [A, B, C] {
        let latest = ops.iter().rposition(
            |op| matches!(op, Op::Thesis { instrument: i, refused: None, .. } if *i == instrument),
        );
        let Some(position) = latest else { continue };
        let Op::Thesis {
            thesis,
            lineage,
            expires,
            ..
        } = &ops[position]
        else {
            unreachable!("found a thesis");
        };
        let later = &ops[position + 1..];
        let invalidated = later
            .iter()
            .any(|op| matches!(op, Op::Invalidated { thesis: t } if t == thesis));
        let removed = later.iter().any(|op| {
            matches!(op, Op::Universe { agent, change, reason, thesis: t, .. }
                if *agent == AGENT && *change == "removed" && t == thesis
                    && ["thesis_invalidated", "thesis_expired", "lineage_retired"].contains(reason))
        });
        let retired = ops.iter().any(|op| {
            matches!(op, Op::Thesis { lineage: l, refused: Some("lineage_retired"), .. } if l == lineage)
        });
        if invalidated || removed || retired || at >= *expires {
            out.insert(instrument.to_owned());
        }
    }
    out
}

const INSTRUMENTS: [&str; 3] = [A, B, C];
const LINEAGES: [&str; 3] = ["lin0", "lin1", "lin2"];

/// A random op, given how many theses were admitted before it, so an invalidation or a removal
/// can name an earlier thesis (current, superseded, or unknown).
fn op(admitted_before: usize) -> impl Strategy<Value = Op> {
    let named = 0..admitted_before.max(1) + 1;
    prop_oneof![
        4 => (0..3usize, 0..3usize, 1..7_200i64).prop_map(move |(i, l, horizon)| Op::Thesis {
            thesis: format!("th{admitted_before}"),
            lineage: LINEAGES[l].to_owned(),
            instrument: INSTRUMENTS[i],
            expires: T0 + horizon,
            refused: None,
        }),
        1 => (0..3usize, 0..3usize, 0..2usize).prop_map(move |(i, l, r)| Op::Thesis {
            thesis: format!("ref{admitted_before}"),
            lineage: LINEAGES[l].to_owned(),
            instrument: INSTRUMENTS[i],
            expires: T0 + 86_400,
            refused: Some(["lineage_retired", "no_corroboration"][r]),
        }),
        2 => named.clone().prop_map(|n| Op::Invalidated { thesis: format!("th{n}") }),
        1 => (named, 0..3usize, 0..3usize, 0..2usize).prop_map(|(n, i, r, who)| Op::Universe {
            agent: [AGENT, "agent-b"][who],
            change: "removed",
            reason: ["thesis_invalidated", "lineage_retired", "operator_halt"][r],
            thesis: format!("th{n}"),
            instrument: INSTRUMENTS[i],
        }),
        2 => (0..7_200i64).prop_map(|s| Op::Tick(T0 + s)),
    ]
}

/// Random journals of up to 24 ops, every thesis id unique.
fn journal_ops() -> impl Strategy<Value = Vec<Op>> {
    prop::collection::vec(any::<u8>(), 0..24).prop_flat_map(|seeds| {
        let mut strategy: BoxedStrategy<Vec<Op>> = Just(Vec::new()).boxed();
        for _ in seeds {
            strategy = strategy
                .prop_flat_map(|ops| {
                    let count = ops
                        .iter()
                        .filter(|o| matches!(o, Op::Thesis { refused: None, .. }))
                        .count();
                    op(count).prop_map(move |next| {
                        let mut ops = ops.clone();
                        ops.push(next);
                        ops
                    })
                })
                .boxed();
        }
        strategy
    })
}

/// Folds `ops` event by event, checking the fold against the oracle at every tick and at the
/// end, and returns the first disagreement.
fn disagreement(ops: &[Op], fed: &[Op]) -> Option<String> {
    let mut state = RuntimeState::new(common::deployment());
    let feed = events(fed);
    let mut fed_events = feed.iter();
    let mut at = T0;
    for (index, one) in fed.iter().enumerate() {
        if let Op::Tick(t) = one {
            at = at.max(*t);
        } else if let Some(next) = fed_events.next() {
            fold(&mut state, next).unwrap_or_else(|e| panic!("{e}"));
        }
        let got: BTreeSet<String> = state
            .ended_theses(clock(at))
            .iter()
            .map(|i| i.as_str().to_owned())
            .collect();
        let prefix = &ops[..(index + 1).min(ops.len())];
        let want = oracle(prefix, at);
        if got != want {
            return Some(format!(
                "after op {index} at {at}: fold {got:?}, oracle {want:?}"
            ));
        }
    }
    None
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn fuzz(ops in journal_ops()) {
        prop_assert_eq!(disagreement(&ops, &ops), None);
        let mut replayed = RuntimeState::new(common::deployment());
        for one in events(&ops) {
            fold(&mut replayed, &one).unwrap_or_else(|e| panic!("{e}"));
        }
        prop_assert_eq!(&replayed, &folded(&ops), "a replay folds the same state");
    }
}

#[test]
fn the_oracle_catches_seeded_breaches() {
    let base = |end: Op| {
        vec![
            admitted("th1", "lin", A, T0 + 600),
            admitted("th2", "lin2", B, T0 + 86_400),
            end,
            Op::Tick(T0 + 1),
        ]
    };
    let breaches = [
        (
            base(invalidated("th1")),
            "an invalidation the fold never saw",
        ),
        (
            base(refused("ref", "lin", C, "lineage_retired")),
            "a retirement the fold never saw",
        ),
        (
            base(removed("thesis_invalidated", "th2", B)),
            "a removal the fold never saw",
        ),
    ];
    for (ops, what) in breaches {
        assert_eq!(
            disagreement(&ops, &ops),
            None,
            "{what}: the honest fold agrees"
        );
        let mut dropped = ops.clone();
        dropped[2] = invalidated("unknown");
        assert!(
            disagreement(&ops, &dropped).is_some(),
            "{what}: the oracle sees the breach"
        );
    }
    let horizon = [admitted("th1", "lin", A, T0 + 600), Op::Tick(T0 + 600)];
    let later = [admitted("th1", "lin", A, T0 + 601), Op::Tick(T0 + 600)];
    assert_eq!(disagreement(&horizon, &horizon), None);
    assert!(
        disagreement(&horizon, &later).is_some(),
        "a horizon moved one second: the oracle sees it"
    );
}

#[test]
fn a_working_exit_counts_until_the_account_stream_ends_it() {
    let held = BTreeSet::from([InstrumentId::new(A).unwrap_or_else(|e| panic!("{e}"))]);
    let plan = ThesisExitPlan {
        held,
        seen: Default::default(),
    };
    let (ids, view) = (TestIds, universe(&[]));
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let mut shell = Shell::new(1);
    booted(&mut shell, &ports);
    assert!(shell.state.working_exits().is_empty());
    journal(
        &mut shell,
        "ThesisProposed",
        thesis_record("th1", "th1", A, T0 + 86_400, None),
    );
    journal(
        &mut shell,
        "ThesisInvalidated",
        object(&[("thesis_id", text("th1"))]),
    );
    let ran = shell.run(Input::Tick(clock(T0 + 1)), &ports);
    let intent = ran
        .drafts
        .iter()
        .find(|d| d.event_type == "IntentProposed")
        .map(|d| d.event_id.clone())
        .unwrap_or_else(|| panic!("proposed: {:?}", ran.draft_types()));
    let working = shell.state.working_exits();
    assert_eq!(
        working
            .get(&InstrumentId::new(A).unwrap_or_else(|e| panic!("{e}")))
            .map(ToString::to_string),
        Some("10".to_owned())
    );
    assert_eq!(working.len(), 1);
    shell.run(Input::Tick(clock(T0 + 2)), &ports);
    assert_eq!(
        plan.seen.borrow().len(),
        2,
        "the plan was asked again, and handed the working exit"
    );
    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            2,
            "OrderAbandoned",
            with_clock(&[("intent_id", text(&intent.0))], T0 + 3),
        ))
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(
        shell.state.working_exits().is_empty(),
        "an ended exit no longer counts"
    );
}

/// Rule 3: a risk clock the ending rules cannot compare (before the epoch) ends every thesis the
/// fold holds rather than none, since a sell adds no risk and a silent hold might.
#[test]
fn a_clock_the_ending_rules_cannot_read_ends_every_thesis() {
    let ops = [
        admitted("th1", "th1", A, T0 + 86_400),
        admitted("th2", "th2", B, T0 + 86_400),
    ];
    assert_eq!(ended(&ops, T0), set(&[]));
    assert_eq!(ended(&ops, -1), set(&[A, B]));
}

/// The sells the runtime proposed and the account stream has not ended, summed per instrument; a
/// buy is never one.
#[test]
fn working_exits_sum_the_live_sells_per_instrument_and_count_no_buy() {
    let mut state = RuntimeState::new(common::deployment());
    let intent = |seq: u64, instrument: &str, side: &str, qty: &str, purpose: &str| {
        event(
            AGENT_STREAM,
            seq,
            "IntentProposed",
            object(&[
                ("instrument_id", text(instrument)),
                ("side", text(side)),
                ("type", text("limit")),
                ("tif", text("day")),
                ("qty", text(qty)),
                ("limit_price", text("99.5")),
                ("purpose", text(purpose)),
            ]),
        )
    };
    for one in [
        intent(1, A, "sell", "3", "discretionary_exit"),
        intent(2, A, "sell", "4", "discretionary_exit"),
        intent(3, A, "buy", "9", "open"),
        intent(4, B, "buy", "2", "open"),
    ] {
        fold(&mut state, &one).unwrap_or_else(|e| panic!("{e}"));
    }
    let working: BTreeMap<String, String> = state
        .working_exits()
        .iter()
        .map(|(i, q)| (i.as_str().to_owned(), q.to_string()))
        .collect();
    assert_eq!(
        working,
        BTreeMap::from([(A.to_owned(), "7".to_owned())]),
        "two sells in A sum; neither buy counts"
    );
    fold(
        &mut state,
        &event(
            ACCOUNT_STREAM,
            1,
            "OrderAbandoned",
            with_clock(&[("intent_id", text(&format!("{AGENT_STREAM}-1")))], T0),
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        state
            .working_exits()
            .values()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        vec!["4".to_owned()],
        "an ended sell no longer counts"
    );
}

/// Each seeded fold bug, emulated by handing the fold the journal that bug would read: the oracle
/// reads the true journal and must disagree. A fold that ignores the agent id, the lineage a
/// retirement names, the removal's reason, or which thesis is current each ends a thesis that has
/// not ended.
#[test]
fn the_oracle_catches_each_seeded_fold_bug() {
    let base = || {
        vec![
            admitted("th1", "lin", A, T0 + 86_400),
            admitted("th2", "lin2", B, T0 + 86_400),
        ]
    };
    let foreign = Op::Universe {
        agent: "agent-b",
        change: "removed",
        reason: "thesis_invalidated",
        thesis: "th1".to_owned(),
        instrument: A,
    };
    let bugs: [(&str, Op, Op); 3] = [
        (
            "a fold that reads another agent's removal as its own",
            foreign,
            removed("thesis_invalidated", "th1", A),
        ),
        (
            "a fold that retires the current thesis on any lineage's retirement",
            refused("ref", "lin9", C, "lineage_retired"),
            refused("ref", "lin", C, "lineage_retired"),
        ),
        (
            "a fold that ends a thesis on any removal reason",
            removed("operator_halt", "th1", A),
            removed("thesis_invalidated", "th1", A),
        ),
    ];
    for (what, truth, read) in bugs {
        let mut ops = base();
        ops.extend([truth, Op::Tick(T0 + 1)]);
        let mut fed = base();
        fed.extend([read, Op::Tick(T0 + 1)]);
        assert_eq!(
            disagreement(&ops, &ops),
            None,
            "{what}: the honest fold agrees"
        );
        assert!(
            disagreement(&ops, &fed).is_some(),
            "{what}: the oracle sees it"
        );
    }
    let superseded = vec![
        admitted("th1", "lin", A, T0 + 86_400),
        admitted("th2", "lin", A, T0 + 86_400),
        invalidated("th1"),
        Op::Tick(T0 + 1),
    ];
    let mut current = superseded.clone();
    current[2] = invalidated("th2");
    assert_eq!(disagreement(&superseded, &superseded), None);
    assert!(
        disagreement(&superseded, &current).is_some(),
        "a fold that lets an invalidation of a superseded thesis end the current one"
    );
}
