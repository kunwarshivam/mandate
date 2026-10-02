//! Delegations as a document (mandate spec §3, §6.5, DEC-181): the parse of `$defs/delegation`,
//! V-022, V-023, V-041, V-042 and V-043 for a delegation, §9.2's `autonomy.delegations` row, and
//! DEC-353's two conditions in the autonomy row (#444, #471), with MI-29 as the founder worded it.
//!
//! E6-13's tests PR (DEC-77, DEC-420): every test here is pending on the stubs, which refuse a
//! document holding `delegations` as unimplemented and classify no autonomy block that holds one.
//! The runtime lift (§6.2 step 4a, MI-26 to MI-28) is E8-8's and is not tested here.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{arr, base, edit, i, obj, s, with};
use mandate_canon::Value;
use mandate_domain::{AutonomyDecision, Environment};
use mandate_num::Usd;
use mandate_spec::change::{ChangeClass, classify, classify_autonomy, classify_delegations};
use mandate_spec::condition::{Condition, ConditionField, ConditionValue, Operator};
use mandate_spec::document::{
    ApprovalId, Autonomy, ConnectionId, Delegation, DelegationId, Lifts, Pointer, Provenance,
    ProvenanceMap, Rule, RuleId, Source,
};
use mandate_spec::validate::{PreviousVersion, ValidationContext, validate};
use mandate_spec::{DecGrammar, Mandate, ParseError, SchemaDec, Violation};
use mandate_time::{Date, UtcNanos};
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

use AutonomyDecision::{Ask, Auto, Deny};
use ChangeClass::{Neutral, RiskIncreasing, RiskReducing};

const STARTS: &str = "2026-09-24T00:00:00.000000000Z";
const EXPIRES: &str = "2026-10-14T00:00:00.000000000Z";
const THIRTY_DAYS_LATER: &str = "2026-10-24T00:00:00.000000000Z";
const APPROVAL: &str = "0b3f2c1d-5e6f-4a7b-8c9d-0e1f2a3b4c5d";

/// One delegation as the document writes it: `d1` lifting the base's `large_orders` rule for an
/// opening or an increase, three orders of up to $1,000 and $3,000 in all, for 20 days.
fn delegation() -> Value {
    obj(vec![
        ("id", s("d1")),
        ("lifts", s("rule:large_orders")),
        (
            "when",
            obj(vec![
                ("field", s("purpose")),
                ("op", s("in")),
                ("value", arr(vec![s("increase"), s("open")])),
            ]),
        ),
        ("max_order_usd", s("1000")),
        ("max_orders", i(3)),
        ("max_total_usd", s("3000")),
        ("starts_at", s(STARTS)),
        ("expires_at", s(EXPIRES)),
        ("source_approval_id", Value::Null),
    ])
}

/// `delegation()` with some members replaced, or removed with `None`.
fn delegation_with(changes: &[(&str, Option<Value>)]) -> Value {
    let paths: Vec<(String, Option<Value>)> = changes
        .iter()
        .map(|(member, value)| (format!("/{member}"), value.clone()))
        .collect();
    let borrowed: Vec<(&str, Option<Value>)> = paths
        .iter()
        .map(|(path, value)| (path.as_str(), value.clone()))
        .collect();
    edit(&delegation(), &borrowed)
}

/// The base with these delegations.
fn delegated(delegations: Vec<Value>) -> Value {
    with("/autonomy/delegations", Some(arr(delegations)))
}

fn parse(document: &Value) -> Mandate {
    Mandate::parse(document).expect("the test document parses")
}

fn context(
    previous_version: Option<PreviousVersion>,
    provenance: ProvenanceMap,
) -> ValidationContext {
    ValidationContext {
        account_equity_usd: Usd::parse("25000").expect("a dollar amount"),
        other_allocations_usd: Usd::ZERO,
        validation_date: Date::parse("2026-09-24").expect("a date"),
        registry: None,
        provenance,
        workspace_users: 1,
        approver_users: 1,
        disclosures_accepted: BTreeSet::new(),
        instrument_groups: BTreeMap::new(),
        claimed_by_other_agents: BTreeSet::new(),
        connection_environment: Some(Environment::Paper),
        connection_loss_carry_usd: Usd::ZERO,
        eligibility_failures: BTreeSet::new(),
        previous_version,
    }
}

/// The V-codes the document breaks, with no previous version and nothing sourced.
fn codes(document: &Value) -> BTreeSet<Violation> {
    validate(&parse(document), &context(None, ProvenanceMap::default()))
        .expect("the document is evaluable")
        .violations
}

/// The V-codes a new version breaks against `previous`, whose document is given or withheld.
fn codes_after(previous: &Value, known: bool, document: &Value) -> BTreeSet<Violation> {
    let previous = PreviousVersion {
        environment: Environment::Paper,
        connection_id: ConnectionId::parse("conn_alpaca_paper_01").expect("a connection id"),
        mandate: known.then(|| parse(previous)),
    };
    validate(
        &parse(document),
        &context(Some(previous), ProvenanceMap::default()),
    )
    .expect("the document is evaluable")
    .violations
}

fn rule_id(id: &str) -> RuleId {
    RuleId::parse(id).expect("a rule id")
}

fn dec(text: &str) -> SchemaDec {
    SchemaDec::parse(text, DecGrammar::PositiveDecimal).expect("a positive decimal")
}

fn instant(text: &str) -> UtcNanos {
    UtcNanos::parse(text).expect("an instant")
}

#[test]
#[ignore = "pending E6-13"]
fn a_delegation_parses_member_by_member() {
    let mandate = parse(&delegated(vec![
        delegation(),
        delegation_with(&[
            ("id", Some(s("d2"))),
            ("lifts", Some(s("default"))),
            ("source_approval_id", Some(s(APPROVAL))),
        ]),
    ]));
    let expected = Delegation {
        id: DelegationId::parse("d1").expect("a delegation id"),
        lifts: Lifts::Rule(rule_id("large_orders")),
        when: Condition::Compare {
            field: ConditionField::Purpose,
            op: Operator::In,
            value: ConditionValue::List(vec!["increase".to_owned(), "open".to_owned()]),
        },
        max_order_usd: dec("1000"),
        max_orders: 3,
        max_total_usd: dec("3000"),
        starts_at: instant(STARTS),
        expires_at: instant(EXPIRES),
        source_approval_id: None,
    };
    let second = Delegation {
        id: DelegationId::parse("d2").expect("a delegation id"),
        lifts: Lifts::Default,
        source_approval_id: Some(ApprovalId::parse(APPROVAL).expect("an approval id")),
        ..expected.clone()
    };
    assert_eq!(mandate.autonomy.delegations, vec![expected, second]);
    assert!(
        parse(&base()).autonomy.delegations.is_empty(),
        "no member is no delegation"
    );
}

/// Each bound `$defs/delegation` and the autonomy block put on a delegation, refused with the code
/// and the pointer a strict parse names (ES-22): what `jsonschema` refuses, this refuses.
#[test]
#[ignore = "pending E6-13"]
fn every_schema_bound_of_a_delegation_is_refused_where_it_fails() {
    let at = |member: &str| Pointer::new(&format!("/autonomy/delegations/0/{member}"));
    let cases: Vec<(&str, Value, ParseError)> = vec![
        (
            "an id off the rule-id grammar",
            delegation_with(&[("id", Some(s("D1")))]),
            ParseError::OffPattern { path: at("id") },
        ),
        (
            "lifts naming neither the default nor a rule",
            delegation_with(&[("lifts", Some(s("defaults")))]),
            ParseError::OffPattern { path: at("lifts") },
        ),
        (
            "lifts with an empty rule id",
            delegation_with(&[("lifts", Some(s("rule:")))]),
            ParseError::OffPattern { path: at("lifts") },
        ),
        (
            "a cap written as a number",
            delegation_with(&[("max_order_usd", Some(i(1000)))]),
            ParseError::DecimalAsNumber {
                path: at("max_order_usd"),
            },
        ),
        (
            "a cap that is not a decimal",
            delegation_with(&[("max_total_usd", Some(s("1e3")))]),
            ParseError::OffGrammar {
                path: at("max_total_usd"),
                grammar: DecGrammar::PositiveDecimal,
            },
        ),
        (
            "no orders",
            delegation_with(&[("max_orders", Some(i(0)))]),
            ParseError::OutOfBounds {
                path: at("max_orders"),
            },
        ),
        (
            "1,001 orders",
            delegation_with(&[("max_orders", Some(i(1001)))]),
            ParseError::OutOfBounds {
                path: at("max_orders"),
            },
        ),
        (
            "an instant without its nine fractional digits",
            delegation_with(&[("starts_at", Some(s("2026-09-24T00:00:00Z")))]),
            ParseError::OffPattern {
                path: at("starts_at"),
            },
        ),
        (
            "an instant the calendar does not have",
            delegation_with(&[("expires_at", Some(s("2026-02-30T00:00:00.000000000Z")))]),
            ParseError::OffPattern {
                path: at("expires_at"),
            },
        ),
        (
            "an approval id that is not a lower-case uuid",
            delegation_with(&[(
                "source_approval_id",
                Some(s("0B3F2C1D-5E6F-4A7B-8C9D-0E1F2A3B4C5D")),
            )]),
            ParseError::OffPattern {
                path: at("source_approval_id"),
            },
        ),
        (
            "a member the schema does not list",
            delegation_with(&[("note", Some(s("for small buys")))]),
            ParseError::UnknownMember { path: at("note") },
        ),
        (
            "a required member missing",
            delegation_with(&[("when", None)]),
            ParseError::MissingMember { path: at("when") },
        ),
    ];
    for (what, item, expected) in cases {
        assert_eq!(
            Mandate::parse(&delegated(vec![item])),
            Err(expected),
            "{what}"
        );
    }
    let many: Vec<Value> = (0..21)
        .map(|n| delegation_with(&[("id", Some(s(&format!("d{n}"))))]))
        .collect();
    assert_eq!(
        Mandate::parse(&delegated(many)),
        Err(ParseError::OutOfBounds {
            path: Pointer::new("/autonomy/delegations"),
        }),
        "at most 20 delegations"
    );
}

/// V-041: ids unique; `lifts` is `default` while the default is `ask`, or names a rule whose `then`
/// is `ask`; and `starts_at` < `expires_at` ≤ `starts_at` + 30 days.
#[test]
#[ignore = "pending E6-13"]
fn v041_bounds_what_a_delegation_names_and_how_long_it_lasts() {
    let refused = |document: Value, what: &str| {
        assert!(
            codes(&document).contains(&Violation::V041),
            "{what} is V-041"
        );
    };
    let allowed = |document: Value, what: &str| {
        assert!(
            !codes(&document).contains(&Violation::V041),
            "{what} is not V-041"
        );
    };
    allowed(
        delegated(vec![delegation()]),
        "a delegation of the ask rule",
    );
    refused(
        delegated(vec![delegation(), delegation()]),
        "two delegations with one id",
    );
    allowed(
        delegated(vec![delegation_with(&[("lifts", Some(s("default")))])]),
        "the default lifted while it is ask",
    );
    refused(
        edit(
            &delegated(vec![delegation_with(&[("lifts", Some(s("default")))])]),
            &[("/autonomy/default", Some(s("deny")))],
        ),
        "the default lifted while it is deny",
    );
    refused(
        edit(
            &delegated(vec![delegation()]),
            &[("/autonomy/rules/0/then", Some(s("auto")))],
        ),
        "a rule lifted whose then is auto",
    );
    refused(
        delegated(vec![delegation_with(&[("lifts", Some(s("rule:missing")))])]),
        "a rule lifted that the version does not have",
    );
    refused(
        delegated(vec![delegation_with(&[("expires_at", Some(s(STARTS)))])]),
        "an empty window",
    );
    allowed(
        delegated(vec![delegation_with(&[(
            "expires_at",
            Some(s(THIRTY_DAYS_LATER)),
        )])]),
        "a window of exactly 30 days",
    );
    refused(
        delegated(vec![delegation_with(&[(
            "expires_at",
            Some(s("2026-10-24T00:00:00.000000001Z")),
        )])]),
        "a window one nanosecond past 30 days",
    );
}

/// V-043: `max_order_usd` ≤ `risk.max_order_usd` and ≤ `max_total_usd`, `max_total_usd` ≤
/// `capital.allocation_usd`, and with `two_approver_above_usd` set, `max_order_usd` ≤ it. Each edge
/// is allowed and one cent past it refused.
#[test]
#[ignore = "pending E6-13"]
fn v043_keeps_a_delegations_caps_inside_the_envelope() {
    let check = |changes: &[(&str, Option<Value>)], extra: &[(&str, Option<Value>)], v043: bool| {
        let document = edit(&delegated(vec![delegation_with(changes)]), extra);
        assert_eq!(
            codes(&document).contains(&Violation::V043),
            v043,
            "{changes:?} with {extra:?}"
        );
    };
    check(&[("max_order_usd", Some(s("1000")))], &[], false);
    check(&[("max_order_usd", Some(s("1000.01")))], &[], true);
    check(
        &[
            ("max_order_usd", Some(s("500"))),
            ("max_total_usd", Some(s("500"))),
        ],
        &[],
        false,
    );
    check(
        &[
            ("max_order_usd", Some(s("500.01"))),
            ("max_total_usd", Some(s("500"))),
        ],
        &[],
        true,
    );
    check(&[("max_total_usd", Some(s("10000")))], &[], false);
    check(&[("max_total_usd", Some(s("10000.01")))], &[], true);
    check(
        &[("max_order_usd", Some(s("800")))],
        &[("/autonomy/approval/two_approver_above_usd", Some(s("800")))],
        false,
    );
    check(
        &[("max_order_usd", Some(s("800.01")))],
        &[("/autonomy/approval/two_approver_above_usd", Some(s("800")))],
        true,
    );
}

/// V-042: a version risk-increasing on any path but the delegations and the review date, classified
/// with the delegations removed from both and the new version's review date in both, carries no
/// delegation over. A new id is not carried. A version increasing only because of what its own
/// delegations lift is not increasing on that basis, so it carries them (DEC-353 item 4). With the
/// previous document withheld, no delegation can be shown new, so V-042 refuses every one
/// (DEC-420).
#[test]
#[ignore = "pending E6-13"]
fn v042_carries_no_delegation_past_a_risk_increasing_version() {
    let previous = delegated(vec![delegation()]);
    let raised = edit(&previous, &[("/risk/max_daily_loss", Some(s("0.03")))]);
    assert!(
        codes_after(&previous, true, &raised).contains(&Violation::V042),
        "a raised limit with d1 carried"
    );
    let renamed = edit(&raised, &[("/autonomy/delegations/0/id", Some(s("d9")))]);
    assert!(
        !codes_after(&previous, true, &renamed).contains(&Violation::V042),
        "a raised limit with only new delegations"
    );
    let lowered = edit(&previous, &[("/risk/max_daily_loss", Some(s("0.01")))]);
    assert!(
        !codes_after(&previous, true, &lowered).contains(&Violation::V042),
        "a lowered limit carries d1"
    );
    let reviewed = edit(&previous, &[("/autonomy/review_by", Some(s("2026-12-31")))]);
    assert!(
        !codes_after(&previous, true, &reviewed).contains(&Violation::V042),
        "a review date set or moved is left out (DEC-188)"
    );
    let narrowed = edit(
        &previous,
        &[("/autonomy/delegations/0/max_orders", Some(i(2)))],
    );
    assert!(
        !codes_after(&previous, true, &narrowed).contains(&Violation::V042),
        "a delegation narrowed is left out"
    );
    let widened_by_its_own_lift = edit(
        &previous,
        &[("/autonomy/rules/0/when/value", Some(s("800")))],
    );
    assert_eq!(
        classify(&parse(&previous), &parse(&widened_by_its_own_lift))
            .expect("two parsed mandates classify")
            .class,
        RiskIncreasing,
        "widening the ask rule d1 lifts is increasing (DEC-353)"
    );
    assert!(
        !codes_after(&previous, true, &widened_by_its_own_lift).contains(&Violation::V042),
        "but only because of what its own delegation lifts, so d1 carries (DEC-353 item 4)"
    );
    assert!(
        codes_after(&previous, false, &lowered).contains(&Violation::V042),
        "with the previous document withheld, every delegation is refused"
    );
}

/// V-022: every delegation is `user_entered` and confirmed; V-023 types its `when` as it types a
/// rule's.
#[test]
#[ignore = "pending E6-13"]
fn a_delegation_is_the_owners_and_its_condition_is_typed() {
    let document = delegated(vec![delegation()]);
    let sourced = |source: Source, confirmed: bool| {
        let provenance = ProvenanceMap::new(BTreeMap::from([(
            Pointer::new("/autonomy/delegations"),
            Provenance { source, confirmed },
        )]));
        validate(&parse(&document), &context(None, provenance))
            .expect("the document is evaluable")
            .violations
    };
    for source in [
        Source::PlatformProposed,
        Source::PlatformDefault,
        Source::UserStated,
        Source::TemplateStructure,
    ] {
        assert!(
            sourced(source, true).contains(&Violation::V022),
            "a delegation from {source:?} is V-022"
        );
    }
    assert!(!sourced(Source::UserEntered, true).contains(&Violation::V022));
    assert!(
        sourced(Source::UserEntered, false).contains(&Violation::V022),
        "confirmed, too"
    );
    let mistyped = delegated(vec![delegation_with(&[(
        "when",
        Some(obj(vec![
            ("field", s("purpose")),
            ("op", s("in")),
            ("value", arr(vec![s("exit")])),
        ])),
    )])]);
    assert!(
        codes(&mistyped).contains(&Violation::V023),
        "purpose is open or increase only"
    );
}

/// One delegation as a typed value, for the classifier tests that build autonomy blocks directly.
fn typed(id: &str, lifts: Lifts) -> Delegation {
    Delegation {
        id: DelegationId::parse(id).expect("a delegation id"),
        lifts,
        when: Condition::Compare {
            field: ConditionField::Purpose,
            op: Operator::In,
            value: ConditionValue::List(vec!["increase".to_owned(), "open".to_owned()]),
        },
        max_order_usd: dec("1000"),
        max_orders: 3,
        max_total_usd: dec("3000"),
        starts_at: instant(STARTS),
        expires_at: instant(EXPIRES),
        source_approval_id: None,
    }
}

/// §9.2's `autonomy.delegations` row, matched by `id`: removing or narrowing is reducing; adding,
/// widening, reordering, or changing `lifts`, `when`, or `source_approval_id` is increasing.
#[test]
#[ignore = "pending E6-13"]
fn the_delegations_row_reduces_only_by_removing_or_narrowing() {
    let d1 = typed("d1", Lifts::Rule(rule_id("large_orders")));
    let d2 = typed("d2", Lifts::Default);
    let both = vec![d1.clone(), d2.clone()];
    let class = |new: Vec<Delegation>| {
        classify_delegations(&both, &new).expect("two delegation lists classify")
    };
    assert_eq!(class(both.clone()), Neutral, "unchanged");
    assert_eq!(class(vec![d1.clone()]), RiskReducing, "one removed");
    assert_eq!(class(Vec::new()), RiskReducing, "both removed");
    let narrowed = [
        Delegation {
            max_order_usd: dec("999.99"),
            ..d1.clone()
        },
        Delegation {
            max_orders: 2,
            ..d1.clone()
        },
        Delegation {
            max_total_usd: dec("2999.99"),
            ..d1.clone()
        },
        Delegation {
            starts_at: instant("2026-09-24T00:00:00.000000001Z"),
            ..d1.clone()
        },
        Delegation {
            expires_at: instant("2026-10-13T23:59:59.999999999Z"),
            ..d1.clone()
        },
    ];
    for d in narrowed {
        assert_eq!(class(vec![d.clone(), d2.clone()]), RiskReducing, "{d:?}");
    }
    let widened = [
        Delegation {
            max_order_usd: dec("1000.01"),
            ..d1.clone()
        },
        Delegation {
            max_orders: 4,
            ..d1.clone()
        },
        Delegation {
            max_total_usd: dec("3000.01"),
            ..d1.clone()
        },
        Delegation {
            starts_at: instant("2026-09-23T23:59:59.999999999Z"),
            ..d1.clone()
        },
        Delegation {
            expires_at: instant("2026-10-14T00:00:00.000000001Z"),
            ..d1.clone()
        },
        Delegation {
            lifts: Lifts::Default,
            ..d1.clone()
        },
        Delegation {
            when: Condition::Compare {
                field: ConditionField::Purpose,
                op: Operator::In,
                value: ConditionValue::List(vec!["open".to_owned()]),
            },
            ..d1.clone()
        },
        Delegation {
            source_approval_id: Some(ApprovalId::parse(APPROVAL).expect("an approval id")),
            ..d1.clone()
        },
    ];
    for d in widened {
        assert_eq!(class(vec![d.clone(), d2.clone()]), RiskIncreasing, "{d:?}");
    }
    assert_eq!(
        class(vec![d1.clone(), d2.clone(), typed("d3", Lifts::Default)]),
        RiskIncreasing,
        "one added"
    );
    assert_eq!(class(vec![d2, d1]), RiskIncreasing, "reordered");
}

/// The base's autonomy block with these rules, this default, and these delegations.
fn block(
    rules: &[(&str, Operator, &str, AutonomyDecision)],
    default: AutonomyDecision,
    delegations: Vec<Delegation>,
) -> Autonomy {
    Autonomy {
        rules: rules
            .iter()
            .map(|(id, op, value, then)| Rule {
                id: rule_id(id),
                when: Condition::Compare {
                    field: ConditionField::OrderUsd,
                    op: *op,
                    value: ConditionValue::Decimal(
                        SchemaDec::parse(value, DecGrammar::Decimal).expect("a decimal"),
                    ),
                },
                then: *then,
            })
            .collect(),
        default,
        delegations,
        ..parse(&base()).autonomy
    }
}

/// DEC-353 in the autonomy row, each shape beside its control with no delegation: widening an `ask`
/// rule a delegation lifts, and removing a non-`auto` rule ahead of a rule or the default a
/// delegation lifts, are increasing. Removing or narrowing an `auto` rule, or a kept rule's `then`
/// going `auto` to `ask`, stays reducing wherever its orders land, since they were `auto` and stay
/// `auto` (item 2). The MC-J cases pin the same shapes through the whole mandate.
#[test]
#[ignore = "pending E6-13"]
fn a_rule_change_that_routes_an_undelegated_ask_to_a_delegation_is_increasing() {
    use Operator::{Gt, Lt};
    let lifting = |source: Lifts| vec![typed("d1", source)];
    let large = Lifts::Rule(rule_id("large"));
    let low = Lifts::Rule(rule_id("low"));
    let class = |old: &Autonomy, new: &Autonomy| {
        classify_autonomy(old, new).expect("two autonomy blocks classify")
    };
    let pairs: Vec<(&str, Autonomy, Autonomy, ChangeClass, ChangeClass)> = vec![
        (
            "an ask rule widened",
            block(&[("large", Gt, "900", Ask)], Auto, Vec::new()),
            block(&[("large", Gt, "800", Ask)], Auto, Vec::new()),
            RiskIncreasing,
            RiskReducing,
        ),
        (
            "an ask rule removed ahead of a delegated rule",
            block(
                &[("large", Gt, "900", Ask), ("low", Lt, "500", Ask)],
                Ask,
                Vec::new(),
            ),
            block(&[("low", Lt, "500", Ask)], Ask, Vec::new()),
            RiskIncreasing,
            RiskReducing,
        ),
        (
            "an ask rule removed ahead of the delegated default",
            block(&[("large", Gt, "900", Ask)], Ask, Vec::new()),
            block(&[], Ask, Vec::new()),
            RiskIncreasing,
            RiskReducing,
        ),
        (
            "an auto rule removed ahead of a delegated rule",
            block(
                &[("small", Lt, "100", Auto), ("low", Lt, "500", Ask)],
                Ask,
                Vec::new(),
            ),
            block(&[("low", Lt, "500", Ask)], Ask, Vec::new()),
            RiskReducing,
            RiskReducing,
        ),
        (
            "an auto rule narrowed ahead of a delegated rule",
            block(
                &[("small", Lt, "100", Auto), ("low", Lt, "500", Ask)],
                Ask,
                Vec::new(),
            ),
            block(
                &[("small", Lt, "50", Auto), ("low", Lt, "500", Ask)],
                Ask,
                Vec::new(),
            ),
            RiskReducing,
            RiskReducing,
        ),
    ];
    let sources = [large.clone(), low.clone(), Lifts::Default, low.clone(), low];
    for ((what, old, new, delegated, control), source) in pairs.into_iter().zip(sources) {
        assert_eq!(class(&old, &new), control, "{what}, with no delegation");
        let (old, new) = (
            Autonomy {
                delegations: lifting(source.clone()),
                ..old
            },
            Autonomy {
                delegations: lifting(source),
                ..new
            },
        );
        assert_eq!(class(&old, &new), delegated, "{what}, with the delegation");
    }
    let auto_to_ask = (
        block(&[("large", Gt, "900", Auto)], Ask, lifting(large.clone())),
        block(&[("large", Gt, "900", Ask)], Ask, lifting(large)),
    );
    assert_eq!(
        class(&auto_to_ask.0, &auto_to_ask.1),
        RiskReducing,
        "a kept rule's then from auto to ask stays reducing, though V-041 refuses the old version"
    );
}

/// A rule in the MI-29 property's own terms: the order sizes it matches, and its verdict.
type Drawn = Vec<(bool, u32, AutonomyDecision)>;

/// The property's own §6.2 steps 4 and 4a, never the crate's: the first rule matching `order`
/// (`true` is `gt`, else `lt`, against a threshold in dollars), else the default; then an `ask`
/// whose source a delegation names is `auto`. The delegations here have no caps an order reaches
/// and a window that holds, so a lift turns on the source alone.
fn decide(
    rules: &Drawn,
    default: AutonomyDecision,
    lifted: &[Option<usize>],
    order: u32,
) -> AutonomyDecision {
    let found = rules.iter().position(|(gt, threshold, _)| {
        if *gt {
            order > *threshold
        } else {
            order < *threshold
        }
    });
    let decision = found.map_or(default, |n| rules[n].2);
    if decision == Ask && lifted.contains(&found) {
        Auto
    } else {
        decision
    }
}

/// `rules` as an autonomy block whose delegations lift the rules at `lifted` (`None` the default).
fn drawn_block(rules: &Drawn, default: AutonomyDecision, lifted: &[Option<usize>]) -> Autonomy {
    let ids: Vec<String> = (0..rules.len()).map(|n| format!("r{n}")).collect();
    let named: Vec<(&str, Operator, String, AutonomyDecision)> = rules
        .iter()
        .zip(&ids)
        .map(|((gt, threshold, then), id)| {
            (
                id.as_str(),
                if *gt { Operator::Gt } else { Operator::Lt },
                threshold.to_string(),
                *then,
            )
        })
        .collect();
    let borrowed: Vec<(&str, Operator, &str, AutonomyDecision)> = named
        .iter()
        .map(|(id, op, value, then)| (*id, *op, value.as_str(), *then))
        .collect();
    let delegations = lifted
        .iter()
        .enumerate()
        .map(|(n, source)| {
            typed(
                &format!("d{n}"),
                source.map_or(Lifts::Default, |k| Lifts::Rule(rule_id(&ids[k]))),
            )
        })
        .collect();
    block(&borrowed, default, delegations)
}

fn verdict() -> impl Strategy<Value = AutonomyDecision> {
    prop_oneof![Just(Auto), Just(Ask), Just(Deny)]
}

/// MI-29, as the founder worded it (DEC-353 item 3): a version classified reducing or neutral never
/// makes any decision less strict, with each version's delegations in force. The old version is
/// valid under V-041: each delegation names an `ask` it has. The rules change and the delegations
/// stand, or lose one, as the delegations row allows; rule ids are positions, so a kept id may change
/// its condition or its verdict. The oracle is [`decide`], which knows nothing of §9.2.
#[test]
#[ignore = "pending E6-13"]
fn a_reducing_rule_change_never_decides_less_strictly_with_the_delegations_in_force() {
    let mut runner = TestRunner::new(ProptestConfig {
        cases: 2048,
        max_shrink_iters: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    });
    let rules = || {
        prop::collection::vec(
            (any::<bool>(), (1u32..10).prop_map(|n| n * 100), verdict()),
            0..4,
        )
    };
    let draw = (
        rules(),
        verdict(),
        rules(),
        verdict(),
        prop::collection::vec(prop::option::of(0usize..4), 1..3),
        any::<bool>(),
    );
    let outcome = runner.run(
        &draw,
        |(old, old_default, new, new_default, lifted, drop_one)| {
            let lifted_old: Vec<Option<usize>> = lifted
                .iter()
                .filter(|s| match s {
                    None => old_default == Ask,
                    Some(k) => old.get(*k).is_some_and(|rule| rule.2 == Ask),
                })
                .copied()
                .collect();
            if lifted_old.is_empty() {
                return Ok(());
            }
            let lifted_new: Vec<Option<usize>> = if drop_one {
                lifted_old[1..].to_vec()
            } else {
                lifted_old.clone()
            };
            if lifted_new.iter().any(|s| s.is_some_and(|k| k >= new.len())) {
                return Ok(());
            }
            let before = drawn_block(&old, old_default, &lifted_old);
            let after = drawn_block(&new, new_default, &lifted_new);
            let class = classify_autonomy(&before, &after)
                .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
            let delegations_class = classify_delegations(&before.delegations, &after.delegations)
                .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
            if matches!(class.max(delegations_class), RiskReducing | Neutral) {
                for order in (50..=1050).step_by(50) {
                    prop_assert!(
                        decide(&new, new_default, &lifted_new, order)
                            >= decide(&old, old_default, &lifted_old, order),
                        "{:?} loosened ${}: {:?} {:?} {:?} to {:?} {:?} {:?}",
                        class,
                        order,
                        old,
                        old_default,
                        lifted_old,
                        new,
                        new_default,
                        lifted_new
                    );
                }
            }
            Ok(())
        },
    );
    if let Err(failure) = outcome {
        panic!("MI-29: {failure}");
    }
}

/// [`decide`] is the oracle the MI-29 property trusts, so it is checked on its own: the first match,
/// then the default, then a lift of an `ask` only, and only from the source a delegation names.
#[test]
fn the_mi29_oracle_lifts_only_the_ask_its_delegation_names() {
    let rules: Drawn = vec![(true, 900, Ask), (false, 100, Auto), (true, 500, Deny)];
    assert_eq!(decide(&rules, Ask, &[], 950), Ask, "the first match");
    assert_eq!(decide(&rules, Ask, &[Some(0)], 950), Auto, "its ask lifted");
    assert_eq!(
        decide(&rules, Ask, &[None], 950),
        Ask,
        "a lift of the default only"
    );
    assert_eq!(
        decide(&rules, Ask, &[Some(2)], 600),
        Deny,
        "a deny is never lifted"
    );
    assert_eq!(
        decide(&rules, Ask, &[Some(1)], 50),
        Auto,
        "an auto stays auto"
    );
    assert_eq!(
        decide(&rules, Ask, &[None], 300),
        Auto,
        "the default's ask lifted"
    );
    assert_eq!(
        decide(&rules, Deny, &[None], 300),
        Deny,
        "a deny default is never lifted"
    );
}
