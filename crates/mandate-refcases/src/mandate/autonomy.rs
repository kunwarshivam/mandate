//! Family A of the `mandate` suite: the sixteen `autonomy` cases (spec §6.2 steps 3 to 5, §6.4), run
//! through `mandate_builder::classify` (E6-2, DEC-130, DEC-162).
//!
//! **The policy is the parsed document's.** The case's patched base goes through the strict parse,
//! and `classify` reads that mandate's own `autonomy` block, not a projection of the JSON. It is not
//! validated: the case file says an `autonomy` case evaluates §6.2 alone, `generate.py` asserts only
//! the schema for family A, and no case states a validation context. Under the fixture's
//! `validation_context_defaults` MC-A10's two-approver threshold breaks V-024, which needs two approver
//! users where the defaults have one, so validating would fail a case for a rule it does not test.
//! `classify` re-checks the rules it reads itself (unique ids, V-017, V-018, V-023). No case states a
//! policy chain, and `PolicyOverlay::narrow` is not part of `classify` (DEC-128 item 9), so no overlay
//! is applied.
//!
//! **Every key is read (DEC-85).** The case, its `action`, and its `expect` are each swept against
//! the members this module reads, and a member it does not know fails the case naming it. An
//! opening action must state all fifteen §6.3 facts, because `ActionContext` infers none (DEC-130
//! item 19). A risk-reducing action must state its `purpose` and nothing else: §6.2 step 3 decides it
//! before any fact is read, so a fact it stated would be one nobody compared.
//!
//! **What the harness fills for a risk-reducing action.** `ActionContext` still needs a value in
//! every field, so [`unread_facts`] supplies zeros, `crypto`, and [`UNREAD_INSTRUMENT`], a
//! placeholder no opening may name: one that does fails naming its case. With those values MC-A01 to
//! MC-A04 must come back labelled `builtin_risk_reducing`, so a `classify` that sent a reducing
//! purpose through the rules fails on `by` (`rule:<id>`, `default`, or `admission_ceiling`). A
//! `classify` that consulted the filled values and still answered with the built-in would pass here;
//! that no facts and no rules change the answer is `mandate-builder`'s
//! `no_rule_set_ever_denies_or_asks_a_reducing_purpose`.
//!
//! **The approval is compared both ways.** An ASK must state `approvers_required` and `on_timeout`
//! and match both; an AUTO or a DENY carries no approval (§6.4), so a case that states either member
//! for one fails rather than having it ignored.
//!
//! **Every case is the order builder's own request** (DEC-262). No family-A case states who
//! asked, and §6.2 step 5a defines the order builder's own proposal as `requested_by: agent`, so
//! the harness passes [`RequestedBy::Agent`]; a case that stated `requested_by` would fail as an
//! unread member. The client ceiling is `mandate-builder`'s own tests' to pin.

use mandate_builder::{ActionContext, BuilderError, Classification, RequestedBy, classify};
use mandate_domain::{AssetClass, AssetId, AutonomyDecision, MarketSession, Purpose};
use mandate_num::{Signed, Unit, Usd};
use mandate_spec::document::OnTimeout;

use super::{at_of, must_parse, not_implemented, num, patched, unknown_members};
use crate::{Json, at, ensure, expect_eq, str_at, u64_at};

/// Every member a family-A case carries at its top level.
const CASE_KEYS: &[&str] = &["id", "kind", "title", "base", "patch", "action", "expect"];
/// The `action` of an opening: its purpose and the fourteen other §6.3 facts, each required.
const OPENING_KEYS: &[&str] = &[
    "purpose",
    "order_usd",
    "combined_score",
    "instrument",
    "asset_class",
    "session",
    "first_trade_in_instrument",
    "new_instrument",
    "thesis_confidence",
    "drawdown",
    "daily_pnl_fraction",
    "position_usd_after",
    "gross_usd_after",
    "bought_today_usd",
    "position_pnl_fraction",
];
/// The members an ASK's expectation adds to `decision` and `by`.
const APPROVAL_KEYS: [&str; 2] = ["approvers_required", "on_timeout"];
/// The instrument [`unread_facts`] names, which [`action`] refuses in every opening.
const UNREAD_INSTRUMENT: &str = "00000000-0000-4000-8000-000000000000";

/// `kind: autonomy` — one action classified against the case's patched mandate.
pub(super) fn autonomy_case(fixture: &Json, case: &Json) -> Result<(), String> {
    unknown_members(case, CASE_KEYS)
        .map_err(|unknown| format!("case keys not interpreted: {unknown}"))?;
    let mandate = must_parse(&patched(fixture, case)?)?;
    let action = action(str_at(case, "id")?, at_of(case, "action")?)?;
    let decided = classify(&mandate.autonomy, &action).map_err(builder)?;
    compare(at_of(case, "expect")?, &decided)
}

/// Case `id`'s `action` as the facts `classify` reads, every member of it read. An opening may not
/// name [`UNREAD_INSTRUMENT`], so no case is ever decided against the placeholder by accident.
fn action(id: &str, stated: &Json) -> Result<ActionContext, String> {
    let purpose = purpose(str_at(stated, "purpose")?)?;
    if purpose.reduces_risk() {
        unknown_members(stated, &["purpose"]).map_err(|unknown| {
            format!("`action` states facts a risk-reducing purpose never reads: {unknown}")
        })?;
        return Ok(ActionContext {
            purpose,
            ..unread_facts()?
        });
    }
    unknown_members(stated, OPENING_KEYS)
        .map_err(|unknown| format!("`action` members not interpreted: {unknown}"))?;
    let usd = |key: &str| num(Usd::parse(str_at(stated, key)?), key);
    let unit = |key: &str| num(Unit::parse(str_at(stated, key)?), key);
    let signed = |key: &str| num(Signed::parse(str_at(stated, key)?), key);
    let flag = |key: &str| {
        at(stated, key)?
            .as_bool()
            .ok_or_else(|| format!("`{key}` is not a boolean"))
    };
    let instrument = AssetId::parse(str_at(stated, "instrument")?)
        .map_err(|e| format!("`instrument`: {}", e.code()))?;
    ensure(instrument != unread_instrument()?, || {
        format!("{id}: `instrument` names the harness's placeholder {UNREAD_INSTRUMENT}")
    })?;
    Ok(ActionContext {
        purpose,
        order_usd: usd("order_usd")?,
        combined_score: unit("combined_score")?,
        instrument,
        asset_class: AssetClass::parse(str_at(stated, "asset_class")?)
            .map_err(|e| format!("`asset_class`: {}", e.code()))?,
        session: MarketSession::parse_condition_form(str_at(stated, "session")?)
            .map_err(|e| format!("`session`: {}", e.code()))?,
        first_trade_in_instrument: flag("first_trade_in_instrument")?,
        new_instrument: flag("new_instrument")?,
        thesis_confidence: unit("thesis_confidence")?,
        drawdown: unit("drawdown")?,
        daily_pnl_fraction: signed("daily_pnl_fraction")?,
        position_usd_after: usd("position_usd_after")?,
        gross_usd_after: usd("gross_usd_after")?,
        bought_today_usd: usd("bought_today_usd")?,
        position_pnl_fraction: signed("position_pnl_fraction")?,
        requested_by: RequestedBy::Agent,
    })
}

/// The facts a risk-reducing action does not state; the module doc says why they cannot matter.
fn unread_facts() -> Result<ActionContext, String> {
    Ok(ActionContext {
        purpose: Purpose::Open,
        order_usd: Usd::ZERO,
        combined_score: Unit::ZERO,
        instrument: unread_instrument()?,
        asset_class: AssetClass::Crypto,
        session: MarketSession::Crypto,
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
    })
}

fn unread_instrument() -> Result<AssetId, String> {
    AssetId::parse(UNREAD_INSTRUMENT).map_err(|e| format!("the unread instrument: {}", e.code()))
}

/// §6.1's six purposes, spelt as the fixture writes them.
pub(super) fn purpose(text: &str) -> Result<Purpose, String> {
    match text {
        "open" => Ok(Purpose::Open),
        "increase" => Ok(Purpose::Increase),
        "discretionary_exit" => Ok(Purpose::DiscretionaryExit),
        "owner_exit" => Ok(Purpose::OwnerExit),
        "risk_exit" => Ok(Purpose::RiskExit),
        "protective" => Ok(Purpose::Protective),
        other => Err(format!("`purpose`: `{other}` is not a §6.1 purpose")),
    }
}

/// Every member of the expectation: the decision, what decided it, and the approval, both ways.
pub(super) fn compare(expect: &Json, decided: &Classification) -> Result<(), String> {
    let decision = decision_name(decided.decision);
    let known: Vec<&str> = ["decision", "by"]
        .into_iter()
        .chain(APPROVAL_KEYS)
        .collect();
    unknown_members(expect, &known)
        .map_err(|unknown| format!("expectations not interpreted: {unknown}"))?;
    expect_eq("decision", decision, str_at(expect, "decision")?)?;
    expect_eq("by", decided.by.label(), str_at(expect, "by")?.to_owned())?;
    match decided.approval {
        Some(approval) => {
            expect_eq(
                "approvers_required",
                u64::from(approval.approvers_required.get()),
                u64_at(expect, "approvers_required")?,
            )?;
            expect_eq(
                "on_timeout",
                on_timeout_name(approval.on_timeout),
                str_at(expect, "on_timeout")?,
            )
        }
        None => APPROVAL_KEYS.iter().try_for_each(|key| {
            ensure(expect.get(*key).is_none(), || {
                format!("`{key}`: the case states one, but a `{decision}` carries no approval")
            })
        }),
    }
}

/// The §6.2 spelling of a decision, written here rather than taken from the crate: the fixture's
/// word is the expectation, so the harness must know it independently.
fn decision_name(decision: AutonomyDecision) -> &'static str {
    match decision {
        AutonomyDecision::Auto => "auto",
        AutonomyDecision::Ask => "ask",
        AutonomyDecision::Deny => "deny",
    }
}

fn on_timeout_name(on_timeout: OnTimeout) -> &'static str {
    match on_timeout {
        OnTimeout::Skip => "skip",
    }
}

/// A `mandate-builder` refusal, with `unimplemented` turned into the message DEC-77 requires.
fn builder(error: BuilderError) -> String {
    if error.code() == "unimplemented" {
        not_implemented("`mandate_builder::classify`")
    } else {
        format!("`mandate_builder::classify`: {error} ({})", error.code())
    }
}

/// The harness's own oracle: a family-A case passes only because every member was read and
/// compared. Each test doctors the real fixture and requires the case to fail naming what changed.
#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use mandate_num::Ratio;
    use mandate_spec::condition::{ConditionField, Facts, FieldKind};
    use serde_json::json;

    use crate::{Json, mandate, read_fixture};

    /// A real case member at the wrong level: other families carry `input` at a case's top level,
    /// so the suite's shared `unread_keys` accepts it there and only this module's own sweep refuses
    /// it. No `action` or `expect` has an `input` member either.
    const PLANTED: &str = "input";

    /// §6.3's fields, less the reserved `unusual_input`, which no action states (V-018).
    const FIELDS: [ConditionField; 15] = [
        ConditionField::Purpose,
        ConditionField::OrderUsd,
        ConditionField::CombinedScore,
        ConditionField::Instrument,
        ConditionField::AssetClass,
        ConditionField::Session,
        ConditionField::FirstTradeInInstrument,
        ConditionField::NewInstrument,
        ConditionField::ThesisConfidence,
        ConditionField::Drawdown,
        ConditionField::DailyPnlFraction,
        ConditionField::PositionUsdAfter,
        ConditionField::GrossUsdAfter,
        ConditionField::BoughtTodayUsd,
        ConditionField::PositionPnlFraction,
    ];

    fn fixture() -> Result<Json, String> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases");
        read_fixture(&dir, "mandate.json").map(Arc::unwrap_or_clone)
    }

    fn family_a(fixture: &Json) -> Result<Vec<Json>, String> {
        let cases: Vec<Json> = crate::list_at(fixture, "cases")?
            .iter()
            .filter(|c| c["kind"] == "autonomy")
            .cloned()
            .collect();
        crate::ensure(cases.len() == 16, || {
            format!("family A is sixteen cases, found {}", cases.len())
        })?;
        Ok(cases)
    }

    fn id(case: &Json) -> Result<String, String> {
        crate::str_at(case, "id").map(str::to_owned)
    }

    fn run(fixture: Json, id: &str) -> Result<(), String> {
        let wanted = format!("mandate::{id}");
        let case = mandate::cases(&Arc::new(fixture))
            .into_iter()
            .find(|c| c.id == wanted)
            .ok_or_else(|| format!("no case {wanted}"))?;
        (case.run)()
    }

    /// The fixture with one case's member at `pointer` rewritten by `doctor`.
    fn doctored(
        fixture: &Json,
        id: &str,
        pointer: &str,
        doctor: impl FnOnce(&mut Json),
    ) -> Result<Json, String> {
        let mut copy = fixture.clone();
        let case = copy
            .get_mut("cases")
            .and_then(Json::as_array_mut)
            .and_then(|cases| cases.iter_mut().find(|c| c["id"] == id))
            .ok_or_else(|| format!("no case {id}"))?;
        doctor(
            case.pointer_mut(pointer)
                .ok_or_else(|| format!("{id} has no {pointer}"))?,
        );
        Ok(copy)
    }

    /// The doctored case must fail with exactly `expected`, never a message that merely contains
    /// it. A member name alone is too generic a needle: `on_timeout` is in its edited, dropped, and
    /// stated failures alike, so a test of one would pass on either of the others.
    fn fails_with(result: Result<(), String>, expected: &str, what: &str) -> Result<(), String> {
        match result {
            Ok(()) => Err(format!("{what}: the case still passed")),
            Err(e) if e == expected => Ok(()),
            Err(e) => Err(format!(
                "{what}: expected the failure `{expected}`, got `{e}`"
            )),
        }
    }

    /// A value of the same JSON type that no family-A expectation holds.
    fn changed(value: &Json) -> Json {
        match value {
            Json::Number(n) => json!(n.as_u64().unwrap_or_default().saturating_add(1)),
            Json::String(s) => Json::String(format!("{s}x")),
            Json::Bool(b) => Json::Bool(!b),
            _ => Json::String(PLANTED.to_owned()),
        }
    }

    #[test]
    fn every_autonomy_case_passes_as_stated() -> Result<(), String> {
        let fixture = fixture()?;
        for case in family_a(&fixture)? {
            let id = id(&case)?;
            run(fixture.clone(), &id).map_err(|e| format!("{id}: {e}"))?;
        }
        Ok(())
    }

    /// Every expected member is compared and none is optional: each one edited, and each one
    /// dropped, fails the case naming it; a member planted beside them fails naming the plant; and
    /// an AUTO or a DENY that states an approval fails naming the approval member.
    #[test]
    fn every_expected_member_is_compared_and_required() -> Result<(), String> {
        let fixture = fixture()?;
        let mut doctorings = 0_usize;
        for case in family_a(&fixture)? {
            let id = id(&case)?;
            let expect = crate::at(&case, "expect")?
                .as_object()
                .ok_or("an expectation object")?;
            for (member, value) in expect {
                let pointer = format!("/expect/{member}");
                let edit = changed(value);
                let edited = doctored(&fixture, &id, &pointer, |v| *v = edit.clone())?;
                fails_with(
                    run(edited, &id),
                    &format!("{member}: expected {edit}, got {value}"),
                    &format!("{id}: {member} edited"),
                )?;
                let dropped = doctored(&fixture, &id, "/expect", |e| {
                    e.as_object_mut().map(|m| m.remove(member));
                })?;
                fails_with(
                    run(dropped, &id),
                    &format!("fixture has no `{member}`"),
                    &format!("{id}: {member} dropped"),
                )?;
                doctorings = doctorings.saturating_add(2);
            }
            let planted = doctored(&fixture, &id, "/expect", |e| {
                e.as_object_mut()
                    .map(|m| m.insert(PLANTED.to_owned(), Json::Null));
            })?;
            fails_with(
                run(planted, &id),
                &format!("expectations not interpreted: {PLANTED}"),
                &format!("{id}: a plant"),
            )?;
            if expect.contains_key("approvers_required") {
                continue;
            }
            let decision = crate::str_at(&case, "expect.decision")?;
            for (member, value) in [
                ("approvers_required", json!(1)),
                ("on_timeout", json!("skip")),
            ] {
                let stated = doctored(&fixture, &id, "/expect", |e| {
                    e.as_object_mut()
                        .map(|m| m.insert(member.to_owned(), value.clone()));
                })?;
                fails_with(
                    run(stated, &id),
                    &format!(
                        "`{member}`: the case states one, but a `{decision}` carries no approval"
                    ),
                    &format!("{id}: {member} stated"),
                )?;
                doctorings = doctorings.saturating_add(1);
            }
        }
        crate::expect_eq(
            "doctorings, seven ASKs of four members and nine others of two",
            doctorings,
            2 * (7 * 4 + 9 * 2) + 9 * 2,
        )
    }

    /// Every opening fact is required and every other member refused: dropping any member of an
    /// opening's `action` fails naming it, and a member planted in any action fails naming the
    /// plant, a fact planted in a risk-reducing action included. So does [`PLANTED`] at the case's
    /// top level, which the suite's shared sweep accepts, so the module's own sweep is under test.
    #[test]
    fn every_action_member_is_required_and_no_other_is_accepted() -> Result<(), String> {
        crate::ensure(
            mandate::CASE_KEYS.contains(&PLANTED) && !super::CASE_KEYS.contains(&PLANTED),
            || format!("`{PLANTED}` must pass the shared top-level sweep and fail this module's"),
        )?;
        let fixture = fixture()?;
        let mut openings = 0_usize;
        for case in family_a(&fixture)? {
            let id = id(&case)?;
            let action = crate::at(&case, "action")?
                .as_object()
                .ok_or("an action object")?;
            let reducing = action.len() == 1;
            if !reducing {
                openings = openings.saturating_add(1);
                crate::expect_eq(
                    "an opening's facts",
                    action.len(),
                    super::OPENING_KEYS.len(),
                )?;
                for member in action.keys() {
                    let dropped = doctored(&fixture, &id, "/action", |a| {
                        a.as_object_mut().map(|m| m.remove(member));
                    })?;
                    fails_with(
                        run(dropped, &id),
                        &format!("fixture has no `{member}`"),
                        &format!("{id}: {member} dropped"),
                    )?;
                }
            }
            let (plant, refusal) = if reducing {
                (
                    "order_usd",
                    "`action` states facts a risk-reducing purpose never reads: order_usd",
                )
            } else {
                (PLANTED, "`action` members not interpreted: input")
            };
            let planted = doctored(&fixture, &id, "/action", |a| {
                a.as_object_mut()
                    .map(|m| m.insert(plant.to_owned(), json!("1")));
            })?;
            fails_with(
                run(planted, &id),
                refusal,
                &format!("{id}: {plant} planted"),
            )?;
            let top = doctored(&fixture, &id, "", |c| {
                c.as_object_mut()
                    .map(|m| m.insert(PLANTED.to_owned(), Json::Null));
            })?;
            fails_with(
                run(top, &id),
                &format!("case keys not interpreted: {PLANTED}"),
                &format!("{id}: a top-level plant"),
            )?;
        }
        crate::expect_eq("openings", openings, 12)
    }

    /// No case is decided against the placeholder [`super::unread_facts`] fills: every opening,
    /// doctored to name it as its `instrument`, fails naming the case and the placeholder.
    #[test]
    fn no_opening_may_name_the_unread_instrument() -> Result<(), String> {
        let fixture = fixture()?;
        let mut openings = 0_usize;
        for case in family_a(&fixture)? {
            let id = id(&case)?;
            if crate::at(&case, "action")?.get("instrument").is_none() {
                continue;
            }
            let named = doctored(&fixture, &id, "/action/instrument", |i| {
                *i = json!("00000000-0000-4000-8000-000000000000");
            })?;
            fails_with(
                run(named, &id),
                &format!(
                    "{id}: `instrument` names the harness's placeholder \
                     00000000-0000-4000-8000-000000000000"
                ),
                &format!("{id}: the placeholder named"),
            )?;
            openings = openings.saturating_add(1);
        }
        crate::expect_eq("openings", openings, 12)
    }

    /// Each stated fact reaches `classify` under its own §6.3 name: the facts the harness builds,
    /// read back through the `Facts` projection a rule reads, equal the fixture's values field by
    /// field. A harness that wired `thesis_confidence` into `drawdown`, say, fails here even where no
    /// case's rules would notice.
    #[test]
    fn every_stated_fact_reaches_its_own_field() -> Result<(), String> {
        let fixture = fixture()?;
        let mut compared = 0_usize;
        for case in family_a(&fixture)? {
            let id = id(&case)?;
            let stated = crate::at(&case, "action")?;
            if stated.as_object().is_some_and(|m| m.len() == 1) {
                continue;
            }
            let facts = super::action(&id, stated).map_err(|e| format!("{id}: {e}"))?;
            for field in FIELDS {
                let name = field.as_str();
                let wanted = &stated[name];
                let same = match field.kind() {
                    FieldKind::Bool => facts.bool_field(field) == wanted.as_bool(),
                    FieldKind::Enum | FieldKind::Text => facts.enum_field(field) == wanted.as_str(),
                    FieldKind::Decimal => {
                        let text = wanted.as_str().ok_or_else(|| format!("{id}: {name}"))?;
                        facts.decimal_field(field)
                            == Some(Ratio::parse(text).map_err(|e| format!("{id}: {e}"))?)
                    }
                };
                crate::ensure(same, || {
                    format!("{id}: `{name}` did not reach its own field")
                })?;
                compared = compared.saturating_add(1);
            }
        }
        crate::expect_eq("facts compared", compared, 12 * 15)
    }
}
