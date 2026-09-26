//! Fixtures and an independent oracle for the gate's tests.
//!
//! The oracle in [`oracle`] recomputes every mandate limit in `i128` at 10^-9 from its own
//! accumulators. It never calls `mandate-risk`'s arithmetic and never reads `Decision::checks`, so
//! a property comparing against it cannot pass because the gate and the oracle share a mistake.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::collections::{BTreeMap, BTreeSet};

use mandate_num::{Fraction, Price, Qty, Usd};
use mandate_risk::spec_types::{GoalState, RiskLimits, Rung, RungAction, ScaleAction};
use mandate_risk::{
    AccountSnapshot, AccountState, AccountType, AgentId, AgentMode, AgentSnapshot, AssetClass,
    AssetId, ClientOrderId, ConductState, DayTradeLedger, DayTradeRegime, EtpClass, Exchange,
    GateConfig, GateInput, GatePass, InstrumentSnapshot, MarketSnapshot, Origin, ProposedKind,
    ProposedOrder, RiskSnapshot, Side, TimeInForce, ValidatedMandate, WorkingOrder,
    WorkingUniverse,
};
use mandate_time::{Date, UtcNanos};

/// The nine-place scale every money and quantity figure is compared at in the oracle.
pub const SCALE: i128 = 1_000_000_000;

/// `mandate.yaml`'s `two_stock_swing` instruments, which the G family uses.
pub const INSTRUMENT_2: &str = "7b4a1c2e-2222-4a2b-9c3d-000000000002";
pub const INSTRUMENT_3: &str = "7b4a1c2e-3333-4a2b-9c3d-000000000003";
pub const INSTRUMENT_4: &str = "7b4a1c2e-4444-4a2b-9c3d-000000000004";
pub const INSTRUMENT_1: &str = "7b4a1c2e-1111-4a2b-9c3d-000000000001";

#[must_use]
pub fn asset(id: &str) -> AssetId {
    AssetId::new(id).unwrap_or_else(|e| panic!("a test asset id is well formed: {e}"))
}

#[must_use]
pub fn usd(text: &str) -> Usd {
    Usd::parse(text).unwrap_or_else(|e| panic!("a test dollar figure parses: {e}"))
}

#[must_use]
pub fn price(text: &str) -> Price {
    Price::parse(text).unwrap_or_else(|e| panic!("a test price parses: {e}"))
}

#[must_use]
pub fn qty(text: &str) -> Qty {
    Qty::parse(text).unwrap_or_else(|e| panic!("a test quantity parses: {e}"))
}

#[must_use]
pub fn fraction(text: &str) -> Fraction {
    Fraction::parse(text).unwrap_or_else(|e| panic!("a test fraction parses: {e}"))
}

#[must_use]
pub fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap_or_else(|e| panic!("a test instant parses: {e}"))
}

/// `configs.test_default.gate` from the trading-domain reference cases.
#[must_use]
pub fn test_default_config() -> GateConfig {
    GateConfig {
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
    }
}

/// `two_stock_swing`'s risk block, read from `reference/mandate/bases.py` rather than guessed: the
/// base `RISK` with the three overrides line 82 applies. `max_position_fraction` is 0.2, so the
/// dollar cap binds at 10000 equity; `reentry_cooldown_s` is 3600, which is why MC-G11 denies at
/// 1800 s elapsed and MC-G13 allows at 3600 s.
#[must_use]
pub fn two_stock_swing_limits() -> RiskLimits {
    RiskLimits {
        max_position_usd: usd("1500"),
        max_position_fraction: fraction("0.2"),
        max_order_usd: usd("1000"),
        max_gross_exposure_usd: usd("2000"),
        max_orders_per_day: 50,
        reentry_cooldown_s: 3600,
        rebalance_band: fraction("0.05"),
        drawdown_ladder: vec![
            Rung {
                index: 0,
                at: fraction("0.03"),
                action: RungAction::ScaleSizes,
                factor: Some(fraction("0.5")),
                scale_action: Some(ScaleAction::LimitBuys),
            },
            Rung {
                index: 1,
                at: fraction("0.06"),
                action: RungAction::ExitsOnly,
                factor: None,
                scale_action: Some(ScaleAction::LimitBuys),
            },
            Rung {
                index: 2,
                at: fraction("0.08"),
                action: RungAction::FlattenAndPause,
                factor: None,
                scale_action: Some(ScaleAction::LimitBuys),
            },
        ],
    }
}

/// A ladder whose two `scale_sizes` factors are 0.5 and 0.4: their product is 0.2 and their sum is
/// 0.9, so a test over it tells multiplication from addition (planted bug 38).
#[must_use]
pub fn two_scaling_rungs() -> RiskLimits {
    let mut limits = two_stock_swing_limits();
    limits.drawdown_ladder = vec![
        Rung {
            index: 0,
            at: fraction("0.03"),
            action: RungAction::ScaleSizes,
            factor: Some(fraction("0.5")),
            scale_action: Some(ScaleAction::LimitBuys),
        },
        Rung {
            index: 1,
            at: fraction("0.06"),
            action: RungAction::ScaleSizes,
            factor: Some(fraction("0.4")),
            scale_action: Some(ScaleAction::LimitBuys),
        },
    ];
    limits
}

#[must_use]
pub fn mandate_with(limits: RiskLimits) -> ValidatedMandate {
    ValidatedMandate::from_validated_parts(limits, GoalState::Running, false, false)
}

#[must_use]
pub fn healthy_risk(equity: &str) -> RiskSnapshot {
    RiskSnapshot {
        agent_equity: usd(equity),
        high_water_mark: usd(equity),
        day_start_equity: usd(equity),
        capital_base: usd(equity),
        inherited_loss: Usd::ZERO,
        latched: BTreeSet::new(),
        active_rungs: BTreeSet::new(),
        size_factor: Fraction::ONE,
        rung_active_for_s: BTreeMap::new(),
        agent_mode: AgentMode::Normal,
    }
}

#[must_use]
pub fn equity_instrument(id: &str) -> InstrumentSnapshot {
    InstrumentSnapshot {
        instrument: asset(id),
        asset_class: AssetClass::UsEquity,
        exchange: Some(Exchange::Nasdaq),
        status_active: true,
        tradable: true,
        fractionable: false,
        ipo: false,
        ptp_no_exception: false,
        etp: EtpClass::Plain,
        etp_classified_at: Some(at("2026-09-21T00:00:00Z")),
        prior_close: Some(price("100")),
        median_dollar_volume_20d: Some(usd("90000000")),
        median_dollar_volume_30d: None,
        min_order_size: qty("1"),
        halted: false,
        status_feed_current: true,
    }
}

#[must_use]
pub fn healthy_account(equity: &str) -> AccountSnapshot {
    AccountSnapshot {
        account_type: AccountType::Margin,
        state: AccountState::Active,
        crypto_active: true,
        regime: DayTradeRegime::IntradayMargin {
            maintenance_excess: usd("100000"),
        },
        equity: usd(equity),
        prior_close_equity: usd(equity),
        model_buying_power: usd("1000000"),
        broker_buying_power: usd("1000000"),
        broker_non_marginable_buying_power: usd("1000000"),
        positions: BTreeMap::new(),
        market_values: BTreeMap::new(),
        working_orders: BTreeMap::new(),
        unknown_orders: BTreeSet::new(),
        related_account_resting: BTreeMap::new(),
    }
}

#[must_use]
pub fn healthy_agent() -> AgentSnapshot {
    AgentSnapshot {
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
    }
}

#[must_use]
pub fn working_universe(ids: &[&str]) -> WorkingUniverse {
    WorkingUniverse::Known {
        instruments: ids.iter().map(|i| asset(i)).collect(),
        pinned: true,
    }
}

#[must_use]
pub fn proposal(
    instrument: &str,
    side: Side,
    q: &str,
    limit: &str,
    origin: Origin,
) -> ProposedOrder {
    ProposedOrder {
        instrument: asset(instrument),
        side,
        qty: qty(q),
        limit_price: price(limit),
        kind: ProposedKind::Plain,
        tif: TimeInForce::Day,
        extended_hours: false,
        origin,
        owner_confirmed_bid: None,
        client_order_id: ClientOrderId(1),
        fee_reservation: Usd::ZERO,
    }
}

#[must_use]
pub fn open_order(agent: AgentId, instrument: &str, max_cost: &str) -> WorkingOrder {
    WorkingOrder {
        agent,
        instrument: asset(instrument),
        side: Side::Buy,
        max_cost: usd(max_cost),
        open_qty: qty("1"),
        protective: false,
        opening: true,
        submitted_on: Date::parse("2026-09-21")
            .unwrap_or_else(|e| panic!("a test date parses: {e}")),
    }
}

/// Everything an [`GateInput`] needs, owned, so a test can build one and hand out references.
pub struct Scenario {
    pub now: UtcNanos,
    pub pass: GatePass,
    pub config: GateConfig,
    pub mandate: ValidatedMandate,
    pub risk: RiskSnapshot,
    pub account: AccountSnapshot,
    pub agent: AgentSnapshot,
    pub instrument: InstrumentSnapshot,
    pub market: MarketSnapshot,
    pub conduct: ConductState,
    pub universe: WorkingUniverse,
    pub proposed: ProposedOrder,
}

impl Scenario {
    /// A scenario in which every check passes, so a test changes exactly the one thing it is about.
    #[must_use]
    pub fn allowing() -> Self {
        Self {
            now: at("2026-09-21T15:00:00Z"),
            pass: GatePass::First,
            config: test_default_config(),
            mandate: mandate_with(two_stock_swing_limits()),
            risk: healthy_risk("10000"),
            account: healthy_account("10000"),
            agent: healthy_agent(),
            instrument: equity_instrument(INSTRUMENT_3),
            market: MarketSnapshot {
                quote: Some(mandate_risk::SaneQuote {
                    bid: price("99.95"),
                    ask: price("100.05"),
                    at: at("2026-09-21T14:59:59Z"),
                }),
                last_trade: Some((price("100"), at("2026-09-21T14:59:59Z"))),
                trailing_5m_volume: Some(qty("100000")),
                adv_20d: Some(qty("1000000")),
            },
            conduct: ConductState::default(),
            universe: working_universe(&[INSTRUMENT_2, INSTRUMENT_3]),
            proposed: proposal(INSTRUMENT_3, Side::Buy, "1", "100", Origin::OrderBuilder),
        }
    }

    #[must_use]
    pub fn input(&self) -> GateInput<'_> {
        GateInput {
            now: self.now,
            pass: self.pass,
            config: &self.config,
            mandate: &self.mandate,
            risk: &self.risk,
            account: &self.account,
            agent: &self.agent,
            instrument: &self.instrument,
            market: &self.market,
            conduct: &self.conduct,
            universe: &self.universe,
            proposed: &self.proposed,
        }
    }
}

/// The independent oracle: every mandate limit recomputed in `i128` at 10^-9, from accumulators
/// this module keeps itself.
pub mod oracle {
    use super::{BTreeMap, SCALE};

    /// A decimal text to scaled `i128`, so the oracle never uses `mandate-num`.
    ///
    /// # Panics
    /// Panics on text a test fixture should never contain.
    #[must_use]
    pub fn scaled(text: &str) -> i128 {
        let (sign, body) = match text.strip_prefix('-') {
            Some(rest) => (-1_i128, rest),
            None => (1_i128, text),
        };
        let (int, frac) = match body.split_once('.') {
            Some((i, f)) => (i, f),
            None => (body, ""),
        };
        assert!(frac.len() <= 9, "the oracle holds nine places: {text}");
        let mut padded = frac.to_owned();
        while padded.len() < 9 {
            padded.push('0');
        }
        let whole: i128 = int
            .parse()
            .unwrap_or_else(|_| panic!("integer part: {text}"));
        let part: i128 = if padded.is_empty() {
            0
        } else {
            padded
                .parse()
                .unwrap_or_else(|_| panic!("fraction: {text}"))
        };
        sign * (whole * SCALE + part)
    }

    /// A shadow ledger the fuzz accumulates itself, so a split sequence cannot walk past a limit
    /// without the oracle noticing.
    #[derive(Debug, Default, Clone)]
    pub struct ShadowLedger {
        pub market_value: BTreeMap<String, i128>,
        pub working_cost: BTreeMap<String, i128>,
        pub opening_orders_today: u32,
        pub last_exit_fill_s: BTreeMap<String, i64>,
    }

    impl ShadowLedger {
        /// The per-instrument total §5.3 compares with the cap: position + working + proposed.
        #[must_use]
        pub fn instrument_total(&self, instrument: &str, proposed: i128) -> i128 {
            self.market_value.get(instrument).copied().unwrap_or(0)
                + self.working_cost.get(instrument).copied().unwrap_or(0)
                + proposed
        }

        /// Σ |MV| + Σ working + the proposed order.
        #[must_use]
        pub fn gross(&self, proposed: i128) -> i128 {
            self.market_value.values().map(|v| v.abs()).sum::<i128>()
                + self.working_cost.values().sum::<i128>()
                + proposed
        }

        /// An allowed opening, applied as if it filled in full at its limit price.
        pub fn apply_opening(&mut self, instrument: &str, notional: i128) {
            *self.market_value.entry(instrument.to_owned()).or_default() += notional;
            self.opening_orders_today += 1;
        }
    }

    /// min(`max_position_usd`, `max_position_fraction` × equity), exact at nine places.
    #[must_use]
    pub fn position_cap(max_usd: i128, fraction_scaled: i128, equity: i128) -> i128 {
        let by_fraction = fraction_scaled * equity / SCALE;
        max_usd.min(by_fraction)
    }

    /// min(`max_gross_exposure_usd`, equity).
    #[must_use]
    pub fn gross_limit(max_usd: i128, equity: i128) -> i128 {
        max_usd.min(equity)
    }

    /// Which mandate limit an allowed order would have exceeded, named so a failure says which.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Breach {
        Concentration,
        OrderSize,
        GrossExposure,
        OrdersPerDay,
        ReentryCooldown,
        NotInUniverse,
    }

    /// The §5.3 limits, each compared with `>` so a value exactly at the limit passes (MC-G02).
    #[must_use]
    pub fn breached(
        instrument_total: i128,
        cap: i128,
        order: i128,
        max_order: i128,
        gross: i128,
        gross_cap: i128,
    ) -> Option<Breach> {
        if instrument_total > cap {
            Some(Breach::Concentration)
        } else if order > max_order {
            Some(Breach::OrderSize)
        } else if gross > gross_cap {
            Some(Breach::GrossExposure)
        } else {
            None
        }
    }
}
