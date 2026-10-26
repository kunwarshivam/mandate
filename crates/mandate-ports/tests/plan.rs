//! DEC-2605 row 20: the order builder's sizing as the shared port runs it. The base is the
//! reference cases' `two_stock_swing` mandate and its exit at the threshold (`mandate-builder`'s
//! `an_exit_at_the_threshold_sells_the_whole_position`): five shares held, both models at −0.3,
//! so the builder sells the whole position at the bid. Each refusal below changes one fact the
//! port checks and names the check that refuses it.

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_builder::{AccountSnapshot, ActionContext, Market, RequestedBy, RiskContext};
use mandate_canon::Digest;
use mandate_domain::{AssetId, Environment, MarketSession, Purpose as BuilderPurpose};
use mandate_num::{
    Conviction, CostBasis, FeeRate, MarkPrice, Price, Qty, Signed, SizeFraction, Unit, Usd,
};
use mandate_ports::{BuilderContext, PortError, size};
use mandate_runtime::{
    ExitOrigin, ModelDirection, ModelOutput, OrderExecution, Proposal, ProtectionPrices, Purpose,
    RiskClock, SignalInputs, TimeInForce,
};
use mandate_spec::document::ProvenanceMap;
use mandate_spec::{Mandate, ValidatedMandate, ValidationContext};
use mandate_time::{Date, UtcNanos};

const SWING: &str = "7b4a1c2e-2222-4a2b-9c3d-000000000002";
const OTHER: &str = "7b4a1c2e-3333-4a2b-9c3d-000000000003";
const NEWS: (&str, &str, &str) = (
    "llm.news_research",
    "0.3.0",
    "3333333333333333333333333333333333333333333333333333333333333333",
);
const MOMENTUM: (&str, &str, &str) = (
    "quant.momentum",
    "1.0.0",
    "2222222222222222222222222222222222222222222222222222222222222222",
);

fn secs(text: &str) -> i64 {
    UtcNanos::parse_rfc3339(text).unwrap().secs()
}

fn validated() -> ValidatedMandate {
    validated_base("two_stock_swing")
}

/// The reference cases' research base: an unpinned universe, so a thesis can end (DEC-2391).
fn researched() -> ValidatedMandate {
    validated_base("research_equity")
}

fn validated_base(base: &str) -> ValidatedMandate {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/refcases/mandate.json")).unwrap();
    let document = fixture.pointer(&format!("/bases/{base}/mandate")).unwrap();
    let canonical = mandate_canon::parse(&serde_json::to_vec(document).unwrap()).unwrap();
    let context = ValidationContext {
        account_equity_usd: Usd::parse("100000").unwrap(),
        other_allocations_usd: Usd::ZERO,
        validation_date: Date::parse("2026-09-20").unwrap(),
        registry: None,
        provenance: ProvenanceMap::default(),
        workspace_users: 2,
        approver_users: 2,
        independent_approval_required: false,
        disclosures_accepted: BTreeSet::new(),
        instrument_groups: BTreeMap::new(),
        claimed_by_other_agents: BTreeSet::new(),
        connection_environment: Some(Environment::Paper),
        connection_loss_carry_usd: Usd::ZERO,
        eligibility_failures: BTreeSet::new(),
        previous_version: None,
        current_mandate_version: None,
        stop_limit_asset_classes: mandate_spec::context::stop_limit_asset_classes("alpaca"),
    };
    ValidatedMandate::new(Mandate::parse(&canonical).unwrap(), &context, &[]).unwrap()
}

fn sell_execution() -> OrderExecution {
    OrderExecution {
        asset_class: AssetClass::UsEquity,
        tif: TimeInForce::Day,
        protection_required: false,
        protection: None,
    }
}

fn builder() -> BuilderContext {
    let usd = |text: &str| Usd::parse(text).unwrap();
    let instrument = AssetId::parse(SWING).unwrap();
    BuilderContext {
        account: AccountSnapshot {
            agent_equity: usd("10000"),
            position_qty: Qty::parse("5").unwrap(),
            cost_basis: CostBasis::parse("0").unwrap(),
            risk_mark: MarkPrice::parse("99.9").unwrap(),
            gross_usd: Usd::ZERO,
            working_opening_cost: Usd::ZERO,
            goal_spent_usd: Usd::ZERO,
        },
        market: Market {
            instrument: instrument.clone(),
            asset_class: AssetClass::UsEquity,
            session: MarketSession::Regular,
            in_close_window: false,
            bid: Price::parse("99.9").unwrap(),
            ask: Price::parse("100").unwrap(),
            increment: Qty::parse("1").unwrap(),
            min_order_usd: usd("1"),
            fee_rate_cash: FeeRate::parse("0").unwrap(),
            fee_rate_asset: FeeRate::parse("0").unwrap(),
        },
        risk: RiskContext {
            size_factor: SizeFraction::ONE,
            drawdown: Unit::ZERO,
            daily_pnl_fraction: Signed::ZERO,
            position_pnl_fraction: Signed::ZERO,
            bought_today_usd: Usd::ZERO,
            has_prior_fill: false,
            new_instrument: false,
            thesis_confidence: Unit::ZERO,
            risk_day: Date::parse("2026-09-22").unwrap(),
        },
        action: ActionContext {
            purpose: BuilderPurpose::DiscretionaryExit,
            order_usd: usd("499.5"),
            combined_score: Unit::ZERO,
            instrument,
            asset_class: AssetClass::UsEquity,
            session: MarketSession::Regular,
            first_trade_in_instrument: false,
            new_instrument: false,
            thesis_confidence: Unit::ZERO,
            drawdown: Unit::ZERO,
            daily_pnl_fraction: Signed::ZERO,
            position_usd_after: Usd::ZERO,
            gross_usd_after: Usd::ZERO,
            bought_today_usd: Usd::ZERO,
            position_pnl_fraction: Signed::ZERO,
            requested_by: RequestedBy::Agent,
            risk_day: Date::parse("2026-09-22").unwrap(),
        },
        model_content_hashes: [NEWS, MOMENTUM]
            .into_iter()
            .map(|(id, version, hash)| {
                (
                    (id.to_owned(), version.to_owned()),
                    Digest::from_hex(hash).unwrap(),
                )
            })
            .collect(),
        execution: sell_execution(),
    }
}

fn output((id, version, hash): (&str, &str, &str), instrument: &str) -> ModelOutput {
    ModelOutput {
        model_id: id.to_owned(),
        model_version: version.to_owned(),
        content_hash: Digest::from_hex(hash).unwrap(),
        instrument_id: InstrumentId::new(instrument).unwrap(),
        as_of: RiskClock::from_secs(secs("2026-09-22T13:59:00Z")),
        expires_at: RiskClock::from_secs(secs("2026-09-22T15:00:00Z")),
        direction: ModelDirection::Long,
        conviction: Conviction::parse("-0.3").unwrap(),
        confidence: Unit::parse("1").unwrap(),
        horizon_s: 3600,
        thesis_ref: None,
        evidence: Vec::new(),
        invalidation: None,
        thesis_id: None,
        lineage_id: None,
        ignored: None,
    }
}

/// Both models' outputs, each keyed by its own model and instrument.
fn inputs() -> SignalInputs {
    let swing = InstrumentId::new(SWING).unwrap();
    SignalInputs {
        outputs: [NEWS, MOMENTUM]
            .into_iter()
            .map(|model| {
                (
                    model.0.to_owned(),
                    BTreeMap::from([(swing.clone(), output(model, SWING))]),
                )
            })
            .collect(),
        output_events: BTreeMap::new(),
        now: RiskClock::from_secs(secs("2026-09-22T14:00:00Z")),
        ended_theses: Default::default(),
        working_exits: Default::default(),
    }
}

fn refused(result: Result<Option<Proposal>, PortError>) -> String {
    match result {
        Err(PortError::Absent { what }) => what.to_owned(),
        other => panic!("refused as absent, not {other:?}"),
    }
}

#[test]
fn the_exit_at_the_threshold_sells_the_whole_position_at_the_bid() {
    let proposal = size(&validated(), &builder(), &inputs()).unwrap().unwrap();
    assert_eq!(proposal.side, Side::Sell);
    assert_eq!(proposal.purpose, Purpose::DiscretionaryExit);
    assert_eq!(proposal.qty, Qty::parse("5").unwrap());
    assert_eq!(proposal.limit, Price::parse("99.9").unwrap());
    assert_eq!(proposal.execution, Some(sell_execution()));
}

#[test]
fn an_output_keyed_under_another_instrument_is_refused() {
    let mut inputs = inputs();
    let news = inputs.outputs.get_mut(NEWS.0).unwrap();
    let moved = news.remove(&InstrumentId::new(SWING).unwrap()).unwrap();
    news.insert(InstrumentId::new(OTHER).unwrap(), moved);
    assert_eq!(
        refused(size(&validated(), &builder(), &inputs)),
        "a consistently keyed model output"
    );
}

#[test]
fn an_output_keyed_under_another_model_is_refused() {
    let mut inputs = inputs();
    let news = inputs.outputs.remove(NEWS.0).unwrap();
    inputs.outputs.insert("quant.other".to_owned(), news);
    assert_eq!(
        refused(size(&validated(), &builder(), &inputs)),
        "a consistently keyed model output"
    );
}

#[test]
fn an_output_of_a_model_the_mandate_does_not_pin_is_refused() {
    let mut inputs = inputs();
    let mut news = inputs.outputs.remove(NEWS.0).unwrap();
    for output in news.values_mut() {
        output.model_id = "quant.other".to_owned();
    }
    inputs.outputs.insert("quant.other".to_owned(), news);
    let mut builder = builder();
    builder.model_content_hashes.insert(
        ("quant.other".to_owned(), NEWS.1.to_owned()),
        Digest::from_hex(NEWS.2).unwrap(),
    );
    assert_eq!(
        refused(size(&validated(), &builder, &inputs)),
        "a model output pinned by the validated mandate"
    );
}

#[test]
fn an_output_of_a_version_the_mandate_does_not_pin_is_refused() {
    let mut inputs = inputs();
    let news = inputs.outputs.get_mut(NEWS.0).unwrap();
    for output in news.values_mut() {
        output.model_version = "0.4.0".to_owned();
    }
    let mut builder = builder();
    builder.model_content_hashes.insert(
        (NEWS.0.to_owned(), "0.4.0".to_owned()),
        Digest::from_hex(NEWS.2).unwrap(),
    );
    assert_eq!(
        refused(size(&validated(), &builder, &inputs)),
        "a model output pinned by the validated mandate"
    );
}

#[test]
fn a_sell_that_would_require_protection_is_refused() {
    let mut builder = builder();
    builder.execution = OrderExecution {
        protection_required: true,
        protection: Some(ProtectionPrices {
            stop: Price::parse("90").unwrap(),
            take_profit: None,
        }),
        ..sell_execution()
    };
    assert_eq!(
        refused(size(&validated(), &builder, &inputs())),
        "the mandate-derived execution policy"
    );
}

#[test]
fn an_execution_policy_for_another_asset_class_is_refused() {
    let mut builder = builder();
    builder.execution = OrderExecution {
        asset_class: AssetClass::Crypto,
        tif: TimeInForce::Gtc,
        ..sell_execution()
    };
    assert_eq!(
        refused(size(&validated(), &builder, &inputs())),
        "the mandate-derived execution policy"
    );
}

/// `inputs` with SWING's thesis ended (mandate spec v0.10 §8.3 step 2, MI-34, DEC-2391).
fn ended(mut inputs: SignalInputs, instrument: &str) -> SignalInputs {
    inputs
        .ended_theses
        .insert(InstrumentId::new(instrument).unwrap());
    inputs
}

/// Both models now bullish: the signals alone would buy, never sell.
fn bullish() -> SignalInputs {
    let mut inputs = inputs();
    for by_instrument in inputs.outputs.values_mut() {
        for output in by_instrument.values_mut() {
            output.conviction = Conviction::parse("0.9").unwrap();
        }
    }
    inputs
}

#[test]
fn an_ended_thesis_sells_the_whole_position_at_the_bid_whatever_the_signals() {
    for (what, inputs) in [
        ("bullish outputs", bullish()),
        ("bearish outputs", inputs()),
        (
            "no outputs",
            SignalInputs {
                outputs: BTreeMap::new(),
                ..inputs()
            },
        ),
    ] {
        let proposal = size(&researched(), &builder(), &ended(inputs, SWING))
            .unwrap()
            .unwrap_or_else(|| panic!("{what}: proposed"));
        assert_eq!(proposal.side, Side::Sell, "{what}");
        assert_eq!(proposal.purpose, Purpose::DiscretionaryExit, "{what}");
        assert_eq!(
            proposal.exit_origin,
            Some(ExitOrigin::RemovedInstrument),
            "{what}"
        );
        assert_eq!(proposal.qty, Qty::parse("5").unwrap(), "{what}");
        assert_eq!(proposal.limit, Price::parse("99.9").unwrap(), "{what}");
        assert_eq!(proposal.execution, Some(sell_execution()), "{what}");
        assert!(
            proposal.outputs_used.is_empty(),
            "{what}: it read no output"
        );
        assert!(proposal.model_weights.is_empty(), "{what}: journal rule 8");
        assert!(proposal.clips_applied.is_empty(), "{what}: journal rule 8");
        assert_eq!(proposal.exit_conviction, None, "{what}: journal rule 8");
        assert_eq!(proposal.buy_conviction, None, "{what}: journal rule 8");
        assert_eq!(
            proposal.combined_score,
            mandate_canon::Value::Null,
            "{what}: journal rule 8"
        );
    }
}

#[test]
fn an_ended_thesis_needs_no_output_the_mandate_still_pins() {
    let mut inputs = inputs();
    for by_instrument in inputs.outputs.values_mut() {
        for output in by_instrument.values_mut() {
            output.model_version = "9.9.9".to_owned();
        }
    }
    assert_eq!(
        refused(size(&validated(), &builder(), &inputs)),
        "a model output pinned by the validated mandate",
        "the signal path refuses the unpinned output"
    );
    let proposal = size(&researched(), &builder(), &ended(inputs, SWING))
        .unwrap()
        .unwrap();
    assert_eq!(proposal.exit_origin, Some(ExitOrigin::RemovedInstrument));
}

#[test]
fn an_ended_thesis_with_nothing_held_proposes_nothing() {
    let mut flat = builder();
    flat.account.position_qty = Qty::ZERO;
    assert_eq!(
        size(&researched(), &flat, &ended(bullish(), SWING)).unwrap(),
        None
    );
}

#[test]
fn another_instruments_ended_thesis_leaves_the_signal_path() {
    let proposal = size(&validated(), &builder(), &ended(inputs(), OTHER))
        .unwrap()
        .unwrap();
    assert_eq!(proposal.exit_origin, Some(ExitOrigin::Signal));
    assert!(!proposal.outputs_used.is_empty());
}

#[test]
fn a_pinned_universe_takes_no_thesis_exit() {
    let signal = size(&validated(), &builder(), &ended(inputs(), SWING))
        .unwrap()
        .unwrap();
    assert_eq!(
        signal.exit_origin,
        Some(ExitOrigin::Signal),
        "two_stock_swing is pinned: the bearish signals sell, the ended thesis does not"
    );
    assert!(
        !matches!(
            size(&validated(), &builder(), &ended(bullish(), SWING)),
            Ok(Some(ref proposal)) if proposal.side == Side::Sell
        ),
        "and with bullish signals a pinned universe never sells for an ended thesis"
    );
}

/// `inputs` with a working exit of `qty` in `instrument` (§8.3 step 2, "minus working exits").
fn working(mut inputs: SignalInputs, instrument: &str, qty: &str) -> SignalInputs {
    inputs.working_exits.insert(
        InstrumentId::new(instrument).unwrap(),
        Qty::parse(qty).unwrap(),
    );
    inputs
}

#[test]
fn an_ended_thesis_sells_only_what_no_working_exit_already_sells() {
    let part = size(
        &researched(),
        &builder(),
        &working(ended(bullish(), SWING), SWING, "2"),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        part.qty,
        Qty::parse("3").unwrap(),
        "5 held, 2 already working"
    );
    assert_eq!(part.exit_origin, Some(ExitOrigin::RemovedInstrument));
    for covered in ["5", "7"] {
        assert_eq!(
            size(
                &researched(),
                &builder(),
                &working(ended(bullish(), SWING), SWING, covered)
            )
            .unwrap(),
            None,
            "{covered} working covers the 5 held: nothing is stacked"
        );
    }
    let elsewhere = size(
        &researched(),
        &builder(),
        &working(ended(bullish(), SWING), OTHER, "5"),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        elsewhere.qty,
        Qty::parse("5").unwrap(),
        "another instrument's working exit takes nothing off"
    );
}
