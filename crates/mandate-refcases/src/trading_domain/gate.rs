//! The gate driver of the `trading_domain` suite (E6-9, DEC-199): each `propose_order` step is
//! decided by `mandate_risk::evaluate` and its `decision` compared, and the account's trading
//! status — trading-domain spec §7.3's first row, from `initial.account` and from
//! `broker_account_update` steps — is the account state the gate's check 1 reads. Nothing the gate
//! decides is computed here.
//!
//! **What the harness fills, and why (DEC-199).** The case file's header states the scene every
//! gate case assumes, and the harness builds that scene and nothing more:
//!
//! - **The account's status** is §7.3's first row: any `status` other than `ACTIVE`, or any of
//!   `trading_blocked`, `account_blocked` and `trade_suspended_by_user`, is `blocked`. A detected
//!   restriction is stored "until the owner acknowledges and the account is refreshed", so a later
//!   `ACTIVE` never lifts it here; lifting it needs `owner_ack` (E7-5).
//! - **Each proposal is decided alone**, against the account the case's other steps build. An
//!   allowed proposal places no working order, reserves nothing and counts toward no conduct
//!   figure: that is its submission, which `actions` (E7-4) and the account ledger (E7-5) own.
//!   RC-25 is written this way — its steps 3 and 4 both sell the whole position.
//! - **One agent holds the whole account** (header: "one agent `agent_a` whose universe is every
//!   instrument"), in mode `normal`. The case's `purpose` picks the proposer's [`Origin`] as
//!   DEC-178 does, the step's `side` is the order's side, and the gate assigns the purpose itself.
//! - **No mandate limit binds**: the trading-domain cases state no mandate and the header says "no
//!   concentration cap", so the position cap is the account's equity and every other limit is out
//!   of reach. The account's own 1× gross bound, buying power and every account rule still apply.
//! - **The market** is DEC-178's: a fresh, sane quote at `at` with bid and ask at the limit price
//!   (the header's midpoint), or the step's `quote`; the last trade at the limit price; volumes
//!   that pass every conduct check; a tradable, unhalted listing on the case's `exchange` that
//!   last closed at the limit price with a 90,000,000 median, one share its minimum order, and
//!   fractionable only when the case says so.
//! - **Marks** are the account's own (the latest mark, else the last fill). A held instrument with
//!   neither, when it is the one proposed, is marked at the limit price on a copy of the account;
//!   any other unmarked position, and an unmarked one whose step states a quote, is refused.
//! - **Buying power** is the model's with no reservations, and the broker's figures equal it (the
//!   header). A margin account is `intraday_margin`, the alpaca profile's (the header); a cash
//!   account is handed `legacy_pdt`, which check 8 never reads for one. The fee reservation is
//!   `round(fees, 2, ceiling)` of the proposal filled on a copy of the account (§2.1); a sell
//!   reserves nothing, since check 7 reads it for an opening and check 4 denies every opening
//!   sell.
//! - **The configuration** is the case's resolved `gate` block, with DEC-178's values for the five
//!   settings `test_default` does not carry.
//! - **A proposal's form**: `type` must be `limit`; an absent `tif` is `day` for whole shares and
//!   refused for a fractional quantity (§5.3 rule 7 makes `day` the only TIF a fractional order
//!   may have); no proposal asks for extended hours.

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::{
    Account, AccountType, AssetClass, Config, Execution, Input, Reservations, Side,
};
use mandate_num::{Fraction, Price, Qty, Ratio, Rounding, ShareIncrement, Usd};
use mandate_risk::spec_types::{GoalState, RiskLimits};
use mandate_risk::{
    AccountSnapshot, AccountState, AgentId, AgentMode, AgentSnapshot, AssetId, ClientOrderId,
    ConductState, DayTradeLedger, DayTradeRegime, Decision, EtpClass, Exchange, GateConfig,
    GateError, GateInput, GatePass, InstrumentSnapshot, MarketSnapshot, Origin, ProposedKind,
    ProposedOrder, QuoteCurrency, ReasonCode, RiskSnapshot, SaneQuote, TimeInForce,
    ValidatedMandate, Verdict, WorkingUniverse, evaluate,
};
use mandate_time::UtcNanos;

use super::{Instruments, acct, dec_at, fields, instrument, num, resolved_config};
use crate::{Json, at, ensure, list_at, str_at, u64_at};

/// §7.3's account fields, read from `initial.account` and from `broker_account_update` data.
/// `crypto_status` is not among them: it gates only a crypto opening, which E6-10 still owes.
pub(super) const STATUS_FIELDS: &[&str] = &[
    "status",
    "trading_blocked",
    "account_blocked",
    "trade_suspended_by_user",
];

/// The `propose_order` members this driver reads; the rest are owned by later stories.
const PROPOSAL_KEYS: &[&str] = &[
    "name",
    "instrument",
    "side",
    "type",
    "qty",
    "limit_price",
    "purpose",
    "tif",
    "quote",
];

/// The `gate` block's members. The executor's settings (the 403 threshold, the watchdog, the
/// ladder, the protective timers) are read by the executor's own drivers, not by the gate.
const GATE_CONFIG_KEYS: &[&str] = &[
    "data_profile",
    "price_floor",
    "liquidity_floor_usd",
    "collar",
    "opposite_fill_interval_s",
    "order_to_fill",
    "unexplained_403_threshold",
    "crypto_stop_limit_offset",
    "stop_watchdog_s",
    "close_window_minutes",
    "protective_replace_buffer_trading_days",
    "bracket_partial_fill_timeout_s",
    "max_unprotected_s",
    "exit_ladder",
    "legacy_pdt_equity_threshold",
];

/// The one agent every trading-domain case is about.
const THE_AGENT: AgentId = AgentId(1);

/// A dollar figure no trading-domain case reaches, for the mandate limits no case states.
const OUT_OF_REACH_USD: &str = "1000000000000000";

/// The gate's inputs that last across steps: the organisation's settings, the registered reason
/// codes, each instrument's listing, and the account's state.
pub(super) struct Gate {
    config: GateConfig,
    registered: BTreeSet<String>,
    exchanges: BTreeMap<String, Exchange>,
    state: AccountState,
}

impl Gate {
    pub(super) fn read(fixture: &Json, case: &Json) -> Result<Self, String> {
        let registered = list_at(fixture, "reason_codes")?
            .iter()
            .map(|code| {
                code.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "a registered reason code is not a string".to_owned())
            })
            .collect::<Result<_, String>>()?;
        let mut exchanges = BTreeMap::new();
        let listed = at(case, "instruments")?
            .as_object()
            .ok_or("`instruments` is not an object")?;
        for (name, listing) in listed {
            if let Some(code) = listing.get("exchange") {
                let code = code
                    .as_str()
                    .ok_or_else(|| format!("instrument `{name}`: `exchange` is not a string"))?;
                exchanges.insert(name.clone(), exchange_named(code)?);
            }
        }
        let mut gate = Self {
            config: gate_config(at(&resolved_config(fixture, case)?, "gate")?)?,
            registered,
            exchanges,
            state: AccountState::Active,
        };
        let account = at(case, "initial.account")?
            .as_object()
            .ok_or("`initial.account` is not an object")?;
        for (key, value) in account {
            if STATUS_FIELDS.contains(&key.as_str()) {
                gate.observe(key, value)?;
            }
        }
        Ok(gate)
    }

    /// A `broker_account_update` step: §7.3's first row, which only ever tightens the state.
    pub(super) fn account_update(&mut self, data: &Json) -> Result<(), String> {
        for (key, value) in fields(data, "broker_account_update data", STATUS_FIELDS)? {
            self.observe(key, value)?;
        }
        Ok(())
    }

    fn observe(&mut self, key: &str, value: &Json) -> Result<(), String> {
        let blocks = if key == "status" {
            value
                .as_str()
                .ok_or_else(|| format!("account `{key}` is not a string"))?
                != "ACTIVE"
        } else {
            value
                .as_bool()
                .ok_or_else(|| format!("account `{key}` is not a boolean"))?
        };
        if blocks {
            self.state = AccountState::Blocked;
        }
        Ok(())
    }

    /// One `propose_order` step, decided by `mandate_risk::evaluate` in the scene the module doc
    /// describes.
    pub(super) fn decide(
        &self,
        account: &Account,
        accounting: &Config,
        instruments: &Instruments,
        data: &Json,
        step: usize,
        now: UtcNanos,
    ) -> Result<Decision, String> {
        let d = fields(data, "propose_order data", PROPOSAL_KEYS)?;
        if let Some(name) = d.get("name") {
            name.as_str().ok_or("`name` is not a string")?;
        }
        let (name, id, class, fractionable) = instrument(instruments, str_at(data, "instrument")?)?;
        ensure(*class == AssetClass::UsEquity, || {
            format!("`{name}` is not a US equity; a crypto proposal waits for E6-10")
        })?;
        let side = match str_at(data, "side")? {
            "buy" => Side::Buy,
            "sell" => Side::Sell,
            other => return Err(format!("unknown side `{other}`")),
        };
        let order_type = str_at(data, "type")?;
        ensure(order_type == "limit", || {
            format!("order type `{order_type}` is not interpreted: every proposal is a limit order")
        })?;
        let qty = num(Qty::parse(dec_at(data, "qty")?.as_str()), "qty")?;
        let limit_price = num(
            Price::parse(dec_at(data, "limit_price")?.as_str()),
            "limit_price",
        )?;
        let origin = origin_named(str_at(data, "purpose")?)?;
        let whole = num(qty.portion(Fraction::ONE, ShareIncrement::Whole), "qty")? == qty;
        let tif = match d.get("tif").map(Json::as_str) {
            None if whole => TimeInForce::Day,
            None => return Err("a fractional proposal states no `tif`".to_owned()),
            Some(Some("day")) => TimeInForce::Day,
            Some(Some("gtc")) => TimeInForce::Gtc,
            Some(other) => return Err(format!("unknown tif {other:?}")),
        };
        let (bid, ask) = match d.get("quote") {
            None => (limit_price, limit_price),
            Some(quote) => {
                fields(quote, "quote", &["bid", "ask"])?;
                (
                    num(Price::parse(dec_at(quote, "bid")?.as_str()), "quote.bid")?,
                    num(Price::parse(dec_at(quote, "ask")?.as_str()), "quote.ask")?,
                )
            }
        };
        let exchange = *self.exchanges.get(name).ok_or_else(|| {
            format!("instrument `{name}` states no `exchange`, which the eligibility floor reads")
        })?;

        let mut marked = account.clone();
        if account.mark(id).is_none() && account.positions().any(|(held, _)| held == id) {
            ensure(d.get("quote").is_none(), || {
                format!(
                    "`{name}` is held with no mark and its step states a quote, whose midpoint the harness does not compute"
                )
            })?;
            marked = acct(marked.apply(
                &Input::Mark {
                    instrument: id.clone(),
                    price: limit_price,
                },
                accounting,
            ))?
            .account;
        }
        let mut positions = BTreeMap::new();
        let mut market_values = BTreeMap::new();
        for (held, position) in marked.positions() {
            let held_name = held.as_str();
            ensure(!position.qty().is_negative(), || {
                format!("`{held_name}` is held short, which no v1 gate snapshot holds")
            })?;
            let mark = marked.mark(held).ok_or_else(|| {
                format!("`{held_name}` is held with no mark, so its market value is unknown")
            })?;
            let asset = asset(held_name)?;
            market_values.insert(
                asset.clone(),
                num(position.qty().value_at_mark(mark), "market value")?,
            );
            positions.insert(asset, position.qty().abs());
        }
        let equity = acct(marked.equity())?;
        let buying_power = acct(marked.buying_power(Reservations::NONE))?;
        let fee_reservation = match side {
            Side::Sell => Usd::ZERO,
            Side::Buy => {
                let filled = acct(marked.apply(
                    &Input::Fill(Execution {
                        fill_id: format!("fee_reservation_{step}"),
                        client_order_id: None,
                        instrument: id.clone(),
                        asset_class: *class,
                        side,
                        qty_gross: qty,
                        price: limit_price,
                        liquidity: None,
                        executed_at: now,
                    }),
                    accounting,
                ))?
                .account;
                let fees = num(
                    acct(filled.fees_accrued())?.checked_sub(acct(marked.fees_accrued())?),
                    "fee reservation",
                )?;
                num(fees.round(2, Rounding::Ceiling), "fee reservation")?
            }
        };

        let proposed = asset(name)?;
        let universe = WorkingUniverse::Known {
            instruments: instruments
                .iter()
                .map(|(listed, _, _, _)| asset(listed))
                .collect::<Result<_, _>>()?,
            pinned: false,
        };
        let one_share = num(Qty::parse("1"), "one share")?;
        let usd = |text: &str| num(Usd::parse(text), "a harness figure");
        let risk = RiskSnapshot {
            agent_equity: equity,
            high_water_mark: equity,
            day_start_equity: equity,
            capital_base: equity,
            inherited_loss: Usd::ZERO,
            latched: BTreeSet::new(),
            active_rungs: BTreeMap::new(),
            size_factor: num(Ratio::parse("1"), "size_factor")?,
            agent_mode: AgentMode::Normal,
        };
        let regime = match account.account_type() {
            AccountType::Margin => DayTradeRegime::IntradayMargin {
                maintenance_excess: equity,
            },
            AccountType::Cash => DayTradeRegime::LegacyPdt,
        };
        let account_snapshot = AccountSnapshot {
            account_type: account.account_type(),
            state: self.state,
            crypto_active: true,
            regime,
            equity,
            prior_close_equity: equity,
            model_buying_power: buying_power,
            broker_buying_power: buying_power,
            broker_non_marginable_buying_power: buying_power,
            positions: positions.clone(),
            market_values: market_values.clone(),
            working_orders: BTreeMap::new(),
            unknown_orders: BTreeSet::new(),
            related_account_resting: BTreeMap::new(),
        };
        let agent = AgentSnapshot {
            agent: THE_AGENT,
            mode: AgentMode::Normal,
            instrument_restrictions: BTreeMap::new(),
            positions,
            market_values,
            working_orders: BTreeSet::new(),
            instrument_groups: BTreeMap::new(),
            last_exit_fill_at: BTreeMap::new(),
            orders_today: 0,
            day_trades: DayTradeLedger::default(),
        };
        let listing = InstrumentSnapshot {
            instrument: proposed.clone(),
            asset_class: AssetClass::UsEquity,
            exchange: Some(exchange),
            status_active: true,
            tradable: true,
            fractionable: fractionable.unwrap_or(false),
            ipo: false,
            ptp_no_exception: false,
            etp: EtpClass::Plain,
            etp_classified_at: Some(now),
            quote_currency: Some(QuoteCurrency::Usd),
            prior_close: Some(limit_price),
            median_dollar_volume_20d: Some(usd("90000000")?),
            median_dollar_volume_30d: None,
            min_order_size: one_share,
            halted: false,
            status_feed_current: true,
        };
        let market = MarketSnapshot {
            quote: Some(SaneQuote { bid, ask, at: now }),
            last_trade: Some((limit_price, now)),
            trailing_5m_volume: Some(num(Qty::parse("1000000"), "trailing_5m_volume")?),
            adv_20d: Some(num(Qty::parse("10000000"), "adv_20d")?),
        };
        let out_of_reach = usd(OUT_OF_REACH_USD)?;
        let mandate = ValidatedMandate::from_validated_parts(
            RiskLimits {
                max_position_usd: out_of_reach,
                max_position_fraction: Fraction::ONE,
                max_order_usd: out_of_reach,
                max_gross_exposure_usd: out_of_reach,
                max_orders_per_day: u32::MAX,
                reentry_cooldown_s: 0,
                rebalance_band: Fraction::ZERO,
                breach_confirm_s: 0,
                drawdown_ladder: Vec::new(),
            },
            GoalState::Running,
            false,
            false,
        );
        let order = ProposedOrder {
            instrument: proposed,
            side,
            qty,
            limit_price,
            kind: ProposedKind::Plain,
            tif,
            extended_hours: false,
            origin,
            owner_confirmed_bid: None,
            client_order_id: ClientOrderId(u64::try_from(step).map_err(|e| e.to_string())?),
            fee_reservation,
        };
        evaluate(&GateInput {
            now,
            pass: GatePass::First,
            config: &self.config,
            mandate: &mandate,
            risk: &risk,
            account: &account_snapshot,
            agent: &agent,
            instrument: &listing,
            market: &market,
            conduct: &ConductState::default(),
            universe: &universe,
            proposed: &order,
        })
        .map_err(|e| gate_error(&e))
    }

    /// The step's `decision` against the gate's: the verdict, and the reason code when the case
    /// states one, which must be a code the case file registers.
    pub(super) fn check_decision(
        &self,
        decision: Option<&Decision>,
        expected: &Json,
    ) -> Result<(), String> {
        let decision =
            decision.ok_or("`decision` expected on a step that made no gate decision")?;
        let stated = fields(expected, "decision", &["verdict", "reason_code"])?;
        for key in ["verdict", "reason_code"] {
            let Some(value) = stated.get(key) else {
                continue;
            };
            let wanted = value
                .as_str()
                .ok_or_else(|| format!("`decision.{key}` is not a string"))?;
            let actual = if key == "verdict" {
                verdict_name(decision.verdict)
            } else {
                ensure(self.registered.contains(wanted), || {
                    format!("reason code `{wanted}` is not registered in the case file")
                })?;
                decision.reason.map_or("none", ReasonCode::as_str)
            };
            ensure(actual == wanted, || {
                format!("decision.{key}: expected {wanted}, got {actual}")
            })?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn restrict(&mut self, state: AccountState) {
        self.state = state;
    }
}

/// `configs.<name>.gate` as the gate's settings, with DEC-178's values for the five it does not
/// carry: the crypto liquidity floor, the minimum resting time, the two participation caps and the
/// ETP classification age.
fn gate_config(gate: &Json) -> Result<GateConfig, String> {
    fields(gate, "config gate", GATE_CONFIG_KEYS)?;
    let collar = at(gate, "collar")?;
    fields(
        collar,
        "config gate.collar",
        &[
            "liquid_threshold_usd",
            "liquid_x",
            "other_x",
            "crypto_x",
            "passive_band",
        ],
    )?;
    let order_to_fill = at(gate, "order_to_fill")?;
    fields(
        order_to_fill,
        "config gate.order_to_fill",
        &["max", "min_orders"],
    )?;
    let usd = |value: &Json, key: &str| num(Usd::parse(dec_at(value, key)?.as_str()), key);
    let fraction =
        |value: &Json, key: &str| num(Fraction::parse(dec_at(value, key)?.as_str()), key);
    let whole = |value: &Json, key: &str| {
        u32::try_from(u64_at(value, key)?).map_err(|_| format!("config `{key}` does not fit a u32"))
    };
    let default = |text: &str| num(Fraction::parse(text), "a DEC-178 setting");
    Ok(GateConfig {
        price_floor: usd(gate, "price_floor")?,
        liquidity_floor_usd: usd(gate, "liquidity_floor_usd")?,
        crypto_liquidity_floor_usd: num(Usd::parse("1000000"), "a DEC-178 setting")?,
        collar_liquid_threshold_usd: usd(collar, "liquid_threshold_usd")?,
        collar_liquid_x: fraction(collar, "liquid_x")?,
        collar_other_x: fraction(collar, "other_x")?,
        collar_crypto_x: fraction(collar, "crypto_x")?,
        collar_passive_band: fraction(collar, "passive_band")?,
        opposite_fill_interval_s: whole(gate, "opposite_fill_interval_s")?,
        min_resting_time_s: 2,
        order_to_fill_max: dec_at(order_to_fill, "max")?
            .as_str()
            .parse()
            .map_err(|_| "config `order_to_fill.max` is not a whole number".to_owned())?,
        order_to_fill_min_orders: whole(order_to_fill, "min_orders")?,
        order_size_participation: default("0.05")?,
        daily_participation: default("0.05")?,
        close_window_minutes: whole(gate, "close_window_minutes")?,
        legacy_pdt_equity_threshold: usd(gate, "legacy_pdt_equity_threshold")?,
        etp_classification_max_age_s: 604_800,
    })
}

/// Spec §3.1's exchange codes as the case file writes them.
fn exchange_named(code: &str) -> Result<Exchange, String> {
    match code {
        "NASDAQ" => Ok(Exchange::Nasdaq),
        "NYSE" => Ok(Exchange::Nyse),
        "ARCA" => Ok(Exchange::Arca),
        "AMEX" => Ok(Exchange::Amex),
        "BATS" => Ok(Exchange::Bats),
        "OTC" => Ok(Exchange::Otc),
        other => Err(format!("unknown exchange `{other}`")),
    }
}

/// The proposer a case's `purpose` names (DEC-178): the order builder opens, increases and exits
/// at its discretion, the risk engine exits for risk, the owner closes, and a protective leg
/// protects. The gate assigns the purpose from this origin, the side and the position.
fn origin_named(purpose: &str) -> Result<Origin, String> {
    match purpose {
        "open" | "increase" | "discretionary_exit" => Ok(Origin::OrderBuilder),
        "risk_exit" => Ok(Origin::RiskEngine),
        "owner_exit" => Ok(Origin::OwnerClose),
        "protective" => Ok(Origin::ProtectiveLeg),
        other => Err(format!("`{other}` is not a purpose")),
    }
}

fn verdict_name(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Allow => "allow",
        Verdict::Deny => "deny",
        Verdict::Defer => "defer",
        Verdict::Hold => "hold",
    }
}

fn asset(text: &str) -> Result<AssetId, String> {
    AssetId::new(text).map_err(|e| format!("`{text}`: {e}"))
}

/// A `mandate-risk` refusal to decide, which is never a verdict.
fn gate_error(e: &GateError) -> String {
    format!("`mandate_risk::evaluate`: {e} ({})", e.code())
}

#[cfg(test)]
mod tests;
