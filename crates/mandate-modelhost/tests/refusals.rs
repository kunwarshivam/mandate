//! The model host's refusals and properties (E15-13 M1b; DEC-503 item 3, FT-4, FT-8, DEC-52).
//! Each change breaks one input of an agreeing case, and its refusal is the first of the brief's
//! checks it fails, in order: content, hashes, registry, parameters, instrument, closes, session,
//! count. The signal's oracle is an `i128` comparison of window sums in cents.

mod common;

use common::{Case, at, dec, id, long, params};
use std::collections::BTreeSet;

use mandate_canon::Digest;
use mandate_modelhost::{Evaluation, Refusal, Signal};
use mandate_num::Price;
use mandate_spec::document::{ModelId, ParamValue};
use mandate_time::{Date, ExchangeCalendar};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};

/// Each failure is its own typed refusal with its own stable code (ADR-0001 ES-09): an arithmetic
/// failure in the crossover is not a crossed window, an unrepresentable output is not an expiry,
/// and a content object the host cannot build is not an unknown model. A live test, as the
/// mutation gate needs before M2 (DEC-139).
#[test]
fn every_refusal_has_its_own_stable_code() {
    let rows = [
        (Refusal::Unimplemented { story: "E15-13" }, "unimplemented"),
        (Refusal::UnknownModel, "unknown_model"),
        (Refusal::PinHashMismatch, "pin_hash_mismatch"),
        (Refusal::NotRegistered, "not_registered"),
        (Refusal::RegistryMismatch, "registry_mismatch"),
        (Refusal::ParamKeys, "param_keys"),
        (
            Refusal::ParamValue {
                key: "slow_periods",
            },
            "param_value",
        ),
        (Refusal::WindowsCrossed, "windows_crossed"),
        (Refusal::WrongInstrument, "wrong_instrument"),
        (Refusal::ClosesIncomplete, "closes_incomplete"),
        (Refusal::ClosesEnd, "closes_end"),
        (Refusal::TooFewCloses, "too_few_closes"),
        (Refusal::CalendarCannotName, "calendar_cannot_name"),
        (Refusal::ExpiryOverflow, "expiry_overflow"),
        (Refusal::SignalArithmetic, "signal_arithmetic"),
        (Refusal::OutputUnrepresentable, "output_unrepresentable"),
        (Refusal::ContentObject, "content_object"),
    ];
    for (refusal, code) in &rows {
        assert_eq!(refusal.code(), *code, "{refusal:?}");
    }
    let distinct: BTreeSet<&str> = rows.iter().map(|(r, _)| r.code()).collect();
    assert_eq!(distinct.len(), rows.len(), "no two refusals share a code");
}

#[derive(Debug, Clone, Copy)]
enum Change {
    Id,
    Version,
    PinHash,
    NoEntry,
    EntryHash,
    EntryVersion,
    EntryParams,
    EntryAdmits,
    PinAdmits,
    MissingKey,
    ExtraKey,
    Text,
    Fraction,
    Zero,
    TooLong,
    Crossed,
    Instrument,
    Skipped,
    Repeated,
    Stale,
    BeforeClose,
    TwoCloses,
    PastCalendar,
}

fn fast_slow(case: &mut Case, fast: ParamValue, slow: &str) {
    case.pin.model.params = params(&[("fast_periods", fast), ("slow_periods", dec(slow))]);
}

fn days(case: &mut Case, days: [&str; 3]) {
    let days = days.into_iter().map(|d| Date::parse(d).unwrap());
    case.closes.closes = days.zip(case.closes.closes.iter().map(|c| c.1)).collect();
}

fn apply(case: &mut Case, change: Change) {
    let (model, entries) = (&mut case.pin.model, case.registry.values_mut());
    match change {
        Change::Id => model.id = ModelId::parse("quant.ma_crossover_b").unwrap(),
        Change::Version => model.version = "1.0.1".to_owned(),
        Change::PinHash => model.content_hash = Digest::of(br#"{"id","version"}"#),
        Change::NoEntry => case.registry.clear(),
        Change::EntryHash => entries.for_each(|e| e.content_hash = Digest::ZERO),
        Change::EntryVersion => entries.for_each(|e| e.version = "0.9.0".to_owned()),
        Change::EntryParams => entries.for_each(|e| e.params.retain(|k| k == "fast_periods")),
        Change::EntryAdmits => entries.for_each(|e| e.admits_instruments = true),
        Change::PinAdmits => model.admits_instruments = true,
        Change::MissingKey => model.params.truncate(1),
        Change::ExtraKey => model.params.extend(params(&[("collar", dec("0"))])),
        Change::Text => fast_slow(case, ParamValue::Text("2".to_owned()), "3"),
        Change::Fraction => fast_slow(case, dec("2.5"), "3"),
        Change::Zero => fast_slow(case, dec("0"), "3"),
        Change::TooLong => fast_slow(case, dec("2"), "1001"),
        Change::Crossed => fast_slow(case, dec("3"), "3"),
        Change::Instrument => case.closes.instrument_id = id("AAPL"),
        Change::Skipped => days(case, ["2026-10-02", "2026-10-05", "2026-10-07"]),
        Change::Repeated => days(case, ["2026-10-06", "2026-10-06", "2026-10-07"]),
        Change::Stale => days(case, ["2026-10-02", "2026-10-05", "2026-10-06"]),
        Change::BeforeClose => case.now = at("2026-10-07T19:59:59Z"),
        Change::TwoCloses => drop(case.closes.closes.drain(..1)),
        Change::PastCalendar => case.now = at("2029-01-01T05:00:00Z"),
    }
}

/// Every one-input disagreement is its own typed refusal, and no refusal is an output (INF-4).
#[test]
#[ignore = "pending E15-13"]
fn every_disagreement_is_its_own_refusal_and_no_output() {
    let fast = |key| Refusal::ParamValue { key };
    let rows = [
        (Change::Id, Refusal::UnknownModel),
        (Change::Version, Refusal::UnknownModel),
        (Change::PinHash, Refusal::PinHashMismatch),
        (Change::NoEntry, Refusal::NotRegistered),
        (Change::EntryHash, Refusal::RegistryMismatch),
        (Change::EntryVersion, Refusal::RegistryMismatch),
        (Change::EntryParams, Refusal::RegistryMismatch),
        (Change::EntryAdmits, Refusal::RegistryMismatch),
        (Change::PinAdmits, Refusal::RegistryMismatch),
        (Change::MissingKey, Refusal::ParamKeys),
        (Change::ExtraKey, Refusal::ParamKeys),
        (Change::Text, fast("fast_periods")),
        (Change::Fraction, fast("fast_periods")),
        (Change::Zero, fast("fast_periods")),
        (Change::TooLong, fast("slow_periods")),
        (Change::Crossed, Refusal::WindowsCrossed),
        (Change::Instrument, Refusal::WrongInstrument),
        (Change::Skipped, Refusal::ClosesIncomplete),
        (Change::Repeated, Refusal::ClosesIncomplete),
        (Change::Stale, Refusal::ClosesEnd),
        (Change::BeforeClose, Refusal::ClosesEnd),
        (Change::TwoCloses, Refusal::TooFewCloses),
        (Change::PastCalendar, Refusal::CalendarCannotName),
    ];
    for (change, refusal) in rows {
        let mut case = Case::rising();
        apply(&mut case, change);
        assert_eq!(case.run(), Err(refusal), "{change:?}");
    }
}

/// FT-4: an output exists only when the pin, the registry entry and the host agree. A random
/// non-empty set of the six identity changes is refused at the first check any of them fails.
#[test]
#[ignore = "pending E15-13"]
fn an_output_exists_only_when_pin_registry_and_host_agree() {
    assert_eq!(
        Case::rising().run(),
        Ok(long("2026-10-07T20:00:00Z")),
        "all agree"
    );
    let identity = [
        Change::Id,
        Change::Version,
        Change::PinHash,
        Change::EntryHash,
        Change::EntryVersion,
        Change::NoEntry,
    ];
    let config = Config {
        cases: 64,
        failure_persistence: None,
        ..Config::default()
    };
    let result = TestRunner::new(config).run(&(1u8..64), |mask| {
        let mut case = Case::rising();
        let on = |i: usize| mask & (1 << i) != 0;
        identity
            .iter()
            .enumerate()
            .filter(|(i, _)| on(*i))
            .for_each(|(_, c)| apply(&mut case, *c));
        let want = match (on(0) || on(1), on(2), on(5)) {
            (true, _, _) => Refusal::UnknownModel,
            (false, true, _) => Refusal::PinHashMismatch,
            (false, false, true) => Refusal::NotRegistered,
            (false, false, false) => Refusal::RegistryMismatch,
        };
        prop_assert_eq!(case.run(), Err(want), "mask {:06b}", mask);
        Ok(())
    });
    result.unwrap();
}

/// The signal against an independent oracle, and the same inputs giving the same evaluation:
/// random cents on the last `n` sessions to 2026-10-07 and windows `fast < slow <= n`; `Long`
/// exactly when `fast_sum × slow > slow_sum × fast` in `i128` cents.
#[test]
#[ignore = "pending E15-13"]
fn the_signal_is_the_window_means_comparison_and_evaluation_is_deterministic() {
    let calendar = ExchangeCalendar::us_equities().unwrap();
    let (mut sessions, mut day) = (Vec::new(), Date::parse("2026-09-01").unwrap());
    while day <= Date::parse("2026-10-07").unwrap() {
        if calendar.is_trading_day(day).unwrap() {
            sessions.push(day);
        }
        day = day.next().unwrap();
    }
    assert_eq!(
        sessions.len(),
        26,
        "21 September sessions after Labor Day, 5 in October"
    );
    let windows = |n: usize| (prop::collection::vec(1u32..=100_000, n), 1..n);
    let slow = |(cents, fast): (Vec<u32>, usize)| {
        let n = cents.len();
        (Just(cents), Just(fast), fast + 1..=n)
    };
    let strategy = (3usize..=25).prop_flat_map(windows).prop_flat_map(slow);
    let config = Config {
        cases: 64,
        failure_persistence: None,
        ..Config::default()
    };
    let result = TestRunner::new(config).run(&strategy, |(cents, fast, slow)| {
        let text = |c: u32| match (c / 100, c % 100) {
            (whole, 0) => whole.to_string(),
            (whole, part) if part % 10 == 0 => format!("{whole}.{}", part / 10),
            (whole, part) => format!("{whole}.{part:02}"),
        };
        let price = |c: &u32| Price::parse(&text(*c)).unwrap();
        let mut case = Case::rising();
        let days = sessions[sessions.len() - cents.len()..].iter().copied();
        case.closes.closes = days.zip(cents.iter().map(price)).collect();
        fast_slow(&mut case, dec(&fast.to_string()), &slow.to_string());
        let sum = |k: usize| {
            cents[cents.len() - k..]
                .iter()
                .map(|c| i128::from(*c))
                .sum::<i128>()
        };
        let (f, s) = (i128::try_from(fast).unwrap(), i128::try_from(slow).unwrap());
        let want = match sum(fast) * s > sum(slow) * f {
            true => long("2026-10-07T20:00:00Z"),
            false => Evaluation::NoOutput(Signal::Flat),
        };
        prop_assert_eq!(case.run(), Ok(want), "fast {} slow {}", fast, slow);
        prop_assert_eq!(
            case.run(),
            case.run(),
            "the same inputs, the same evaluation"
        );
        Ok(())
    });
    result.unwrap();
}
