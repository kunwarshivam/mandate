//! Journal spec §9.16 (v0.34, DEC-869): `OrderStateChanged` at `schema_version` 2 is version 1's
//! members and `broker_order_id` (`text?`) last; version 1 is never edited and keeps appending.
//! The members are typed here from the spec's table, never read from the crate's registration.

mod common;

use common::{edit, journal_with, mark_draft, now, stream};
use mandate_journal::{AppendOutcome, Draft, InvalidReason};

/// Version 1's members in §9's row, with the value each takes on an accepted order.
const VERSION_1: [(&str, &str); 17] = [
    ("client_order_id", r#""md-order-1""#),
    ("state", r#""accepted""#),
    ("attempted", "null"),
    ("broker_status", r#""accepted""#),
    ("filled_qty", r#""0""#),
    ("reject_code", "null"),
    ("replaces", "null"),
    ("replaced_by", "null"),
    ("replaced_by_broker_order_id", "null"),
    ("lookup", "null"),
    ("ignored", "false"),
    ("cancel_requested", "false"),
    ("cancel_confirmed", "false"),
    ("cancel_overdue", "false"),
    ("adopted", "false"),
    ("ladder_step", "false"),
    ("risk_clock", r#""2026-09-21T14:00:00.000000000Z""#),
];

/// An `OrderStateChanged` draft numbered `n` at `version`, with `broker_order_id` (JSON text)
/// appended to version 1's members when given.
fn order_state(n: u64, version: u64, broker_order_id: Option<&str>) -> Vec<u8> {
    let mut members: Vec<String> = VERSION_1
        .iter()
        .map(|(name, value)| format!(r#""{name}":{value}"#))
        .collect();
    members.extend(broker_order_id.map(|id| format!(r#""broker_order_id":{id}"#)));
    let draft = edit(
        &mark_draft(n, "1"),
        "event_type",
        Some(r#""OrderStateChanged""#),
    );
    let draft = edit(&draft, "schema_version", Some(&version.to_string()));
    edit(
        &draft,
        "payload",
        Some(&format!("{{{}}}", members.join(","))),
    )
}

fn refusal(draft: &[u8]) -> Option<(InvalidReason, String)> {
    Draft::parse(draft).err().map(|e| (e.reason, e.path))
}

fn schema_at(member: &str) -> Option<(InvalidReason, String)> {
    Some((InvalidReason::Schema, format!("payload.{member}")))
}

#[test]
#[ignore = "pending E7-6"]
fn version_2_carries_the_brokers_order_id_or_null() {
    for id in [r#""rh-order-1""#, "null"] {
        let parsed = Draft::parse(&order_state(1, 2, Some(id)))
            .map(|d| (d.event_type().to_owned(), d.schema_version()));
        assert_eq!(parsed, Ok(("OrderStateChanged".to_owned(), 2)), "{id}");
    }
}

#[test]
#[ignore = "pending E7-6"]
fn version_2_is_closed_and_every_member_is_required() {
    let complete = order_state(1, 2, Some(r#""rh-order-1""#));
    for (member, _) in VERSION_1 {
        let missing = edit(&complete, &format!("payload.{member}"), None);
        assert_eq!(refusal(&missing), schema_at(member), "{member}");
    }
    assert_eq!(
        refusal(&order_state(1, 2, None)),
        schema_at("broker_order_id"),
        "present on every record (§4.2)"
    );
    for wrong in ["7", "false", r#"["rh-order-1"]"#] {
        let typed = order_state(1, 2, Some(wrong));
        assert_eq!(refusal(&typed), schema_at("broker_order_id"), "{wrong}");
    }
    assert_eq!(
        refusal(&order_state(1, 2, Some(r#""""#))),
        Some((
            InvalidReason::NonCanonical,
            "payload.broker_order_id".to_owned()
        )),
        "§9.2's `text`: an empty value is `null`, and empty text is `non_canonical`"
    );
    let extra = edit(&complete, "payload.unregistered", Some("true"));
    assert_eq!(refusal(&extra), schema_at("unregistered"));
}

#[test]
fn version_1_is_never_edited_and_no_later_version_is_registered() {
    assert_eq!(Draft::parse(&order_state(1, 1, None)).map(|_| ()), Ok(()));
    let carried = order_state(1, 1, Some(r#""rh-order-1""#));
    assert_eq!(refusal(&carried), schema_at("broker_order_id"));
    let later = refusal(&order_state(1, 3, Some(r#""rh-order-1""#)));
    assert_eq!(
        later.map(|(reason, _)| reason),
        Some(InvalidReason::UnknownSchema)
    );
}

#[test]
#[ignore = "pending E7-6"]
fn a_stream_holding_both_versions_appends_them_in_order() {
    let mut journal = journal_with(0);
    let epoch = journal.take_ownership(&stream());
    let drafts = [
        order_state(1, 1, None),
        order_state(2, 2, Some("null")),
        order_state(3, 2, Some(r#""rh-order-1""#)),
    ];
    for (head, draft) in (0_u64..).zip(&drafts) {
        let outcome = journal.append(&stream(), head + 1, epoch, now(), &[draft]);
        assert!(
            matches!(outcome, AppendOutcome::Committed(_)),
            "{outcome:?}"
        );
    }
    let versions: Vec<(&str, u64)> = journal
        .rows(&stream())
        .iter()
        .map(|row| (row.event_type.as_str(), row.schema_version))
        .collect();
    assert_eq!(
        versions,
        [
            ("StreamOpened", 1),
            ("OrderStateChanged", 1),
            ("OrderStateChanged", 2),
            ("OrderStateChanged", 2)
        ]
    );
}
