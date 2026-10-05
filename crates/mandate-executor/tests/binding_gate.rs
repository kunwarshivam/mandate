//! E7-7's binding §9.1 gate: full trusted inputs in, `mandate-risk` decides.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{
    ACCOUNT_STREAM, AGENT, FixedInstruments, FixedMandate, Shell, TestIds, VERSION, config,
    discretionary_exit, event, handoff, opening, ports, risk_exit, stream_opened, text, with_clock,
};
use mandate_executor::{
    BindingGateConfigRefs, BindingGateInput, BindingGateRequest, BindingGateSource, BrokerRequest,
    EventDraft, ExecutorError, Ports,
};
use mandate_journal::{Draft, InvalidReason};
use mandate_num::{Fraction, Price, Qty, Ratio, Usd};
use mandate_risk::spec_types::{GoalState, RiskLimits};
use mandate_risk::{
    AccountSnapshot, AccountState, AccountType, AgentId, AgentMode, AgentSnapshot, AssetClass,
    AssetId, ConductState, DayTradeLedger, DayTradeRegime, EtpClass, Exchange, GateConfig,
    InstrumentSnapshot, MarketSnapshot, QuoteCurrency, RiskSnapshot, SaneQuote, ValidatedMandate,
    WorkingUniverse,
};
use mandate_time::UtcNanos;
use serde_json::{Map, Value as Json};

const AAPL: &str = "AAPL";
const INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ0";

struct GateFixture {
    input: Option<BindingGateInput>,
}

impl GateFixture {
    fn allowing() -> Self {
        Self {
            input: Some(input(AccountState::Active, false)),
        }
    }

    fn denying() -> Self {
        Self {
            input: Some(input(AccountState::Active, true)),
        }
    }
}

impl BindingGateSource for GateFixture {
    fn input(&self, _request: &BindingGateRequest<'_>) -> Option<BindingGateInput> {
        self.input.clone()
    }
}

fn decimal<T>(raw: &str, parse: impl FnOnce(&str) -> Result<T, mandate_num::NumError>) -> T {
    parse(raw).unwrap_or_else(|error| panic!("{raw} parses: {error}"))
}

fn usd(raw: &str) -> Usd {
    decimal(raw, Usd::parse)
}

fn qty(raw: &str) -> Qty {
    decimal(raw, Qty::parse)
}

fn price(raw: &str) -> Price {
    decimal(raw, Price::parse)
}

fn fraction(raw: &str) -> Fraction {
    decimal(raw, Fraction::parse)
}

fn ratio(raw: &str) -> Ratio {
    decimal(raw, Ratio::parse)
}

fn at(raw: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(raw).unwrap_or_else(|error| panic!("{raw} parses: {error}"))
}

fn input(state: AccountState, halted: bool) -> BindingGateInput {
    let asset = AssetId::new(AAPL).unwrap_or_else(|error| panic!("asset: {error}"));
    let limits = RiskLimits {
        max_position_usd: usd("100000"),
        max_position_fraction: fraction("1"),
        max_order_usd: usd("100000"),
        max_gross_exposure_usd: usd("100000"),
        max_orders_per_day: 100,
        reentry_cooldown_s: 0,
        rebalance_band: fraction("0.05"),
        breach_confirm_s: 60,
        drawdown_ladder: Vec::new(),
    };
    BindingGateInput {
        config_refs: BindingGateConfigRefs::complete(
            format!("sha256:{}", "1".repeat(64)),
            format!("sha256:{}", "2".repeat(64)),
            format!("sha256:{}", "3".repeat(64)),
            format!("sha256:{}", "4".repeat(64)),
            VERSION,
        ),
        now: at("2026-09-21T15:00:00Z"),
        config: GateConfig {
            price_floor: usd("5"),
            liquidity_floor_usd: usd("1000000"),
            crypto_liquidity_floor_usd: usd("1000000"),
            collar_liquid_threshold_usd: usd("50000000"),
            collar_liquid_x: fraction("0.01"),
            collar_other_x: fraction("0.02"),
            collar_crypto_x: fraction("0.02"),
            collar_passive_band: fraction("0.2"),
            opposite_fill_interval_s: 60,
            min_resting_time_s: 2,
            order_to_fill_max: 10,
            order_to_fill_min_orders: 20,
            order_size_participation: fraction("0.05"),
            daily_participation: fraction("0.05"),
            close_window_minutes: 10,
            legacy_pdt_equity_threshold: usd("25000"),
            etp_classification_max_age_s: 604_800,
        },
        mandate: ValidatedMandate::from_validated_parts(limits, GoalState::Running, false, false),
        risk: RiskSnapshot {
            agent_equity: usd("10000"),
            high_water_mark: usd("10000"),
            day_start_equity: usd("10000"),
            capital_base: usd("10000"),
            inherited_loss: Usd::ZERO,
            latched: BTreeSet::new(),
            active_rungs: BTreeMap::new(),
            size_factor: ratio("1"),
            agent_mode: AgentMode::Normal,
        },
        account: AccountSnapshot {
            account_type: AccountType::Margin,
            state,
            crypto_active: true,
            regime: DayTradeRegime::IntradayMargin {
                maintenance_excess: usd("100000"),
            },
            equity: usd("10000"),
            prior_close_equity: usd("10000"),
            model_buying_power: usd("100000"),
            broker_buying_power: usd("100000"),
            broker_non_marginable_buying_power: usd("100000"),
            positions: BTreeMap::new(),
            market_values: BTreeMap::new(),
            working_orders: BTreeMap::new(),
            unknown_orders: BTreeSet::new(),
            related_account_resting: BTreeMap::new(),
        },
        agent: AgentSnapshot {
            agent: AgentId(1),
            mode: AgentMode::Normal,
            instrument_restrictions: BTreeMap::new(),
            positions: BTreeMap::new(),
            market_values: BTreeMap::new(),
            working_orders: BTreeSet::new(),
            instrument_groups: BTreeMap::new(),
            last_exit_fill_at: BTreeMap::new(),
            orders_today: 0,
            day_trades: DayTradeLedger::default(),
        },
        instrument: InstrumentSnapshot {
            instrument: asset.clone(),
            asset_class: AssetClass::UsEquity,
            exchange: Some(Exchange::Nasdaq),
            status_active: true,
            tradable: true,
            fractionable: false,
            ipo: false,
            ptp_no_exception: false,
            etp: EtpClass::Plain,
            etp_classified_at: Some(at("2026-09-21T00:00:00Z")),
            quote_currency: Some(QuoteCurrency::Usd),
            prior_close: Some(price("150")),
            median_dollar_volume_20d: Some(usd("90000000")),
            median_dollar_volume_30d: None,
            min_order_size: qty("1"),
            qty_increment: qty("1"),
            halted,
            status_feed_current: true,
        },
        market: MarketSnapshot {
            quote: Some(SaneQuote {
                bid: price("149.98"),
                ask: price("150.02"),
                at: at("2026-09-21T14:59:59Z"),
            }),
            last_trade: Some((price("150"), at("2026-09-21T14:59:59Z"))),
            trailing_5m_volume: Some(qty("100000")),
            adv_20d: Some(qty("1000000")),
        },
        conduct: ConductState::default(),
        universe: WorkingUniverse::Known {
            instruments: BTreeSet::from([asset]),
            pinned: true,
        },
        asset: AssetId::new(AAPL).unwrap_or_else(|error| panic!("asset: {error}")),
        gate_agent: AgentId(1),
        gate_client_order_id: mandate_risk::ClientOrderId(1),
        owner_confirmed_bid: None,
        fee_reservation: Usd::ZERO,
        data_profile: "iex".to_owned(),
        feed: "iex".to_owned(),
    }
}

fn input_with_position() -> BindingGateInput {
    let mut trusted = input(AccountState::Active, false);
    let asset = trusted.asset.clone();
    trusted.account.positions.insert(asset.clone(), qty("10"));
    trusted.agent.positions.insert(asset, qty("10"));
    trusted
}

fn bound_ports<'a>(
    ids: &'a TestIds,
    mandates: &'a FixedMandate,
    instruments: &'a FixedInstruments,
    config: &'a mandate_executor::ExecutorConfig,
) -> Ports<'a> {
    ports(ids, mandates, instruments, config)
}

fn ready() -> (Shell, Ports<'static>) {
    let ids = Box::leak(Box::new(TestIds));
    let mandates = Box::leak(Box::new(FixedMandate::covering(&[AAPL])));
    let instruments = Box::leak(Box::new(FixedInstruments));
    let executor_config = Box::leak(Box::new(config()));
    let ports = bound_ports(ids, mandates, instruments, executor_config);
    let mut shell = Shell::new(1);
    shell
        .fold_one(&stream_opened())
        .unwrap_or_else(|error| panic!("{error}"));
    (shell.restart_ready(&ports), ports)
}

fn unreconciled() -> (Shell, Ports<'static>) {
    let ids = Box::leak(Box::new(TestIds));
    let mandates = Box::leak(Box::new(FixedMandate::covering(&[AAPL])));
    let instruments = Box::leak(Box::new(FixedInstruments));
    let executor_config = Box::leak(Box::new(config()));
    let ports = bound_ports(ids, mandates, instruments, executor_config);
    let mut shell = Shell::new(1);
    shell
        .fold_one(&stream_opened())
        .unwrap_or_else(|error| panic!("{error}"));
    let (shell, _) = shell.restart(&ports);
    (shell, ports)
}

fn gate_draft(draft: &EventDraft) -> Map<String, Json> {
    let fixture = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/refcases/journal.json"
    ))
    .unwrap_or_else(|error| panic!("journal fixture: {error}"));
    let fixture: Json =
        serde_json::from_str(&fixture).unwrap_or_else(|error| panic!("journal fixture: {error}"));
    let mut body = fixture
        .pointer("/account_stream/chain/3/body")
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_else(|| panic!("version 2 GateDecided fixture"));
    for sealed in ["seq", "recorded_at", "prev_hash"] {
        body.remove(sealed);
    }
    let convert = |value: &mandate_canon::Value| {
        serde_json::from_slice(&mandate_canon::to_canonical(value))
            .unwrap_or_else(|error| panic!("canonical value: {error}"))
    };
    body.insert(
        "config_refs".to_owned(),
        convert(&mandate_canon::Value::Object(draft.config_refs.clone())),
    );
    body.insert("payload".to_owned(), convert(&draft.payload));
    body
}

fn local_gate_draft() -> EventDraft {
    let gate = GateFixture::allowing();
    let (mut shell, ports) = unreconciled();
    let ran = shell.run_with_binding(
        handoff(INTENT, AGENT, opening(AAPL, "1", "150")),
        &ports,
        &gate,
    );
    ran.draft("GateDecided")
        .cloned()
        .unwrap_or_else(|| panic!("local GateDecided"))
}

fn ready_with_position() -> (Shell, Ports<'static>) {
    let (mut shell, ports) = ready();
    for (event_type, fields) in [
        (
            "OrderSubmitted",
            vec![
                ("client_order_id", text("md-held-1")),
                ("agent", text(AGENT)),
                ("instrument", text(AAPL)),
                ("side", text("buy")),
                ("qty", text("10")),
                ("limit", text("150")),
            ],
        ),
        (
            "FillApplied",
            vec![
                ("fill_id", text("f-held-1")),
                ("client_order_id", text("md-held-1")),
                ("instrument", text(AAPL)),
                ("side", text("buy")),
                ("qty_gross", text("10")),
                ("price", text("150")),
            ],
        ),
        (
            "OrderStateChanged",
            vec![
                ("client_order_id", text("md-held-1")),
                ("state", text("filled")),
            ],
        ),
    ] {
        let fact = event(
            ACCOUNT_STREAM,
            shell.head().0.saturating_add(1),
            event_type,
            with_clock(&fields, 10),
        );
        shell
            .fold_one(&fact)
            .unwrap_or_else(|error| panic!("{event_type} folds: {error}"));
    }
    (shell.restart_ready(&ports), ports)
}

fn submitted(ran: &common::Ran) -> bool {
    ran.requests
        .iter()
        .any(|request| matches!(request, BrokerRequest::Submit(_)))
}

#[test]
fn full_binding_input_allows_and_records_its_evidence_profile() {
    let gate = GateFixture::allowing();
    let (mut shell, ports) = ready();
    let ran = shell.run_with_binding(
        handoff(INTENT, AGENT, opening(AAPL, "1", "150")),
        &ports,
        &gate,
    );
    assert!(submitted(&ran));
    let decided = ran
        .draft("GateDecided")
        .unwrap_or_else(|| panic!("gate decision"));
    assert_eq!(
        decided.payload.get("verdict").and_then(|v| v.as_str()),
        Some("allow")
    );
    assert!(
        decided.payload.get("evaluation").is_none(),
        "a full §9.1 decision carries no unregistered member"
    );
    assert_eq!(
        decided
            .payload
            .get("data_profile")
            .and_then(|value| value.as_str()),
        Some("iex")
    );
    let received = ran
        .draft("IntentReceived")
        .unwrap_or_else(|| panic!("intent receipt"));
    assert_eq!(received.config_refs.len(), 1);
    assert_eq!(
        received
            .config_refs
            .get("mandate_version")
            .and_then(|value| value.as_str()),
        Some(VERSION)
    );
    assert_eq!(decided.config_refs.len(), 5);
    for name in [
        "fee_config",
        "trading_calendar",
        "instrument_snapshot",
        "rule_set",
        "mandate_version",
    ] {
        assert!(
            decided
                .config_refs
                .get(name)
                .and_then(|value| value.as_str())
                .is_some(),
            "{name}"
        );
    }
}

#[test]
fn local_only_hold_uses_the_registered_schema_and_remains_a_denial() {
    let draft = local_gate_draft();
    assert_eq!(
        draft
            .payload
            .get("verdict")
            .and_then(|value| value.as_str()),
        Some("hold")
    );
    assert_eq!(
        draft
            .payload
            .get("reason_code")
            .and_then(|value| value.as_str()),
        Some("startup_reconciliation_pending")
    );
    assert_eq!(
        draft
            .payload
            .get("data_profile")
            .and_then(|value| value.as_str()),
        Some("account_stream_only")
    );
    assert_eq!(
        draft.payload.get("quotes_used"),
        Some(&mandate_canon::Value::Array(Vec::new()))
    );
    assert_eq!(
        draft.payload.get("marks_used"),
        Some(&mandate_canon::Value::Array(Vec::new()))
    );
    assert_eq!(
        draft
            .payload
            .get("checks")
            .and_then(mandate_canon::Value::as_array)
            .map(|checks| checks.len()),
        Some(8)
    );
    assert!(draft.payload.get("evaluation").is_none());
    let bytes = serde_json::to_vec(&Json::Object(gate_draft(&draft)))
        .unwrap_or_else(|error| panic!("draft bytes: {error}"));
    assert_eq!(Draft::parse(&bytes).map(|_| ()), Ok(()));
}

#[test]
fn local_only_gate_rejects_the_superseded_evaluation_member() {
    let draft = local_gate_draft();
    let mut body = gate_draft(&draft);
    let payload = body
        .get_mut("payload")
        .and_then(Json::as_object_mut)
        .unwrap_or_else(|| panic!("GateDecided payload"));
    payload.insert(
        "evaluation".to_owned(),
        Json::String("account_stream_only".to_owned()),
    );
    let bytes = serde_json::to_vec(&Json::Object(body))
        .unwrap_or_else(|error| panic!("draft bytes: {error}"));
    let refusal = Draft::parse(&bytes).expect_err("evaluation is not registered");
    assert_eq!(refusal.reason, InvalidReason::Schema);
    assert_eq!(refusal.path, "payload.evaluation");
}

#[test]
fn binding_denial_sends_no_order_even_when_the_account_stream_checks_allow() {
    let gate = GateFixture::denying();
    let (mut shell, ports) = ready();
    let ran = shell.run_with_binding(
        handoff(INTENT, AGENT, opening(AAPL, "1", "150")),
        &ports,
        &gate,
    );
    assert!(!submitted(&ran));
    let decided = ran
        .draft("GateDecided")
        .unwrap_or_else(|| panic!("gate decision"));
    assert_eq!(
        decided.payload.get("verdict").and_then(|v| v.as_str()),
        Some("deny")
    );
    assert_eq!(
        decided.payload.get("reason_code").and_then(|v| v.as_str()),
        Some("instrument_halted")
    );
}

#[test]
fn missing_binding_input_fails_closed_before_any_submission() {
    let (mut shell, ports) = ready();
    let gate = GateFixture { input: None };
    let error = shell
        .step_with_binding(
            handoff(INTENT, AGENT, opening(AAPL, "1", "150")),
            &ports,
            &gate,
        )
        .expect_err("an opening without §9.1 inputs must fail closed");
    assert_eq!(error, ExecutorError::BindingGateInputMissing);
    assert_eq!(shell.connector.total_accepted(), 0);
}

#[test]
fn each_missing_config_reference_fails_closed_before_any_submission() {
    let cases: [(&str, fn(&mut BindingGateConfigRefs)); 5] = [
        ("fee_config", |refs| refs.fee_config = None),
        ("trading_calendar", |refs| refs.trading_calendar = None),
        ("instrument_snapshot", |refs| {
            refs.instrument_snapshot = None
        }),
        ("rule_set", |refs| refs.rule_set = None),
        ("mandate_version", |refs| refs.mandate_version = None),
    ];
    for (name, remove) in cases {
        let (mut shell, ports) = ready();
        let mut trusted = input(AccountState::Active, false);
        remove(&mut trusted.config_refs);
        let gate = GateFixture {
            input: Some(trusted),
        };
        let result = shell.step_with_binding(
            handoff(INTENT, AGENT, opening(AAPL, "1", "150")),
            &ports,
            &gate,
        );
        let error = result.expect_err(&format!("{name} must fail closed"));
        assert_eq!(error, ExecutorError::BindingGateInputMissing, "{name}");
        assert_eq!(shell.connector.total_accepted(), 0, "{name}");
    }
}

#[test]
fn a_mandate_reference_that_differs_from_the_resolved_version_fails_closed() {
    let (mut shell, ports) = ready();
    let mut trusted = input(AccountState::Active, false);
    trusted.config_refs.mandate_version = Some(format!("sha256:{}", "9".repeat(64)));
    let gate = GateFixture {
        input: Some(trusted),
    };
    let error = shell
        .step_with_binding(
            handoff(INTENT, AGENT, opening(AAPL, "1", "150")),
            &ports,
            &gate,
        )
        .expect_err("mismatched mandate evidence must fail closed");
    assert_eq!(error, ExecutorError::BindingGateInputMissing);
    assert_eq!(shell.connector.total_accepted(), 0);
}

#[test]
fn each_mismatched_binding_identity_fails_closed_before_any_submission() {
    for mismatch in ["instrument", "agent"] {
        let (mut shell, ports) = ready();
        let mut trusted = input(AccountState::Active, false);
        match mismatch {
            "instrument" => {
                trusted.asset =
                    AssetId::new("MSFT").unwrap_or_else(|error| panic!("asset: {error}"));
            }
            "agent" => trusted.gate_agent = AgentId(2),
            other => panic!("unknown mismatch {other}"),
        }
        let gate = GateFixture {
            input: Some(trusted),
        };
        let error = shell
            .step_with_binding(
                handoff(INTENT, AGENT, opening(AAPL, "1", "150")),
                &ports,
                &gate,
            )
            .expect_err("mismatched gate identities must fail closed");
        assert_eq!(error, ExecutorError::BindingGateInputMissing, "{mismatch}");
        assert_eq!(shell.connector.total_accepted(), 0, "{mismatch}");
    }
}

#[test]
fn a_binding_gate_error_fails_an_opening_but_does_not_block_a_risk_exit() {
    let outside_calendar = at("2027-01-02T15:00:00Z");

    let (mut opening_shell, opening_ports) = ready();
    let mut opening_input = input(AccountState::Active, false);
    opening_input.now = outside_calendar;
    opening_input.universe = WorkingUniverse::Unavailable;
    let opening_gate = GateFixture {
        input: Some(opening_input),
    };
    let error = opening_shell
        .step_with_binding(
            handoff(INTENT, AGENT, opening(AAPL, "1", "150")),
            &opening_ports,
            &opening_gate,
        )
        .expect_err("a binding-gate error must fail a risk-adding order closed");
    assert_eq!(
        error,
        ExecutorError::BindingGateFailed {
            code: "working_universe_unavailable"
        }
    );
    assert_eq!(opening_shell.connector.total_accepted(), 0);

    let (mut exit_shell, exit_ports) = ready_with_position();
    let mut exit_input = input_with_position();
    exit_input.now = outside_calendar;
    let exit_gate = GateFixture {
        input: Some(exit_input),
    };
    let ran = exit_shell.run_with_binding(
        handoff(INTENT, AGENT, risk_exit(AAPL, "10", "149")),
        &exit_ports,
        &exit_gate,
    );
    assert!(
        submitted(&ran),
        "rule 13 keeps the locally allowed risk exit routable when the binding gate cannot decide"
    );
}

#[test]
fn a_binding_deferral_is_journaled_as_defer_and_sends_no_order() {
    let (mut shell, ports) = ready_with_position();
    let mut trusted = input_with_position();
    trusted.now = at("2026-09-21T21:00:00Z");
    let gate = GateFixture {
        input: Some(trusted),
    };
    let ran = shell.run_with_binding(
        handoff(INTENT, AGENT, discretionary_exit(AAPL, "10", "149")),
        &ports,
        &gate,
    );
    assert!(!submitted(&ran));
    let decision = ran
        .draft("GateDecided")
        .unwrap_or_else(|| panic!("deferred gate decision"));
    assert_eq!(
        decision
            .payload
            .get("verdict")
            .and_then(mandate_canon::Value::as_str),
        Some("defer")
    );
    assert_eq!(
        decision
            .payload
            .get("reason_code")
            .and_then(mandate_canon::Value::as_str),
        Some("discretionary_exit_regular_session_only")
    );
}

#[test]
fn binding_pacing_changes_the_submitted_quantity_and_limit() {
    let (mut shell, ports) = ready_with_position();
    let mut trusted = input_with_position();
    trusted.market.trailing_5m_volume = Some(qty("100"));
    let gate = GateFixture {
        input: Some(trusted),
    };
    let ran = shell.run_with_binding(
        handoff(INTENT, AGENT, discretionary_exit(AAPL, "10", "100")),
        &ports,
        &gate,
    );
    let submitted = ran
        .submissions()
        .first()
        .copied()
        .unwrap_or_else(|| panic!("paced exit submission"));
    assert_eq!(submitted.qty, qty("5"));
    assert_eq!(submitted.limit_price, Some(price("148.49")));
}

#[test]
fn missing_binding_input_does_not_block_risk_reduction() {
    let (mut shell, ports) = ready_with_position();
    let gate = GateFixture { input: None };
    let ran = shell.run_with_binding(
        handoff(INTENT, AGENT, risk_exit(AAPL, "10", "149")),
        &ports,
        &gate,
    );
    assert!(
        submitted(&ran),
        "rule 13 keeps a locally allowed risk exit routable when external snapshots are unavailable"
    );
}

#[test]
fn replay_keeps_a_binding_denial_terminal() {
    let gate = GateFixture::denying();
    let (mut shell, ports) = ready();
    let first = shell.run_with_binding(
        handoff(INTENT, AGENT, opening(AAPL, "1", "150")),
        &ports,
        &gate,
    );
    assert!(!submitted(&first));
    let mut replayed = shell.restart_ready(&ports);
    let again = replayed.run(handoff(INTENT, AGENT, opening(AAPL, "1", "150")), &ports);
    assert!(again.effects.is_empty());
    assert_eq!(replayed.connector.total_accepted(), 0);
}

#[test]
fn an_advisory_allow_has_no_api_that_can_override_the_binding_denial() {
    let advisory = mandate_executor::GateVerdict::Allow;
    assert_eq!(advisory, mandate_executor::GateVerdict::Allow);
    let gate = GateFixture::denying();
    let (mut shell, ports) = ready();
    let ran = shell.run_with_binding(
        handoff(INTENT, AGENT, opening(AAPL, "1", "150")),
        &ports,
        &gate,
    );
    assert!(!submitted(&ran));
}
