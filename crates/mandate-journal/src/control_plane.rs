//! The control-plane records journal spec v0.44 §9.17 closes at schema version 1 (DEC-981): the
//! workspace deployment's records of its link to the global control plane, on the workspace control
//! stream, with consistency rules 137 to 142, each refusing at its member. No member is free text
//! (control-plane design CP-1): IDs, closed enumerations, integers, timestamps, and bare-hex
//! digests only. Workspace control services write them as a `system` actor (rule 137); the
//! control-plane agent never appends. Every rule only refuses a draft, and none compares one record
//! with another: anti-rollback (CP-9) is the writer's at this version (DEC-981 item 4).

use mandate_canon::Value;

use crate::schema::Ty;
use crate::{Invalid, InvalidReason, StreamType};

const ENROLLED: &str = "ControlPlaneEnrolled";
const LICENSE: &str = "LicenseApplied";
const LICENSE_STATE: &str = "LicenseStateChanged";
const OFFERED: &str = "ReleaseOffered";
const INSTALLED: &str = "ReleaseInstalled";
const WITHDRAWN: &str = "ReleaseWithdrawnNoticed";
const CATALOG: &str = "CatalogEntryRegistered";
const BUNDLE: &str = "DataBundleImported";
const USAGE: &str = "UsageReportSealed";
const REFUSED: &str = "ControlPlaneMessageRefused";

/// The control-stream event types §9.17 adds to [`crate::control::governs`].
pub(crate) const CONTROL: [&str; 10] = [
    ENROLLED,
    LICENSE,
    LICENSE_STATE,
    OFFERED,
    INSTALLED,
    WITHDRAWN,
    CATALOG,
    BUNDLE,
    USAGE,
    REFUSED,
];

/// The records that always carry a document's `sequence` (rule 139).
const SEQUENCED: [&str; 5] = [LICENSE, OFFERED, INSTALLED, CATALOG, BUNDLE];

/// A license's states (control-plane design §3.3).
const LICENSE_STATES: &[&str] = &["valid", "renewal_due", "grace", "lapsed"];

/// The §9.17 schema of `event_type` at `schema_version` on `stream`, or `None` when this module
/// registers none there.
pub(crate) fn schema(
    event_type: &str,
    schema_version: u64,
    stream: StreamType,
) -> Option<&'static Ty> {
    if schema_version != 1 || stream != StreamType::Control {
        return None;
    }
    match event_type {
        ENROLLED => Some(&CONTROL_PLANE_ENROLLED),
        LICENSE => Some(&LICENSE_APPLIED),
        LICENSE_STATE => Some(&LICENSE_STATE_CHANGED),
        OFFERED => Some(&RELEASE_OFFERED),
        INSTALLED => Some(&RELEASE_INSTALLED),
        WITHDRAWN => Some(&RELEASE_WITHDRAWN_NOTICED),
        CATALOG => Some(&CATALOG_ENTRY_REGISTERED),
        BUNDLE => Some(&DATA_BUNDLE_IMPORTED),
        USAGE => Some(&USAGE_REPORT_SEALED),
        REFUSED => Some(&CONTROL_PLANE_MESSAGE_REFUSED),
        _ => None,
    }
}

/// Rules 137 to 142 on a payload its schema has normalized, in rule order: the first that fails
/// is reported. `actor` is the envelope's. Timestamps are canonical and fixed-width (journal spec
/// §4.7), so their bytes order them as instants.
pub(crate) fn rules(
    event_type: &str,
    payload: &Value,
    actor: Option<&Value>,
) -> Result<(), Invalid> {
    if !CONTROL.contains(&event_type) {
        return Ok(());
    }
    ensure(text(actor, "kind") == "system", "actor.kind")?;
    if event_type == LICENSE {
        let deployments = texts(payload, "deployment_ids");
        ensure(!deployments.is_empty(), "payload.deployment_ids")?;
        ascending(&deployments, "payload.deployment_ids")?;
        ascending(&texts(payload, "features"), "payload.features")?;
        ensure(
            text(Some(payload), "not_before") < text(Some(payload), "not_after"),
            "payload.not_after",
        )?;
    }
    let sequence = payload.get("sequence").and_then(Value::as_int);
    if SEQUENCED.contains(&event_type) || event_type == REFUSED {
        ensure(sequence.is_none_or(|s| s >= 1), "payload.sequence")?;
    }
    if event_type == LICENSE_STATE {
        let from = payload.get("from").and_then(Value::as_str);
        ensure(from != Some(text(Some(payload), "to")), "payload.to")?;
    }
    if event_type == USAGE {
        ensure(
            text(Some(payload), "period_start") < text(Some(payload), "period_end"),
            "payload.period_end",
        )?;
    }
    if event_type == REFUSED {
        let unknown = text(Some(payload), "message") == "unknown";
        let reason = text(Some(payload), "reason");
        ensure(
            !unknown || matches!(reason, "out_of_list" | "malformed"),
            "payload.reason",
        )?;
        ensure(!unknown || sequence.is_none(), "payload.sequence")?;
        ensure(
            reason != "replayed" || sequence.is_some(),
            "payload.sequence",
        )?;
    }
    Ok(())
}

fn text<'a>(value: Option<&'a Value>, member: &str) -> &'a str {
    value
        .and_then(|v| v.get(member))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn texts<'a>(value: &'a Value, member: &str) -> Vec<&'a str> {
    value
        .get(member)
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .collect()
}

fn ensure(holds: bool, path: &str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::Schema, path))
    }
}

/// Strictly ascending by bytes, so a repeat is refused as well as a swap (rule 138).
fn ascending(items: &[&str], path: &str) -> Result<(), Invalid> {
    if items.windows(2).all(|w| matches!(w, [a, b] if a < b)) {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::NonCanonical, path))
    }
}

static CONTROL_PLANE_ENROLLED: Ty = Ty::Record(&[
    ("deployment_id", Ty::Ident),
    ("org_id", Ty::Ident),
    ("mode", Ty::OneOf(&["managed_cell", "hybrid"])),
    ("certificate_fingerprint", Ty::Digest),
]);

static LICENSE_APPLIED: Ty = Ty::Record(&[
    ("license_id", Ty::Ident),
    ("sequence", Ty::Int),
    ("org_id", Ty::Ident),
    ("deployment_ids", Ty::List(&Ty::Ident)),
    ("max_workspaces", Ty::Int),
    ("max_agents_deployed", Ty::Int),
    (
        "features",
        Ty::List(&Ty::OneOf(&["hybrid", "connected_clients"])),
    ),
    ("not_before", Ty::Timestamp),
    ("not_after", Ty::Timestamp),
    ("grace_days", Ty::Int),
    ("key_id", Ty::Ident),
    ("document_digest", Ty::Digest),
]);

static LICENSE_STATE_CHANGED: Ty = Ty::Record(&[
    ("license_id", Ty::Ident),
    ("from", Ty::Nullable(&Ty::OneOf(LICENSE_STATES))),
    ("to", Ty::OneOf(LICENSE_STATES)),
    ("judged_at", Ty::Timestamp),
]);

static RELEASE_OFFERED: Ty = Ty::Record(&[
    ("release_id", Ty::Ident),
    ("sequence", Ty::Int),
    ("manifest_digest", Ty::Digest),
    ("key_id", Ty::Ident),
]);

static RELEASE_INSTALLED: Ty = Ty::Record(&[
    ("release_id", Ty::Ident),
    ("sequence", Ty::Int),
    ("manifest_digest", Ty::Digest),
    (
        "trigger",
        Ty::OneOf(&["promotion", "operator", "update_window", "rollback"]),
    ),
]);

static RELEASE_WITHDRAWN_NOTICED: Ty = Ty::Record(&[
    ("release_id", Ty::Ident),
    ("reason", Ty::OneOf(&["defect", "security", "superseded"])),
    ("key_id", Ty::Ident),
    ("document_digest", Ty::Digest),
]);

static CATALOG_ENTRY_REGISTERED: Ty = Ty::Record(&[
    (
        "kind",
        Ty::OneOf(&["connector_package", "model_registry_entry"]),
    ),
    ("entry_id", Ty::Ident),
    ("content_hash", Ty::Digest),
    ("sequence", Ty::Int),
    ("key_id", Ty::Ident),
]);

static DATA_BUNDLE_IMPORTED: Ty = Ty::Record(&[
    ("dataset", Ty::Ident),
    ("bundle_digest", Ty::Digest),
    ("sequence", Ty::Int),
    ("key_id", Ty::Ident),
]);

static USAGE_REPORT_SEALED: Ty = Ty::Record(&[
    ("report_id", Ty::Ident),
    ("deployment_id", Ty::Ident),
    ("org_id", Ty::Ident),
    ("period_start", Ty::Timestamp),
    ("period_end", Ty::Timestamp),
    ("prev_report_hash", Ty::Digest),
    ("report_digest", Ty::Digest),
]);

static CONTROL_PLANE_MESSAGE_REFUSED: Ty = Ty::Record(&[
    (
        "message",
        Ty::OneOf(&[
            "license",
            "release_manifest",
            "release_withdrawn",
            "catalog_entry",
            "data_bundle",
            "unknown",
        ]),
    ),
    (
        "reason",
        Ty::OneOf(&[
            "unsigned",
            "bad_signature",
            "unknown_key",
            "replayed",
            "out_of_list",
            "malformed",
            "not_addressed",
        ]),
    ),
    ("sequence", Ty::Nullable(&Ty::Int)),
    ("document_digest", Ty::Digest),
]);
