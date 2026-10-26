//! The closes object's bytes, written out by hand: the digest an `ObservationRecorded` names is of
//! exactly these, so the runner, the runtime process and the data process must agree on every byte.

use mandate_bars::closes_object;
use mandate_num::Price;
use mandate_time::Date;

#[test]
fn the_closes_object_is_canonical_json_of_the_closes_and_the_instrument() {
    let closes = [
        (
            Date::new(2026, 10, 22).unwrap(),
            Price::parse("149.9").unwrap(),
        ),
        (
            Date::new(2026, 10, 23).unwrap(),
            Price::parse("150").unwrap(),
        ),
    ];
    let bytes = closes_object(&closes, "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415").unwrap();
    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        r#"{"closes":[["2026-10-22","149.9"],["2026-10-23","150"]],"instrument_id":"b0b6dd9d-8b9b-48a9-ba46-b9d54906e415"}"#
    );
}

#[test]
fn no_closes_is_an_empty_list() {
    let bytes = closes_object(&[], "x").unwrap();
    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        r#"{"closes":[],"instrument_id":"x"}"#
    );
}
