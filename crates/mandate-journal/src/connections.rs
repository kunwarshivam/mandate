//! The connection records journal spec v0.20 §9.8 closes (E7-17, DEC-800): on the control stream,
//! `ConnectionEstablished` at version 2, `ConnectionRefused`, `ConnectionCredentialRotated`, and
//! v0.32's `ConnectionRequested` (DEC-699); on the account stream, the executor's
//! `ConnectionChecked`, `ConnectionStateChanged`, and `ConnectionCredentialRefreshed`, and its
//! copies of the establishment (version 2 only) and of each rotation, with `risk_clock`.
//! Consistency rules 54 to 60, 62, 64 and 65, and copy rules 61 and 63. The owner's fold (rules 66
//! to 68 and 131) and §11 are not `append`'s. Every rule only refuses a draft.

use mandate_canon::Value;

use crate::schema::Ty;
use crate::{Invalid, InvalidReason, StreamType};

/// The control-stream event types §9.8 adds to [`crate::control::governs`].
pub(crate) const CONTROL: [&str; 3] = ["ConnectionRefused", "ConnectionRequested", ROTATED];

/// The account-stream event types §9.8 adds to [`crate::control::governs`].
pub(crate) const ACCOUNT: [&str; 5] = [
    "ConnectionChecked",
    "ConnectionStateChanged",
    "ConnectionCredentialRefreshed",
    ESTABLISHED,
    ROTATED,
];

const ESTABLISHED: &str = "ConnectionEstablished";
const ROTATED: &str = "ConnectionCredentialRotated";

/// Connections spec §5.2 step 6's teardowns: a connect that no check refused (rule 54).
const TEARDOWNS: [&str; 4] = [
    "timeout",
    "restart_past_deadline",
    "executor_stopped",
    "start_failed",
];

/// The checks an executor always reports (rule 57).
const REQUIRED_CHECKS: [&str; 3] = ["account", "environment", "scope"];

const DEGRADING: [&str; 3] = ["network_errors", "rate_headroom", "contract_drift"];
const SUSPENDING: [&str; 5] = [
    "authorization_failed",
    "credential_expired",
    "refresh_failed",
    "check_failed",
    "lease_expired",
];

/// The reasons a check refuses for (rules 54 and 58).
fn check_reasons(check: &str) -> &'static [&'static str] {
    match check {
        "scope" => &["scope_mismatch", "fund_movement", "permissions_unreadable"],
        "environment" => &["wrong_environment", "reaches_both"],
        "account" => &["account_unreadable", "account_mismatch", "not_dedicated"],
        "uniqueness" => &["already_connected"],
        "contract" => &["tools_missing", "contract_drift"],
        _ => &[],
    }
}

/// The §9.8 schema of `event_type` at `schema_version` on `stream`, or `None` when this module
/// registers none there. `ConnectionEstablished` version 1 on the control stream stays §9.2's.
pub(crate) fn schema(
    event_type: &str,
    schema_version: u64,
    stream: StreamType,
) -> Option<&'static Ty> {
    match (event_type, schema_version, stream) {
        (ESTABLISHED, 2, StreamType::Control) => Some(&ESTABLISHED_V2),
        (ESTABLISHED, 2, StreamType::Account) => Some(&ESTABLISHED_COPY),
        ("ConnectionRefused", 1, StreamType::Control) => Some(&CONNECTION_REFUSED),
        ("ConnectionRequested", 1, StreamType::Control) => Some(&CONNECTION_REQUESTED),
        (ROTATED, 1, StreamType::Control) => Some(&ROTATED_RECORD),
        (ROTATED, 1, StreamType::Account) => Some(&ROTATED_COPY),
        ("ConnectionChecked", 1, StreamType::Account) => Some(&CONNECTION_CHECKED),
        ("ConnectionStateChanged", 1, StreamType::Account) => Some(&CONNECTION_STATE_CHANGED),
        ("ConnectionCredentialRefreshed", 1, StreamType::Account) => {
            Some(&CONNECTION_CREDENTIAL_REFRESHED)
        }
        _ => None,
    }
}

/// Consistency rules 54 to 60, 62, 64 and 65 on a payload its schema has normalized, in rule
/// order. `causation_id` and `pii_refs` are the envelope's.
pub(crate) fn rules(
    event_type: &str,
    schema_version: u64,
    payload: &Value,
    causation_id: Option<&Value>,
    pii_refs: Option<&Value>,
) -> Result<(), Invalid> {
    let p = View(payload);
    match (event_type, schema_version) {
        ("ConnectionRefused", _) => {
            let check = p.text("check");
            let reason = p.text("reason");
            let fits = if p.is_null("check") {
                TEARDOWNS.contains(&reason)
            } else {
                check_reasons(check).contains(&reason)
            };
            ensure(fits, InvalidReason::Schema, "payload.reason")?;
            let existing = p.text("existing_connection_id");
            let named = if check == "uniqueness" {
                !p.is_null("existing_connection_id") && existing != p.text("connection_id")
            } else {
                p.is_null("existing_connection_id")
            };
            ensure(
                named,
                InvalidReason::Schema,
                "payload.existing_connection_id",
            )?;
            let caused = causation_id.is_some_and(|c| *c != Value::Null);
            ensure(
                !p.is_null("check") || !caused,
                InvalidReason::Schema,
                "causation_id",
            )
        }
        (ROTATED | "ConnectionCredentialRefreshed", _) => ascending(&p.texts("scopes")),
        ("ConnectionChecked", _) => checked_rules(p, pii_refs),
        ("ConnectionStateChanged", _) => state_rules(p),
        (ESTABLISHED, 2) => ensure(
            p.is_null("margin_attestation") != (p.text("environment") == "live"),
            InvalidReason::Schema,
            "payload.margin_attestation",
        ),
        _ => Ok(()),
    }
}

/// Rules 57, 58 and 62 on a `ConnectionChecked`.
fn checked_rules(p: View<'_>, pii_refs: Option<&Value>) -> Result<(), Invalid> {
    let results = p.list("results");
    let checks: Vec<&str> = results.iter().map(|r| View(r).text("check")).collect();
    if !checks.windows(2).all(|w| matches!(w, [a, b] if a < b)) {
        return Err(Invalid::new(InvalidReason::NonCanonical, "payload.results"));
    }
    ensure(
        REQUIRED_CHECKS.iter().all(|c| checks.contains(c)),
        InvalidReason::Schema,
        "payload.results",
    )?;
    for (i, result) in results.iter().enumerate() {
        let r = View(result);
        let failed = r.text("result") == "failed";
        let reason = r.text("reason");
        let fits = if r.is_null("reason") {
            !failed
        } else {
            failed
                && check_reasons(r.text("check")).contains(&reason)
                && reason != "account_mismatch"
        };
        ensure(
            fits,
            InvalidReason::Schema,
            &format!("payload.results[{i}].reason"),
        )?;
    }
    let unread = results.iter().any(|r| {
        let r = View(r);
        r.text("check") == "account" && r.text("reason") == "account_unreadable"
    });
    let reference = p.text("account_pii_ref");
    let listed = p.is_null("account_pii_ref")
        || pii_refs
            .and_then(Value::as_array)
            .is_some_and(|refs| refs.iter().any(|r| r.as_str() == Some(reference)));
    ensure(
        p.is_null("account_pii_ref") == unread && listed,
        InvalidReason::Schema,
        "payload.account_pii_ref",
    )
}

/// Rules 59 and 60 on a `ConnectionStateChanged`: the reason fits the state entered, and the
/// state left fits it.
fn state_rules(p: View<'_>) -> Result<(), Invalid> {
    let (reason, to, from) = (p.text("reason"), p.text("to"), p.text("from"));
    let fits = if DEGRADING.contains(&reason) {
        to == "degraded"
    } else if SUSPENDING.contains(&reason) {
        to == "suspended"
    } else if reason == "condition_cleared" {
        to == from && to != "active"
    } else {
        to == "active"
    };
    ensure(fits, InvalidReason::Schema, "payload.reason")?;
    let allowed: &[&str] = match to {
        "active" => &["degraded", "suspended"],
        "degraded" => &["active", "degraded"],
        _ => &["active", "degraded", "suspended"],
    };
    ensure(
        allowed.contains(&from),
        InvalidReason::Schema,
        "payload.from",
    )
}

/// Copy rules 63 and 61, after `artifact_refs` and `pii_refs` (§9.1's order): an establishment at
/// version 2 and a rotation, on either stream, name their cause, and an `acknowledged` state change
/// names the acknowledgment.
pub(crate) fn copy(
    event_type: &str,
    schema_version: u64,
    payload: &Value,
    causation_id: Option<&Value>,
) -> Result<(), Invalid> {
    let caused = causation_id.is_some_and(|c| *c != Value::Null);
    let named = event_type == ROTATED || (event_type == ESTABLISHED && schema_version == 2);
    ensure(!named || caused, InvalidReason::Schema, "causation_id")?;
    let acknowledged =
        event_type == "ConnectionStateChanged" && View(payload).text("reason") == "acknowledged";
    ensure(
        !acknowledged || caused,
        InvalidReason::Schema,
        "causation_id",
    )
}

fn ensure(holds: bool, reason: InvalidReason, path: &str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(reason, path))
    }
}

/// Rule 56: scopes strictly ascending by bytes (`non_canonical`).
fn ascending(items: &[&str]) -> Result<(), Invalid> {
    ensure(
        items.windows(2).all(|w| matches!(w, [a, b] if a < b)),
        InvalidReason::NonCanonical,
        "payload.scopes",
    )
}

/// A normalized payload, read member by member; a `null` or absent member reads as empty text.
#[derive(Clone, Copy)]
struct View<'a>(&'a Value);

impl<'a> View<'a> {
    fn text(self, member: &str) -> &'a str {
        self.0
            .get(member)
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    fn is_null(self, member: &str) -> bool {
        matches!(self.0.get(member), None | Some(Value::Null))
    }

    fn list(self, member: &str) -> &'a [Value] {
        self.0
            .get(member)
            .and_then(Value::as_array)
            .unwrap_or_default()
    }

    fn texts(self, member: &str) -> Vec<&'a str> {
        self.list(member).iter().filter_map(Value::as_str).collect()
    }
}

const STEP_UP: Ty = Ty::Record(&[
    ("assertion_id", Ty::Str),
    ("authenticated_at", Ty::Timestamp),
    ("method", Ty::Str),
]);

const MARGIN: Ty = Ty::Nullable(&Ty::OneOf(&["cash_account", "margin_disabled"]));

static ESTABLISHED_V2: Ty = Ty::Record(&[
    ("connection_id", Ty::Ident),
    ("broker", Ty::Str),
    ("environment", Ty::OneOf(&["paper", "live"])),
    ("scopes", Ty::List(&Ty::Str)),
    ("account_ref", Ty::Ulid),
    ("user", Ty::Str),
    ("step_up", STEP_UP),
    ("margin_attestation", MARGIN),
]);

static ESTABLISHED_COPY: Ty = Ty::Record(&[
    ("connection_id", Ty::Ident),
    ("broker", Ty::Str),
    ("environment", Ty::OneOf(&["paper", "live"])),
    ("scopes", Ty::List(&Ty::Str)),
    ("account_ref", Ty::Ulid),
    ("user", Ty::Str),
    ("step_up", STEP_UP),
    ("margin_attestation", MARGIN),
    ("risk_clock", Ty::RiskClock),
]);

static CONNECTION_REFUSED: Ty = Ty::Record(&[
    ("connection_id", Ty::Ident),
    ("broker", Ty::Str),
    ("environment", Ty::OneOf(&["paper", "live"])),
    (
        "occasion",
        Ty::OneOf(&["connect", "reconnect", "reauthorize"]),
    ),
    (
        "check",
        Ty::Nullable(&Ty::OneOf(&[
            "scope",
            "environment",
            "account",
            "uniqueness",
            "contract",
        ])),
    ),
    (
        "reason",
        Ty::OneOf(&[
            "scope_mismatch",
            "fund_movement",
            "permissions_unreadable",
            "wrong_environment",
            "reaches_both",
            "account_unreadable",
            "account_mismatch",
            "not_dedicated",
            "already_connected",
            "tools_missing",
            "contract_drift",
            "timeout",
            "restart_past_deadline",
            "executor_stopped",
            "start_failed",
        ]),
    ),
    ("existing_connection_id", Ty::Nullable(&Ty::Ident)),
    ("user", Ty::Str),
    ("step_up", STEP_UP),
]);

/// The pending connection at a connect's start; no member carries a secret (CN-1, CN-10).
static CONNECTION_REQUESTED: Ty = Ty::Record(&[
    ("connection_id", Ty::Ident),
    ("account_ref", Ty::Ulid),
    ("broker", Ty::Str),
    ("environment", Ty::OneOf(&["paper", "live"])),
    ("user", Ty::Str),
    ("step_up", STEP_UP),
]);

static ROTATED_RECORD: Ty = Ty::Record(&[
    ("connection_id", Ty::Ident),
    ("scopes", Ty::List(&Ty::Str)),
    ("user", Ty::Str),
    ("step_up", STEP_UP),
]);

static ROTATED_COPY: Ty = Ty::Record(&[
    ("connection_id", Ty::Ident),
    ("scopes", Ty::List(&Ty::Str)),
    ("user", Ty::Str),
    ("step_up", STEP_UP),
    ("risk_clock", Ty::RiskClock),
]);

static CONNECTION_CHECKED: Ty = Ty::Record(&[
    ("connection_id", Ty::Ident),
    (
        "occasion",
        Ty::OneOf(&[
            "connect",
            "reconnect",
            "reauthorize",
            "executor_start",
            "daily",
        ]),
    ),
    (
        "results",
        Ty::List(&Ty::Record(&[
            (
                "check",
                Ty::OneOf(&["account", "contract", "environment", "scope"]),
            ),
            ("result", Ty::OneOf(&["passed", "failed"])),
            (
                "reason",
                Ty::Nullable(&Ty::OneOf(&[
                    "scope_mismatch",
                    "fund_movement",
                    "permissions_unreadable",
                    "wrong_environment",
                    "reaches_both",
                    "account_unreadable",
                    "account_mismatch",
                    "not_dedicated",
                    "already_connected",
                    "tools_missing",
                    "contract_drift",
                ])),
            ),
        ])),
    ),
    ("account_pii_ref", Ty::Nullable(&Ty::PiiRef)),
    ("risk_clock", Ty::RiskClock),
]);

static CONNECTION_STATE_CHANGED: Ty = Ty::Record(&[
    ("connection_id", Ty::Ident),
    ("from", Ty::OneOf(&["active", "degraded", "suspended"])),
    ("to", Ty::OneOf(&["active", "degraded", "suspended"])),
    (
        "reason",
        Ty::OneOf(&[
            "network_errors",
            "rate_headroom",
            "contract_drift",
            "authorization_failed",
            "credential_expired",
            "refresh_failed",
            "check_failed",
            "lease_expired",
            "condition_cleared",
            "acknowledged",
        ]),
    ),
    ("risk_clock", Ty::RiskClock),
]);

static CONNECTION_CREDENTIAL_REFRESHED: Ty = Ty::Record(&[
    ("connection_id", Ty::Ident),
    ("scopes", Ty::List(&Ty::Str)),
    ("risk_clock", Ty::RiskClock),
]);
