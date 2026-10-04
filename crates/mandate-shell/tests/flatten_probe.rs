//! The flatten probe's flip, tests first (DEC-77 stage 1; E7-7 stream L, claim #171): what
//! `RiskExitPath` must do once it binds `mandate_risk::agent_flatten` over the journal it is
//! handed (DEC-449).
//!
//! Every test here is pending on the flip: the adapter still refuses with its own
//! `Cause::Unimplemented` report, and each test fails by that refusal propagating (DEC-110,
//! DEC-137). The implementation PR deletes the markers and changes nothing else here. The
//! fail-closed suite keeps its own pin of today's refusal
//! (`the_production_exit_probes_answer_unimplemented`).
//!
//! The journal is a scripted double returning §9.5-shaped records, mirroring the vectors' own
//! bodies: a bought position of ten, a resting protective sell of four, and a broker position of
//! fifteen — the shape §5.5 exists for (MC-F01's, at the shell). The oracle is the risk crate
//! itself: `plan` must equal `agent_flatten`'s plan for the state those records fold to, mapped
//! into the runtime's types by the mapping written once below, so the adapter invents nothing
//! (DEC-166 item 2). The numeric client order id the risk crate keys working orders by is the
//! executor's own public derivation from the intent (DEC-449 item 3's mapping).

use std::collections::BTreeMap;

use mandate_accounting::InstrumentId;
use mandate_canon::{Int, Key, Object, Value};
use mandate_journal::{AppendOutcome, StoredEvent};
use mandate_num::{Fraction, Qty, Usd};
use mandate_risk::{
    AgentPosition, AssetId, ClientOrderId, FlattenInitiator, FlattenInput, FlattenPlan, Side,
    WorkingOrder, agent_flatten,
};
use mandate_runtime::{FlattenLeg, FlattenPlan as RuntimePlan, FlattenRequest, Initiator, Purpose};
use mandate_shell::adapters::RiskExitPath;
use mandate_shell::stages::{ExitPath, JournalWriter};
use mandate_time::Date;

/// The account stream the fixture lives on, as the shell's runs name it.
const STREAM: &str = "account/ws_01J8Z2/01J8Z2ACCT00000000000000A1";
/// The instrument everything in the fixture is: the vectors' own AAPL asset id.
const INSTRUMENT: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415";
/// The intent that opened the position, in the ULID form the executor derives ids from.
const OPEN_INTENT: &str = "01J8Z3M1P0000000000000000X";
/// The intent whose protective sell still rests.
const PROTECTIVE_INTENT: &str = "01J8Z3M1P0000000000000000Y";

/// The journal the adapter is handed: §9.5-shaped records, mirroring the vectors' bodies — a
/// bought position of ten, a resting protective sell of four, and a broker position of fifteen.
/// It answers `read` with the fixture and refuses every write, which is not its job here.
struct ScriptedJournal(Vec<StoredEvent>);

impl JournalWriter for ScriptedJournal {
    fn take_ownership(&mut self, stream: &str) -> Result<u64, mandate_shell::Cause> {
        let _ = stream;
        Err(mandate_shell::Cause::Absent {
            what: "a scripted journal takes no ownership",
        })
    }

    fn read(&self, stream: &str) -> Result<Vec<StoredEvent>, mandate_shell::Cause> {
        if stream == STREAM {
            Ok(self.0.clone())
        } else {
            Ok(Vec::new())
        }
    }

    fn append(
        &mut self,
        stream: &str,
        expected_head: u64,
        writer_epoch: u64,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        let _ = (stream, expected_head, writer_epoch, drafts);
        AppendOutcome::Unavailable
    }
}

/// One fixture event: the body as canonical JSON, everything else the stored row's shape.
fn row(seq: u64, event_id: &str, event_type: &str, payload: Object) -> StoredEvent {
    let mut body = Object::new();
    let mut members = vec![
        ("stream_id", Value::Str(STREAM.to_owned())),
        ("event_id", Value::Str(event_id.to_owned())),
        ("event_type", Value::Str(event_type.to_owned())),
        (
            "schema_version",
            Value::Int(Int::new(2).expect("a schema version")),
        ),
        ("environment", Value::Str("paper".to_owned())),
        ("causation_id", Value::Null),
        ("correlation_id", Value::Null),
        ("actor", Value::Str("agent/tracer-aapl".to_owned())),
        (
            "event_time",
            Value::Str("2026-09-21T14:00:00.000000000Z".to_owned()),
        ),
        ("clock_source", Value::Str("wall".to_owned())),
        ("config_refs", Value::Array(Vec::new())),
        ("artifact_refs", Value::Array(Vec::new())),
        ("pii_refs", Value::Array(Vec::new())),
        ("payload", Value::Object(payload)),
    ];
    members.push(("seq", Value::Int(Int::new(seq).expect("a seq"))));
    for (name, value) in members {
        body.insert(Key::new(name).expect("a canonical key"), value);
    }
    StoredEvent {
        stream_id: STREAM.to_owned(),
        seq,
        event_id: event_id.to_owned(),
        event_type: event_type.to_owned(),
        schema_version: 2,
        environment: "paper".to_owned(),
        recorded_at: "2026-09-21T14:00:00.000000000Z".to_owned(),
        prev_hash: mandate_canon::Digest::ZERO,
        hash: mandate_canon::Digest::ZERO,
        body: mandate_canon::to_canonical(&Value::Object(body)),
    }
}

/// `payload` with each `(name, value)` member, as a canonical object.
fn payload(members: Vec<(&str, Value)>) -> Object {
    let mut out = Object::new();
    for (name, value) in members {
        out.insert(Key::new(name).expect("a canonical key"), value);
    }
    out
}

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

fn number(value: u64) -> Value {
    Value::Int(Int::new(value).expect("a small integer"))
}

fn decimal(value: &str) -> Value {
    Value::Str(value.to_owned())
}

/// The fixture: the run opened ten shares (intent, companion, submission, fill), a protective
/// sell of four still rests (intent, companion, submission, no fill), and the broker reports
/// fifteen — MC-F01's shape, at the shell.
fn journal() -> ScriptedJournal {
    let risk_clock = text("2026-09-21T14:00:01.000000000Z");
    let mut rows = vec![row(
        1,
        "01J8Z2S0000000000000000001",
        "StreamOpened",
        payload(vec![
            ("stream_type", text("account")),
            ("workspace_id", text("ws_01J8Z2")),
            ("broker", text("alpaca")),
            ("account_ref", text("01J8Z2ACCT00000000000000A1")),
        ]),
    )];
    let opening = [
        (
            OPEN_INTENT,
            "buy",
            "10",
            "150",
            "01J8Z3M1P0000000000000000X-1",
        ),
        (
            PROTECTIVE_INTENT,
            "sell",
            "4",
            "149",
            "01J8Z3M1P0000000000000000Y-1",
        ),
    ];
    for (index, (intent, side, qty, price, client_order_id)) in opening.iter().enumerate() {
        let base: u64 = 2 + u64::try_from(index).expect("an index") * 4;
        rows.push(row(
            base,
            &format!("{intent}i"),
            "IntentReceived",
            payload(vec![
                ("agent_id", text("agent-a")),
                ("intent_id", text(intent)),
                ("instrument_id", text(INSTRUMENT)),
                ("side", text(side)),
                ("type", text("limit")),
                ("tif", text("day")),
                ("qty", decimal(qty)),
                ("limit_price", decimal(price)),
                (
                    "purpose",
                    text(if side == &"buy" { "open" } else { "protective" }),
                ),
                ("risk_clock", risk_clock.clone()),
            ]),
        ));
        rows.push(row(
            base + 1,
            &format!("{intent}c"),
            "OrderRequestRecorded",
            payload(vec![
                ("agent_id", text("agent-a")),
                ("intent_id", text(intent)),
                (
                    "purpose",
                    text(if side == &"buy" { "open" } else { "protective" }),
                ),
                ("extended_hours", Value::Bool(false)),
                ("stop_price", Value::Null),
                ("order_class", Value::Null),
                ("take_profit", Value::Null),
                ("stop", Value::Null),
                ("rung", Value::Null),
                ("at_floor", Value::Null),
                ("risk_clock", risk_clock.clone()),
            ]),
        ));
        rows.push(row(
            base + 2,
            &format!("{intent}s"),
            "OrderSubmitted",
            payload(vec![
                ("client_order_id", text(client_order_id)),
                ("attempt", number(1)),
                ("instrument_id", text(INSTRUMENT)),
                ("side", text(side)),
                ("type", text("limit")),
                ("tif", text("day")),
                ("qty", decimal(qty)),
                ("limit_price", decimal(price)),
                ("risk_clock", risk_clock.clone()),
            ]),
        ));
    }
    rows.push(row(
        10,
        "01J8Z3M1P0000000000000000Xf",
        "FillApplied",
        payload(vec![
            ("fill_id", text("exec-7f3a")),
            ("client_order_id", text("01J8Z3M1P0000000000000000X-1")),
            ("instrument_id", text(INSTRUMENT)),
            ("side", text("buy")),
            ("qty_gross", decimal("10")),
            ("price", decimal("150")),
            ("trade_date", text("2026-09-21")),
            ("risk_clock", risk_clock.clone()),
            ("fees", Value::Array(Vec::new())),
        ]),
    ));
    rows.push(row(
        11,
        "01J8Z3M1P0000000000000000Z",
        "BrokerPositionObserved",
        payload(vec![
            ("instrument_id", text(INSTRUMENT)),
            ("qty", decimal("15")),
            ("mismatch", Value::Bool(false)),
        ]),
    ));
    ScriptedJournal(rows)
}

/// A risk-limit kill switch: the whole agent, nothing confirmed, the fold's outstanding ids.
fn kill_switch(working_orders: Vec<String>) -> FlattenRequest {
    FlattenRequest {
        initiator: Initiator::RiskLimit,
        instrument: None,
        confirmation: None,
        working_orders,
    }
}

/// The state the fixture folds to, as the risk crate reads it: the agent's sub-ledger of ten,
/// the protective sell of four resting, and the broker's fifteen (which the plan must ignore).
/// The working order's numeric id is arbitrary — the risk crate's numeric space is the adapter's
/// internal mapping, invisible to the runtime — so the oracle keys it by any id and the equality
/// pinned is on the sells, purpose, and confirmation, never on the cancels.
fn folded_state() -> (mandate_risk::AgentId, BTreeMap<ClientOrderId, WorkingOrder>) {
    let agent = mandate_risk::AgentId(1);
    let working = ClientOrderId(7);
    let mut open_orders = BTreeMap::new();
    open_orders.insert(
        working,
        WorkingOrder {
            agent,
            instrument: AssetId::new(INSTRUMENT).expect("an asset id"),
            side: Side::Sell,
            max_cost: Usd::parse("0").expect("a cost"),
            open_qty: Qty::parse("4").expect("a qty"),
            protective: true,
            opening: false,
            submitted_on: Date::parse("2026-09-21").expect("a date"),
        },
    );
    (agent, open_orders)
}

/// The mapping the adapter owes, written once so the oracle can use it (DEC-449 item 3): the
/// risk crate's plan in the runtime's types. Cancels become the strings the fold keys orders by;
/// each sell and each deferred sell becomes a leg, a deferred one waiting for the regular
/// session; the purpose carries over; the request's confirmation carries over; the account-wide
/// endpoints and the mode-first flag have no runtime counterpart, because the runtime type cannot
/// carry them (§5.5's unrepresentable half).
fn runtime_plan_of(
    plan: &FlattenPlan,
    positions: &[AgentPosition],
    request: &FlattenRequest,
) -> RuntimePlan {
    let class_of = |instrument: &AssetId| {
        positions
            .iter()
            .find(|position| &position.instrument == instrument)
            .map(|position| position.asset_class)
            .expect("the fixture's instrument is in the positions")
    };
    let leg_of = |instrument: &AssetId, qty: Qty, deferred: bool| FlattenLeg {
        instrument: InstrumentId::new(instrument.as_str()).expect("an instrument id"),
        asset_class: class_of(instrument),
        qty,
        deferred_to_regular_session: deferred,
    };
    RuntimePlan {
        cancel_client_order_ids: plan
            .cancel_client_order_ids
            .iter()
            .map(|id| id.0.to_string())
            .collect(),
        sells: plan
            .sells
            .iter()
            .map(|sell| leg_of(&sell.instrument, sell.qty, false))
            .chain(
                plan.deferred_sells
                    .iter()
                    .map(|sell| leg_of(&sell.instrument, sell.qty, true)),
            )
            .collect(),
        purpose: match plan.purpose {
            mandate_risk::Purpose::OwnerExit => Purpose::OwnerExit,
            _ => Purpose::RiskExit,
        },
        confirmation: request.confirmation.clone(),
    }
}

/// The oracle's input over the folded state: the agent, its sub-ledger, the broker's quantities,
/// a regular session, and the exit offset the mandate's §5.6 tier sets.
fn input_of<'a>(
    agent: mandate_risk::AgentId,
    open_orders: &'a BTreeMap<ClientOrderId, WorkingOrder>,
    positions: &'a [AgentPosition],
    broker: &'a BTreeMap<AssetId, Qty>,
) -> FlattenInput<'a> {
    FlattenInput {
        agent,
        open_orders,
        agent_positions: positions,
        broker_positions: broker,
        session: mandate_risk::Session::Regular,
        initiator: FlattenInitiator::RiskLimit,
        owner_confirmed_bid: None,
        max_exit_offset: Fraction::parse("0.03").expect("an offset"),
        owner_floor_price: None,
    }
}

fn positions_of(agent: mandate_risk::AgentId) -> Vec<AgentPosition> {
    vec![AgentPosition {
        agent,
        instrument: AssetId::new(INSTRUMENT).expect("an asset id"),
        asset_class: mandate_risk::AssetClass::UsEquity,
        qty: Qty::parse("10").expect("a qty"),
    }]
}

fn broker_of() -> BTreeMap<AssetId, Qty> {
    BTreeMap::from([(
        AssetId::new(INSTRUMENT).expect("an asset id"),
        Qty::parse("15").expect("a qty"),
    )])
}

/// TI-4: the probe answers whether an agent-scoped flatten can be planned at all, over a
/// synthetic kill-switch request built from the mandate fixture. It is asked once, before
/// anything is armed, so a run that cannot plan its exit never opens a position (task brief,
/// step 17).
#[ignore = "pending E7-7"]
#[test]
fn the_probe_answers_ok_over_the_mandate_fixtures_synthetic_request() {
    RiskExitPath::new(fixture_mandate())
        .probe()
        .expect("the probe plans a synthetic kill switch");
}

/// The plan is the risk crate's own plan for the state the journal folds to, mapped into the
/// runtime's types by the mapping above: the adapter invents nothing. Equality is pinned on the
/// sells, the purpose, and the confirmation — the members whose mapping is determined — while
/// the cancels are pinned by the string round-trip in the structural test below: the risk
/// crate's numeric working-order ids are the adapter's internal mapping, invisible to the
/// runtime, which keys orders by strings end to end.
#[ignore = "pending E7-7"]
#[test]
fn the_plan_is_the_risk_crates_own_plan_for_the_journals_folded_state() {
    let exit = RiskExitPath::new(fixture_mandate());
    let journal = journal();
    let (agent, open_orders) = folded_state();
    let positions = positions_of(agent);
    let broker = broker_of();
    let request = kill_switch(vec!["01J8Z3M1P0000000000000000Y-1".to_owned()]);
    let plan = exit
        .plan(&request, &journal)
        .expect("the adapter plans over the journal it is handed");

    let oracle = agent_flatten(&input_of(agent, &open_orders, &positions, &broker))
        .expect("the risk crate plans the folded state");
    let mapped = runtime_plan_of(&oracle, &positions, &request);
    assert_eq!(plan.sells, mapped.sells);
    assert_eq!(plan.purpose, mapped.purpose);
    assert_eq!(plan.confirmation, mapped.confirmation);
}

/// §5.5's structural pins: the plan sells exactly the sub-ledger — ten, never the broker's
/// fifteen — as a risk exit whose cancels name only the agent's own working order the request
/// names, and a regular session defers nothing.
#[ignore = "pending E7-7"]
#[test]
fn the_plan_sells_the_sub_ledger_and_never_the_brokers_position() {
    let exit = RiskExitPath::new(fixture_mandate());
    let journal = journal();
    let request = kill_switch(vec!["01J8Z3M1P0000000000000000Y-1".to_owned()]);
    let plan = exit
        .plan(&request, &journal)
        .expect("the adapter plans over the journal it is handed");

    assert_eq!(plan.purpose, Purpose::RiskExit);
    let sold: Vec<Qty> = plan.sells.iter().map(|leg| leg.qty).collect();
    assert_eq!(sold, vec![Qty::parse("10").expect("a qty")]);
    assert!(
        plan.sells
            .iter()
            .all(|leg| !leg.deferred_to_regular_session)
    );
    assert!(
        plan.cancel_client_order_ids
            .iter()
            .all(|id| id == "01J8Z3M1P0000000000000000Y-1"),
        "cancels name only the agent's own resting protective sell"
    );
}

/// The plan reads the journal it is handed at the call, so two plans over the same journal and
/// request are equal: a replay shares the plan the first run made (task brief, "Durability").
#[ignore = "pending E7-7"]
#[test]
fn two_plans_over_the_same_journal_and_request_are_equal() {
    let exit = RiskExitPath::new(fixture_mandate());
    let journal = journal();
    let request = kill_switch(vec!["01J8Z3M1P0000000000000000Y-1".to_owned()]);
    assert_eq!(
        exit.plan(&request, &journal).expect("the first plan"),
        exit.plan(&request, &journal).expect("the second plan")
    );
}

/// The shell's own mandate fixture, as `Sources` names it.
fn fixture_mandate() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mandate.json")
}
