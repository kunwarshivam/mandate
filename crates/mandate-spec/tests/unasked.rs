//! The unasked dollars (mandate spec §4.2; DEC-189, DEC-695), E10-7 slice S1a.
//!
//! The DEC-77 tests: every test here is pending on [`unasked_usd`]'s stub. The table's figures were
//! produced once by running `reference/mandate/ref.py`'s `unasked_usd` over the same scenarios.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{arr, edit, i, obj, s, with};
use mandate_canon::Value;
use mandate_domain::Environment;
use mandate_num::Usd;
use mandate_spec::document::{DelegationId, ProvenanceMap};
use mandate_spec::unasked::{DelegationUsage, EffectivePolicy, UnaskedInputs, unasked_usd};
use mandate_spec::validate::{ValidatedMandate, ValidationContext};
use mandate_spec::{Mandate, SpecError};
use mandate_time::{Date, UtcNanos};

/// The risk clock is 10:00 New York on 2026-09-24, a risk day that ends 14 hours later, at [`END`].
const NOW: &str = "2026-09-24T14:00:00.000000000Z";
const TWO_DAYS_AGO: &str = "2026-09-22T14:00:00.000000000Z";
const PAST: &str = "2026-09-24T13:00:00.000000000Z";
const NOW_1S: &str = "2026-09-24T14:00:01.000000000Z";
const LATER: &str = "2026-09-24T16:00:00.000000000Z";
const LATER_3H: &str = "2026-09-24T17:00:00.000000000Z";
const END: &str = "2026-09-25T04:00:00.000000000Z";
const END_1H: &str = "2026-09-25T05:00:00.000000000Z";
const WEEK: &str = "2026-10-01T14:00:00.000000000Z";

/// Dollars are counted in mills (thousandths) here, so the test adds integers, never floats.
fn mills(text: &str) -> i64 {
    let (whole, part) = text.split_once('.').unwrap_or((text, ""));
    let part = format!("{part:0<3}");
    whole.parse::<i64>().expect("dollars") * 1000 + part.parse::<i64>().expect("mills")
}

fn text(mills: i64) -> String {
    let fraction = format!("{:03}", mills % 1000);
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() {
        format!("{}", mills / 1000)
    } else {
        format!("{}.{fraction}", mills / 1000)
    }
}

fn instant(text: &str) -> UtcNanos {
    UtcNanos::parse(text).expect("an instant")
}

fn usd(mills: i64) -> Usd {
    Usd::parse(&text(mills)).expect("a dollar amount")
}

/// A condition as this test models it: only `order_usd` bounds and the catch-all purpose, which
/// every opening or increase matches.
#[derive(Debug, Clone)]
enum C {
    Cmp(&'static str, i64),
    Purpose,
    All(Vec<C>),
    Any(Vec<C>),
    Not(Box<C>),
}

fn cmp(op: &'static str, value: &str) -> C {
    C::Cmp(op, mills(value))
}
fn all(members: Vec<C>) -> C {
    C::All(members)
}
fn any(members: Vec<C>) -> C {
    C::Any(members)
}
fn not(member: C) -> C {
    C::Not(Box::new(member))
}

impl C {
    fn value(&self) -> Value {
        match self {
            C::Cmp(op, x) => obj(vec![
                ("field", s("order_usd")),
                ("op", s(op)),
                ("value", s(&text(*x))),
            ]),
            C::Purpose => obj(vec![
                ("field", s("purpose")),
                ("op", s("in")),
                ("value", arr(vec![s("increase"), s("open")])),
            ]),
            C::All(members) => obj(vec![("all", arr(members.iter().map(C::value).collect()))]),
            C::Any(members) => obj(vec![("any", arr(members.iter().map(C::value).collect()))]),
            C::Not(member) => obj(vec![("not", member.value())]),
        }
    }
}

/// One delegation: the rule it lifts (`None` is the default), its condition, caps, and window.
#[derive(Debug, Clone)]
struct Del {
    lifts: Option<usize>,
    when: C,
    max_order: i64,
    max_orders: u32,
    max_total: i64,
    window: (&'static str, &'static str),
}

fn del(
    lifts: Option<usize>,
    when: C,
    caps: (&str, u32, &str),
    window: (&'static str, &'static str),
) -> Del {
    Del {
        lifts,
        when,
        max_order: mills(caps.0),
        max_orders: caps.1,
        max_total: mills(caps.2),
        window,
    }
}

/// The autonomy block and order count a case varies, on the base mandate otherwise. Rules are
/// `r0`, `r1`, ... and delegations `d0`, `d1`, ... by position. `review_passed` is no review date,
/// or one that has (2026-09-23) or has not (2026-09-24) passed at the risk clock.
#[derive(Debug, Clone)]
struct Model {
    default: &'static str,
    rules: Vec<(&'static str, C)>,
    delegations: Vec<Del>,
    per_day: u32,
    review_passed: Option<bool>,
}

impl Model {
    fn base() -> Self {
        Model {
            default: "ask",
            rules: vec![],
            delegations: vec![],
            per_day: 50,
            review_passed: None,
        }
    }

    fn validated(&self) -> ValidatedMandate {
        let rules = self.rules.iter().enumerate().map(|(k, (then, when))| {
            obj(vec![
                ("id", s(&format!("r{k}"))),
                ("when", when.value()),
                ("then", s(then)),
            ])
        });
        let delegations = self.delegations.iter().enumerate().map(|(k, d)| {
            let lifts = d
                .lifts
                .map_or("default".to_owned(), |r| format!("rule:r{r}"));
            obj(vec![
                ("id", s(&format!("d{k}"))),
                ("lifts", s(&lifts)),
                ("when", d.when.value()),
                ("max_order_usd", s(&text(d.max_order))),
                ("max_orders", i(d.max_orders.into())),
                ("max_total_usd", s(&text(d.max_total))),
                ("starts_at", s(d.window.0)),
                ("expires_at", s(d.window.1)),
                ("source_approval_id", Value::Null),
            ])
        });
        let review = self
            .review_passed
            .map(|passed| s(["2026-09-24", "2026-09-23"][usize::from(passed)]));
        let changes = [
            ("/autonomy/default", Some(s(self.default))),
            ("/autonomy/rules", Some(arr(rules.collect()))),
            ("/autonomy/delegations", Some(arr(delegations.collect()))),
            ("/risk/max_orders_per_day", Some(i(self.per_day.into()))),
        ];
        let document = edit(&with("/autonomy/review_by", review), &changes);
        let document = Mandate::parse(&document).expect("the test mandate parses");
        ValidatedMandate::new(document, &context(), &[]).expect("the test mandate is valid")
    }
}

fn context() -> ValidationContext {
    ValidationContext {
        account_equity_usd: usd(25_000_000),
        other_allocations_usd: Usd::ZERO,
        validation_date: Date::parse("2026-09-20").expect("a date"),
        registry: None,
        provenance: ProvenanceMap::default(),
        workspace_users: 1,
        approver_users: 1,
        independent_approval_required: false,
        disclosures_accepted: BTreeSet::new(),
        instrument_groups: BTreeMap::new(),
        claimed_by_other_agents: BTreeSet::new(),
        connection_environment: Some(Environment::Paper),
        connection_loss_carry_usd: Usd::ZERO,
        eligibility_failures: BTreeSet::new(),
        previous_version: None,
        current_mandate_version: None,
    }
}

/// Usage per delegation by position, `None` for one with no entry.
type Usage = Vec<Option<(u32, i64)>>;

fn figure(
    model: &Model,
    now: Option<&str>,
    orders_today: Option<u32>,
    usage: Option<&Usage>,
    policy: Option<(bool, bool)>,
) -> Result<Option<Usd>, SpecError> {
    let usage = usage.map(|usage| {
        usage
            .iter()
            .enumerate()
            .filter_map(|(k, used)| {
                let (orders, total) = (*used)?;
                let id = DelegationId::parse(&format!("d{k}")).expect("a delegation id");
                let total_usd = usd(total);
                Some((id, DelegationUsage { orders, total_usd }))
            })
            .collect()
    });
    let inputs = UnaskedInputs {
        now: now.map(instant),
        orders_today,
        usage,
        policy: policy.map(|(auto_allowed, nonconforming)| EffectivePolicy {
            auto_allowed,
            nonconforming,
        }),
    };
    unasked_usd(&model.validated(), &inputs)
}

struct Case {
    name: &'static str,
    model: Model,
    now: Option<&'static str>,
    orders_today: Option<u32>,
    usage: Option<Vec<(usize, u32, &'static str)>>,
    policy: Option<(bool, bool)>,
    expect: Option<&'static str>,
}

impl Case {
    fn base() -> Self {
        Case {
            name: "",
            model: Model::base(),
            now: Some(NOW),
            orders_today: Some(0),
            usage: Some(vec![]),
            policy: None,
            expect: None,
        }
    }
}

/// `ref.py`'s `unasked_usd` over each scenario, written in as the figure it gave.
#[rustfmt::skip]
fn reference_cases() -> Vec<Case> {
    vec![
        Case { name: "an unknown risk clock is not known", model: Model { default: "auto", ..Model::base() }, now: None, expect: None, ..Case::base() },
        Case { name: "an unknown order count is not known", model: Model { default: "auto", ..Model::base() }, orders_today: None, expect: None, ..Case::base() },
        Case { name: "unknown delegation usage is not known", model: Model { default: "auto", ..Model::base() }, usage: None, expect: None, ..Case::base() },
        Case { name: "an unknown policy reads as allowing auto", model: Model { default: "auto", per_day: 2, ..Model::base() }, expect: Some("2000"), ..Case::base() },
        Case { name: "a passed review date is a known zero", model: Model { default: "auto", review_passed: Some(true), ..Model::base() }, expect: Some("0"), ..Case::base() },
        Case { name: "a review date that is today has not passed", model: Model { default: "auto", per_day: 2, review_passed: Some(false), ..Model::base() }, expect: Some("2000"), ..Case::base() },
        Case { name: "a policy forbidding auto is a known zero", model: Model { default: "auto", ..Model::base() }, policy: Some((false, false)), expect: Some("0"), ..Case::base() },
        Case { name: "a nonconforming version is a known zero", model: Model { default: "auto", ..Model::base() }, policy: Some((true, true)), expect: Some("0"), ..Case::base() },
        Case { name: "a conforming policy allowing auto leaves the figure", model: Model { default: "auto", per_day: 2, ..Model::base() }, policy: Some((true, false)), expect: Some("2000"), ..Case::base() },
        Case { name: "no auto path is a known zero", model: Model { rules: vec![("ask", cmp("gt", "900"))], ..Model::base() }, expect: Some("0"), ..Case::base() },
        Case { name: "an auto default counts max_order_usd per order left", model: Model { default: "auto", per_day: 3, ..Model::base() }, orders_today: Some(1), expect: Some("2000"), ..Case::base() },
        Case { name: "the figure has no gross headroom cap", model: Model { default: "auto", per_day: 5, ..Model::base() }, expect: Some("5000"), ..Case::base() },
        Case { name: "every order of the day used is zero", model: Model { default: "auto", per_day: 3, ..Model::base() }, orders_today: Some(3), expect: Some("0"), ..Case::base() },
        Case { name: "an auto rule's bound rounds the figure up to the cent", model: Model { rules: vec![("auto", cmp("lte", "450.505"))], per_day: 3, ..Model::base() }, expect: Some("1351.52"), ..Case::base() },
        Case { name: "a tenth of a cent rounds up to a cent", model: Model { rules: vec![("auto", cmp("lte", "0.001"))], per_day: 3, ..Model::base() }, expect: Some("0.01"), ..Case::base() },
        Case { name: "an any of two bounds gives the larger", model: Model { default: "deny", rules: vec![("auto", any(vec![cmp("lte", "100"), cmp("lte", "300.5")]))], per_day: 2, ..Model::base() }, expect: Some("601"), ..Case::base() },
        Case { name: "an any with an unbounded member bounds nothing", model: Model { default: "deny", rules: vec![("auto", any(vec![cmp("lte", "100"), C::Purpose]))], per_day: 2, ..Model::base() }, expect: Some("2000"), ..Case::base() },
        Case { name: "an all gives its tightest bound", model: Model { default: "deny", rules: vec![("auto", all(vec![cmp("lte", "900"), cmp("lt", "500"), C::Purpose]))], per_day: 2, ..Model::base() }, expect: Some("1000"), ..Case::base() },
        Case { name: "a not bounds nothing", model: Model { default: "deny", rules: vec![("auto", not(cmp("lte", "300")))], per_day: 2, ..Model::base() }, expect: Some("2000"), ..Case::base() },
        Case { name: "eq bounds", model: Model { default: "deny", rules: vec![("auto", cmp("eq", "300"))], per_day: 2, ..Model::base() }, expect: Some("600"), ..Case::base() },
        Case { name: "gt bounds nothing", model: Model { default: "deny", rules: vec![("auto", cmp("gt", "300"))], per_day: 2, ..Model::base() }, expect: Some("2000"), ..Case::base() },
        Case { name: "gte bounds nothing", model: Model { default: "deny", rules: vec![("auto", cmp("gte", "300"))], per_day: 2, ..Model::base() }, expect: Some("2000"), ..Case::base() },
        Case { name: "the largest auto rule's bound is the slice", model: Model { default: "deny", rules: vec![("auto", cmp("lte", "200")), ("auto", cmp("lte", "600"))], per_day: 2, ..Model::base() }, expect: Some("1200"), ..Case::base() },
        Case { name: "a delegation gives full slices and its partial last", model: Model { delegations: vec![del(None, C::Purpose, ("300", 3, "700"), (PAST, WEEK))], per_day: 10, ..Model::base() }, expect: Some("700"), ..Case::base() },
        Case { name: "a delegation's used orders and total are counted", model: Model { delegations: vec![del(None, C::Purpose, ("300", 3, "700"), (PAST, WEEK))], per_day: 10, ..Model::base() }, usage: Some(vec![(0, 1, "300")]), expect: Some("400"), ..Case::base() },
        Case { name: "a delegation with every order used gives nothing", model: Model { delegations: vec![del(None, C::Purpose, ("300", 3, "700"), (PAST, WEEK))], per_day: 10, ..Model::base() }, usage: Some(vec![(0, 3, "300")]), expect: Some("0"), ..Case::base() },
        Case { name: "a delegation with its total spent gives nothing", model: Model { delegations: vec![del(None, C::Purpose, ("300", 3, "700"), (PAST, WEEK))], per_day: 10, ..Model::base() }, usage: Some(vec![(0, 1, "700")]), expect: Some("0"), ..Case::base() },
        Case { name: "a delegation is bounded by its when and its lifted rule", model: Model { default: "deny", rules: vec![("ask", cmp("lte", "200"))], delegations: vec![del(Some(0), cmp("lte", "250"), ("1000", 2, "1000"), (PAST, WEEK))], per_day: 10, ..Model::base() }, expect: Some("400"), ..Case::base() },
        Case { name: "a delegation's when bounds its slices", model: Model { delegations: vec![del(None, cmp("lte", "250"), ("1000", 2, "1000"), (PAST, WEEK))], per_day: 10, ..Model::base() }, expect: Some("500"), ..Case::base() },
        Case { name: "the largest slices are taken first", model: Model { rules: vec![("auto", cmp("lte", "500"))], delegations: vec![del(None, C::Purpose, ("900", 1, "900"), (PAST, WEEK)), del(None, C::Purpose, ("300", 2, "600"), (PAST, WEEK))], per_day: 3, ..Model::base() }, expect: Some("1900"), ..Case::base() },
        Case { name: "orders_today reduces how many slices are taken", model: Model { rules: vec![("auto", cmp("lte", "500"))], delegations: vec![del(None, C::Purpose, ("900", 1, "900"), (PAST, WEEK)), del(None, C::Purpose, ("300", 2, "600"), (PAST, WEEK))], per_day: 3, ..Model::base() }, orders_today: Some(2), expect: Some("900"), ..Case::base() },
        Case { name: "delegation slices beyond the unlimited one", model: Model { rules: vec![("auto", cmp("lte", "100"))], delegations: vec![del(None, C::Purpose, ("900", 1, "900"), (PAST, WEEK)), del(None, C::Purpose, ("300", 2, "600"), (PAST, WEEK))], per_day: 10, ..Model::base() }, expect: Some("2200"), ..Case::base() },
        Case { name: "a delegation expiring at t gives nothing", model: Model { delegations: vec![del(None, C::Purpose, ("300", 3, "900"), (PAST, NOW))], per_day: 10, ..Model::base() }, expect: Some("0"), ..Case::base() },
        Case { name: "a delegation starting later today counts", model: Model { delegations: vec![del(None, C::Purpose, ("300", 3, "900"), (LATER, LATER_3H))], per_day: 10, ..Model::base() }, expect: Some("900"), ..Case::base() },
        Case { name: "a delegation that expired two days ago gives nothing", model: Model { delegations: vec![del(None, C::Purpose, ("300", 3, "900"), (TWO_DAYS_AGO, PAST))], per_day: 10, ..Model::base() }, expect: Some("0"), ..Case::base() },
        Case { name: "a delegation live for a second after t counts", model: Model { delegations: vec![del(None, C::Purpose, ("300", 3, "900"), (PAST, NOW_1S))], per_day: 10, ..Model::base() }, expect: Some("900"), ..Case::base() },
        Case { name: "a delegation starting at the day's end gives nothing", model: Model { delegations: vec![del(None, C::Purpose, ("300", 3, "900"), (END, END_1H))], per_day: 10, ..Model::base() }, expect: Some("0"), ..Case::base() },
    ]
}

/// §4.2 items 1 to 6 as the reference model computes them: unknown inputs, the known zeros, each
/// condition's order bound, delegation slices, the count, and rounding up to the cent.
#[test]
#[ignore = "pending E10-7"]
fn the_figure_matches_the_reference_model() {
    for case in reference_cases() {
        let usage = case.usage.as_ref().map(|rows| {
            let mut usage: Usage = vec![None; case.model.delegations.len()];
            for (k, orders, total) in rows {
                usage[*k] = Some((*orders, mills(total)));
            }
            usage
        });
        let got = figure(
            &case.model,
            case.now,
            case.orders_today,
            usage.as_ref(),
            case.policy,
        )
        .unwrap_or_else(|e| panic!("{}: {e:?}", case.name));
        let expect = case
            .expect
            .map(|figure| Usd::parse(figure).expect("a figure"));
        assert_eq!(got, expect, "{}", case.name);
    }
}
