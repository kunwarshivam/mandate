//! `quant.ma_crossover` 1.1.0 (DEC-2390): 1.0.0's crossover whose `Flat` is an exit output,
//! direction `long` with conviction −1 and confidence 1 (mandate spec §8.2), so a crossover down
//! reaches the builder's §8.3 discretionary exit. The oracles are written out by hand: the content
//! object as canonical JSON text with the listed files' SHA-256 read from disk, and the crossover in
//! integer cents. 1.0.0 is unchanged beside it.

mod common;

use std::fs;

use common::{AGE, Case, ID, VERSION, WEEK, at, hash, long};
use mandate_canon::Digest;
use mandate_modelhost::{Content, Evaluation, Refusal, Signal, content};
use mandate_num::{Conviction, Unit};
use mandate_runtime::{ModelDirection, ModelOutput, RiskClock};
use proptest::prelude::*;

const EXIT_VERSION: &str = "1.1.0";
const LISTED: [&str; 3] = [
    "crates/mandate-backtest/src/strategy/ma_crossover.rs",
    "crates/mandate-modelhost/src/ma_crossover.rs",
    "crates/mandate-modelhost/src/ma_crossover_exit.rs",
];
const NOW: &str = "2026-10-07T21:00:00Z";
const CLOSE: &str = "2026-10-07T20:00:00Z";

/// DEC-504 item 1's object for 1.1.0, in canonical form, each listed file hashed from disk.
fn oracle_content() -> String {
    let code = LISTED.map(|path| {
        let bytes = fs::read(format!("{}/../../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        format!(r#"{{"path":"{path}","sha256":"{}"}}"#, Digest::of(&bytes))
    });
    format!(
        concat!(
            r#"{{"authorship":"platform","code":[{},{},{}],"content_version":1,"inputs":{{"bars":"#,
            r#""daily_close","instrument":"pinned","minimum_count":"slow_periods","trading_day":"#,
            r#""us_equity_regular_session"}},"kind":"quant_model_content","methodology":"Long "#,
            r#"with conviction 1 when the mean of the last fast_periods daily closes is strictly "#,
            r#"above the mean of the last slow_periods daily closes, compared by cross-multiplying "#,
            r#"the window sums; otherwise long with conviction -1, which favors exiting.","#,
            r#""model_id":"quant.ma_crossover","model_version":"1.1.0","output":{{"flat":"#,
            r#"{{"confidence":"1","conviction":"-1","direction":"long"}},"long":{{"confidence":"#,
            r#""1","conviction":"1","direction":"long"}},"undecided":"none"}},"params_schema":"#,
            r#"{{"constraints":["fast_periods < slow_periods"],"params":[{{"max":1000,"min":1,"#,
            r#""name":"fast_periods","type":"integer"}},{{"max":1000,"min":2,"name":"#,
            r#""slow_periods","type":"integer"}}]}}}}"#
        ),
        code[0], code[1], code[2]
    )
}

fn exit_hash() -> Digest {
    Digest::of(oracle_content().as_bytes())
}

/// `case` with its pin and its registry entry naming `version` under `hash`.
fn pinning(mut case: Case, version: &str, hash: Digest) -> Case {
    case.pin.model.version = version.to_owned();
    case.pin.model.content_hash = hash;
    for entry in case.registry.values_mut() {
        entry.version = version.to_owned();
        entry.content_hash = hash;
    }
    case
}

/// The case pinning 1.1.0 over `prices` on Monday to Wednesday.
fn exit_case(prices: [&str; 3]) -> Case {
    pinning(Case::new(WEEK, prices, NOW), EXIT_VERSION, exit_hash())
}

/// 1.1.0's output at Wednesday's close with `conviction`.
fn output(conviction: &str) -> Box<ModelOutput> {
    let as_of = at(CLOSE).secs();
    Box::new(ModelOutput {
        model_id: ID.to_owned(),
        model_version: EXIT_VERSION.to_owned(),
        content_hash: exit_hash(),
        instrument_id: mandate_accounting::InstrumentId::new("SPY").unwrap(),
        as_of: RiskClock::from_secs(as_of),
        expires_at: RiskClock::from_secs(as_of + i64::from(AGE)),
        direction: ModelDirection::Long,
        conviction: Conviction::parse(conviction).unwrap(),
        confidence: Unit::ONE,
        horizon_s: u64::from(AGE),
        thesis_ref: None,
        evidence: Vec::new(),
        invalidation: None,
        thesis_id: None,
        lineage_id: None,
        ignored: None,
    })
}

#[test]
fn the_host_computes_1_1_0_s_content_object_from_its_three_listed_files() {
    let text = oracle_content();
    let hash = Digest::of(text.as_bytes());
    assert_eq!(
        content(ID, EXIT_VERSION),
        Ok(Content {
            canonical: text.into_bytes(),
            hash,
        })
    );
    assert_ne!(
        content(ID, EXIT_VERSION).map(|c| c.hash),
        content(ID, VERSION).map(|c| c.hash),
        "a new version has a new hash"
    );
    for version in ["1.1.1", "1.2.0", "2.0.0"] {
        assert_eq!(
            content(ID, version),
            Err(Refusal::UnknownModel),
            "{version}"
        );
    }
}

#[test]
fn a_rising_crossover_is_1_1_0_s_long_output_with_conviction_1() {
    assert_eq!(
        pinning(Case::rising(), EXIT_VERSION, exit_hash()).run(),
        Ok(Evaluation::Long(output("1")))
    );
}

/// A crossover down, and a tie, which the backtest of the same strategy also sells on, are the exit
/// output: conviction −1 at the close it read, never a buy.
#[test]
fn a_flat_or_a_tie_is_1_1_0_s_exit_output_with_conviction_minus_1() {
    for prices in [["103", "101", "100"], ["100"; 3], ["100", "99", "101"]] {
        assert_eq!(
            exit_case(prices).run(),
            Ok(Evaluation::Exit(output("-1"))),
            "{prices:?}"
        );
    }
}

/// 1.0.0 still answers a `Flat` with no output, beside 1.1.0.
#[test]
fn version_1_0_0_still_answers_a_flat_with_no_output() {
    assert_eq!(
        Case::new(WEEK, ["103", "101", "100"], NOW).run(),
        Ok(Evaluation::NoOutput(Signal::Flat))
    );
}

/// The host runs only the version the pin names under that version's own hash, and only when the
/// registry agrees: 1.1.0 under 1.0.0's hash, 1.0.0 under 1.1.0's, and a 1.1.0 pin against a 1.0.0
/// registration are each refused (DEC-503 item 3).
#[test]
fn a_pin_or_registry_naming_the_other_version_s_hash_is_refused() {
    assert_eq!(
        pinning(exit_case(["103", "101", "100"]), EXIT_VERSION, hash()).run(),
        Err(Refusal::PinHashMismatch)
    );
    assert_eq!(
        pinning(
            Case::new(WEEK, ["103", "101", "100"], NOW),
            VERSION,
            exit_hash()
        )
        .run(),
        Err(Refusal::PinHashMismatch)
    );
    let mut stale = exit_case(["103", "101", "100"]);
    for entry in stale.registry.values_mut() {
        entry.version = VERSION.to_owned();
        entry.content_hash = hash();
    }
    assert_eq!(stale.run(), Err(Refusal::RegistryMismatch));
}

/// `cents` as the shortest decimal.
fn price(cents: u32) -> String {
    match (cents / 100, cents % 100) {
        (whole, 0) => whole.to_string(),
        (whole, part) if part % 10 == 0 => format!("{whole}.{}", part / 10),
        (whole, part) => format!("{whole}.{part:02}"),
    }
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::default() })]

    /// For any three closes, each version answers what the crossover in integer cents says: the
    /// fast mean of two above the slow mean of three is `Long` for both; otherwise 1.1.0 exits with
    /// conviction −1 and 1.0.0 gives no output.
    #[test]
    fn each_version_answers_the_integer_crossover(
        cents in prop::array::uniform3(1u32..100_000),
    ) {
        let prices = cents.map(price);
        let prices = [prices[0].as_str(), prices[1].as_str(), prices[2].as_str()];
        let [a, b, c] = cents.map(u64::from);
        let rising = (b + c) * 3 > (a + b + c) * 2;
        let (exit, plain) = (exit_case(prices).run(), Case::new(WEEK, prices, NOW).run());
        if rising {
            prop_assert_eq!(exit, Ok(Evaluation::Long(output("1"))));
            prop_assert_eq!(plain, Ok(long(CLOSE)));
        } else {
            prop_assert_eq!(exit, Ok(Evaluation::Exit(output("-1"))));
            prop_assert_eq!(plain, Ok(Evaluation::NoOutput(Signal::Flat)));
        }
    }
}
