//! The unasked dollars (mandate spec §4.2; DEC-189, DEC-695), E10-7 slice S1a.
//!
//! The DEC-77 tests: every test here is pending on [`unasked_usd`]'s stub. The table's figures were
//! produced once by running `reference/mandate/ref.py`'s `unasked_usd` over the same scenarios. The
//! two properties check the figure against oracles of their own: a simulated risk day with its own
//! decision walk and accumulators (fuzz.py's `own_unasked_run`), and a greedy day in the family
//! where the bound is reached exactly (fuzz.py's `fuzz_unasked`).

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
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

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

/// The base mandate's `max_order_usd` and `max_gross_exposure_usd`, in mills.
const MAX_ORDER: i64 = 1_000_000;
const GROSS_LIMIT: i64 = 3_000_000;

/// The test's own reading of a condition, which the oracles below decide by: whether an opening or
/// an increase of `v` mills matches it, and every amount it compares against.
impl C {
    fn holds(&self, v: i64) -> bool {
        match self {
            C::Cmp("lt", x) => v < *x,
            C::Cmp("lte", x) => v <= *x,
            C::Cmp("eq", x) => v == *x,
            C::Cmp("gt", x) => v > *x,
            C::Cmp("gte", x) => v >= *x,
            C::Cmp(_, x) => v != *x,
            C::Purpose => true,
            C::All(members) => members.iter().all(|c| c.holds(v)),
            C::Any(members) => members.iter().any(|c| c.holds(v)),
            C::Not(member) => !member.holds(v),
        }
    }

    fn values(&self, out: &mut Vec<i64>) {
        match self {
            C::Cmp(_, x) => out.push(*x),
            C::Purpose => {}
            C::All(members) | C::Any(members) => members.iter().for_each(|c| c.values(out)),
            C::Not(member) => member.values(out),
        }
    }
}

/// Windows a random delegation takes: past, expiring at or just after the risk clock, live all
/// day, starting later today, starting at the day's end, and starting at the clock.
const WINDOWS: [(&str, &str); 7] = [
    (TWO_DAYS_AGO, PAST),
    (PAST, NOW),
    (PAST, NOW_1S),
    (PAST, WEEK),
    (LATER, LATER_3H),
    (END, END_1H),
    (NOW, WEEK),
];

/// An instant as seconds from the risk clock, which is how the oracles below keep time.
fn seconds(text: &str) -> i64 {
    instant(text).secs() - instant(NOW).secs()
}

/// A small deterministic generator, seeded by proptest, so a case is one `u64` and shrinks to it.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        (z ^ (z >> 31)) % n
    }
    fn pick<T: Clone>(&mut self, items: &[T]) -> T {
        items[self.below(items.len() as u64) as usize].clone()
    }
    fn usage(&mut self, model: &Model) -> Usage {
        let spent = |d: &Del| [0, 100_000, 899_500, d.max_total];
        let mut usage = vec![];
        for d in &model.delegations {
            let used = (self.below(2) == 0).then(|| {
                (
                    self.below(u64::from(d.max_orders) + 1) as u32,
                    self.pick(&spent(d)),
                )
            });
            usage.push(used);
        }
        usage
    }
}

/// The delegation an order of `v` mills at `t` seconds runs `auto` through (`Some(None)` for a rule
/// or the default), or `None` when it is not `auto`: §6.2 steps 4, 4a, and 5b walked here, and the
/// gate's per-order cap, with this test's own usage state.
fn decide(m: &Model, v: i64, t: i64, used: &Usage) -> Option<Option<usize>> {
    if m.review_passed == Some(true) || v <= 0 || v > MAX_ORDER {
        return None;
    }
    let source = m.rules.iter().position(|(_, when)| when.holds(v));
    match source.map_or(m.default, |r| m.rules[r].0) {
        "auto" => Some(None),
        "ask" => {
            let live = |(d, used): &(&Del, &Option<(u32, i64)>)| {
                let (orders, total) = used.unwrap_or((0, 0));
                d.lifts == source
                    && (seconds(d.window.0)..seconds(d.window.1)).contains(&t)
                    && orders < d.max_orders
                    && total + v <= d.max_total
                    && v <= d.max_order
                    && d.when.holds(v)
            };
            let k = m
                .delegations
                .iter()
                .zip(used)
                .position(|pair| live(&pair))?;
            Some(Some(k))
        }
        _ => None,
    }
}

/// The value one risk day runs `auto`: at each of `times`, the first candidate that runs `auto`
/// and fits the gross headroom, counted against the orders left today. With `frees`, a mark fall,
/// a cancelled order, or an exit may free headroom before each order. `greedy` offers the largest
/// amounts the mandate names; otherwise a random mix with the headroom itself.
fn run_day(
    m: &Model,
    today: u32,
    usage: &Usage,
    times: &[i64],
    rng: &mut Rng,
    greedy: bool,
) -> i64 {
    let (mut left, mut used, mut total) = (m.per_day.saturating_sub(today), usage.clone(), 0);
    let (mut equity, mut gross) = if greedy {
        (i64::MAX / 4, 0)
    } else {
        (
            rng.pick(&[10_000_000, 3_000_123, 800_000, -5_000]),
            rng.pick(&[0, 250_000, 1_999_990, 12_000_000]),
        )
    };
    for &t in times {
        if !greedy && gross > 0 && rng.below(2) == 0 {
            let freed = gross / rng.pick(&[1, 2]);
            equity -= if rng.below(10) < 3 { freed } else { 0 };
            gross -= freed;
        }
        if left == 0 {
            break;
        }
        let headroom = if greedy {
            i64::MAX / 4
        } else {
            GROSS_LIMIT.min(equity) - gross
        };
        let mut offers = vec![];
        if greedy {
            offers.push(MAX_ORDER);
            m.rules
                .iter()
                .for_each(|(_, when)| when.values(&mut offers));
            for (d, u) in m.delegations.iter().zip(&used) {
                offers.extend([d.max_order, d.max_total - u.map_or(0, |u| u.1)]);
                d.when.values(&mut offers);
            }
            offers.sort_unstable_by(|a, b| b.cmp(a));
        } else {
            let some = [
                10, 1_000, 99_990, 100_000, 300_000, 450_500, 500_000, 899_990, 900_000, 950_000,
            ];
            offers = (0..6)
                .map(|_| rng.pick(&[&some[..], &[MAX_ORDER, 5_000_000, headroom]].concat()))
                .collect();
        }
        for v in offers.into_iter().filter(|v| *v <= headroom) {
            let Some(via) = decide(m, v, t, &used) else {
                continue;
            };
            if let Some(k) = via {
                let (orders, spent) = used[k].unwrap_or((0, 0));
                used[k] = Some((orders + 1, spent + v));
            }
            (total, gross, left) = (total + v, gross + v, left - 1);
            break;
        }
    }
    total
}

fn random_condition(rng: &mut Rng) -> C {
    let leaf = |rng: &mut Rng| {
        let op = rng.pick(&["lt", "lte", "eq", "gt", "gte", "ne"]);
        C::Cmp(op, rng.pick(&[100_000, 300_000, 500_000, 900_000]))
    };
    let member = |rng: &mut Rng| {
        if rng.below(2) == 0 {
            leaf(rng)
        } else {
            C::Purpose
        }
    };
    match rng.below(5) {
        0 => leaf(rng),
        1 => C::Purpose,
        2 => not(leaf(rng)),
        3 => all((0..=rng.below(3)).map(|_| member(rng)).collect()),
        _ => any((0..=rng.below(3)).map(|_| member(rng)).collect()),
    }
}

/// A random delegation lifting one of the `ask`s in `m`, if it has any.
fn random_delegation(rng: &mut Rng, m: &Model, when: C, ends: &[&'static str]) -> Option<Del> {
    let mut asks: Vec<Option<usize>> = (0..m.rules.len())
        .filter(|r| m.rules[*r].0 == "ask")
        .map(Some)
        .collect();
    if m.default == "ask" {
        asks.push(None);
    }
    let lifts = *asks.get(rng.below(asks.len().max(1) as u64) as usize)?;
    let max_order = rng.pick(&[100_000, 300_000, 450_500, 900_000, 1_000_000]);
    let max_total = rng.pick(&[max_order, 2 * max_order, 2_500_000, 3_000_000]);
    let max_orders = 1 + rng.below(4) as u32;
    let window = if ends.is_empty() {
        rng.pick(&WINDOWS)
    } else {
        (PAST, rng.pick(ends))
    };
    Some(Del {
        lifts,
        when,
        max_order,
        max_orders,
        max_total,
        window,
    })
}

fn run(cases: u32, property: impl Fn(&mut Rng) -> Result<(), TestCaseError>) {
    let config = ProptestConfig {
        cases,
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    if let Err(failure) =
        TestRunner::new(config).run(&prop::num::u64::ANY, |seed| property(&mut Rng(seed)))
    {
        panic!("{failure}");
    }
}

fn known(figure: Result<Option<Usd>, SpecError>) -> Result<Usd, TestCaseError> {
    figure
        .map_err(|e| TestCaseError::fail(format!("{e:?}")))?
        .ok_or_else(|| TestCaseError::fail("every input is known, yet the figure is not"))
}

/// DEC-695 items 2 and 4: no risk day runs more unasked than the figure, whatever its orders'
/// values and times, while marks, cancels, and exits free gross headroom within the day.
#[test]
#[ignore = "pending E10-7"]
fn no_day_runs_more_unasked_than_the_figure() {
    run(512, |rng| {
        let mut m = Model::base();
        m.default = rng.pick(&["auto", "ask", "ask", "deny"]);
        m.rules = (0..rng.below(3))
            .map(|_| (rng.pick(&["auto", "ask", "deny"]), random_condition(rng)))
            .collect();
        for _ in 0..rng.below(3) {
            let when = if rng.below(2) == 0 {
                random_condition(rng)
            } else {
                C::Purpose
            };
            m.delegations.extend(random_delegation(rng, &m, when, &[]));
        }
        m.per_day = 1 + rng.below(8) as u32;
        m.review_passed = rng.pick(&[None, None, None, Some(false), Some(true)]);
        let today = rng.below(4) as u32;
        let usage = rng.usage(&m);
        let bound = known(figure(&m, Some(NOW), Some(today), Some(&usage), None))?;
        for _ in 0..3 {
            let mut times: Vec<i64> = (0..12)
                .map(|_| rng.below(seconds(END) as u64) as i64)
                .collect();
            times.sort_unstable();
            let ran = run_day(&m, today, &usage, &times, rng, false);
            prop_assert!(
                usd(ran) <= bound,
                "{m:?} {today} {usage:?}: ran {ran} mills, figure {bound}"
            );
        }
        Ok(())
    });
}

/// DEC-695's rationale: where every condition is a catch-all or an `lte` bound and no `ask` or
/// `deny` rule comes before an `auto` one, a greedy day reaches the figure, to the cent above.
#[test]
#[ignore = "pending E10-7"]
fn a_greedy_day_reaches_the_figure() {
    run(512, |rng| {
        let lte = |rng: &mut Rng| C::Cmp("lte", rng.pick(&[300_000, 450_505, 500_000, 900_000]));
        let bound = |rng: &mut Rng| match rng.below(4) {
            0 => C::Purpose,
            1 => lte(rng),
            2 => all(vec![lte(rng), lte(rng)]),
            _ => any(vec![lte(rng), lte(rng)]),
        };
        let mut m = Model::base();
        if rng.below(2) == 0 {
            m.rules = (0..rng.below(3)).map(|_| ("auto", bound(rng))).collect();
        } else {
            (m.default, m.rules) = ("deny", vec![("ask", bound(rng))]);
        }
        for _ in 0..rng.below(4) {
            let when = bound(rng);
            m.delegations
                .extend(random_delegation(rng, &m, when, &[NOW, NOW_1S, WEEK]));
        }
        m.per_day = 1 + rng.below(6) as u32;
        m.review_passed = (rng.below(5) == 0).then_some(true);
        let today = rng.below(4) as u32;
        let usage = rng.usage(&m);
        let fig = known(figure(&m, Some(NOW), Some(today), Some(&usage), None))?;
        let best = run_day(&m, today, &usage, &[0; 20], rng, true);
        let cents = (best + 9) / 10 * 10;
        prop_assert_eq!(
            fig,
            usd(cents),
            "{:?} {} {:?}: greedy {} mills",
            m,
            today,
            usage,
            best
        );
        Ok(())
    });
}
