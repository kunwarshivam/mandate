//! DEC-940 item 6, gap 2, closed by DEC-942: a grant releases the asked order with the execution
//! plan the deterministic order builder produced for it, recovered from the journaled request
//! whose content hash the grant repeated, and never a new or re-sized plan (`AGENTS.md` rules 3
//! and 4). Expectations are the test's own: the plan the fixture's builder returned, the oracle's
//! `execution` member built field by field, and [`hash_of`] over the committed content.

mod common;

use common::escalation::{
    ASKED_AT, Answer, Asked, BOUND_LIMIT, asking_shell, hash_of, next_control_seq, reconciliation,
    tail,
};
use common::{
    AllowGate, FixedPlan, Shell, TestIds, clock, fresh_output, key, object, ports, price, qty,
    text, universe,
};
use mandate_accounting::AssetClass;
use mandate_canon::{Object, Value};
use mandate_runtime::{
    Autonomy, EventDraft, FoldedEvent, Input, IntentHandoff, OrderExecution, ProtectionPrices,
    RuntimeError, TimeInForce, fold,
};
use proptest::prelude::*;

fn planned(execution: Option<OrderExecution>) -> FixedPlan {
    let mut plan = FixedPlan::opening(Autonomy::Ask);
    if let Some(proposal) = plan.proposal.as_mut() {
        proposal.execution = execution;
    }
    plan
}

fn bracket() -> OrderExecution {
    OrderExecution {
        asset_class: AssetClass::UsEquity,
        tif: TimeInForce::Gtc,
        protection_required: true,
        protection: Some(ProtectionPrices {
            stop: price("147.25"),
            take_profit: Some(price("170.5")),
        }),
    }
}

/// The oracle's `execution` member for `plan` (DEC-942 item 2), written without the runtime or
/// `mandate-approval`.
fn expected(plan: &OrderExecution) -> Value {
    let class = match plan.asset_class {
        AssetClass::UsEquity => "us_equity",
        AssetClass::Crypto => "crypto",
    };
    let protection = plan.protection.map_or(Value::Null, |p| {
        object(&[
            ("stop", text(&p.stop.to_string())),
            (
                "take_profit",
                p.take_profit
                    .map_or(Value::Null, |tp| text(&tp.to_string())),
            ),
        ])
    });
    object(&[
        ("asset_class", text(class)),
        ("tif", text(tif_name(plan.tif))),
        ("protection_required", Value::Bool(plan.protection_required)),
        ("protection", protection),
    ])
}

fn tif_name(tif: TimeInForce) -> &'static str {
    match tif {
        TimeInForce::Day => "day",
        TimeInForce::Gtc => "gtc",
        TimeInForce::Ioc => "ioc",
    }
}

fn content(request: &EventDraft) -> &Value {
    request
        .payload
        .get("content")
        .expect("the request carries its content object")
}

fn granted(
    shell: &mut Shell,
    asked: &Asked,
    ports: &mandate_runtime::Ports<'_>,
) -> Vec<IntentHandoff> {
    let grant = Answer::grant(next_control_seq(shell), asked, ASKED_AT + 30).event();
    tail(shell, &grant, ports).handed
}

fn result_of(ran: &common::Ran, event_type: &str) -> Option<String> {
    ran.drafts
        .iter()
        .find(|d| d.event_type == event_type)
        .and_then(|d| d.payload.get("result"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// DEC-942 items 1 and 3: the plan the builder produced at the ask is the content's `execution`
/// member, the hash covers it, and the grant hands exactly that plan to the sink.
#[test]
fn a_granted_ask_hands_the_plan_its_request_bound() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        planned(Some(bracket())),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    assert_eq!(
        content(&asked.request).get("execution"),
        Some(&expected(&bracket())),
        "the request binds the builder's plan"
    );
    assert_eq!(asked.content_hash, hash_of(content(&asked.request)));
    let handed = granted(&mut shell, &asked, &ports);
    assert_eq!(handed.len(), 1, "the grant acts once");
    assert_eq!(
        handed[0].execution,
        Some(bracket()),
        "the asked plan, unchanged"
    );
}

/// Rule 4: re-validation never re-plans. A builder that would now produce another plan (a moved
/// stop, another time in force) changes nothing the grant hands.
#[test]
fn a_grant_hands_the_journaled_plan_and_never_a_new_one() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        planned(Some(bracket())),
        universe(&["AAPL"]),
    );
    let asking = ports(&ids, &gate, &plan, &view);
    let (mut shell, asked) = asking_shell(&asking, Some(BOUND_LIMIT));
    let replanned = planned(Some(OrderExecution {
        tif: TimeInForce::Day,
        protection: Some(ProtectionPrices {
            stop: price("140"),
            take_profit: None,
        }),
        ..bracket()
    }));
    let later = ports(&ids, &gate, &replanned, &view);
    let handed = granted(&mut shell, &asked, &later);
    assert_eq!(handed.len(), 1);
    assert_eq!(handed[0].execution, Some(bracket()));
}

/// The asked plan survives a restart: a new process folds it back from the journal alone.
#[test]
fn a_restarted_runtime_hands_the_plan_from_the_journal() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        planned(Some(bracket())),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let (mut restarted, _) = shell.restart(&ports);
    let handed = granted(&mut restarted, &asked, &ports);
    assert_eq!(handed.len(), 1);
    assert_eq!(handed[0].execution, Some(bracket()));
}

/// No plan, no member: the content keeps version 1's shape and the grant hands no plan, which the
/// executor's sink refuses (DEC-940 item 6's pin, unchanged for a builder with no plan).
#[test]
fn an_ask_without_a_plan_binds_none_and_hands_none() {
    let (ids, gate, plan, view) = (TestIds, AllowGate, planned(None), universe(&["AAPL"]));
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    assert_eq!(content(&asked.request).get("execution"), None);
    let handed = granted(&mut shell, &asked, &ports);
    assert_eq!(handed.len(), 1);
    assert_eq!(handed[0].execution, None);
}

/// Rule 3: a plan that names another asset class than the order is ambiguous, so it is not asked
/// and nothing can be granted from it; the same proposal with a matching plan is asked.
#[test]
fn a_plan_for_another_asset_class_is_never_asked() {
    let crypto = OrderExecution {
        asset_class: AssetClass::Crypto,
        ..bracket()
    };
    for (execution, asks) in [(crypto, false), (bracket(), true)] {
        let (ids, gate, plan, view) = (
            TestIds,
            AllowGate,
            planned(Some(execution)),
            universe(&["AAPL"]),
        );
        let ports = ports(&ids, &gate, &plan, &view);
        let mut shell = Shell::new(1);
        shell
            .fold_one(&reconciliation(1, ASKED_AT))
            .expect("reconciliation");
        let (mut shell, _) = shell.restart(&ports);
        shell.run(Input::ModelOutput(fresh_output(ASKED_AT)), &ports);
        let batch = shell.run(Input::Tick(clock(ASKED_AT)), &ports);
        let types = batch.draft_types();
        assert!(types.contains(&"DecisionMade"), "{types:?}");
        assert_eq!(types.contains(&"ApprovalRequested"), asks, "{types:?}");
    }
}

/// The asked shell's journal with its request's `content` changed by `change`, and its
/// `content_hash` restated as the hash of the changed content when `restate` holds.
fn rewritten(shell: &Shell, change: impl Fn(&mut Object), restate: bool) -> Vec<FoldedEvent> {
    shell
        .agent_journal
        .iter()
        .map(|event| {
            if event.event_type != "ApprovalRequested" {
                return event.clone();
            }
            let mut members = event.payload.as_object().cloned().unwrap_or_default();
            let mut content = members
                .get(&key("content"))
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            change(&mut content);
            let content = Value::Object(content);
            if restate {
                members.insert(key("content_hash"), text(&hash_of(&content)));
            }
            members.insert(key("content"), content);
            FoldedEvent {
                payload: Value::Object(members),
                ..event.clone()
            }
        })
        .collect()
}

/// Replays `journal` with `shell`'s followed streams into a new process, as a restart folds it,
/// returning the first refusal.
fn replay(shell: &Shell, journal: &[FoldedEvent]) -> Result<Shell, RuntimeError> {
    let mut next = Shell::new(shell.epoch.0.saturating_add(1));
    next.followed = shell.followed.clone();
    next.agent_journal = journal.to_vec();
    for event in journal.iter().chain(shell.followed.iter()) {
        fold(&mut next.state, event)?;
    }
    Ok(next)
}

/// One rewrite of a request's content.
type Change = Box<dyn Fn(&mut Object)>;

fn with_stop(stop: &str) -> impl Fn(&mut Object) + '_ {
    move |content: &mut Object| {
        let mut execution = content
            .get(&key("execution"))
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        execution.insert(
            key("protection"),
            object(&[("stop", text(stop)), ("take_profit", text("170.5"))]),
        );
        content.insert(key("execution"), Value::Object(execution));
    }
}

fn asked_with_a_bracket() -> (Shell, Asked) {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        planned(Some(bracket())),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    asking_shell(&ports, Some(BOUND_LIMIT))
}

/// The unchanged journal replays: what the refusals below refuse is the change alone.
#[test]
fn the_asked_journal_replays_unchanged() {
    let (shell, _) = asked_with_a_bracket();
    let journal = rewritten(&shell, |_| {}, false);
    assert!(replay(&shell, &journal).is_ok());
    let restated = rewritten(&shell, |_| {}, true);
    assert!(replay(&shell, &restated).is_ok());
}

/// A plan changed under the hash the request stated no longer matches its hash: the fold refuses
/// the request rather than recover a plan the grant never approved.
#[test]
fn a_plan_changed_under_its_old_hash_refuses() {
    let (shell, _) = asked_with_a_bracket();
    let journal = rewritten(&shell, with_stop("140"), false);
    assert!(replay(&shell, &journal).is_err());
}

/// A hash that is not the hash of the content refuses, even with the plan untouched.
#[test]
fn a_changed_hash_refuses() {
    let (shell, _) = asked_with_a_bracket();
    let journal: Vec<FoldedEvent> = shell
        .agent_journal
        .iter()
        .map(|event| {
            if event.event_type != "ApprovalRequested" {
                return event.clone();
            }
            let mut members = event.payload.as_object().cloned().unwrap_or_default();
            members.insert(
                key("content_hash"),
                text(&format!("sha256:{}", "a".repeat(64))),
            );
            FoldedEvent {
                payload: Value::Object(members),
                ..event.clone()
            }
        })
        .collect();
    assert!(replay(&shell, &journal).is_err());
}

/// A plan restated under a new hash is a new request's content: the owner's grant repeats the hash
/// they were shown, so admission refuses it (check 5) and nothing is handed.
#[test]
fn a_plan_restated_under_a_new_hash_refuses_the_grant_of_the_old_one() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        planned(Some(bracket())),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let journal = rewritten(&shell, with_stop("140"), true);
    let mut replayed = replay(&shell, &journal).expect("a restated request is well formed");
    let epoch = replayed.epoch;
    replayed.run(Input::Started(epoch), &ports);
    let grant = Answer::grant(next_control_seq(&replayed), &asked, ASKED_AT + 30).event();
    let ran = tail(&mut replayed, &grant, &ports);
    assert!(ran.handed.is_empty(), "nothing is handed");
    assert_eq!(
        result_of(&ran, "ApprovalResponded").as_deref(),
        Some("refused")
    );
}

/// Byte for byte: a plan member in a spelling the builder never writes (a trailing zero, a quantity
/// where a price belongs, an unknown time in force, a missing member) refuses, even under a
/// restated hash.
#[test]
fn a_plan_not_written_as_the_builder_writes_it_refuses() {
    let (shell, _) = asked_with_a_bracket();
    let changes: Vec<Change> = vec![
        Box::new(with_stop("147.250")),
        Box::new(|content: &mut Object| {
            let mut execution = content
                .get(&key("execution"))
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            execution.insert(key("tif"), text("fok"));
            content.insert(key("execution"), Value::Object(execution));
        }),
        Box::new(|content: &mut Object| {
            let mut execution = content
                .get(&key("execution"))
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            execution.remove(&key("protection_required"));
            content.insert(key("execution"), Value::Object(execution));
        }),
        Box::new(|content: &mut Object| {
            let mut execution = content
                .get(&key("execution"))
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            execution.insert(key("protection_required"), text("true"));
            content.insert(key("execution"), Value::Object(execution));
        }),
        Box::new(|content: &mut Object| {
            let mut execution = content
                .get(&key("execution"))
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            execution.insert(key("extra"), Value::Null);
            content.insert(key("execution"), Value::Object(execution));
        }),
        Box::new(|content: &mut Object| {
            content.insert(key("execution"), text("gtc"));
        }),
    ];
    for (n, change) in changes.iter().enumerate() {
        let journal = rewritten(&shell, change, true);
        assert!(replay(&shell, &journal).is_err(), "change {n} refuses");
    }
}

/// A plan whose asset class is not the order's refuses on replay, even under a restated hash.
#[test]
fn a_plan_for_another_asset_class_than_the_order_refuses() {
    let (shell, _) = asked_with_a_bracket();
    let journal = rewritten(
        &shell,
        |content: &mut Object| {
            let mut execution = content
                .get(&key("execution"))
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            execution.insert(key("asset_class"), text("crypto"));
            content.insert(key("execution"), Value::Object(execution));
        },
        true,
    );
    assert!(replay(&shell, &journal).is_err());
}

fn cents() -> impl Strategy<Value = String> {
    (1u64..1_000_000).prop_map(|c| {
        let written = format!("{}.{:02}", c / 100, c % 100);
        written
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
    })
}

fn plan_strategy() -> impl Strategy<Value = (String, String, OrderExecution)> {
    (
        (1u64..500).prop_map(|n| n.to_string()),
        cents(),
        prop_oneof![
            Just(TimeInForce::Day),
            Just(TimeInForce::Gtc),
            Just(TimeInForce::Ioc)
        ],
        any::<bool>(),
        proptest::option::of((cents(), proptest::option::of(cents()))),
        prop_oneof![Just(AssetClass::UsEquity), Just(AssetClass::Crypto)],
    )
        .prop_map(
            |(q, limit, tif, protection_required, protection, asset_class)| {
                (
                    q,
                    limit,
                    OrderExecution {
                        asset_class,
                        tif,
                        protection_required,
                        protection: protection.map(|(stop, take_profit)| ProtectionPrices {
                            stop: price(&stop),
                            take_profit: take_profit.as_deref().map(price),
                        }),
                    },
                )
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 48,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    /// Over proposals of every size, limit, asset class, time in force and protection: the request binds the
    /// builder's plan as the oracle writes it under the hash of its own content, and the grant hands
    /// that plan with the bound size and limit, its `IntentProposed` naming the plan's time in force.
    #[test]
    fn every_granted_proposal_hands_its_own_plan((q, limit, execution) in plan_strategy()) {
        let mut plan = planned(Some(execution));
        if let Some(proposal) = plan.proposal.as_mut() {
            proposal.qty = qty(&q);
            proposal.limit = price(&limit);
            proposal.asset_class = execution.asset_class;
        }
        let (ids, gate, view) = (TestIds, AllowGate, universe(&["AAPL"]));
        let ports = ports(&ids, &gate, &plan, &view);
        let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
        prop_assert_eq!(content(&asked.request).get("execution"), Some(&expected(&execution)));
        prop_assert_eq!(&asked.content_hash, &hash_of(content(&asked.request)));
        let grant = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 30).event();
        let ran = tail(&mut shell, &grant, &ports);
        prop_assert_eq!(ran.handed.len(), 1);
        prop_assert_eq!(ran.handed[0].execution, Some(execution));
        let mandate_runtime::IntentBody::Order { qty: handed_qty, limit: handed_limit, .. } =
            &ran.handed[0].body
        else {
            return Err(TestCaseError::fail("the grant hands an order"));
        };
        prop_assert_eq!(*handed_qty, qty(&q));
        prop_assert_eq!(*handed_limit, Some(price(&limit)));
        let intent = ran.drafts.iter().find(|d| d.event_type == "IntentProposed");
        prop_assert_eq!(
            intent.and_then(|d| d.payload.get("tif")).and_then(Value::as_str),
            Some(tif_name(execution.tif))
        );
    }
}
