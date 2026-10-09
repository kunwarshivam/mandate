//! The loss answer and the ladder proposed beneath it (mandate spec §7; DEC-182, DEC-695 items 6
//! and 7), E10-7 slice S1b.
//!
//! The DEC-77 tests: every test here is pending on the stubs of `loss_answer_fields` and
//! `proposed_ladder`. The tables' values were produced once by running the functions of the same
//! names in `reference/mandate/ref.py` over the same inputs. The property checks every draft
//! against the crate's `validate`, and its values against an exact oracle of its own, in integers
//! over powers of ten.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{arr, edit, obj, s, with};
use mandate_canon::Value;
use mandate_domain::Environment;
use mandate_num::{Ratio, Usd};
use mandate_spec::document::{
    LadderAction, LadderRung, Pointer, Provenance, ProvenanceMap, Source,
};
use mandate_spec::draft::{
    AskAgain, Draft, Drafted, LossAnswer, LossFields, ProposedLadder, loss_answer_fields,
    proposed_ladder,
};
use mandate_spec::validate::{ValidationContext, validate};
use mandate_spec::{DecGrammar, Mandate, SchemaDec};
use mandate_time::Date;
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

use AskAgain::{LadderCollapses, NoLoss, WholeAllocation};
use Source::{PlatformProposed, UserStated};

#[derive(Debug, Clone, Copy)]
enum Kind {
    Fraction,
    Usd,
}

fn answer(kind: Kind, value: &str) -> LossAnswer {
    match kind {
        Kind::Fraction => LossAnswer::Fraction(Ratio::parse(value).expect("a fraction")),
        Kind::Usd => LossAnswer::Usd(dollars(value)),
    }
}

fn dollars(text: &str) -> Usd {
    Usd::parse(text).expect("a dollar amount")
}

fn dec(text: &str) -> SchemaDec {
    SchemaDec::parse(text, DecGrammar::OpenFraction).expect("an open fraction")
}

fn drafted<T>(value: T, source: Source) -> Drafted<T> {
    Drafted { value, source }
}

fn fields(floor: &str, drawdown: &str, daily: &str) -> LossFields {
    LossFields {
        max_loss_from_allocation: drafted(dec(floor), UserStated),
        max_drawdown: drafted(dec(drawdown), PlatformProposed),
        max_daily_loss: drafted(dec(daily), PlatformProposed),
    }
}

fn ladder(halve: &str, exits: &str, flatten: &str, hysteresis: &str) -> ProposedLadder {
    let rung = |at: &str, action, factor: Option<&str>| LadderRung {
        at: dec(at),
        action,
        factor: factor.map(dec),
    };
    let rungs = vec![
        rung(halve, LadderAction::ScaleSizes, Some("0.5")),
        rung(exits, LadderAction::ExitsOnly, None),
        rung(flatten, LadderAction::FlattenAndPause, None),
    ];
    ProposedLadder {
        drawdown_ladder: drafted(rungs, PlatformProposed),
        hysteresis: drafted(dec(hysteresis), PlatformProposed),
    }
}

/// A row's expected draft as text, or why the answer is asked again: F, D, and the daily loss, or
/// the three rungs' `at` and the hysteresis.
type Expected<T> = Result<T, AskAgain>;
type Fields = (&'static str, &'static str, &'static str);
type Rungs = (&'static str, &'static str, &'static str, &'static str);

/// `ref.py`'s `loss_answer_fields` over each answer and allocation, as (F, D, daily loss), or the
/// reason it is asked again (`ref.py` gives `None`; the reason is F's, from DEC-695 items 6 and 7).
#[rustfmt::skip]
const LOSS: [(Kind, &str, &str, Expected<Fields>); 22] = [
        (Kind::Fraction, "0.1", "10000", Ok(("0.1", "0.08", "0.02"))),
        (Kind::Fraction, "0.05", "10000", Ok(("0.05", "0.04", "0.01"))),
        (Kind::Fraction, "0.2", "2500", Ok(("0.2", "0.16", "0.04"))),
        (Kind::Fraction, "0.12", "1000", Ok(("0.12", "0.096", "0.024"))),
        (Kind::Fraction, "0.12345", "10000", Ok(("0.1234", "0.09872", "0.02468"))),
        (Kind::Fraction, "0.002", "10000", Ok(("0.002", "0.0016", "0.0004"))),
        (Kind::Fraction, "0.0002", "10000", Err(LadderCollapses)),
        (Kind::Fraction, "0.0003", "10000", Err(LadderCollapses)),
        (Kind::Fraction, "0.0004", "10000", Err(LadderCollapses)),
        (Kind::Fraction, "0.0009", "10000", Err(LadderCollapses)),
        (Kind::Fraction, "0.00009", "10000", Err(NoLoss)),
        (Kind::Fraction, "0.9999", "1000", Ok(("0.9999", "0.79992", "0.19998"))),
        (Kind::Fraction, "1", "1000", Err(WholeAllocation)),
        (Kind::Fraction, "0", "1000", Err(NoLoss)),
        (Kind::Usd, "0", "1000", Err(NoLoss)),
        (Kind::Usd, "1", "10000", Err(LadderCollapses)),
        (Kind::Usd, "250", "2500", Ok(("0.1", "0.08", "0.02"))),
        (Kind::Usd, "999.99", "1000", Ok(("0.9999", "0.79992", "0.19998"))),
        (Kind::Usd, "1000", "1000", Err(WholeAllocation)),
        (Kind::Usd, "5000.55", "33333.33", Ok(("0.15", "0.12", "0.03"))),
        (Kind::Usd, "40000", "33333.33", Err(WholeAllocation)),
        (Kind::Usd, "250", "33333.33", Ok(("0.0075", "0.006", "0.0015"))),
];

/// `ref.py`'s `proposed_ladder` over each drawdown D, as the three rungs' `at` and the hysteresis.
#[rustfmt::skip]
const LADDER: [(&str, Expected<Rungs>); 14] = [
        ("0.08", Ok(("0.03", "0.06", "0.08", "0.01"))),
        ("0.096", Ok(("0.036", "0.072", "0.096", "0.012"))),
        ("0.04", Ok(("0.015", "0.03", "0.04", "0.005"))),
        ("0.16", Ok(("0.06", "0.12", "0.16", "0.02"))),
        ("0.09872", Ok(("0.037", "0.074", "0.09872", "0.0123"))),
        ("0.0016", Ok(("0.0006", "0.0012", "0.0016", "0.0002"))),
        ("0.0008", Ok(("0.0003", "0.0006", "0.0008", "0.0001"))),
        ("0.00079", Err(LadderCollapses)),
        ("0.0007", Err(LadderCollapses)),
        ("0.00024", Err(LadderCollapses)),
        ("0.00016", Err(LadderCollapses)),
        ("0.00072", Err(LadderCollapses)),
        ("0.0001", Err(LadderCollapses)),
        ("0.79992", Ok(("0.2999", "0.5999", "0.79992", "0.0999"))),
];

/// DEC-695 item 6: F rounded down to whole basis points is the floor, `user_stated`; D = 0.8 F and
/// the daily loss 0.2 F are `platform_proposed`; no loss, the whole allocation, and an F too small
/// for a ladder are asked again.
#[test]
#[ignore = "pending E10-7"]
fn the_loss_answer_matches_the_reference_model() {
    for (kind, value, allocation, expect) in LOSS {
        let got = loss_answer_fields(&answer(kind, value), dollars(allocation))
            .unwrap_or_else(|e| panic!("{kind:?} {value} of {allocation}: {e:?}"));
        let expect = match expect {
            Ok((floor, drawdown, daily)) => Draft::Proposed(fields(floor, drawdown, daily)),
            Err(why) => Draft::AskAgain(why),
        };
        assert_eq!(got, expect, "{kind:?} {value} of {allocation}");
    }
}

/// DEC-695 item 7: the halving at 0.375 D, exits only at 0.75 D, and the hysteresis at 0.125 D are
/// rounded down to basis points, the flatten rung is exactly D (V-011), and a ladder that rounding
/// collapses is refused, never proposed with a rung dropped.
#[test]
#[ignore = "pending E10-7"]
fn the_proposed_ladder_matches_the_reference_model() {
    for (drawdown, expect) in LADDER {
        let got = proposed_ladder(&dec(drawdown)).unwrap_or_else(|e| panic!("{drawdown}: {e:?}"));
        let expect = match expect {
            Ok((halve, exits, flatten, hysteresis)) => {
                Draft::Proposed(ladder(halve, exits, flatten, hysteresis))
            }
            Err(why) => Draft::AskAgain(why),
        };
        assert_eq!(got, expect, "{drawdown}");
    }
}

/// `units` ÷ 10^`places` as canonical decimal text.
fn decimal(units: u64, places: u32) -> String {
    let scale = 10u64.pow(places);
    let fraction = format!("{:0width$}", units % scale, width = places as usize);
    match fraction.trim_end_matches('0') {
        "" => format!("{}", units / scale),
        digits => format!("{}.{digits}", units / scale),
    }
}

/// ⌊`num` ÷ `den`⌋ in whole basis points, exactly.
fn floor_bp(num: u64, den: u64) -> u64 {
    num * 10_000 / den
}

fn context(provenance: ProvenanceMap) -> ValidationContext {
    ValidationContext {
        account_equity_usd: dollars("25000"),
        other_allocations_usd: Usd::ZERO,
        validation_date: Date::parse("2026-09-24").expect("a date"),
        registry: None,
        provenance,
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

/// The base mandate with the draft written in, and the provenance it carries, as the owner
/// confirms it.
fn drafted_mandate(f: &LossFields, l: &ProposedLadder) -> (Mandate, ProvenanceMap) {
    let rung = |r: &LadderRung| {
        let factor = r.factor.as_ref().map_or(Value::Null, |x| s(x.as_str()));
        obj(vec![
            ("at", s(r.at.as_str())),
            ("action", s(r.action.as_str())),
            ("factor", factor),
        ])
    };
    let rungs = arr(l.drawdown_ladder.value.iter().map(rung).collect());
    let scalars = [
        (
            "/capital/max_loss_from_allocation",
            &f.max_loss_from_allocation,
        ),
        ("/risk/max_drawdown", &f.max_drawdown),
        ("/risk/max_daily_loss", &f.max_daily_loss),
        ("/risk/hysteresis", &l.hysteresis),
    ];
    let changes: Vec<_> = scalars
        .iter()
        .map(|(p, d)| (*p, Some(s(d.value.as_str()))))
        .collect();
    let document = edit(&with("/risk/drawdown_ladder", Some(rungs)), &changes);
    let mut sources: Vec<_> = scalars.iter().map(|(p, d)| (*p, d.source)).collect();
    sources.push(("/risk/drawdown_ladder", l.drawdown_ladder.source));
    let confirmed = |source| Provenance {
        source,
        confirmed: true,
    };
    let provenance = sources
        .into_iter()
        .map(|(p, source)| (Pointer::new(p), confirmed(source)));
    let mandate = Mandate::parse(&document).expect("the drafted mandate parses");
    (mandate, ProvenanceMap::new(provenance.collect()))
}

/// DEC-695 items 6 and 7 over random answers (fractions to 6 places, dollars of four allocations;
/// half with F at most 20 bp, where ladders collapse): each is asked again exactly when the oracle
/// says so, else its fields and ladder are the oracle's and the draft breaks no V-rule.
#[test]
#[ignore = "pending E10-7"]
fn a_drafted_loss_answer_and_its_ladder_validate() {
    let mut config = ProptestConfig::with_cases(1024);
    config.failure_persistence = None;
    let draw = (
        any::<(bool, bool)>(),
        0u64..1_000_000_000,
        0usize..4,
        1u32..7,
    );
    let outcome = TestRunner::new(config).run(&draw, |((in_dollars, small), n, a, places)| {
        let allocation = [100_000, 250_000, 1_000_000, 3_333_333][a];
        let places = [places, 6][usize::from(small)];
        let scale = [10u64.pow(places), allocation][usize::from(in_dollars)];
        let units = n % ([scale * 6 / 5, scale / 500][usize::from(small)] + 1);
        let said = match in_dollars {
            true => LossAnswer::Usd(dollars(&decimal(units, 2))),
            false => answer(Kind::Fraction, &decimal(units, places)),
        };
        let f = floor_bp(units, scale);
        let got = loss_answer_fields(&said, dollars(&decimal(allocation, 2)))
            .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
        let dd = (8 * f, 100_000);
        let (halve, exits, hysteresis) = (
            floor_bp(3 * dd.0, 8 * dd.1),
            floor_bp(3 * dd.0, 4 * dd.1),
            floor_bp(dd.0, 8 * dd.1),
        );
        let refused = if f == 0 {
            Some(NoLoss)
        } else if f >= 10_000 {
            Some(WholeAllocation)
        } else {
            (!(0 < hysteresis && hysteresis < halve && halve < exits)).then_some(LadderCollapses)
        };
        let drafted = match (got, refused) {
            (Draft::AskAgain(why), Some(expected)) => {
                prop_assert_eq!(why, expected, "{:?}", said);
                return Ok(());
            }
            (Draft::Proposed(drafted), None) => drafted,
            (got, _) => {
                return Err(TestCaseError::fail(format!(
                    "{said:?}: {got:?}, oracle {refused:?}"
                )));
            }
        };
        let own = fields(&decimal(f, 4), &decimal(dd.0, 5), &decimal(2 * f, 5));
        prop_assert_eq!(&drafted, &own, "{:?}", said);
        let proposed = proposed_ladder(&drafted.max_drawdown.value)
            .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
        let own = ladder(
            &decimal(halve, 4),
            &decimal(exits, 4),
            &decimal(dd.0, 5),
            &decimal(hysteresis, 4),
        );
        prop_assert_eq!(&proposed, &Draft::Proposed(own.clone()), "{:?}", said);
        let (mandate, provenance) = drafted_mandate(&drafted, &own);
        let report = validate(&mandate, &context(provenance))
            .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
        prop_assert!(report.is_valid(), "{said:?}: {:?}", report.violations);
        Ok(())
    });
    if let Err(failure) = outcome {
        panic!("{failure}");
    }
}
