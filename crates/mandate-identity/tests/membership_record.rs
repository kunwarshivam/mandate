//! E7-31 (DEC-2384): `membership_record` maps a record's members exactly as `control_entry` maps
//! its envelope, so a reader that holds the members, as the run does, folds the same records.

use mandate_canon::{Value, parse};
use mandate_identity::{ControlEntry, control_entry, membership_record};
use mandate_time::UtcNanos;

const AT: &str = "2026-10-01T00:00:00.000000000Z";
const FOUNDING: &str = r#"{"cool_off_ends_at":"2026-10-01T00:00:00.000000000Z","independent_approval_required":false,"invitation":null,"member":"01K6C00000000000000000000F","reason":"founding","roles":["approver","operator","workspace_admin"]}"#;

fn envelope(event_type: &str, payload: &str) -> Value {
    let text =
        format!(r#"{{"event_time":"{AT}","event_type":"{event_type}","payload":{payload}}}"#);
    parse(text.as_bytes()).unwrap()
}

/// A membership record maps to the record `control_entry` gives for its envelope.
#[test]
fn a_membership_record_maps_as_its_envelope_does() {
    let payload = parse(FOUNDING.as_bytes()).unwrap();
    let at = UtcNanos::parse(AT).unwrap();
    let mapped = membership_record(4, "MemberActivated", at, &payload).unwrap();
    let ControlEntry::Membership(expected) =
        control_entry(4, &envelope("MemberActivated", FOUNDING)).unwrap()
    else {
        panic!("the founding grant is a membership record");
    };
    assert_eq!(mapped, Some(expected));
}

/// Any other type is no membership record, whatever its payload; a membership payload that does
/// not map is refused, never passed over.
#[test]
fn another_type_is_none_and_an_unmapped_payload_is_refused() {
    let payload = parse(FOUNDING.as_bytes()).unwrap();
    let at = UtcNanos::parse(AT).unwrap();
    for event_type in [
        "ConnectionEstablished",
        "StreamOpened",
        "Member",
        "memberactivated",
    ] {
        assert_eq!(
            membership_record(4, event_type, at, &payload),
            Ok(None),
            "{event_type}"
        );
    }
    let unmapped = parse(br#"{"member":"01K6C00000000000000000000F"}"#).unwrap();
    let refused = membership_record(4, "MemberActivated", at, &unmapped).unwrap_err();
    assert_eq!(refused.seq, 4);
}
