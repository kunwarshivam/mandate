//! The flatten probe's flip, tests first (DEC-77 stage 1; E7-7 stream L, claim #171): what
//! `RiskExitPath` must do once it binds `mandate_risk::agent_flatten` over the journal, the
//! stream, and the clock the bridge hands it, for the agent it is constructed for (DEC-449).
//!
//! The adapter implements the flip these tests pinned while it was a stub (DEC-77 stage 2):
//! every test that was pending now runs against the real fold, and the fail-closed suite pins
//! the probe over the shell's fixture
//! (`the_production_exit_probes_over_the_shells_fixture`).
//!
//! The journal is a scripted double returning §9.5-shaped records, and the builder proves the
//! fixture before any test runs it: every row parses through `mandate_journal::Draft::parse`
//! and the whole sequence folds through `mandate_executor::fold`, the two crates DEC-449 names,
//! so the contract cannot be pinned on records the journal or the fold would refuse. The scene
//! is MC-F01's at the shell: agent-a holds ten of an instrument with a protective sell of four
//! resting, agent-b holds five more of the same instrument, and the account as a whole holds
//! fifteen — the plan sells agent-a's ten, never the account's position.

use std::collections::BTreeMap;

use mandate_accounting::InstrumentId;
use mandate_canon::{Int, Key, Object, Value};
use mandate_executor::{ExecutorState, FoldedEvent, fold};
use mandate_journal::{AppendOutcome, Draft, MemoryJournal, StoredEvent, StreamId};
use mandate_num::{Fraction, Qty, Usd};
use mandate_risk::{
    AgentPosition, AssetId, ClientOrderId as RiskClientId, FlattenInitiator, FlattenInput,
    FlattenPlan, Side, WorkingOrder, agent_flatten,
};
use mandate_runtime::{
    AgentId, FlattenLeg, FlattenPlan as RuntimePlan, FlattenRequest, Initiator, Purpose, RiskClock,
};
use mandate_shell::adapters::RiskExitPath;
use mandate_shell::stages::{ExitPath, JournalWriter};
use mandate_time::{Date, UtcNanos};

/// The account stream the fixture lives on, in the `acct:{workspace}:{account}` form every
/// journal stream id takes.
const STREAM: &str = "acct:ws_01J8Z2:01J8Z2ACCT00000000000000A1";
/// The instrument everything in the fixture is: the vectors' own AAPL asset id.
const INSTRUMENT: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415";
/// The intent that opened agent-a's position, in the 26-character ULID form the agent stream
/// keys events by.
const OPEN_INTENT: &str = "01J8Z3M1P0000000000000000X";
/// The intent whose protective sell still rests. The request names intents — what the runtime's
/// fold keys outstanding work by — and the plan's cancels come back as the `md-`-prefixed client
/// order ids the journal's submissions carry, so the mapping is journal-derived, never a copy
/// (DEC-449 item 3).
const PROTECTIVE_INTENT: &str = "01J8Z3M1P0000000000000000Y";
/// The client order id the protective intent's submission carries: the executor's own
/// derivation, `md-` plus the intent id.
const PROTECTIVE_ORDER: &str = "md-01J8Z3M1P0000000000000000Y";
/// The intent by which agent-b opened its five shares of the same instrument.
const OTHER_INTENT: &str = "01J8Z3M1P0000000000000000Z";
/// The intent behind agent-b's resting protective sell of one: unfilled, so a plan that
/// cancelled every agent's working orders would name it and fail the cancel pin.
const OTHER_PROTECTIVE_INTENT: &str = "01J8Z3M1P0000000000000000V";
/// The broker-created protective leg no holder explains — two agents hold the instrument, so
/// the fold's single-holder rule attributes it to nobody (§5.4, DEC-160 item 3d) — and §5.5's
/// Close column makes an agent-scoped flatten cancel it in the instrument it closes.
const BROKER_LEG: &str = "md-broker-leg";
/// The second broker-created leg, filled to its whole quantity but never transitioned — a fill
/// record does not itself change an order's state — so the union's working rule must exclude it
/// by its filled quantity alone: a plan that cancelled it names an order that is done.
const BROKER_LEG_FILLED: &str = "md-broker-leg-filled";
/// A Monday mid-morning in New York: the regular session, so an equity sell plans now.
const REGULAR_CLOCK: &str = "2026-09-21T14:00:01.000000000Z";
/// The same Monday, before the open: an equity sell waits for the regular session.
const PRE_MARKET_CLOCK: &str = "2026-09-21T12:00:01.000000000Z";
/// The Saturday before: the market is closed inside the calendar's range, which is the
/// overnight session — the sell waits for Monday's open rather than the run halting (rule 13:
/// risk reduction is never denied).
const SATURDAY_CLOCK: &str = "2026-09-19T14:00:01.000000000Z";
/// An instrument neither agent holds, for the owner exit's scope.
const UNHELD_INSTRUMENT: &str = "cccccccc-cccc-cccc-cccc-cccccccccccc";

fn clock(at: &str) -> RiskClock {
    RiskClock::from_secs(UtcNanos::parse(at).expect("a timestamp").secs())
}

/// The journal the adapter is handed. It answers `read` with the fixture and refuses every
/// write, which is not its job here.
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

/// A journal populated through the real append path, so every returned row carries the three
/// journal-assigned sealed-body members.
struct MemoryBackedJournal {
    journal: MemoryJournal,
    stream: StreamId,
}

impl JournalWriter for MemoryBackedJournal {
    fn take_ownership(&mut self, stream: &str) -> Result<u64, mandate_shell::Cause> {
        let parsed = StreamId::parse(stream).ok_or(mandate_shell::Cause::Absent {
            what: "an account stream id",
        })?;
        Ok(self.journal.take_ownership(&parsed))
    }

    fn read(&self, stream: &str) -> Result<Vec<StoredEvent>, mandate_shell::Cause> {
        if stream == self.stream.as_str() {
            Ok(self.journal.rows(&self.stream).to_vec())
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
        let Some(parsed) = StreamId::parse(stream) else {
            return AppendOutcome::Unavailable;
        };
        let refs: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
        self.journal.append(
            &parsed,
            expected_head,
            writer_epoch,
            UtcNanos::parse(REGULAR_CLOCK).expect("the fixture's recorded time"),
            &refs,
        )
    }
}

/// One fixture event: the body as canonical JSON in the envelope the journal's own draft
/// validation accepts — the actor as the executor's system actor, `envelope_version` 1, the
/// per-record `schema_version` the catalogue registers, `config_refs` carrying the refs the
/// record type requires, `clock_source` in the envelope's vocabulary, and the sealed members
/// (`seq`, `prev_hash`, `recorded_at`) left to the stored row.
fn row(
    seq: u64,
    event_id: &str,
    event_type: &str,
    schema_version: u64,
    causation_id: Option<&str>,
    config_refs: Vec<(&str, &str)>,
    payload: Object,
) -> StoredEvent {
    let mut body = Object::new();
    let mut refs = Object::new();
    for (kind, digest) in config_refs {
        refs.insert(
            Key::new(kind).expect("a config ref kind"),
            Value::Str(digest.to_owned()),
        );
    }
    let mut actor = Object::new();
    for (name, value) in [
        ("kind", "system"),
        ("id", "executor"),
        ("version", "0.1.0"),
        (
            "build",
            "sha256:3333333333333333333333333333333333333333333333333333333333333333",
        ),
    ] {
        actor.insert(
            Key::new(name).expect("an actor key"),
            Value::Str(value.to_owned()),
        );
    }
    let members: Vec<(&str, Value)> = vec![
        ("stream_id", Value::Str(STREAM.to_owned())),
        ("event_id", Value::Str(event_id.to_owned())),
        ("event_type", Value::Str(event_type.to_owned())),
        (
            "schema_version",
            Value::Int(Int::new(schema_version).expect("a version")),
        ),
        (
            "envelope_version",
            Value::Int(Int::new(1).expect("the envelope version")),
        ),
        ("environment", Value::Str("paper".to_owned())),
        (
            "causation_id",
            causation_id.map_or(Value::Null, |id| Value::Str(id.to_owned())),
        ),
        ("correlation_id", Value::Null),
        ("actor", Value::Object(actor)),
        (
            "event_time",
            Value::Str("2026-09-21T14:00:00.000000000Z".to_owned()),
        ),
        ("clock_source", Value::Str("local".to_owned())),
        ("config_refs", Value::Object(refs)),
        ("artifact_refs", Value::Array(Vec::new())),
        ("pii_refs", Value::Array(Vec::new())),
        ("payload", Value::Object(payload)),
    ];
    for (name, value) in members {
        body.insert(Key::new(name).expect("a canonical key"), value);
    }
    body.insert(
        Key::new("seq").expect("the assigned sequence key"),
        Value::Int(Int::new(seq).expect("a sequence")),
    );
    body.insert(
        Key::new("prev_hash").expect("the assigned previous-hash key"),
        Value::Str(mandate_canon::Digest::ZERO.to_hex()),
    );
    body.insert(
        Key::new("recorded_at").expect("the assigned recorded-at key"),
        Value::Str("2026-09-21T14:00:00.000000000Z".to_owned()),
    );
    StoredEvent {
        stream_id: STREAM.to_owned(),
        seq,
        event_id: event_id.to_owned(),
        event_type: event_type.to_owned(),
        schema_version,
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

/// The config refs `IntentReceived` requires: the mandate the gate enforces.
fn mandate_refs() -> Vec<(&'static str, &'static str)> {
    vec![(
        "mandate_version",
        "sha256:5555555555555555555555555555555555555555555555555555555555555555",
    )]
}

/// The config refs `FillApplied` requires: the fee config and the three calendars.
fn fill_refs() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "fee_config",
            "sha256:1111111111111111111111111111111111111111111111111111111111111111",
        ),
        (
            "trading_calendar",
            "sha256:2222222222222222222222222222222222222222222222222222222222222222",
        ),
        (
            "settlement_calendar",
            "sha256:3333333333333333333333333333333333333333333333333333333333333333",
        ),
        (
            "instrument_snapshot",
            "sha256:4444444444444444444444444444444444444444444444444444444444444444",
        ),
    ]
}

/// The fixture: the stream opened, agent-a bought ten shares (intent, companion, submission,
/// fill), a protective sell of four still rests (intent, companion, submission, no fill), and
/// agent-b bought five more of the same instrument (intent, companion, submission, fill) — so
/// the account as a whole holds fifteen while agent-a's sub-ledger holds ten. The plan sells
/// agent-a's ten, never the account's fifteen.
fn journal() -> ScriptedJournal {
    let risk_clock = text("2026-09-21T14:00:01.000000000Z");
    let mut rows = vec![row(
        1,
        "01J8Z3M1Q0000000000000000A",
        "StreamOpened",
        1,
        None,
        Vec::new(),
        payload(vec![
            ("stream_type", text("account")),
            ("workspace_id", text("ws_01J8Z2")),
            ("broker", text("alpaca")),
            ("account_ref", text("01J8Z2ACCT00000000000000A1")),
        ]),
    )];
    let opening: [(&str, &str, &str, &str, &str, &str, &str); 4] = [
        (
            OPEN_INTENT,
            "agent-a",
            "buy",
            "10",
            "open",
            "150",
            "01J8Z3M1Q0000000000000000B",
        ),
        (
            PROTECTIVE_INTENT,
            "agent-a",
            "sell",
            "4",
            "protective",
            "149",
            "01J8Z3M1Q0000000000000000C",
        ),
        (
            OTHER_INTENT,
            "agent-b",
            "buy",
            "5",
            "open",
            "150",
            "01J8Z3M1Q0000000000000000D",
        ),
        (
            OTHER_PROTECTIVE_INTENT,
            "agent-b",
            "sell",
            "1",
            "protective",
            "151",
            "01J8Z3M1Q0000000000000000R",
        ),
    ];
    for (index, (intent, agent, side, qty, purpose, price, event_id)) in opening.iter().enumerate()
    {
        let seq: u64 = 2 + u64::try_from(index).expect("an index") * 3;
        rows.push(row(
            seq,
            event_id,
            "IntentReceived",
            2,
            None,
            mandate_refs(),
            payload(vec![
                ("intent_id", text(intent)),
                ("agent_id", text(agent)),
                ("instrument_id", text(INSTRUMENT)),
                ("side", text(side)),
                ("type", text("limit")),
                ("tif", text("day")),
                ("qty", decimal(qty)),
                ("limit_price", decimal(price)),
                ("purpose", text(purpose)),
                ("risk_clock", risk_clock.clone()),
            ]),
        ));
        let companion_id = format!("01J8Z3M1Q0000000000000000{}", ['E', 'F', 'G', 'S'][index]);
        rows.push(row(
            seq + 1,
            &companion_id,
            "OrderRequestRecorded",
            1,
            None,
            Vec::new(),
            payload(vec![
                ("agent_id", text(agent)),
                ("intent_id", text(intent)),
                ("purpose", text(purpose)),
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
            seq + 2,
            &format!("01J8Z3M1Q0000000000000000{}", ['H', 'J', 'K', 'T'][index]),
            "OrderSubmitted",
            2,
            Some(&companion_id),
            Vec::new(),
            payload(vec![
                ("client_order_id", text(&format!("md-{intent}"))),
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
    for (seq, event_id, order, side, qty) in [
        (
            14_u64,
            "01J8Z3M1Q0000000000000000M",
            OPEN_INTENT,
            "buy",
            "10",
        ),
        (15, "01J8Z3M1Q0000000000000000N", OTHER_INTENT, "buy", "5"),
    ] {
        rows.push(row(
            seq,
            event_id,
            "FillApplied",
            1,
            None,
            fill_refs(),
            payload(vec![
                ("fill_id", text(&format!("exec-{seq}"))),
                ("client_order_id", text(&format!("md-{order}"))),
                ("instrument_id", text(INSTRUMENT)),
                ("side", text(side)),
                ("qty_gross", decimal(qty)),
                ("price", decimal("150")),
                ("trade_date", text("2026-09-21")),
                ("risk_clock", risk_clock.clone()),
                ("fees", Value::Array(Vec::new())),
            ]),
        ));
    }
    rows.push(row(
        16,
        "01J8Z3M1Q0000000000000000W",
        "ProtectionChanged",
        1,
        None,
        Vec::new(),
        payload(vec![
            ("instrument_id", text(INSTRUMENT)),
            ("action", text("placed")),
            (
                "orders",
                Value::Array(vec![text(BROKER_LEG), text(BROKER_LEG_FILLED)]),
            ),
            ("awaiting", Value::Array(Vec::new())),
            ("qty", decimal("2")),
            ("stop", decimal("139")),
            ("take_profit", decimal("159")),
            ("intent_id", Value::Null),
            ("bracket", Value::Null),
            ("entry", Value::Null),
            ("agent_id", Value::Null),
            ("replacing", Value::Null),
            ("created_on", text("2026-09-21")),
            ("sent", Value::Null),
            ("uncovered", Value::Null),
            ("acknowledged", Value::Null),
            ("risk_clock", risk_clock.clone()),
        ]),
    ));
    rows.push(row(
        17,
        "01J8Z3M1Q0000000000000000P",
        "FillApplied",
        1,
        None,
        fill_refs(),
        payload(vec![
            ("fill_id", text("exec-broker-leg")),
            ("client_order_id", text(BROKER_LEG_FILLED)),
            ("instrument_id", text(INSTRUMENT)),
            ("side", text("sell")),
            ("qty_gross", decimal("2")),
            ("price", decimal("149")),
            ("trade_date", text("2026-09-21")),
            ("risk_clock", risk_clock.clone()),
            ("fees", Value::Array(Vec::new())),
        ]),
    ));
    let journal = ScriptedJournal(rows);
    prove_the_fixture(&journal);
    journal
}

fn sealed_journal() -> MemoryBackedJournal {
    let fixture = journal();
    let stream = StreamId::parse(STREAM).expect("the fixture's stream");
    let mut journal = MemoryJournal::new();
    let epoch = journal.take_ownership(&stream);
    let draft_bodies: Vec<Vec<u8>> = fixture.0.iter().map(draft_body).collect();
    let drafts: Vec<&[u8]> = draft_bodies.iter().map(Vec::as_slice).collect();
    let appended = journal.append(
        &stream,
        0,
        epoch,
        UtcNanos::parse(REGULAR_CLOCK).expect("the fixture's recorded time"),
        &drafts,
    );
    assert!(
        matches!(appended, AppendOutcome::Committed(_)),
        "the real memory journal seals the fixture: {appended:?}"
    );
    let first = journal.rows(&stream).first().expect("a sealed row");
    let sealed = mandate_canon::parse(&first.body).expect("the sealed canonical body");
    for assigned in ["seq", "prev_hash", "recorded_at"] {
        assert!(
            sealed.get(assigned).is_some(),
            "the real stored body carries journal-assigned `{assigned}`"
        );
    }
    MemoryBackedJournal { journal, stream }
}

fn draft_body(row: &StoredEvent) -> Vec<u8> {
    let Value::Object(mut body) =
        mandate_canon::parse(&row.body).expect("the fixture's canonical stored body")
    else {
        panic!("the fixture's stored body is an object")
    };
    for assigned in ["seq", "prev_hash", "recorded_at"] {
        body.remove(assigned)
            .unwrap_or_else(|| panic!("the fixture carries assigned `{assigned}`"));
    }
    mandate_canon::to_canonical(&Value::Object(body))
}

/// The fixture proves itself before any test runs it: every row parses through the journal's own
/// `Draft::parse`, and the whole sequence folds through the executor's public `fold` — the two
/// crates DEC-449 names — so the contract is never pinned on records they would refuse.
fn prove_the_fixture(journal: &ScriptedJournal) {
    let mut state = ExecutorState::new(mandate_executor::AccountScope {
        account: mandate_executor::AccountRef("01J8Z2ACCT00000000000000A1".to_owned()),
        workspace: mandate_executor::WorkspaceId("ws_01J8Z2".to_owned()),
    });
    for stored in &journal.0 {
        let draft = Draft::parse(&draft_body(stored)).unwrap_or_else(|refusal| {
            panic!(
                "the fixture's {} row is one the journal accepts: {refusal:?}",
                stored.event_type
            )
        });
        let normalized =
            mandate_canon::parse(draft.canonical_bytes()).expect("the fixture's canonical form");
        let event = FoldedEvent {
            stream: stored.stream_id.clone(),
            seq: mandate_executor::Seq(stored.seq),
            event_id: mandate_executor::EventId(stored.event_id.clone()),
            event_type: stored.event_type.clone(),
            causation_id: normalized
                .get("causation_id")
                .and_then(Value::as_str)
                .map(|id| mandate_executor::EventId(id.to_owned())),
            payload: normalized
                .get("payload")
                .cloned()
                .expect("the fixture's payload"),
        };
        fold(&mut state, &event).unwrap_or_else(|refusal| {
            panic!(
                "the fixture's {} row is one the fold accepts: {refusal:?}",
                stored.event_type
            )
        });
    }
}

/// A risk-limit kill switch: the whole agent, nothing confirmed, the fold's outstanding intents.
fn kill_switch(working_orders: Vec<String>) -> FlattenRequest {
    FlattenRequest {
        initiator: Initiator::RiskLimit,
        instrument: None,
        confirmation: None,
        working_orders,
    }
}

/// An owner's exit of one instrument: the scope the plan filters to, nothing confirmed, the
/// fold's outstanding intents.
fn owner_exit(instrument: Option<InstrumentId>) -> FlattenRequest {
    FlattenRequest {
        initiator: Initiator::Owner,
        instrument,
        confirmation: None,
        working_orders: Vec::new(),
    }
}

/// The state the fixture folds to, as the risk crate reads it: agent-a's sub-ledger of ten and
/// its protective sell of four resting — never agent-b's five, which belongs to another agent's
/// sub-ledger. The working order's numeric id is arbitrary — the risk crate's numeric space is
/// the adapter's internal mapping, invisible to the runtime — so the equality pinned is on the
/// sells, purpose, and confirmation, never on the cancels, which the structural test pins as the
/// journal's own client order strings.
fn folded_state() -> (mandate_risk::AgentId, BTreeMap<RiskClientId, WorkingOrder>) {
    let agent = mandate_risk::AgentId(1);
    let mut open_orders = BTreeMap::new();
    open_orders.insert(
        RiskClientId(7),
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
/// risk crate's plan in the runtime's types. Each sell and each deferred sell becomes a leg, a
/// deferred one waiting for the regular session; the purpose carries over; the request's
/// confirmation carries over; and the account-wide endpoints and the mode-first flag have no
/// runtime counterpart, because the runtime type cannot carry them (§5.5's unrepresentable
/// half). The cancels are not mapped here: they are pinned, as the journal's own client order
/// strings, by the structural test.
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
        cancel_client_order_ids: Vec::new(),
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

/// The oracle's input over the folded state: the agent, its sub-ledger, an empty broker map —
/// the broker's quantities are deliberately not the plan's input (MC-F01: the plan sells the
/// sub-ledger, never the broker's position) — and the session the clock derives.
fn input_of<'a>(
    agent: mandate_risk::AgentId,
    open_orders: &'a BTreeMap<RiskClientId, WorkingOrder>,
    positions: &'a [AgentPosition],
    broker: &'a BTreeMap<AssetId, Qty>,
    session: mandate_risk::Session,
) -> FlattenInput<'a> {
    FlattenInput {
        agent,
        open_orders,
        agent_positions: positions,
        broker_positions: broker,
        session,
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

/// TI-4: the probe answers whether an agent-scoped flatten can be planned at all, over a
/// synthetic kill-switch request built from the mandate fixture. It is asked once, before
/// anything is armed, so a run that cannot plan its exit never opens a position (task brief,
/// step 17).
#[test]
fn the_probe_answers_ok_over_the_mandate_fixtures_synthetic_request() {
    RiskExitPath::new(the_agent(), fixture_mandate())
        .probe()
        .expect("the probe plans a synthetic kill switch");
}

/// The probe reads the mandate it is handed: one it cannot read is a spec refusal naming the
/// document, never the stub's own refusal, so an adapter that ignores the mandate cannot pass
/// by refusing everything alike.
#[test]
fn the_probe_refuses_over_a_mandate_it_cannot_read() {
    let unreadable = RiskExitPath::new(
        the_agent(),
        std::path::PathBuf::from("no/such/mandate.json"),
    );
    match unreadable.probe() {
        Err(mandate_shell::Cause::Spec(_)) => {}
        Err(cause) => {
            panic!("an unreadable mandate is a spec refusal naming the document: {cause:?}")
        }
        Ok(()) => panic!("a probe over an unreadable mandate refuses"),
    }
}

/// The plan is the risk crate's own plan for the state the journal folds to, mapped into the
/// runtime's types by the mapping above: the adapter invents nothing. Equality is pinned on the
/// sells, the purpose, and the confirmation — the members whose mapping is determined — while
/// the cancels are pinned by the client-order round trip in the structural test below: the risk
/// crate's numeric working-order ids are the adapter's internal mapping, invisible to the
/// runtime.
#[test]
fn the_plan_is_the_risk_crates_own_plan_for_the_journals_folded_state() {
    let exit = RiskExitPath::new(the_agent(), fixture_mandate());
    let journal = journal();
    let (agent, open_orders) = folded_state();
    let positions = positions_of(agent);
    let request = kill_switch(vec![PROTECTIVE_INTENT.to_owned()]);
    let plan = exit
        .plan(&request, &journal, STREAM, Some(clock(REGULAR_CLOCK)))
        .expect("the adapter plans over the journal it is handed");

    let oracle = agent_flatten(&input_of(
        agent,
        &open_orders,
        &positions,
        &BTreeMap::new(),
        mandate_risk::Session::Regular,
    ))
    .expect("the risk crate plans the folded state");
    let mapped = runtime_plan_of(&oracle, &positions, &request);
    assert_eq!(plan.sells, mapped.sells);
    assert_eq!(plan.purpose, mapped.purpose);
    assert_eq!(plan.confirmation, mapped.confirmation);
}

/// Review blocker 1: production reads sealed stored rows, not draft bodies. A real
/// `MemoryJournal::append` adds `seq`, `prev_hash`, and `recorded_at`; planning must validate the
/// original draft strictly and fold the stored row without feeding those assigned keys to
/// `Draft::parse`.
#[test]
fn the_plan_folds_rows_sealed_by_the_real_memory_journal() {
    let exit = RiskExitPath::new(the_agent(), fixture_mandate());
    let journal = sealed_journal();
    let request = kill_switch(vec![PROTECTIVE_INTENT.to_owned()]);
    let plan = exit
        .plan(&request, &journal, STREAM, Some(clock(REGULAR_CLOCK)))
        .expect("the adapter plans over real sealed stored rows");
    assert_eq!(
        plan.sells.iter().map(|leg| leg.qty).collect::<Vec<_>>(),
        vec![Qty::parse("10").expect("a qty")]
    );
}

/// Removing the journal-assigned members does not weaken either boundary: each assigned value must
/// match its stored column, and the remaining writer draft is still closed to unknown keys.
#[test]
fn sealed_row_normalization_keeps_assigned_and_draft_validation_strict() {
    let request = kill_switch(vec![PROTECTIVE_INTENT.to_owned()]);
    let refuses = |rows: Vec<StoredEvent>| {
        matches!(
            RiskExitPath::new(the_agent(), fixture_mandate()).plan(
                &request,
                &ScriptedJournal(rows),
                STREAM,
                Some(clock(REGULAR_CLOCK))
            ),
            Err(mandate_shell::Cause::Spec(_))
        )
    };

    let journal = sealed_journal();
    let mut mismatched = journal.journal.rows(&journal.stream).to_vec();
    let first = mismatched.first_mut().expect("the first stored row");
    let Value::Object(mut body) =
        mandate_canon::parse(&first.body).expect("the sealed body parses")
    else {
        panic!("the sealed body is an object")
    };
    body.insert(
        Key::new("recorded_at").expect("the assigned key"),
        text("2026-09-21T14:00:02.000000000Z"),
    );
    first.body = mandate_canon::to_canonical(&Value::Object(body));
    assert!(
        refuses(mismatched),
        "assigned fields must match row columns"
    );

    let journal = sealed_journal();
    let mut unknown = journal.journal.rows(&journal.stream).to_vec();
    let first = unknown.first_mut().expect("the first stored row");
    let Value::Object(mut body) =
        mandate_canon::parse(&first.body).expect("the sealed body parses")
    else {
        panic!("the sealed body is an object")
    };
    body.insert(
        Key::new("unexpected").expect("a canonical unknown key"),
        Value::Null,
    );
    first.body = mandate_canon::to_canonical(&Value::Object(body));
    assert!(
        refuses(unknown),
        "strict draft validation still refuses unknown writer keys"
    );
}

/// §5.5's structural pins, in the regular session: the plan sells exactly the deciding agent's
/// sub-ledger — ten, never the account's fifteen, with agent-b's five shares left untouched —
/// as a risk exit that cancels the agent's own resting protective sell by the `md-` client
/// order id the journal's submission carries, though the request named the intent. The mapping
/// from intent to order is journal-derived; a copy of the request's strings would name an
/// order the broker never saw and fail here.
#[test]
fn the_plan_sells_the_agents_sub_ledger_not_the_accounts_position() {
    let exit = RiskExitPath::new(the_agent(), fixture_mandate());
    let journal = journal();
    let request = kill_switch(vec![PROTECTIVE_INTENT.to_owned()]);
    let plan = exit
        .plan(&request, &journal, STREAM, Some(clock(REGULAR_CLOCK)))
        .expect("the adapter plans over the journal it is handed");

    assert_eq!(plan.purpose, Purpose::RiskExit);
    let sold: Vec<Qty> = plan.sells.iter().map(|leg| leg.qty).collect();
    assert_eq!(
        sold,
        vec![Qty::parse("10").expect("a qty")],
        "the plan sells agent-a's ten, never the account's fifteen"
    );
    assert!(
        plan.sells
            .iter()
            .all(|leg| !leg.deferred_to_regular_session)
    );
    assert_eq!(
        plan.cancel_client_order_ids,
        vec![PROTECTIVE_ORDER.to_owned(), BROKER_LEG.to_owned()],
        "cancels name the agent's own md- order and the unattributed leg of the closed instrument"
    );
}

/// The session comes from the clock the bridge hands: before the open, an equity sell waits for
/// the regular session instead of planning to sell now, so a wrong session guess fails the
/// regular-session test above and an ignored clock fails this one.
#[test]
fn before_the_open_the_equity_sell_waits_for_the_regular_session() {
    let exit = RiskExitPath::new(the_agent(), fixture_mandate());
    let journal = journal();
    let request = kill_switch(vec![PROTECTIVE_INTENT.to_owned()]);
    let plan = exit
        .plan(&request, &journal, STREAM, Some(clock(PRE_MARKET_CLOCK)))
        .expect("the adapter plans over the journal it is handed");
    assert_eq!(
        plan.sells,
        vec![FlattenLeg {
            instrument: InstrumentId::new(INSTRUMENT).expect("an instrument id"),
            asset_class: mandate_risk::AssetClass::UsEquity,
            qty: Qty::parse("10").expect("a qty"),
            deferred_to_regular_session: true,
        }],
        "an equity sell outside the regular session defers to the open"
    );
}

/// The clock is the session's only source, so a plan asked before any clock has advanced is
/// refused with a real refusal that names the clock — never the stub's own — and no default
/// invents a session (the fail-closed suite's own discipline, which caught this shape's first
/// draft).
#[test]
fn a_plan_with_no_clock_advanced_is_refused() {
    let exit = RiskExitPath::new(the_agent(), fixture_mandate());
    let journal = journal();
    let request = kill_switch(vec![PROTECTIVE_INTENT.to_owned()]);
    match exit.plan(&request, &journal, STREAM, None) {
        Err(cause @ mandate_shell::Cause::Unimplemented { .. }) => {
            panic!("a plan with no clock is a real refusal, not the E7-7 stub: {cause:?}")
        }
        Err(_) => {}
        Ok(plan) => panic!("a plan with no clock advanced is refused, never defaulted: {plan:?}"),
    }
}

/// An owner's exit names the one instrument it closes (mandate spec §6.1, trading-domain spec
/// §5.5, DEC-257): the plan sells exactly the agent's sub-ledger of that instrument and cancels
/// only its orders in it. An exit of the held instrument sells the same ten and cancels the
/// same protective order a kill switch would; an exit of an instrument the agent does not hold
/// sells nothing and cancels nothing — never another instrument's sub-ledger or protection.
#[test]
fn an_owner_exit_sells_only_the_named_instruments_sub_ledger() {
    let exit = RiskExitPath::new(the_agent(), fixture_mandate());
    let journal = journal();
    let held = InstrumentId::new(INSTRUMENT).expect("an instrument id");
    let unheld = InstrumentId::new(UNHELD_INSTRUMENT).expect("an instrument id");

    let of_the_held = exit
        .plan(
            &owner_exit(Some(held)),
            &journal,
            STREAM,
            Some(clock(REGULAR_CLOCK)),
        )
        .expect("the adapter plans an owner exit of the held instrument");
    assert_eq!(
        of_the_held
            .sells
            .iter()
            .map(|leg| leg.qty)
            .collect::<Vec<_>>(),
        vec![Qty::parse("10").expect("a qty")]
    );
    assert_eq!(
        of_the_held.cancel_client_order_ids,
        vec![PROTECTIVE_ORDER.to_owned(), BROKER_LEG.to_owned()]
    );

    let of_an_unheld = exit
        .plan(
            &owner_exit(Some(unheld)),
            &journal,
            STREAM,
            Some(clock(REGULAR_CLOCK)),
        )
        .expect("the adapter plans an owner exit of an instrument the agent does not hold");
    assert!(
        of_an_unheld.sells.is_empty(),
        "an exit of an unheld instrument sells nothing: {:?}",
        of_an_unheld.sells
    );
    assert!(
        of_an_unheld.cancel_client_order_ids.is_empty(),
        "an exit of an unheld instrument cancels nothing: {:?}",
        of_an_unheld.cancel_client_order_ids
    );
}

/// The market closed inside the calendar's range is the overnight session, not a refusal: the
/// equity sell waits for Monday's open, so a weekend kill switch still plans — risk reduction
/// is never denied (AGENTS.md rule 13), and only an instant the calendar cannot cover at all
/// refuses.
#[test]
fn on_a_saturday_the_equity_sell_waits_for_the_open() {
    let exit = RiskExitPath::new(the_agent(), fixture_mandate());
    let journal = journal();
    let request = kill_switch(vec![PROTECTIVE_INTENT.to_owned()]);
    let plan = exit
        .plan(&request, &journal, STREAM, Some(clock(SATURDAY_CLOCK)))
        .expect("the adapter plans over a closed market inside the calendar's range");
    assert_eq!(
        plan.sells,
        vec![FlattenLeg {
            instrument: InstrumentId::new(INSTRUMENT).expect("an instrument id"),
            asset_class: mandate_risk::AssetClass::UsEquity,
            qty: Qty::parse("10").expect("a qty"),
            deferred_to_regular_session: true,
        }],
        "an equity sell on a closed market defers to the open"
    );
}

/// The plan reads the journal it is handed at the call, so two plans over the same journal,
/// stream, and clock are equal: a replay shares the plan the first run made (task brief,
/// "Durability").
#[test]
fn two_plans_over_the_same_journal_and_request_are_equal() {
    let exit = RiskExitPath::new(the_agent(), fixture_mandate());
    let journal = journal();
    let request = kill_switch(vec![PROTECTIVE_INTENT.to_owned()]);
    assert_eq!(
        exit.plan(&request, &journal, STREAM, Some(clock(REGULAR_CLOCK)))
            .expect("the first plan"),
        exit.plan(&request, &journal, STREAM, Some(clock(REGULAR_CLOCK)))
            .expect("the second plan")
    );
}

/// The agent the adapter flattens, as `Sources` names it.
fn the_agent() -> AgentId {
    AgentId("agent-a".to_owned())
}

/// The shell's own mandate fixture, as `Sources` names it.
fn fixture_mandate() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tracer/mandate.json")
}
