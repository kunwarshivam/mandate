//! The model host's content object, output mapping and `as_of` (E15-13 M1a; DEC-503, DEC-504,
//! DEC-518). The oracles are written out by hand: the content object as canonical JSON text with
//! the listed files' SHA-256 read from disk, and every time from NYSE's published hours.

mod common;

use common::{Case, ID, RISING, VERSION, WEEK, at, long, oracle_content};
use mandate_canon::Digest;
use mandate_modelhost::{Content, Evaluation, Refusal, Signal, content};
use mandate_time::ExchangeCalendar;

/// A refusal's message names the check that failed and no value of the run's, and the stub's names
/// its story: the live test the mutation gate needs before M2 (DEC-139).
#[test]
fn a_refusal_names_the_failed_check_and_no_value() {
    let stub = Refusal::Unimplemented { story: "E15-13" };
    assert_eq!(stub.to_string(), "E15-13 has not been implemented yet");
    let key = "fast_periods";
    let bounds = "parameter fast_periods is not an integer within the model's bounds";
    assert_eq!(Refusal::ParamValue { key }.to_string(), bounds);
}

/// The content hash `quant.ma_crossover` 1.0.0 is pinned to (DEC-504 item 2, DEC-518 item 1),
/// computed from M2's bytes of the two files it lists. An edit to either file, the crossover in
/// `mandate-backtest` or the model's own host file, fails this test: the model must take a new
/// version, with a new pinned hash, before its code can change.
const PINNED_1_0_0: &str = "cf36ebf8746a95c79443a2a0b5b49762809e082bced8a0a5bfdda7a3dd6c469d";

/// A model whose code changed cannot keep its version (DEC-504 item 2), so its hash is a literal
/// here rather than whatever the build gives.
#[test]
fn quant_ma_crossover_1_0_0_keeps_its_pinned_content_hash() {
    let hash = content(ID, VERSION).map(|c| c.hash.to_hex());
    let why = "a listed file changed: give the model a new version and pin its new hash";
    assert_eq!(hash, Ok(PINNED_1_0_0.to_owned()), "{why}");
}

/// DEC-504 item 2 and DEC-518: the host computes the content object from the bytes it was built
/// with, so it equals the object written from the files on disk; it has content for no other model.
#[test]
fn the_content_object_lists_the_source_bytes_the_host_was_built_from() {
    let text = oracle_content();
    let hash = Digest::of(text.as_bytes());
    let want = Ok(Content {
        canonical: text.into_bytes(),
        hash,
    });
    assert_eq!(content(ID, VERSION), want, "DEC-504 item 1's object");
    for (id, version) in [(ID, "1.0.1"), ("quant.other", VERSION)] {
        let unknown = Err(Refusal::UnknownModel);
        assert_eq!(content(id, version), unknown, "{id} {version}");
    }
}

/// DEC-157 item 4: `Long` is the one output; `Flat`, a tie included, is none (100.5 and 100 are
/// not above 304/3 and 100).
#[test]
fn a_long_is_the_one_output_and_a_flat_or_a_tie_is_none() {
    let rising = Case::rising().run();
    assert_eq!(rising, Ok(long("2026-10-07T20:00:00Z")), "102 above 304/3");
    for prices in [["103", "101", "100"], ["100"; 3]] {
        let flat = Case::new(WEEK, prices, "2026-10-07T21:00:00Z").run();
        assert_eq!(flat, Ok(Evaluation::NoOutput(Signal::Flat)), "{prices:?}");
    }
}

/// `as_of` is the end of the last completed regular session in the calendar handed in, never the
/// run clock (X-3, mandate spec §8.2): an early close's 13:00 EST with the holiday before it
/// skipped, a Monday morning reading Friday's 16:00 EDT, and a parsed calendar's own early close.
#[test]
fn as_of_is_the_last_completed_sessions_close_in_the_calendar_given() {
    let thanksgiving = ["2025-11-25", "2025-11-26", "2025-11-28"];
    let early = Case::new(thanksgiving, RISING, "2025-11-28T19:00:00Z").run();
    assert_eq!(early, Ok(long("2025-11-28T18:00:00Z")), "the early close");
    let to_friday = ["2026-09-30", "2026-10-01", "2026-10-02"];
    let monday = Case::new(to_friday, RISING, "2026-10-05T14:00:00Z").run();
    assert_eq!(monday, Ok(long("2026-10-02T20:00:00Z")), "Friday's close");
    let header = "valid 2026-01-01 2026-12-31\nhours 04:00 09:30 16:00 20:00\n";
    let text = format!("{header}early_close 2026-10-07 13:00 17:00 t\n");
    let mut parsed = Case::rising();
    parsed.calendar = ExchangeCalendar::parse(&text).unwrap();
    parsed.now = at("2026-10-07T17:30:00Z");
    let given = parsed.run();
    assert_eq!(
        given,
        Ok(long("2026-10-07T17:00:00Z")),
        "the given calendar's close"
    );
}
