//! Family W's `kind: tripwire` reference-case boundary (E6-13).
//!
//! Every case, step, patch operation, expectation, event, state, and probe member is interpreted.
//! Fills first pass through `mandate-accounting`; the adapter gives the executor the resulting
//! gross realized P&L less the case's explicit fill fee. No binary floating-point arithmetic is
//! involved.

use std::collections::BTreeSet;

use mandate_accounting::{
    Account, AccountType, AssetClass, Config, CryptoFees, EquityFees, Execution, Input,
    InstrumentId, Position, Record, Side, TafCapBasis,
};
use mandate_approval::{AssertionId, Environment, RiskClock, StepUp, StepUpMethod};
use mandate_builder::{ActionContext, Classification, DecidedBy, classify};
use mandate_domain::AutonomyDecision;
use mandate_executor::tripwire::{
    TripwireAcknowledgment, TripwireEvent, TripwireFill, TripwireInput, TripwireSnapshot,
    TripwireState, TripwireValue, fold,
};
use mandate_num::{Bps, FeeCap, FeePerShare, FeeRate, Price, Qty, SignedQty, Usd};
use mandate_spec::document::{Autonomy, Lifts, Mandate, TripwireAction};
use mandate_spec::risk;
use mandate_time::{Date, TradingCalendar, UtcNanos};
use serde_json::{Map, json};

use super::{at_of, instant, must_parse, patched, unknown_members};
use crate::{Json, at, ensure, expect_eq, list_at, str_at};

const CASE_KEYS: &[&str] = &[
    "id", "kind", "title", "base", "context", "steps", "expect", "probes",
];
const CONTEXT_KEYS: &[&str] = &["environment"];
const VERSION_KEYS: &[&str] = &["event", "at", "patch"];
const FILL_KEYS: &[&str] = &["event", "at", "instrument", "side", "qty", "price", "fees"];
const SIMPLE_KEYS: &[&str] = &["event", "at"];
const ACK_KEYS: &[&str] = &[
    "event",
    "at",
    "tripwire",
    "step_up",
    "user",
    "requester",
    "independent_approval_required",
    "independent_now",
];
const STEP_UP_KEYS: &[&str] = &["assertion", "authenticated_at", "method"];
const EXPECT_KEYS: &[&str] = &["journal", "state"];
const STATE_KEYS: &[&str] = &["fired", "restriction", "delegations_suspended", "metrics"];
const PROBE_KEYS: &[&str] = &["action", "expect"];
const PATCH_KEYS: &[&str] = &["op", "path", "value"];

struct Scene {
    tripwire: TripwireState,
    account: Account,
    instruments: BTreeSet<InstrumentId>,
    mandate: Option<Mandate>,
    last_at: Option<UtcNanos>,
    last_snapshot: Option<TripwireSnapshot>,
    environment: Environment,
    accounting: Config,
}

/// Runs MC-W27 to MC-W51 and MC-W53 to MC-W56 through the public tripwire fold.
pub(super) fn tripwire_case(fixture: &Json, case: &Json) -> Result<(), String> {
    unknown_members(case, CASE_KEYS)
        .map_err(|unknown| format!("case keys not interpreted: {unknown}"))?;
    expect_eq("kind", str_at(case, "kind")?, "tripwire")?;
    let context = at_of(case, "context")?;
    unknown_members(context, CONTEXT_KEYS)
        .map_err(|unknown| format!("`context` members not interpreted: {unknown}"))?;
    let environment = environment(str_at(context, "environment")?)?;
    let steps = list_at(case, "steps")?;
    let expected = list_at(case, "expect")?;
    expect_eq("one expectation per step", expected.len(), steps.len())?;

    let mut scene = Scene {
        tripwire: TripwireState::default(),
        account: Account::opening(AccountType::Margin, Usd::ZERO, []),
        instruments: BTreeSet::new(),
        mandate: None,
        last_at: None,
        last_snapshot: None,
        environment,
        accounting: accounting_config()?,
    };
    for (index, (step, wanted)) in steps.iter().zip(expected).enumerate() {
        let number = index.saturating_add(1);
        let outcome = apply_step(fixture, case, step, index, &mut scene)
            .map_err(|error| format!("step {number}: {error}"))?;
        compare_outcome(wanted, &outcome.snapshot, &outcome.journal)
            .map_err(|error| format!("step {number}: {error}"))?;
        scene.last_snapshot = Some(outcome.snapshot.clone());
        scene.tripwire = outcome.state;
    }

    match case.get("probes") {
        Some(probes) => compare_probes(
            probes,
            scene
                .mandate
                .as_ref()
                .ok_or("the probes have no applied mandate version")?,
            scene
                .last_at
                .ok_or("the probes have no preceding step risk clock")?,
            scene
                .last_snapshot
                .as_ref()
                .ok_or("the probes have no preceding tripwire snapshot")?,
            str_at(case, "id")?,
        ),
        None => Ok(()),
    }
}

fn apply_step(
    fixture: &Json,
    case: &Json,
    step: &Json,
    index: usize,
    scene: &mut Scene,
) -> Result<mandate_executor::tripwire::TripwireOutcome, String> {
    let event = str_at(step, "event")?;
    let clock = instant(at(step, "at")?, "at")?;
    scene.last_at = Some(clock);
    let input = match event {
        "MandateVersionApplied" => {
            unknown_members(step, VERSION_KEYS)
                .map_err(|unknown| format!("version-step members not interpreted: {unknown}"))?;
            for operation in list_at(step, "patch")? {
                unknown_members(operation, PATCH_KEYS)
                    .map_err(|unknown| format!("patch members not interpreted: {unknown}"))?;
                let op = str_at(operation, "op")?;
                ensure(matches!(op, "add" | "replace" | "remove"), || {
                    format!("patch op `{op}` is not interpreted")
                })?;
                str_at(operation, "path")?;
                ensure(
                    operation.get("value").is_some() == matches!(op, "add" | "replace"),
                    || format!("patch op `{op}` has the wrong `value` shape"),
                )?;
            }
            let version_case = json!({
                "base": str_at(case, "base")?,
                "patch": at(step, "patch")?.clone(),
            });
            let mandate = must_parse(&patched(fixture, &version_case)?)?;
            let tripwires = mandate.autonomy.tripwires.clone();
            scene.mandate = Some(mandate);
            TripwireInput::MandateVersionApplied { tripwires }
        }
        "FillApplied" => {
            unknown_members(step, FILL_KEYS)
                .map_err(|unknown| format!("fill-step members not interpreted: {unknown}"))?;
            let fill = accounting_fill(step, index, clock, scene)?;
            TripwireInput::FillApplied { fill }
        }
        "LateFillApplied" => {
            unknown_members(step, FILL_KEYS)
                .map_err(|unknown| format!("fill-step members not interpreted: {unknown}"))?;
            let fill = accounting_fill(step, index, clock, scene)?;
            TripwireInput::LateFillApplied { fill }
        }
        "RiskDayStarted" => {
            unknown_members(step, SIMPLE_KEYS)
                .map_err(|unknown| format!("risk-day-step members not interpreted: {unknown}"))?;
            let day = risk::risk_day(clock)
                .map_err(|error| format!("`at` has no risk day: {}", error.code()))?
                .day;
            TripwireInput::RiskDayStarted { day }
        }
        "Mark" => {
            unknown_members(step, SIMPLE_KEYS)
                .map_err(|unknown| format!("mark-step members not interpreted: {unknown}"))?;
            TripwireInput::Mark
        }
        "Clock" => {
            unknown_members(step, SIMPLE_KEYS)
                .map_err(|unknown| format!("clock-step members not interpreted: {unknown}"))?;
            TripwireInput::Clock
        }
        "Restart" => {
            unknown_members(step, SIMPLE_KEYS)
                .map_err(|unknown| format!("restart-step members not interpreted: {unknown}"))?;
            TripwireInput::Restart
        }
        "OwnerAcknowledged" => {
            unknown_members(step, ACK_KEYS).map_err(|unknown| {
                format!("acknowledgment-step members not interpreted: {unknown}")
            })?;
            TripwireInput::OwnerAcknowledged(acknowledgment(step, clock, scene.environment)?)
        }
        other => {
            return Err(format!(
                "`{other}` is not a tripwire input the harness applies"
            ));
        }
    };
    fold(&scene.tripwire, &input)
        .map_err(|error| format!("`mandate_executor::tripwire::fold`: {error}"))
}

fn accounting_fill(
    step: &Json,
    index: usize,
    clock: UtcNanos,
    scene: &mut Scene,
) -> Result<TripwireFill, String> {
    let instrument = InstrumentId::new(str_at(step, "instrument")?)
        .map_err(|error| format!("`instrument`: {error}"))?;
    let side = side(str_at(step, "side")?)?;
    let qty = Qty::parse(str_at(step, "qty")?).map_err(|error| format!("`qty`: {error}"))?;
    let price_text = str_at(step, "price")?;
    let stated_fees =
        Usd::parse(str_at(step, "fees")?).map_err(|error| format!("`fees`: {error}"))?;
    ensure(!stated_fees.is_negative(), || {
        "`fees` must not be negative".to_owned()
    })?;
    let mandate = scene
        .mandate
        .as_ref()
        .ok_or("a fill arrived before a mandate version")?;
    let asset_class = fill_asset_class(mandate, instrument.as_str())?;
    ensure(asset_class == AssetClass::UsEquity, || {
        "the tripwire fixture's explicit USD `fees` adapter supports US-equity fills only"
            .to_owned()
    })?;
    let realized_gross = match Price::parse(price_text) {
        Ok(price) => {
            let input = Input::Fill(Execution {
                fill_id: format!("tripwire-fill-{}", index.saturating_add(1)),
                client_order_id: None,
                instrument: instrument.clone(),
                asset_class,
                side,
                qty_gross: qty,
                price,
                liquidity: None,
                executed_at: clock,
            });
            let applied = scene
                .account
                .apply(&input, &scene.accounting)
                .map_err(|error| format!("`mandate_accounting::Account::apply`: {error}"))?;
            let Record::Fill { realized_gross, .. } = applied.record else {
                return Err("the accounting fill returned a non-fill record".to_owned());
            };
            scene.account = applied.account;
            scene.instruments.insert(instrument.clone());
            realized_gross
        }
        Err(_) => apply_full_precision_unit_buy(scene, &instrument, side, qty, price_text)?,
    };
    let net_realized_usd = realized_gross
        .checked_sub(stated_fees)
        .map_err(|error| format!("net realized P&L: {error}"))?;
    Ok(TripwireFill {
        instrument,
        side,
        net_realized_usd,
    })
}

fn apply_full_precision_unit_buy(
    scene: &mut Scene,
    instrument: &InstrumentId,
    side: Side,
    qty: Qty,
    price: &str,
) -> Result<Usd, String> {
    ensure(side == Side::Buy, || {
        "`price` exceeds accounting's execution precision on a non-buy fill".to_owned()
    })?;
    let one = Qty::parse("1").map_err(|error| error.to_string())?;
    ensure(qty == one, || {
        "`price` exceeds accounting's execution precision on a non-unit buy".to_owned()
    })?;
    let exact_price = Usd::parse(price).map_err(|error| format!("`price`: {error}"))?;
    ensure(exact_price > Usd::ZERO, || {
        "`price` is not positive".to_owned()
    })?;
    let before = scene.account.position(instrument);
    let after = Position::new(
        before
            .qty()
            .checked_add(SignedQty::parse("1").map_err(|error| error.to_string())?)
            .map_err(|error| format!("full-precision buy quantity: {error}"))?,
        before
            .basis()
            .checked_add(exact_price)
            .map_err(|error| format!("full-precision buy basis: {error}"))?,
    )
    .map_err(|error| format!("full-precision buy position: {error}"))?;
    scene.instruments.insert(instrument.clone());
    let positions = scene
        .instruments
        .iter()
        .map(|known| {
            let position = if known == instrument {
                after
            } else {
                scene.account.position(known)
            };
            (known.clone(), position)
        })
        .collect::<Vec<_>>();
    scene.account = Account::opening(AccountType::Margin, Usd::ZERO, positions);
    Ok(Usd::ZERO)
}

fn fill_asset_class(mandate: &Mandate, instrument: &str) -> Result<AssetClass, String> {
    if let Some(found) = mandate
        .universe
        .pinned_instruments
        .iter()
        .find(|entry| entry.asset_id.as_str() == instrument)
    {
        return Ok(found.asset_class);
    }
    let mut classes = mandate.universe.asset_classes.iter();
    let only = classes
        .next()
        .copied()
        .ok_or("the mandate allows no asset class for this fill")?;
    ensure(classes.next().is_none(), || {
        format!(
            "`{instrument}` is not pinned and the mandate allows more than one asset class, so its \
             fill class is ambiguous"
        )
    })?;
    Ok(only)
}

fn acknowledgment(
    step: &Json,
    clock: UtcNanos,
    context_environment: Environment,
) -> Result<TripwireAcknowledgment, String> {
    let (step_up, environment) = match step.get("step_up") {
        None | Some(Json::Null) => (None, context_environment),
        Some(stated) => {
            unknown_members(stated, STEP_UP_KEYS)
                .map_err(|unknown| format!("`step_up` members not interpreted: {unknown}"))?;
            let method = str_at(stated, "method")?;
            let environment = match method {
                "cli_confirm" => context_environment,
                "password" => Environment::Live,
                other => return Err(format!("`{other}` is not a step-up method")),
            };
            let authenticated_at = instant(at(stated, "authenticated_at")?, "authenticated_at")?;
            (
                Some(StepUp {
                    assertion: AssertionId(str_at(stated, "assertion")?.to_owned()),
                    authenticated_at: RiskClock(authenticated_at.secs()),
                    method: StepUpMethod::CliConfirm,
                }),
                environment,
            )
        }
    };
    let independent_required_at_request =
        optional_bool(step, "independent_approval_required")?.unwrap_or(false);
    let independent_required_now =
        optional_bool(step, "independent_now")?.unwrap_or(independent_required_at_request);
    Ok(TripwireAcknowledgment {
        tripwire: mandate_spec::document::TripwireId::parse(str_at(step, "tripwire")?)
            .map_err(|error| format!("`tripwire`: {error}"))?,
        step_up,
        committed_at: RiskClock(clock.secs()),
        processed_at: RiskClock(clock.secs()),
        environment,
        requester: optional_text(step, "requester")?,
        acknowledging_user: optional_text(step, "user")?,
        independent_required_at_request,
        independent_required_now,
    })
}

fn compare_outcome(
    expected: &Json,
    snapshot: &TripwireSnapshot,
    journal: &[TripwireEvent],
) -> Result<(), String> {
    unknown_members(expected, EXPECT_KEYS)
        .map_err(|unknown| format!("expectation members not interpreted: {unknown}"))?;
    let expected_state = at_of(expected, "state")?;
    unknown_members(expected_state, STATE_KEYS)
        .map_err(|unknown| format!("state members not interpreted: {unknown}"))?;
    ensure(
        snapshot.allocation_increase_blocked == snapshot.delegations_suspended,
        || {
            "the tripwire projection disagrees on delegation suspension and allocation blocking"
                .to_owned()
        },
    )?;
    let actual_journal = Json::Array(
        journal
            .iter()
            .map(journal_json)
            .collect::<Result<Vec<_>, _>>()?,
    );
    expect_eq("journal", &actual_journal, at(expected, "journal")?)?;
    expect_eq("state", state_json(snapshot), expected_state.clone())
}

fn state_json(snapshot: &TripwireSnapshot) -> Json {
    let fired = snapshot
        .fired
        .iter()
        .map(|(id, action)| {
            (
                id.as_str().to_owned(),
                Json::String(action.as_str().to_owned()),
            )
        })
        .collect::<Map<_, _>>();
    let metrics = snapshot
        .metrics
        .iter()
        .map(|(id, value)| {
            let text = match value {
                TripwireValue::Count(count) => count.to_string(),
                TripwireValue::Usd(value) => value.to_string(),
            };
            (id.as_str().to_owned(), Json::String(text))
        })
        .collect::<Map<_, _>>();
    let restriction = match snapshot.effective_action {
        Some(TripwireAction::ExitsOnly) => json!("exits_only"),
        Some(TripwireAction::EndDelegations) | None => Json::Null,
    };
    json!({
        "fired": fired,
        "restriction": restriction,
        "delegations_suspended": snapshot.delegations_suspended,
        "metrics": metrics,
    })
}

fn journal_json(event: &TripwireEvent) -> Result<Json, String> {
    Ok(match event {
        TripwireEvent::RiskLimitTriggered {
            id,
            action,
            metric,
            threshold,
            value,
        } => json!({
            "type": "RiskLimitTriggered",
            "limit": format!("tripwire:{}", id.as_str()),
            "action": action.as_str(),
            "reason": "tripwire_condition",
            "metric": metric.as_str(),
            "threshold": threshold.as_str(),
            "value": value_text(*value),
        }),
        TripwireEvent::OwnerAlertSent(alert) => json!({
            "type": "OwnerAlertSent",
            "subject": format!("RiskLimitTriggered:{}", alert.triggered_event_index()),
            "text": alert.generic_text(),
        }),
        TripwireEvent::RiskLimitLifted { id } => json!({
            "type": "RiskLimitLifted",
            "limit": format!("tripwire:{}", id.as_str()),
            "reason": "owner_acknowledged",
        }),
        TripwireEvent::OwnerCommandRefused { reason } => json!({
            "type": "OwnerCommandRefused",
            "command": "acknowledge",
            "reason": reason.as_str(),
        }),
    })
}

fn compare_probes(
    probes: &Json,
    mandate: &Mandate,
    now: UtcNanos,
    tripwires: &TripwireSnapshot,
    case_id: &str,
) -> Result<(), String> {
    let probes = probes.as_array().ok_or("`probes` is not a list")?;
    let risk_day = risk::risk_day(now)
        .map_err(|error| format!("the probe clock has no risk day: {}", error.code()))?
        .day;
    for (index, probe) in probes.iter().enumerate() {
        let number = index.saturating_add(1);
        unknown_members(probe, PROBE_KEYS)
            .map_err(|unknown| format!("probe {number} members not interpreted: {unknown}"))?;
        let action = super::autonomy::action(case_id, at_of(probe, "action")?, risk_day)
            .map_err(|error| format!("probe {number}: {error}"))?;
        let basic = classify(&mandate.autonomy, &action)
            .map_err(|error| format!("probe {number}: `mandate_builder::classify`: {error}"))?;
        let actual = probe_result(
            &mandate.autonomy,
            &action,
            now,
            tripwires.delegations_suspended,
            basic,
        )?;
        expect_eq(
            &format!("probe {number}"),
            actual,
            at_of(probe, "expect")?.clone(),
        )?;
    }
    Ok(())
}

fn probe_result(
    autonomy: &Autonomy,
    action: &ActionContext,
    now: UtcNanos,
    delegations_suspended: bool,
    basic: Classification,
) -> Result<Json, String> {
    if !delegations_suspended && basic.decision == AutonomyDecision::Ask {
        let source = basic.by.label();
        for delegation in &autonomy.delegations {
            let lifts = match &delegation.lifts {
                Lifts::Default => "default".to_owned(),
                Lifts::Rule(id) => format!("rule:{}", id.as_str()),
            };
            if lifts != source {
                continue;
            }
            let starts_at = delegation.starts_at.ok_or_else(|| {
                format!("delegation `{}` has no valid start", delegation.id.as_str())
            })?;
            let expires_at = delegation.expires_at.ok_or_else(|| {
                format!(
                    "delegation `{}` has no valid expiry",
                    delegation.id.as_str()
                )
            })?;
            let max_order = Usd::parse(delegation.max_order_usd.as_str())
                .map_err(|error| format!("delegation max order: {error}"))?;
            let max_total = Usd::parse(delegation.max_total_usd.as_str())
                .map_err(|error| format!("delegation max total: {error}"))?;
            if starts_at <= now
                && now < expires_at
                && delegation
                    .when
                    .matches(action)
                    .map_err(|error| error.to_string())?
                && action.order_usd <= max_order
                && action.order_usd <= max_total
                && delegation.max_orders > 0
            {
                let id = delegation.id.as_str();
                return Ok(json!({
                    "decision": "auto",
                    "by": format!("delegation:{id}"),
                    "delegation_id": id,
                    "lifted": source,
                }));
            }
        }
    }
    Ok(json!({
        "decision": decision_name(basic.decision),
        "by": decided_by_name(&basic.by),
    }))
}

pub(super) fn accounting_config() -> Result<Config, String> {
    let date = |text: &str| Date::parse(text).map_err(|error| error.to_string());
    Ok(Config {
        equities: EquityFees {
            sec_rate: FeeRate::parse("0").map_err(|error| error.to_string())?,
            taf_per_share: FeePerShare::parse("0").map_err(|error| error.to_string())?,
            taf_cap: FeeCap::parse("0").map_err(|error| error.to_string())?,
            taf_cap_basis: TafCapBasis::PerExecution,
            cat_per_share: FeePerShare::parse("0").map_err(|error| error.to_string())?,
        },
        crypto: CryptoFees {
            maker: Bps::parse("0").map_err(|error| error.to_string())?,
            taker: Bps::parse("0").map_err(|error| error.to_string())?,
        },
        calendar: TradingCalendar::new(
            date("2026-09-01")?,
            date("2026-12-31")?,
            [date("2026-11-26")?, date("2026-12-25")?],
            [date("2026-10-12")?, date("2026-11-11")?],
        )
        .map_err(|error| error.to_string())?,
    })
}

fn optional_text(value: &Json, key: &str) -> Result<Option<String>, String> {
    match value.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(format!("`{key}` is neither text nor null")),
    }
}

fn optional_bool(value: &Json, key: &str) -> Result<Option<bool>, String> {
    match value.get(key) {
        None => Ok(None),
        Some(Json::Bool(flag)) => Ok(Some(*flag)),
        Some(_) => Err(format!("`{key}` is not a boolean")),
    }
}

fn environment(text: &str) -> Result<Environment, String> {
    match text {
        "paper" => Ok(Environment::Paper),
        "live" => Ok(Environment::Live),
        other => Err(format!("`{other}` is not a trading environment")),
    }
}

fn side(text: &str) -> Result<Side, String> {
    match text {
        "buy" => Ok(Side::Buy),
        "sell" => Ok(Side::Sell),
        other => Err(format!("`{other}` is not a fill side")),
    }
}

fn value_text(value: TripwireValue) -> String {
    match value {
        TripwireValue::Count(count) => count.to_string(),
        TripwireValue::Usd(value) => value.to_string(),
    }
}

fn decision_name(decision: AutonomyDecision) -> &'static str {
    match decision {
        AutonomyDecision::Auto => "auto",
        AutonomyDecision::Ask => "ask",
        AutonomyDecision::Deny => "deny",
    }
}

fn decided_by_name(by: &DecidedBy) -> String {
    by.label()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use mandate_accounting::{Account, AccountType, InstrumentId, Side};
    use mandate_approval::Environment;
    use mandate_builder::{ActionContext, Classification, classify};
    use mandate_domain::{AssetClass, AutonomyDecision};
    use mandate_executor::tripwire::TripwireState;
    use mandate_num::{Qty, Unit, Usd};
    use mandate_spec::document::{Autonomy, Lifts};
    use mandate_spec::{DecGrammar, SchemaDec, risk};
    use serde_json::json;

    use super::{
        Scene, accounting_config, apply_full_precision_unit_buy, apply_step, at_of,
        fill_asset_class, must_parse, patched, probe_result, tripwire_case,
    };
    use crate::{Json, list_at, read_fixture};

    fn fixture() -> Result<Json, String> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases");
        read_fixture(&dir, "mandate.json").map(|fixture| (*fixture).clone())
    }

    fn cases(fixture: &Json) -> Result<Vec<Json>, String> {
        Ok(fixture
            .get("cases")
            .and_then(Json::as_array)
            .ok_or("the fixture has no case list")?
            .iter()
            .filter(|case| case.get("kind").and_then(Json::as_str) == Some("tripwire"))
            .cloned()
            .collect())
    }

    fn case(fixture: &Json, id: &str) -> Result<Json, String> {
        fixture
            .get("cases")
            .and_then(Json::as_array)
            .and_then(|cases| {
                cases
                    .iter()
                    .find(|case| case.get("id").and_then(Json::as_str) == Some(id))
            })
            .cloned()
            .ok_or_else(|| format!("the fixture has no {id}"))
    }

    fn scene() -> Result<Scene, String> {
        Ok(Scene {
            tripwire: TripwireState::default(),
            account: Account::opening(AccountType::Margin, Usd::ZERO, []),
            instruments: BTreeSet::new(),
            mandate: None,
            last_at: None,
            last_snapshot: None,
            environment: Environment::Paper,
            accounting: accounting_config()?,
        })
    }

    /// Every family-W tripwire case runs live even while its status remains pending.
    #[test]
    fn every_tripwire_case_passes_as_stated() -> Result<(), String> {
        let fixture = fixture()?;
        let cases = cases(&fixture)?;
        if cases.len() != 29 {
            return Err(format!(
                "family W has 29 tripwire cases, found {}",
                cases.len()
            ));
        }
        for case in &cases {
            let id = case.get("id").and_then(Json::as_str).unwrap_or("?");
            tripwire_case(&fixture, case).map_err(|error| format!("{id}: {error}"))?;
        }
        Ok(())
    }

    /// The arm, outcome comparer, and probe comparer each reject a doctored fixture.
    #[test]
    fn doctored_tripwire_cases_fail_at_each_comparison_boundary() -> Result<(), String> {
        let fixture = fixture()?;
        let cases = cases(&fixture)?;
        let first = cases.first().ok_or("family W has no tripwire case")?;

        let mut wrong_kind = first.clone();
        wrong_kind["kind"] = Json::String("not_tripwire".to_owned());
        if tripwire_case(&fixture, &wrong_kind).is_ok() {
            return Err("a wrong case kind passed".to_owned());
        }

        let mut wrong_outcome = first.clone();
        let expectation = wrong_outcome
            .pointer_mut("/expect/0")
            .and_then(Json::as_object_mut)
            .ok_or("the first tripwire case has no first expectation")?;
        expectation.insert("unread".to_owned(), Json::Bool(true));
        if tripwire_case(&fixture, &wrong_outcome).is_ok() {
            return Err("an unread outcome member passed".to_owned());
        }

        let with_probes = cases
            .iter()
            .find(|case| case.get("probes").is_some())
            .ok_or("family W has no probe case")?;
        let mut wrong_probe = with_probes.clone();
        let decision = wrong_probe
            .pointer_mut("/probes/0/expect/decision")
            .ok_or("the probe has no expected decision")?;
        *decision = Json::String("not_a_decision".to_owned());
        if tripwire_case(&fixture, &wrong_probe).is_ok() {
            return Err("a wrong probe decision passed".to_owned());
        }
        Ok(())
    }

    /// The full-precision fallback rejects zero and updates only the named instrument.
    #[test]
    fn full_precision_buys_preserve_other_positions_and_require_a_positive_price()
    -> Result<(), String> {
        let mut scene = scene()?;
        let first = InstrumentId::new("FIRST").map_err(|error| error.to_string())?;
        let second = InstrumentId::new("SECOND").map_err(|error| error.to_string())?;
        let one = Qty::parse("1").map_err(|error| error.to_string())?;

        apply_full_precision_unit_buy(&mut scene, &first, Side::Buy, one, "10.000000000001")?;
        apply_full_precision_unit_buy(&mut scene, &second, Side::Buy, one, "20.000000000001")?;
        apply_full_precision_unit_buy(&mut scene, &first, Side::Buy, one, "11.000000000001")?;

        assert_eq!(scene.account.position(&first).qty().to_string(), "2");
        assert_eq!(
            scene.account.position(&first).basis().to_string(),
            "21.000000000002"
        );
        assert_eq!(scene.account.position(&second).qty().to_string(), "1");
        assert_eq!(
            scene.account.position(&second).basis().to_string(),
            "20.000000000001"
        );
        assert!(apply_full_precision_unit_buy(&mut scene, &first, Side::Buy, one, "0").is_err());
        Ok(())
    }

    /// Pinned-instrument lookup returns the matching entry rather than another pin.
    #[test]
    fn fill_asset_class_uses_the_matching_pin() -> Result<(), String> {
        let fixture = fixture()?;
        let mut mandate = must_parse(&patched(
            &fixture,
            &json!({"base": "two_stock_swing", "patch": []}),
        )?)?;
        if mandate.universe.pinned_instruments.len() < 2 {
            return Err("two_stock_swing has fewer than two pins".to_owned());
        }
        mandate.universe.pinned_instruments[0].asset_class = AssetClass::UsEquity;
        mandate.universe.pinned_instruments[1].asset_class = AssetClass::Crypto;
        let second = mandate.universe.pinned_instruments[1]
            .asset_id
            .as_str()
            .to_owned();
        assert_eq!(fill_asset_class(&mandate, &second)?, AssetClass::Crypto);
        Ok(())
    }

    /// Each delegation predicate independently blocks a lift when false, with exact time edges.
    #[test]
    fn delegation_probe_requires_every_bound() -> Result<(), String> {
        let fixture = fixture()?;
        let case = case(&fixture, "MC-W31")?;
        let mut scene = scene()?;
        let steps = list_at(&case, "steps")?;
        for (index, step) in steps.iter().enumerate() {
            let outcome = apply_step(&fixture, &case, step, index, &mut scene)?;
            scene.tripwire = outcome.state;
            scene.last_snapshot = Some(outcome.snapshot);
        }
        let mandate = scene.mandate.ok_or("MC-W31 applies no mandate")?;
        let now = scene.last_at.ok_or("MC-W31 has no risk clock")?;
        let probe = list_at(&case, "probes")?
            .first()
            .ok_or("MC-W31 has no probe")?;
        let risk_day = risk::risk_day(now).map_err(|error| error.to_string())?.day;
        let action = super::super::autonomy::action("MC-W31", at_of(probe, "action")?, risk_day)?;
        let basic = classify(&mandate.autonomy, &action).map_err(|error| error.to_string())?;
        let delegated = |autonomy: &Autonomy,
                         action: &ActionContext,
                         suspended: bool,
                         classification: Classification|
         -> Result<bool, String> {
            Ok(
                probe_result(autonomy, action, now, suspended, classification)?
                    .get("by")
                    .and_then(Json::as_str)
                    .is_some_and(|by| by.starts_with("delegation:")),
            )
        };
        assert!(delegated(&mandate.autonomy, &action, false, basic.clone())?);
        assert!(!delegated(&mandate.autonomy, &action, true, basic.clone())?);

        let mut not_ask = basic.clone();
        not_ask.decision = AutonomyDecision::Auto;
        assert!(!delegated(&mandate.autonomy, &action, false, not_ask)?);

        let mut wrong_source = mandate.autonomy.clone();
        wrong_source.delegations[0].lifts = Lifts::Default;
        assert!(!delegated(&wrong_source, &action, false, basic.clone())?);

        let mut at_start = mandate.autonomy.clone();
        at_start.delegations[0].starts_at = Some(now);
        assert!(delegated(&at_start, &action, false, basic.clone())?);

        let mut at_expiry = mandate.autonomy.clone();
        at_expiry.delegations[0].expires_at = Some(now);
        assert!(!delegated(&at_expiry, &action, false, basic.clone())?);

        let mut false_condition = action.clone();
        false_condition.combined_score = Unit::parse("0").map_err(|error| error.to_string())?;
        assert!(!delegated(
            &mandate.autonomy,
            &false_condition,
            false,
            basic.clone()
        )?);

        let mut over_order = mandate.autonomy.clone();
        over_order.delegations[0].max_order_usd =
            SchemaDec::parse("949", DecGrammar::PositiveDecimal)
                .map_err(|error| error.to_string())?;
        assert!(!delegated(&over_order, &action, false, basic.clone())?);

        let mut over_total = mandate.autonomy.clone();
        over_total.delegations[0].max_order_usd =
            SchemaDec::parse("1000", DecGrammar::PositiveDecimal)
                .map_err(|error| error.to_string())?;
        over_total.delegations[0].max_total_usd =
            SchemaDec::parse("949", DecGrammar::PositiveDecimal)
                .map_err(|error| error.to_string())?;
        assert!(!delegated(&over_total, &action, false, basic.clone())?);

        let mut exhausted = mandate.autonomy.clone();
        exhausted.delegations[0].max_orders = 0;
        assert!(!delegated(&exhausted, &action, false, basic)?);
        Ok(())
    }
}
