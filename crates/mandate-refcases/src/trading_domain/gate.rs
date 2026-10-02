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
//! - **A later proposal waits for the earlier one's fills** (DEC-199 item 3 as narrowed by DEC-259
//!   item 7). A proposal is decided only once every earlier allowed proposal in the case was filled
//!   in full, on its instrument and side, by the case's `fill` steps before it; otherwise it is
//!   refused as [`super::LATER_PROPOSAL_WAITS`]. A refused proposal leaves no trace. A filled one
//!   is carried into the conduct figures from its `fill` steps, never from the proposal: one order
//!   submitted and one filled in the instrument, the filled quantity in today's participation, the
//!   last fill's instant as the latest fill on its side, and one order today if it opened or
//!   increased. Its working order, reservation and partial fills stay `actions`' (E7-4) and the
//!   account ledger's (E7-5), and this reading lapses when their order path drives these cases.
//! - **A crypto pair** (DEC-285) is quoted in the currency its `symbol` names (`BTC/USD` is USD,
//!   any other quote is not, and a symbol naming none states none), trades on no exchange, has the
//!   header's passing 30-day volume, and has its `min_trade_increment` (else the smallest `Qty`)
//!   as its minimum order. A proposal with no `tif` is `gtc`. A buy's fee is taken from the asset
//!   it receives, so it reserves no cash; the copy is filled as a taker, since the accounting
//!   needs a liquidity for a crypto fill.
//! - **The day-trade regime** (§9.2, DEC-284) is `initial.account.regime`, which the alpaca
//!   profile defaults to `intraday_margin` and a generic margin account must state; a cash account
//!   states none and is handed `legacy_pdt`, which check 8 never reads for one. `last_equity` is
//!   the prior-close equity check 8 compares with the threshold (the account's equity when the
//!   case states none), `prior_day_trades` are the broker's day trades before today, and
//!   `multiplier` must be 1 (§7.2's 1× requirement). A `legacy_pdt` margin account's ledger is
//!   `mandate_risk::fold_day_trades` over the case's `fill` steps, as is every `day_trade_count`.
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
    Account, AccountType, AssetClass, Config, Execution, Input, Liquidity, Reservations, Side,
};
use mandate_num::{Fraction, Price, Qty, Ratio, Rounding, ShareIncrement, SignedQty, Usd};
use mandate_risk::spec_types::{GoalState, RiskLimits};
use mandate_risk::{
    AccountFill, AccountSnapshot, AccountState, AgentId, AgentMode, AgentSnapshot, AssetId,
    ClientOrderId, ConductState, DayTrade, DayTradeFold, DayTradeInput, DayTradeLedger,
    DayTradeRegime, Decision, EtpClass, Exchange, GateConfig, GateError, GateInput, GatePass,
    InstrumentSnapshot, MarketSnapshot, Origin, ProposedKind, ProposedOrder, Purpose,
    QuoteCurrency, ReasonCode, RestingSide, RiskSnapshot, SaneQuote, TimeInForce, ValidatedMandate,
    Verdict, WorkingUniverse, evaluate, fold_day_trades,
};
use mandate_time::{Date, TradingCalendar, UtcNanos, new_york_date_and_hour};

use super::{
    BrokerProfile, Instruments, LATER_PROPOSAL_WAITS, account_type, acct, date, dec_at, fields,
    instrument, num, resolved_config,
};
use crate::{Json, at, ensure, list_at, str_at, u64_at};

/// §7.3's account fields, read from `initial.account` and from `broker_account_update` data.
/// `crypto_status` is read through [`Gate::crypto_active()`] as check 1's `crypto_active` (DEC-285
/// item 5's backlog row, DEC-315): an account whose case states none is crypto-active, as the case
/// file's header defaults it.
pub(super) const STATUS_FIELDS: &[&str] = &[
    "status",
    "trading_blocked",
    "account_blocked",
    "trade_suspended_by_user",
    "crypto_status",
];

/// `initial.account`'s §9.2 members: the regime and the figures its `legacy_pdt` budget reads.
pub(super) const DAY_TRADE_FIELDS: &[&str] =
    &["regime", "prior_day_trades", "last_equity", "multiplier"];

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
/// codes, each instrument's listing, the account's state and day-trade figures, the case's fills,
/// and the orders its allowed proposals placed.
pub(super) struct Gate {
    config: GateConfig,
    registered: BTreeSet<String>,
    exchanges: BTreeMap<String, Exchange>,
    pairs: BTreeMap<String, Pair>,
    state: AccountState,
    /// Check 1's `crypto_active`: `true` as the case file's header defaults it, and `false` once a
    /// `crypto_status` other than `ACTIVE` is detected, which nothing in the harness lifts
    /// (DEC-315 item 2).
    crypto_active: bool,
    day_trades: DayTradeFigures,
    fills: Vec<AccountFill>,
    working: Option<Order>,
    filled: Vec<Order>,
}

/// A crypto instrument's `symbol` and `min_trade_increment`, as check 2 and §5.3 rule 2 read them
/// (DEC-285).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pair {
    quote_currency: Option<QuoteCurrency>,
    min_order_size: Qty,
}

/// §9.2's regime as `initial.account` states it, or as the alpaca profile defaults it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Regime {
    IntradayMargin,
    LegacyPdt,
}

/// The day-trade regime and the figures check 8 reads, from `initial.account` (DEC-284).
struct DayTradeFigures {
    /// `None` for a cash account, and for a generic margin account that states none, which a
    /// proposal then refuses.
    regime: Option<Regime>,
    last_equity: Option<Usd>,
    earlier: Vec<DayTrade>,
}

/// The order an allowed proposal placed, as the case's `fill` steps fill it (DEC-259 item 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Order {
    pub(super) instrument: AssetId,
    pub(super) side: Side,
    pub(super) qty: Qty,
    pub(super) opening: bool,
    pub(super) submitted_at: UtcNanos,
    /// Each fill on the order's side, at its instant: never the proposal's figures.
    pub(super) fills: Vec<(UtcNanos, Qty)>,
    /// A fill larger than what was left, which no full fill can follow.
    pub(super) overfilled: bool,
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
        let mut pairs = BTreeMap::new();
        let listed = at(case, "instruments")?
            .as_object()
            .ok_or("`instruments` is not an object")?;
        for (name, listing) in listed {
            if listing.get("asset_class").and_then(Json::as_str) == Some("crypto") {
                pairs.insert(name.clone(), pair(name, listing)?);
            }
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
            pairs,
            state: AccountState::Active,
            crypto_active: true,
            day_trades: day_trade_figures(case)?,
            fills: Vec::new(),
            working: None,
            filled: Vec::new(),
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
        if key == "crypto_status" {
            return self.crypto_active(value);
        }
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

    /// §7.3's `crypto_status` as check 1's `crypto_active` (DEC-285 item 5's backlog row, DEC-315):
    /// `ACTIVE`, spelled exactly as `status` reads it, leaves the gate's `crypto_active` field as it
    /// is, any other status stores `false`, which refuses crypto openings at check 1, and a stored
    /// `false` is never set back, so a later `ACTIVE` never lifts a detected inactivity here
    /// (lifting is E7-5's). A status that is not a string is refused, as `status` is (DEC-395
    /// item 2). `the_initial_crypto_status_is_check_1s_crypto_active`,
    /// `an_account_update_s_crypto_status_is_check_1s_crypto_active` and
    /// `a_later_active_crypto_status_never_lifts_a_detected_inactivity` pin the reading.
    fn crypto_active(&mut self, value: &Json) -> Result<(), String> {
        let status = value
            .as_str()
            .ok_or("account `crypto_status` is not a string")?;
        if status != "ACTIVE" {
            self.crypto_active = false;
        }
        Ok(())
    }

    /// A `fill` step: one fill on the account for the day-trade fold, and on the working order's
    /// instrument and side, one toward filling that order in full.
    pub(super) fn fill(&mut self, fill: AccountFill) -> Result<(), String> {
        if let Some(order) = self
            .working
            .as_mut()
            .filter(|o| o.instrument == fill.instrument && o.side == fill.side)
        {
            let filled = order.filled()?;
            let left = num(
                order.qty.checked_sub(filled),
                "the working order's quantity",
            )?;
            if fill.qty > left {
                order.overfilled = true;
            } else if !order.overfilled {
                order.fills.push((fill.at, fill.qty));
                if fill.qty == left {
                    self.filled.extend(self.working.take());
                }
            }
        }
        self.fills.push(fill);
        Ok(())
    }

    /// The `day_trade_count` a step expects: the fold's window count at the step's instant.
    pub(super) fn check_day_trade_count(
        &self,
        account: &Account,
        accounting: &Config,
        instruments: &Instruments,
        now: UtcNanos,
        expected: &Json,
    ) -> Result<(), String> {
        let wanted = expected
            .as_u64()
            .ok_or("`day_trade_count` is not a whole number")?;
        let actual = self
            .fold(account, &accounting.calendar, instruments, now)?
            .ledger
            .window_count;
        ensure(u64::from(actual) == wanted, || {
            format!("day_trade_count: expected {wanted}, got {actual}")
        })
    }

    /// §9.2's fold over the account's fills, as `mandate_risk::fold_day_trades` computes it: the
    /// shares held at the start of today are each equity position less today's fills in it. An
    /// equity fill on an earlier trading day is refused, since its day trades would need that
    /// day's fold, which the harness does not run (DEC-284).
    fn fold(
        &self,
        account: &Account,
        calendar: &TradingCalendar,
        instruments: &Instruments,
        now: UtcNanos,
    ) -> Result<DayTradeFold, String> {
        let trade_date = |at: UtcNanos| calendar.equity_trade_date(at).map_err(|e| e.to_string());
        let today = trade_date(now)?;
        let mut held: BTreeMap<AssetId, Qty> = BTreeMap::new();
        for (id, position) in account.positions() {
            let (name, _, class, _) = instrument(instruments, id.as_str())?;
            if *class == AssetClass::UsEquity {
                held.insert(asset(name)?, held_overnight(name, position.qty())?);
            }
        }
        for fill in &self.fills {
            if fill.asset_class == AssetClass::UsEquity {
                ensure(trade_date(fill.at)? == today, || {
                    format!(
                        "`{}` was filled on a trading day before today's, whose day trades would need that day's fold (DEC-284)",
                        fill.instrument.as_str()
                    )
                })?;
            }
        }
        let equities = || {
            self.fills
                .iter()
                .filter(|f| f.asset_class == AssetClass::UsEquity)
        };
        for sold in equities().filter(|f| f.side == Side::Sell) {
            let shares = held.entry(sold.instrument.clone()).or_insert(Qty::ZERO);
            *shares = num(shares.checked_add(sold.qty), "a position held overnight")?;
        }
        for bought in equities().filter(|f| f.side == Side::Buy) {
            let shares = held.entry(bought.instrument.clone()).or_insert(Qty::ZERO);
            *shares = num(shares.checked_sub(bought.qty), "a position held overnight")?;
        }
        let fills_today = self.fills.clone();
        fold_day_trades(&DayTradeInput {
            now,
            earlier: &self.day_trades.earlier,
            held_overnight: &held,
            fills_today: &fills_today,
            flagged_pattern_day_trader: false,
        })
        .map_err(|e| gate_error(&e))
    }

    /// The conduct figures and the agent's opening orders today, from the orders the case filled
    /// in full: each order's own fills, never its proposal (DEC-259 item 7).
    pub(super) fn conduct(&self, now: UtcNanos) -> Result<(ConductState, u32), String> {
        let day = |at: UtcNanos| -> Result<Date, String> {
            Ok(new_york_date_and_hour(at).map_err(|e| e.to_string())?.0)
        };
        let today = day(now)?;
        let mut conduct = ConductState::default();
        let mut orders_today = 0_u32;
        for order in &self.filled {
            let instrument = &order.instrument;
            if day(order.submitted_at)? == today {
                let orders = conduct
                    .orders_today_per_instrument
                    .entry(instrument.clone())
                    .or_insert(0);
                *orders = orders.saturating_add(1);
                if order.opening {
                    orders_today = orders_today.saturating_add(1);
                }
            }
            let mut filled_today = false;
            for (at, qty) in &order.fills {
                if day(*at)? == today {
                    filled_today = true;
                    let total = conduct
                        .participation_today
                        .entry(instrument.clone())
                        .or_insert(Qty::ZERO);
                    *total = num(total.checked_add(*qty), "participation")?;
                }
                let latest = conduct
                    .last_opposite_fill_at
                    .entry((instrument.clone(), RestingSide::from(order.side)))
                    .or_insert(*at);
                *latest = (*latest).max(*at);
            }
            if filled_today {
                let filled = conduct.filled_today.entry(instrument.clone()).or_insert(0);
                *filled = filled.saturating_add(1);
            }
        }
        Ok((conduct, orders_today))
    }

    /// One `propose_order` step, decided by `mandate_risk::evaluate` in the scene the module doc
    /// describes, or refused while an earlier allowed proposal is not filled in full.
    pub(super) fn decide(
        &mut self,
        account: &Account,
        accounting: &Config,
        instruments: &Instruments,
        data: &Json,
        step: usize,
        now: UtcNanos,
    ) -> Result<Decision, String> {
        if self.working.is_some() {
            return Err(LATER_PROPOSAL_WAITS.to_owned());
        }
        let d = fields(data, "propose_order data", PROPOSAL_KEYS)?;
        if let Some(name) = d.get("name") {
            name.as_str().ok_or("`name` is not a string")?;
        }
        let (name, id, class, fractionable) = instrument(instruments, str_at(data, "instrument")?)?;
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
        let crypto = *class == AssetClass::Crypto;
        let tif = match d.get("tif").map(Json::as_str) {
            None if crypto => TimeInForce::Gtc,
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
        let exchange = self.exchanges.get(name).copied();
        ensure(crypto || exchange.is_some(), || {
            format!("instrument `{name}` states no `exchange`, which the eligibility floor reads")
        })?;
        let pair = if crypto {
            Some(
                *self
                    .pairs
                    .get(name)
                    .ok_or_else(|| format!("crypto instrument `{name}` was not read"))?,
            )
        } else {
            None
        };

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
                        liquidity: crypto.then_some(Liquidity::Taker),
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
        let (regime, day_trades) = match (account.account_type(), self.day_trades.regime) {
            (AccountType::Cash, _) => (DayTradeRegime::LegacyPdt, DayTradeLedger::default()),
            (AccountType::Margin, Some(Regime::IntradayMargin)) => (
                DayTradeRegime::IntradayMargin {
                    maintenance_excess: equity,
                },
                DayTradeLedger::default(),
            ),
            (AccountType::Margin, Some(Regime::LegacyPdt)) => (
                DayTradeRegime::LegacyPdt,
                self.fold(account, &accounting.calendar, instruments, now)?
                    .ledger,
            ),
            (AccountType::Margin, None) => {
                return Err(
                    "a generic margin account states no `regime`, which check 8 reads (§9.2)"
                        .to_owned(),
                );
            }
        };
        let (conduct, orders_today) = self.conduct(now)?;
        let account_snapshot = AccountSnapshot {
            account_type: account.account_type(),
            state: self.state,
            crypto_active: self.crypto_active,
            regime,
            equity,
            prior_close_equity: self.day_trades.last_equity.unwrap_or(equity),
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
            orders_today,
            day_trades,
        };
        let listing = listing(
            proposed.clone(),
            (exchange, pair),
            fractionable.unwrap_or(false),
            limit_price,
            now,
        )?;
        let market = market(SaneQuote { bid, ask, at: now }, limit_price)?;
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
        let decision = evaluate(&GateInput {
            now,
            pass: GatePass::First,
            config: &self.config,
            mandate: &mandate,
            risk: &risk,
            account: &account_snapshot,
            agent: &agent,
            instrument: &listing,
            market: &market,
            conduct: &conduct,
            universe: &universe,
            proposed: &order,
        })
        .map_err(|e| gate_error(&e))?;
        if decision.verdict == Verdict::Allow {
            self.working = Some(Order {
                instrument: order.instrument,
                side,
                qty,
                opening: matches!(decision.purpose, Purpose::Open | Purpose::Increase),
                submitted_at: now,
                fills: Vec::new(),
                overfilled: false,
            });
        }
        Ok(decision)
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

impl Order {
    fn filled(&self) -> Result<Qty, String> {
        self.fills.iter().try_fold(Qty::ZERO, |sum, (_, qty)| {
            num(sum.checked_add(*qty), "the working order's fills")
        })
    }
}

/// `initial.account`'s regime and figures (DEC-284): a stated regime, else `intraday_margin` for
/// the alpaca profile's margin account and none for a generic one; a cash account states none. A
/// `multiplier` other than 1 is refused, since §7.2 pauses the agents then.
fn day_trade_figures(case: &Json) -> Result<DayTradeFigures, String> {
    let account = at(case, "initial.account")?;
    let profile = BrokerProfile::parse(str_at(case, "broker_profile")?)?;
    let margin = account_type(account, profile)? == AccountType::Margin;
    let stated = match account.get("regime") {
        None => None,
        Some(regime) => match regime.as_str() {
            Some("intraday_margin") => Some(Regime::IntradayMargin),
            Some("legacy_pdt") => Some(Regime::LegacyPdt),
            _ => return Err(format!("unknown day-trade regime {regime}")),
        },
    };
    let regime = match (margin, stated, profile) {
        (false, None, _) => None,
        (false, Some(_), _) => {
            return Err("a cash account has no day-trade regime (§9.2)".to_owned());
        }
        (true, Some(regime), _) => Some(regime),
        (true, None, BrokerProfile::Alpaca) => Some(Regime::IntradayMargin),
        (true, None, BrokerProfile::Generic) => None,
    };
    if let Some(multiplier) = account.get("multiplier") {
        ensure(multiplier.as_u64() == Some(1), || {
            format!(
                "`multiplier` {multiplier} is not 1, which pauses the agents (§7.2's 1× requirement)"
            )
        })?;
    }
    let last_equity = match account.get("last_equity") {
        None => None,
        Some(_) => Some(num(
            Usd::parse(dec_at(account, "last_equity")?.as_str()),
            "last_equity",
        )?),
    };
    let mut earlier = Vec::new();
    for listed in account
        .get("prior_day_trades")
        .map(|l| l.as_array().ok_or("`prior_day_trades` is not a list"))
        .transpose()?
        .into_iter()
        .flatten()
    {
        fields(listed, "prior day trade", &["date", "instrument"])?;
        earlier.push(DayTrade {
            date: date(at(listed, "date")?, "date")?,
            instrument: asset(str_at(listed, "instrument")?)?,
        });
    }
    Ok(DayTradeFigures {
        regime,
        last_equity,
        earlier,
    })
}

/// DEC-199 item 6's listing: active, tradable and unhalted with a current status feed, a plain ETP
/// classified at `now`, last closed at the limit price, and fractionable only when the case says
/// so. A US equity trades on `exchange` in USD with a 90,000,000 median 20-day dollar volume (the
/// liquid collar tier) and one share its minimum order. A crypto pair is quoted in the currency its
/// `symbol` names, with the same 90,000,000 as its 30-day median (the case file's header: absent
/// crypto volume fields pass) and its `min_trade_increment` as its minimum order (DEC-285).
fn listing(
    instrument: AssetId,
    (exchange, pair): (Option<Exchange>, Option<Pair>),
    fractionable: bool,
    limit_price: Price,
    now: UtcNanos,
) -> Result<InstrumentSnapshot, String> {
    let liquid = Some(num(Usd::parse("90000000"), "a median dollar volume")?);
    let one_share = num(Qty::parse("1"), "min_order_size")?;
    let (asset_class, quote_currency, median_20d, median_30d, min_order_size) = match pair {
        None => (
            AssetClass::UsEquity,
            Some(QuoteCurrency::Usd),
            liquid,
            None,
            one_share,
        ),
        Some(pair) => (
            AssetClass::Crypto,
            pair.quote_currency,
            None,
            liquid,
            pair.min_order_size,
        ),
    };
    Ok(InstrumentSnapshot {
        instrument,
        asset_class,
        exchange,
        status_active: true,
        tradable: true,
        fractionable,
        ipo: false,
        ptp_no_exception: false,
        etp: EtpClass::Plain,
        etp_classified_at: Some(now),
        quote_currency,
        prior_close: Some(limit_price),
        median_dollar_volume_20d: median_20d,
        median_dollar_volume_30d: median_30d,
        min_order_size,
        halted: false,
        status_feed_current: true,
    })
}

/// The smallest quantity a `Qty` holds, a crypto pair's minimum order when the case states no
/// `min_trade_increment`, so that the harness never refuses a quantity the case sizes to (DEC-285).
const QTY_UNIT: &str = "0.000000001";

/// A crypto instrument's quote currency, from its `symbol` ([`crate::quote_currency_of`]), and
/// its minimum order (DEC-285). An instrument that states no `symbol` states no quote currency.
fn pair(name: &str, listing: &Json) -> Result<Pair, String> {
    let quote_currency = match listing.get("symbol") {
        None => None,
        Some(symbol) => {
            let symbol = symbol
                .as_str()
                .ok_or_else(|| format!("instrument `{name}`: `symbol` is not a string"))?;
            crate::quote_currency_of(symbol)
        }
    };
    let increment = match listing.get("min_trade_increment") {
        None => QTY_UNIT.to_owned(),
        Some(_) => dec_at(listing, "min_trade_increment")?.as_str().to_owned(),
    };
    Ok(Pair {
        quote_currency,
        min_order_size: num(Qty::parse(&increment), "min_trade_increment")?,
    })
}

/// DEC-199 item 6's market: `quote`, the last trade at the limit price, a trailing 5-minute volume
/// of 1,000,000 and a 20-day ADV of 10,000,000, so both participation caps pass any order up to
/// 50,000 shares.
fn market(quote: SaneQuote, limit_price: Price) -> Result<MarketSnapshot, String> {
    Ok(MarketSnapshot {
        quote: Some(quote),
        last_trade: Some((limit_price, quote.at)),
        trailing_5m_volume: Some(num(Qty::parse("1000000"), "trailing_5m_volume")?),
        adv_20d: Some(num(Qty::parse("10000000"), "adv_20d")?),
    })
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

/// The shares an equity position holds at the start of today for §9.2's fold. A negative
/// `SignedQty` is a short: no v1 case holds one (`AGENTS.md` rule 12), and folding its magnitude
/// would hold it long and understate the day-trade count (#412 review, nit 3, DEC-314), so the
/// fold refuses it. [`Gate::decide`] refuses a short snapshot before it folds, so the refusal is
/// reached from [`Gate::check_day_trade_count`] (DEC-314 item 2). A flat position, `0`, is not a
/// short and folds as no shares held, which `only_a_negative_quantity_is_a_short_to_the_fold` pins,
/// and `a_short_position_is_refused_where_a_day_trade_count_is_expected` pins the refusal's text.
fn held_overnight(name: &str, qty: SignedQty) -> Result<Qty, String> {
    if qty.is_negative() {
        return Err(format!(
            "`{name}` is held short, which the day-trade fold refuses (its magnitude would fold as long and understate the day-trade count)"
        ));
    }
    Ok(qty.abs())
}

#[cfg(test)]
mod tests;
