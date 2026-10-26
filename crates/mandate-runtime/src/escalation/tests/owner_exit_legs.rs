//! DEC-2387: an owner's exit of one instrument is proposed as one `IntentProposed` per sell of its
//! plan, caused by the `OwnerExitRequested` copy, so the cell's executor, which reads only the
//! journal, sells it; and the same legs are what one process hands, retries and re-hands. The
//! oracle for every case is the planner's own answer, recorded as the planner gave it, never the
//! runtime's reading of it.

use proptest::prelude::*;

use super::*;
use crate::types::{OrderExecution, TimeInForce};

/// Sells exactly the legs it holds in the instrument it is asked for, cancels nothing, and keeps
/// every plan it gave as the oracle.
struct Holding {
    legs: Vec<FlattenLeg>,
    given: RefCell<Vec<FlattenPlan>>,
}

impl Holding {
    fn of(legs: Vec<FlattenLeg>) -> Self {
        Self {
            legs,
            given: RefCell::new(Vec::new()),
        }
    }

    fn last(&self) -> Result<FlattenPlan, String> {
        self.given
            .borrow()
            .last()
            .cloned()
            .ok_or_else(|| "the planner was never asked".to_owned())
    }
}

impl FlattenPlanner for Holding {
    fn plan(&self, request: &FlattenRequest) -> FlattenPlan {
        let plan = FlattenPlan {
            cancel_client_order_ids: request.working_orders.clone(),
            sells: self
                .legs
                .iter()
                .filter(|leg| request.instrument.as_ref() == Some(&leg.instrument))
                .cloned()
                .collect(),
            purpose: request.initiator.sell_purpose(),
            confirmation: request.confirmation.clone(),
        };
        self.given.borrow_mut().push(plan.clone());
        plan
    }
}

fn leg(name: &str, qty: &str, class: AssetClass) -> Result<FlattenLeg, String> {
    Ok(FlattenLeg {
        instrument: instrument(name)?,
        asset_class: class,
        qty: Qty::parse(qty).map_err(failed)?,
        deferred_to_regular_session: false,
    })
}

fn ports<'p>(view: &'p MandateView, plan: &'p Plan, flatten: &'p Holding) -> Ports<'p> {
    Ports {
        ids: &Ids,
        gate: &Allow,
        plan,
        flatten,
        view,
    }
}

fn exit_of(name: &str, step_up: Value, bid: bool) -> Result<Value, String> {
    command("a", "owner_exit", ("instrument", name), step_up, bid)
}

/// The time in force trading spec §5.1 gives an owner's market sell: the day for an equity, good
/// till cancelled for crypto, which takes no `day`.
fn expected_tif(leg: &FlattenLeg) -> (TimeInForce, &'static str) {
    match leg.asset_class {
        AssetClass::UsEquity => (TimeInForce::Day, "day"),
        AssetClass::Crypto => (TimeInForce::Gtc, "gtc"),
    }
}

/// The payload journal spec §9.1 gives an owner's market sell of `leg`, written out member by
/// member rather than through the runtime's writer.
fn expected_payload(leg: &FlattenLeg) -> Result<Value, String> {
    object(vec![
        ("instrument_id", text(leg.instrument.as_str())),
        ("limit_price", Value::Null),
        ("purpose", text("owner_exit")),
        ("qty", text(&leg.qty.to_string())),
        ("side", text("sell")),
        ("tif", text(expected_tif(leg).1)),
        ("type", text("market")),
    ])
}

/// Every check DEC-2387 items 1 and 3 make of one owner exit's effects against the plan the
/// planner gave: the copy first, then exactly one market sell per planned sell, in the plan's
/// order, each caused by the copy and accepted by the journal, each handed right after its own
/// draft as an order with no limit, and no flatten handed at all.
fn check_legs(effects: &[Effect], plan: &FlattenPlan) -> Checked {
    let written = drafts(effects);
    let copy = written
        .first()
        .filter(|draft| draft.event_type == "OwnerExitRequested")
        .ok_or_else(|| format!("the copy comes first: {:?}", types(effects)))?;
    let legs: Vec<&&EventDraft> = written
        .iter()
        .filter(|draft| draft.event_type == "IntentProposed")
        .collect();
    if written.len() != legs.len().saturating_add(1) {
        return Err(format!("only the copy and its legs: {:?}", types(effects)));
    }
    if legs.len() != plan.sells.len() {
        return Err(format!(
            "{} legs for {} sells",
            legs.len(),
            plan.sells.len()
        ));
    }
    for (draft, sell) in legs.iter().zip(&plan.sells) {
        if draft.causation_id.as_ref() != Some(&copy.event_id) {
            return Err(format!("{draft:?} is caused by the copy"));
        }
        if draft.payload != expected_payload(sell)? {
            return Err(format!("{draft:?} records {sell:?}"));
        }
        journal_accepts(draft, &view()?.version)?;
    }
    journal_accepts(copy, &view()?.version)?;
    let mut drafted: Vec<&EventId> = Vec::new();
    let mut handed_ids = Vec::new();
    for effect in effects {
        match effect {
            Effect::Journal(draft) => drafted.push(&draft.event_id),
            Effect::Intent(handoff) => {
                if drafted.last() != Some(&&handoff.intent_id) {
                    return Err(format!("{handoff:?} follows its own draft"));
                }
                handed_ids.push(handoff.intent_id.clone());
            }
            Effect::Timer(_) | Effect::Notify(_) | Effect::NotifyApproval(_) => {}
        }
    }
    let leg_ids: Vec<EventId> = legs.iter().map(|draft| draft.event_id.clone()).collect();
    if handed_ids != leg_ids {
        return Err(format!("handed {handed_ids:?}, proposed {leg_ids:?}"));
    }
    for (handoff, sell) in handed(effects).into_iter().zip(&plan.sells) {
        let want = IntentHandoff {
            intent_id: handoff.intent_id.clone(),
            body: IntentBody::Order {
                instrument: sell.instrument.clone(),
                side: Side::Sell,
                qty: sell.qty,
                limit: None,
                purpose: Purpose::OwnerExit,
            },
            execution: Some(OrderExecution {
                asset_class: sell.asset_class,
                tif: expected_tif(sell).0,
                protection_required: false,
                protection: None,
            }),
        };
        if *handoff != want {
            return Err(format!("{handoff:?} is {want:?}"));
        }
    }
    Ok(())
}

#[test]
fn an_owner_exit_proposes_each_sell_of_its_plan_at_the_market_caused_by_its_copy() -> Checked {
    let (view, plan) = (view()?, Plan::of(Vec::new(), Autonomy::Auto));
    let flatten = Holding::of(vec![
        leg("AAPL", "10", AssetClass::UsEquity)?,
        leg("MSFT", "3", AssetClass::UsEquity)?,
    ]);
    let ports = ports(&view, &plan, &flatten);
    let (mut rig, _) = Rig::started(&ports)?;
    let (_, effects) = rig.control(COMMAND_ISSUED, exit_of("AAPL", Value::Null, false)?, &ports)?;
    let given = flatten.last()?;
    assert_eq!(given.sells.len(), 1, "only the named instrument is planned");
    check_legs(&effects, &given)?;
    assert_eq!(
        member(the(&effects, "IntentProposed")?, "qty"),
        Some("10"),
        "exactly the sub-ledger the plan sells"
    );
    Ok(())
}

#[test]
fn a_crypto_leg_carries_its_asset_class_and_a_stepped_up_bid_changes_no_leg() -> Checked {
    let (view, plan) = (view()?, Plan::of(Vec::new(), Autonomy::Auto));
    let flatten = Holding::of(vec![leg("BTCUSD", "0.25", AssetClass::Crypto)?]);
    let ports = ports(&view, &plan, &flatten);
    let (mut rig, _) = Rig::started(&ports)?;
    let (_, effects) = rig.control(
        COMMAND_ISSUED,
        exit_of("BTCUSD", evidence("s1", AT)?, true)?,
        &ports,
    )?;
    let given = flatten.last()?;
    assert!(given.confirmation.is_some(), "the planner had the bid");
    check_legs(&effects, &given)?;
    assert_eq!(
        member(the(&effects, "OwnerExitRequested")?, "step_up_status"),
        Some("valid")
    );
    Ok(())
}

#[test]
fn an_owner_exit_of_nothing_held_journals_its_copy_alone_and_hands_nothing() -> Checked {
    let (view, plan) = (view()?, Plan::of(Vec::new(), Autonomy::Auto));
    let flatten = Holding::of(vec![leg("MSFT", "3", AssetClass::UsEquity)?]);
    let ports = ports(&view, &plan, &flatten);
    let (mut rig, _) = Rig::started(&ports)?;
    let (_, effects) = rig.control(COMMAND_ISSUED, exit_of("AAPL", Value::Null, false)?, &ports)?;
    assert_eq!(types(&effects), ["OwnerExitRequested"]);
    assert!(handed(&effects).is_empty(), "{effects:?}");
    Ok(())
}

/// Journal spec §5.1, rule 13: a retry re-emits the copy, the legs and their handoffs; a restart
/// re-hands each leg the account stream has not taken, as an order and never as a flatten, and
/// stops once an `IntentReceived` names it.
#[test]
fn an_owner_exits_legs_are_handed_again_on_a_retry_and_a_restart_until_taken() -> Checked {
    let (view, plan) = (view()?, Plan::of(Vec::new(), Autonomy::Auto));
    let flatten = Holding::of(vec![leg("AAPL", "10", AssetClass::UsEquity)?]);
    let ports = ports(&view, &plan, &flatten);
    let (mut rig, _) = Rig::started(&ports)?;
    let exit = rig.external(
        CONTROL_STREAM,
        COMMAND_ISSUED,
        exit_of("AAPL", Value::Null, false)?,
        format!("1{:025}", 1),
    )?;
    let first = handle(&mut rig.state, Input::Journal(exit.clone()), &ports).map_err(failed)?;
    let retry = handle(&mut rig.state, Input::Journal(exit), &ports).map_err(failed)?;
    check_legs(&first, &flatten.last()?)?;
    assert_eq!(retry.len(), first.len(), "{retry:?}");
    assert_eq!(drafts(&retry), drafts(&first), "the same drafts");
    let leg_id = the(&first, "IntentProposed")?.event_id.clone();
    assert!(
        matches!(
            handed(&retry).as_slice(),
            [one] if one.intent_id == leg_id
                && matches!(one.body, IntentBody::Order { limit: None, purpose: Purpose::OwnerExit, .. })
        ),
        "{retry:?}"
    );
    for effect in &retry {
        if let Effect::Journal(draft) = effect {
            let event = FoldedEvent {
                stream: AGENT_STREAM.to_owned(),
                seq: Seq(Rig::next(AGENT_STREAM, &rig.agent)),
                event_id: draft.event_id.clone(),
                event_type: draft.event_type.clone(),
                causation_id: draft.causation_id.clone(),
                actor: mandate_approval::ActorKind::Agent,
                payload: draft.payload.clone(),
            };
            fold(&mut rig.state, &event).map_err(failed)?;
            rig.agent.push(event);
        }
    }
    let (mut restarted, recovered) = rig.restarted(&ports)?;
    let again = handed(&recovered);
    let sold = IntentBody::Order {
        instrument: instrument("AAPL")?,
        side: Side::Sell,
        qty: Qty::parse("10").map_err(failed)?,
        limit: None,
        purpose: Purpose::OwnerExit,
    };
    assert!(
        again
            .iter()
            .all(|h| !matches!(h.body, IntentBody::Flatten(_))),
        "no flatten for an owner exit: {recovered:?}"
    );
    assert!(
        again
            .iter()
            .any(|h| h.intent_id == leg_id && h.body == sold),
        "an untaken leg is handed again: {recovered:?}"
    );
    restarted.account("IntentReceived", vec![("intent_id", text(&leg_id.0))])?;
    let (_, recovered) = restarted.restarted(&ports)?;
    assert!(
        handed(&recovered).iter().all(|h| h.intent_id != leg_id),
        "a taken one is not: {recovered:?}"
    );
    Ok(())
}

/// Journal spec §5.1, rule 13: an owner's kill switch with a confirmed bid and valid step-up whose
/// append went unanswered is retried with the same drafts, and its flatten is handed again with
/// the same confirmation.
#[test]
fn a_retried_kill_switch_hands_its_flatten_with_the_bid_its_copy_confirmed() -> Checked {
    let (view, plan) = (view()?, Plan::of(Vec::new(), Autonomy::Auto));
    let flatten = Holding::of(vec![leg("AAPL", "10", AssetClass::UsEquity)?]);
    let ports = ports(&view, &plan, &flatten);
    let (mut rig, _) = Rig::started(&ports)?;
    let switch = rig.external(
        CONTROL_STREAM,
        COMMAND_ISSUED,
        command(
            "a",
            "kill_switch",
            ("agent", "a"),
            evidence("s1", AT)?,
            true,
        )?,
        format!("1{:025}", 1),
    )?;
    let first = handle(&mut rig.state, Input::Journal(switch.clone()), &ports).map_err(failed)?;
    let retry = handle(&mut rig.state, Input::Journal(switch), &ports).map_err(failed)?;
    let confirmed = |effects: &[Effect]| -> Vec<Option<OwnerConfirmation>> {
        handed(effects)
            .iter()
            .filter_map(|h| match &h.body {
                IntentBody::Flatten(plan) => Some(plan.confirmation.clone()),
                IntentBody::Order { .. } => None,
            })
            .collect()
    };
    let shown = confirmed(&first);
    assert!(
        matches!(shown.as_slice(), [Some(_)]),
        "the live switch carries its bid: {first:?}"
    );
    assert_eq!(confirmed(&retry), shown, "the retry carries the same bid");
    assert_eq!(retry, first);
    Ok(())
}

/// DEC-2387 item 3, DEC-131 item 22: an owner's exit taken while the owner has paused or stopped
/// the agent is re-handed on a restart as the owner's flatten was, in every mode, since the owner
/// asked for it and only the executor's mode gate may hold it (`AGENTS.md` rule 13).
#[test]
fn a_paused_or_stopped_agents_untaken_owner_exit_is_handed_again_on_a_restart() -> Checked {
    for stop in [crate::types::Command::Pause, crate::types::Command::Stop] {
        let (view, plan) = (view()?, Plan::of(Vec::new(), Autonomy::Auto));
        let flatten = Holding::of(vec![leg("AAPL", "10", AssetClass::UsEquity)?]);
        let ports = ports(&view, &plan, &flatten);
        let (mut rig, _) = Rig::started(&ports)?;
        rig.step(Input::Command(stop.clone()), &ports)?;
        let (_, effects) =
            rig.control(COMMAND_ISSUED, exit_of("AAPL", Value::Null, false)?, &ports)?;
        let leg_id = the(&effects, "IntentProposed")?.event_id.clone();
        let (_, recovered) = rig.restarted(&ports)?;
        assert!(
            handed(&recovered).iter().any(|h| h.intent_id == leg_id),
            "{stop:?}: {recovered:?}"
        );
    }
    Ok(())
}

fn folded(id: &str, members: Vec<(&str, Value)>) -> Result<FoldedEvent, String> {
    Ok(FoldedEvent {
        stream: AGENT_STREAM.to_owned(),
        seq: Seq(1),
        event_id: EventId(id.to_owned()),
        event_type: "IntentProposed".to_owned(),
        causation_id: Some(EventId("cause".to_owned())),
        actor: mandate_approval::ActorKind::Agent,
        payload: object(members)?,
    })
}

fn proposed_of(kind: &str, limit: Value, purpose: &str, side: &str) -> Result<FoldedEvent, String> {
    folded(
        "i1",
        vec![
            ("instrument_id", text("AAPL")),
            ("limit_price", limit),
            ("purpose", text(purpose)),
            ("qty", text("5")),
            ("side", text(side)),
            ("tif", text("day")),
            ("type", text(kind)),
        ],
    )
}

/// Journal spec §9.1 rules 2 and 3, as the fold reads them: no limit exactly at the market, and
/// the market only for an exit. Anything else is refused, never read as some other order.
#[test]
fn the_fold_reads_a_market_exit_with_no_limit_and_refuses_every_other_shape() -> Checked {
    for purpose in ["owner_exit", "risk_exit", "discretionary_exit"] {
        let mut state = RuntimeState::new(deployment());
        fold(
            &mut state,
            &proposed_of("market", Value::Null, purpose, "sell")?,
        )
        .map_err(failed)?;
        assert!(
            matches!(
                state.body_of(&EventId("i1".to_owned())),
                Some(IntentBody::Order { limit: None, .. })
            ),
            "{purpose}"
        );
    }
    let mut state = RuntimeState::new(deployment());
    fold(
        &mut state,
        &proposed_of("limit", text("150"), "open", "buy")?,
    )
    .map_err(failed)?;
    assert!(matches!(
        state.body_of(&EventId("i1".to_owned())),
        Some(IntentBody::Order { limit: Some(limit), .. }) if limit.to_string() == "150"
    ));
    for (kind, limit, purpose, side, field) in [
        ("market", Value::Null, "open", "buy", "limit_price"),
        ("market", Value::Null, "increase", "buy", "limit_price"),
        ("market", text("150"), "owner_exit", "sell", "limit_price"),
        ("limit", Value::Null, "owner_exit", "sell", "limit_price"),
        ("stop", Value::Null, "owner_exit", "sell", "type"),
        ("stop", text("150"), "owner_exit", "sell", "type"),
    ] {
        let mut state = RuntimeState::new(deployment());
        let refused = fold(
            &mut state,
            &proposed_of(kind, limit.clone(), purpose, side)?,
        );
        assert_eq!(
            refused,
            Err(crate::error::RuntimeError::NonCanonicalPayload {
                field: field.to_owned()
            }),
            "{kind} {limit:?} {purpose} is refused at {field}"
        );
    }
    Ok(())
}

fn class() -> impl Strategy<Value = AssetClass> {
    prop_oneof![Just(AssetClass::UsEquity), Just(AssetClass::Crypto)]
}

proptest! {
    /// For any held book and any owner exit of one of its instruments, confirmed or not: the
    /// legs are exactly the planner's sells of that instrument, nothing else is handed, and a
    /// retry of the same input re-emits them unchanged.
    #[test]
    fn every_planned_sell_is_proposed_and_handed_once(
        held in proptest::collection::vec(
            (prop_oneof![Just("AAPL"), Just("MSFT"), Just("BTCUSD")], 1u32..100_000, class()),
            0..5,
        ),
        named in prop_oneof![Just("AAPL"), Just("MSFT"), Just("BTCUSD")],
        bid in any::<bool>(),
        stepped in any::<bool>(),
    ) {
        let check = || -> Checked {
            let legs = held
                .iter()
                .map(|(name, qty, class)| leg(name, &format!("{qty}"), *class))
                .collect::<Result<Vec<_>, _>>()?;
            let expected = legs.iter().filter(|leg| leg.instrument.as_str() == named).count();
            let (view, plan) = (view()?, Plan::of(Vec::new(), Autonomy::Auto));
            let flatten = Holding::of(legs);
            let ports = ports(&view, &plan, &flatten);
            let (mut rig, _) = Rig::started(&ports)?;
            let step_up = if stepped { evidence("s1", AT)? } else { Value::Null };
            let exit = rig.external(
                CONTROL_STREAM,
                COMMAND_ISSUED,
                exit_of(named, step_up, bid)?,
                format!("1{:025}", 1),
            )?;
            let first =
                handle(&mut rig.state, Input::Journal(exit.clone()), &ports).map_err(failed)?;
            let given = flatten.last()?;
            if given.sells.len() != expected {
                return Err(format!("the planner sold {} of {expected}", given.sells.len()));
            }
            check_legs(&first, &given)?;
            let retry = handle(&mut rig.state, Input::Journal(exit), &ports).map_err(failed)?;
            if drafts(&retry) != drafts(&first) {
                return Err("a retry re-emits the same drafts".to_owned());
            }
            let ids = |effects: &[Effect]| -> Vec<EventId> {
                handed(effects).iter().map(|h| h.intent_id.clone()).collect()
            };
            if ids(&retry) != ids(&first) {
                return Err("a retry hands the same legs".to_owned());
            }
            Ok(())
        };
        prop_assert_eq!(check(), Ok(()));
    }
}
