//! The payload schemas journal spec §9.2 closes (DEC-261, DEC-302, E7-10): the control stream's
//! records that `ValidationContext::from_journal` reads, and `OwnerCommandRefused` on the agent and
//! account streams, and `AccountSnapshotRecorded` on the account stream, with consistency rules 17
//! to 24, subject rules 25, 26 and 28, and copy rule 27. Every rule only refuses a draft; none
//! changes what a writer may do.
//!
//! `AccountSnapshotRecorded` registered once stream K's fee-step writer conformed (#456), in the
//! change that journals that writer's form in `fees`, so the fee step's snapshot is never refused
//! for a member while it pauses every agent and alerts the owner (`AGENTS.md` rules 3 and 13,
//! DEC-261 item 7, DEC-402).
//!
//! The account stream's `MandateVersionApplied` and `UniverseChanged` are journal spec v0.8 §9.3's,
//! with rules 29 to 33, routed here beside §9.2's (DEC-403, DEC-404).
//!
//! The agent stream's `ThesisProposed` and `ThesisRevised` are journal spec §9.4's, one shared
//! schema with rules 34 to 38, routed here beside §9.2's and §9.3's (DEC-413, DEC-414). The checks
//! that read the mandate the record names (§8.5 checks 4, 5, 6, 10 and 16's cap) are not schema
//! rules: `append` holds no document store, so every reader of a thesis record runs
//! `mandate_spec::context::check_thesis_record` before acting on it (DEC-414 item 3).

use mandate_canon::Value;
use mandate_num::Usd;
use mandate_time::UtcNanos;

use crate::schema::Ty;
use crate::{Invalid, InvalidReason, StreamId, StreamType};

/// The event types §9.2 closes on the control stream.
const CONTROL: [&str; 9] = [
    "StreamOpened",
    "ConnectionEstablished",
    "ConnectionRevoked",
    "DisclosureAccepted",
    "ConfigSnapshotRegistered",
    "MandateVersionCreated",
    "MandateConfirmed",
    "AgentDeployed",
    "AgentStopped",
];

/// The event type §9.2 closes on both the agent and the account stream.
const REFUSAL: &str = "OwnerCommandRefused";

/// The account stream's snapshot §9.2 closes, with rule 24.
const SNAPSHOT: &str = "AccountSnapshotRecorded";

/// The account stream's risk-state records journal spec §9.3 closes, with rules 29 to 33.
const RISK_STATE: [&str; 2] = ["MandateVersionApplied", "UniverseChanged"];

/// The agent stream's research-agent thesis records journal spec §9.4 closes, with rules 34 to 38.
const THESIS: [&str; 2] = ["ThesisProposed", "ThesisRevised"];

/// Whether §9.2 governs `event_type` on `stream`; every other event keeps its own registration.
pub(crate) fn governs(stream: &StreamId, event_type: &str) -> bool {
    match stream.stream_type() {
        StreamType::Control => CONTROL.contains(&event_type),
        StreamType::Agent => event_type == REFUSAL || THESIS.contains(&event_type),
        StreamType::Account => {
            event_type == REFUSAL || event_type == SNAPSHOT || RISK_STATE.contains(&event_type)
        }
        StreamType::Scheduler => false,
    }
}

/// The payload normalized against its §9.2 schema, then consistency rules 17 to 24 in number order
/// (reported only on a well-typed payload). `config_refs` is the envelope's, which rule 22 reads.
pub(crate) fn payload(
    event_type: &str,
    schema_version: u64,
    payload: &Value,
    config_refs: Option<&Value>,
) -> Result<Value, Invalid> {
    let schema = schema(event_type, schema_version)
        .ok_or_else(|| Invalid::new(InvalidReason::UnknownSchema, "payload"))?;
    let payload = crate::schema::normalize(schema, payload, "payload")?;
    let p = Payload(&payload);
    match event_type {
        "MandateVersionCreated" => {
            let paths: Vec<&str> = p
                .list("provenance")
                .iter()
                .map(|entry| {
                    entry
                        .get("path")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                })
                .collect();
            ascending(&paths, "payload.provenance")?;
        }
        "MandateConfirmed" => ascending(&p.texts("confirmed_paths"), "payload.confirmed_paths")?,
        "ConnectionEstablished" => ascending(&p.texts("scopes"), "payload.scopes")?,
        "ConfigSnapshotRegistered" => {
            ascending(&p.texts("params"), "payload.params")?;
            let model = p.text("kind") == MODEL_KIND;
            for member in ["model_id", "model_version", "admits_instruments"] {
                ensure(
                    p.is_null(member) != model,
                    InvalidReason::Schema,
                    &format!("payload.{member}"),
                )?;
            }
            ensure(
                model || p.list("params").is_empty(),
                InvalidReason::Schema,
                "payload.params",
            )?;
        }
        "AgentDeployed" => {
            let bound = config_refs
                .and_then(|refs| refs.get("mandate_version"))
                .and_then(Value::as_str);
            ensure(
                bound.is_none_or(|bound| bound == p.text("mandate_version")),
                InvalidReason::Schema,
                "payload.mandate_version",
            )?;
        }
        "AgentStopped" => ensure(
            !p.text("loss_added").starts_with('-'),
            InvalidReason::Schema,
            "payload.loss_added",
        )?,
        "MandateVersionApplied" => {
            let increasing = p.text("classification") == "risk_increasing";
            ensure(
                !increasing || !p.is_null("step_up"),
                InvalidReason::Schema,
                "payload.step_up",
            )?;
            let rejected = p.text("result") == "rejected";
            ensure(
                p.is_null("reason") != rejected,
                InvalidReason::Schema,
                "payload.reason",
            )?;
            for member in ["allocation_change", "max_loss_from_allocation"] {
                ensure(
                    !rejected || p.is_null(member),
                    InvalidReason::Schema,
                    &format!("payload.{member}"),
                )?;
            }
            let raised = !p.is_null("allocation_change") && {
                let change = p.text("allocation_change");
                !change.starts_with('-') && change != "0"
            };
            let implied = (!rejected && (raised || !p.is_null("max_loss_from_allocation")))
                || (rejected
                    && [
                        "increase_blocked_while_latched",
                        "waiting_period",
                        "still_below_new_floor",
                    ]
                    .contains(&p.text("reason")));
            ensure(
                !implied || increasing,
                InvalidReason::Schema,
                "payload.classification",
            )?;
        }
        "UniverseChanged" => {
            let reason = p.text("reason");
            let admitted = p.text("change") == "admitted";
            let allowed = if admitted {
                reason == "thesis_admitted" || reason == "version_applied"
            } else {
                reason != "thesis_admitted"
            };
            ensure(allowed, InvalidReason::Schema, "payload.reason")?;
            let (thesis, lineage) = (!p.is_null("thesis_id"), !p.is_null("lineage_id"));
            if thesis != lineage {
                let null = if thesis {
                    "payload.lineage_id"
                } else {
                    "payload.thesis_id"
                };
                return Err(Invalid::new(InvalidReason::Schema, null));
            }
            let from_thesis = [
                "thesis_admitted",
                "thesis_expired",
                "thesis_invalidated",
                "lineage_retired",
                "operator_halt",
            ]
            .contains(&reason);
            ensure(
                !from_thesis || thesis,
                InvalidReason::Schema,
                "payload.thesis_id",
            )?;
            ensure(
                !(reason == "version_applied" && admitted && thesis),
                InvalidReason::Schema,
                "payload.thesis_id",
            )?;
        }
        "ThesisProposed" | "ThesisRevised" => thesis_rules(event_type, p, config_refs)?,
        SNAPSHOT => {
            let compared = !p.is_null("model_cash");
            for member in ["cash_band", "cash_in_band"] {
                ensure(
                    p.is_null(member) != compared,
                    InvalidReason::Schema,
                    &format!("payload.{member}"),
                )?;
            }
            if compared {
                cash_in_band(p)?;
            }
        }
        _ => {}
    }
    Ok(payload)
}

/// Subject rules 25, 26 and 28 (`stream_mismatch`), then copy rule 27, on a payload that passed
/// [`payload`]: reported after `artifact_refs` and `pii_refs` (§9.1's order, which §9.2 keeps).
pub(crate) fn subject_and_copy(
    event_type: &str,
    stream: &StreamId,
    payload: &Value,
    causation_id: Option<&Value>,
) -> Result<(), Invalid> {
    let p = Payload(payload);
    if event_type == "StreamOpened" {
        let opened = format!("ctl:{}", p.text("workspace_id"));
        return ensure(
            opened == stream.as_str(),
            InvalidReason::StreamMismatch,
            "stream_id",
        );
    }
    if event_type != REFUSAL {
        return Ok(());
    }
    let allowed: &[&str] = match stream.stream_type() {
        StreamType::Account => &["acknowledge"],
        StreamType::Agent => &["resume", "stop"],
        StreamType::Control | StreamType::Scheduler => &[],
    };
    ensure(
        allowed.contains(&p.text("command")),
        InvalidReason::StreamMismatch,
        "payload.command",
    )?;
    ensure(
        p.text("reason") != NOT_INDEPENDENT || stream.stream_type() == StreamType::Account,
        InvalidReason::StreamMismatch,
        "payload.reason",
    )?;
    ensure(
        causation_id.is_some_and(|c| *c != Value::Null),
        InvalidReason::Schema,
        "causation_id",
    )
}

/// The refusal only the executor writes, of an acknowledgment that lifts a fired tripwire under
/// `independent_approval_required` (rule 28; mandate spec §5.8, §6.7; DEC-351 items 5 and 6).
const NOT_INDEPENDENT: &str = "not_independent";

/// `ConfigSnapshotRegistered`'s kind for a signal model.
const MODEL_KIND: &str = "model_version";

/// A normalized payload, read member by member; a `null` or absent member reads as empty.
#[derive(Clone, Copy)]
struct Payload<'a>(&'a Value);

impl<'a> Payload<'a> {
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

fn ensure(holds: bool, reason: InvalidReason, path: &str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(reason, path))
    }
}

/// Strictly ascending by bytes, so a repeat is refused as well as a swap (`non_canonical`).
fn ascending(items: &[&str], path: &str) -> Result<(), Invalid> {
    ensure(
        items
            .windows(2)
            .all(|w| matches!(w, [a, b] if a.as_bytes() < b.as_bytes())),
        InvalidReason::NonCanonical,
        path,
    )
}

/// Mandate spec §8.5's refusal reasons in check order: a reason's check number is its position, from
/// 1 (journal spec §9.4).
const THESIS_REFUSALS: [&str; 17] = [
    "direction_not_allowed",
    "horizon_mismatch",
    "revision_without_predecessor",
    "research_disabled",
    "universe_pinned",
    "admission_denied",
    "cost_cap_reached",
    "not_in_data_universe",
    "operator_halt",
    "not_allowed_asset_class",
    "leveraged_etp_not_enabled",
    "eligibility_floor",
    "instrument_group_claimed",
    "source_not_allowlisted",
    "no_corroboration",
    "lineage_retired",
    "universe_full",
];

/// Check 15, corroboration (mandate spec §8.5), which rule 37 recomputes.
const CORROBORATION_CHECK: usize = 15;

/// Journal spec §9.4's rules 34 to 38 on a well-typed `ThesisProposed` or `ThesisRevised`, the first
/// that fails reported, in number order (DEC-413, DEC-414). Rules 35 to 37 all report at
/// `payload.reason`. Rule 36 recomputes checks 1 to 3 from the record's own members, with check 2
/// compared to the nanosecond; rule 38 reads the envelope's `config_refs.model_version`, as rule 22
/// reads `AgentDeployed`'s mandate version.
fn thesis_rules(
    event_type: &str,
    p: Payload<'_>,
    config_refs: Option<&Value>,
) -> Result<(), Invalid> {
    let first_thesis = event_type == "ThesisProposed";
    let revised =
        p.0.get("revision")
            .and_then(Value::as_int)
            .is_some_and(|n| n > 0);
    ensure(
        first_thesis != revised,
        InvalidReason::Schema,
        "payload.revision",
    )?;
    ensure(
        !first_thesis || p.is_null("autopsy_ref"),
        InvalidReason::Schema,
        "payload.autopsy_ref",
    )?;
    let admitted = p.0.get("admitted") == Some(&Value::Bool(true));
    ensure(
        p.is_null("reason") == admitted,
        InvalidReason::Schema,
        "payload.reason",
    )?;
    let reason = p.text("reason");
    let failing = [
        p.text("direction") != "long",
        !horizon_agrees(p),
        !p.is_null("predecessor_thesis_id") != revised,
    ];
    let first_failing = failing.iter().position(|fails| *fails);
    let decided = match first_failing.and_then(|check| THESIS_REFUSALS.get(check)) {
        Some(first) => reason == *first,
        None => !THESIS_REFUSALS
            .iter()
            .take(failing.len())
            .any(|check| *check == reason),
    };
    ensure(decided, InvalidReason::Schema, "payload.reason")?;
    let corroborated = if p.is_null("corroboration") {
        THESIS_REFUSALS
            .iter()
            .position(|known| *known == reason)
            .is_some_and(|index| index < CORROBORATION_CHECK)
    } else {
        reason != "no_corroboration"
    };
    ensure(corroborated, InvalidReason::Schema, "payload.reason")?;
    let model = config_refs
        .and_then(|refs| refs.get("model_version"))
        .and_then(Value::as_str);
    ensure(
        model.is_none_or(|model| model == p.text("content_hash")),
        InvalidReason::Schema,
        "payload.content_hash",
    )
}

const NANOS_PER_SECOND: i128 = 1_000_000_000;

/// Check 2 (mandate spec §8.2, §8.5): `expires_at` is exactly `as_of` plus `horizon_s` seconds, to
/// the nanosecond. An instant past the type's range never agrees, so it fails check 2 rather than
/// being skipped.
fn horizon_agrees(p: Payload<'_>) -> bool {
    let nanos = |member: &str| {
        UtcNanos::parse(p.text(member)).ok().and_then(|t| {
            i128::from(t.secs())
                .checked_mul(NANOS_PER_SECOND)
                .and_then(|whole| whole.checked_add(i128::from(t.nanos())))
        })
    };
    let horizon = p.0.get("horizon_s").and_then(Value::as_int).map(i128::from);
    match (nanos("as_of"), nanos("expires_at"), horizon) {
        (Some(as_of), Some(expires_at), Some(horizon)) => {
            horizon
                .checked_mul(NANOS_PER_SECOND)
                .and_then(|span| as_of.checked_add(span))
                == Some(expires_at)
        }
        _ => false,
    }
}

/// Rule 24 on a compared snapshot: `cash_band` ≥ 0, and `cash_in_band` is `true` exactly when
/// |`cash` − `model_cash`| ≤ `cash_band`. The amounts are compared as exact `Usd`; one too large to
/// compare exactly is refused as `schema` at that member, never compared approximately (DEC-402).
fn cash_in_band(p: Payload<'_>) -> Result<(), Invalid> {
    let amount = |member: &str| {
        Usd::parse(p.text(member))
            .map_err(|_| Invalid::new(InvalidReason::Schema, format!("payload.{member}")))
    };
    let band = amount("cash_band")?;
    ensure(
        !band.is_negative(),
        InvalidReason::Schema,
        "payload.cash_band",
    )?;
    let drift = amount("cash")?
        .checked_sub(amount("model_cash")?)
        .map_err(|_| Invalid::new(InvalidReason::Schema, "payload.model_cash"))?;
    let drift = if drift.is_negative() {
        drift.negated()
    } else {
        drift
    };
    ensure(
        p.0.get("cash_in_band") == Some(&Value::Bool(drift <= band)),
        InvalidReason::Schema,
        "payload.cash_in_band",
    )
}

fn schema(event_type: &str, schema_version: u64) -> Option<&'static Ty> {
    if schema_version != 1 {
        return None;
    }
    Some(match event_type {
        "StreamOpened" => &STREAM_OPENED,
        "ConnectionEstablished" => &CONNECTION_ESTABLISHED,
        "ConnectionRevoked" => &CONNECTION_REVOKED,
        "DisclosureAccepted" => &DISCLOSURE_ACCEPTED,
        "ConfigSnapshotRegistered" => &CONFIG_SNAPSHOT_REGISTERED,
        "MandateVersionCreated" => &MANDATE_VERSION_CREATED,
        "MandateConfirmed" => &MANDATE_CONFIRMED,
        "AgentDeployed" => &AGENT_DEPLOYED,
        "AgentStopped" => &AGENT_STOPPED,
        "OwnerCommandRefused" => &OWNER_COMMAND_REFUSED,
        SNAPSHOT => &ACCOUNT_SNAPSHOT_RECORDED,
        "MandateVersionApplied" => &MANDATE_VERSION_APPLIED,
        "UniverseChanged" => &UNIVERSE_CHANGED,
        "ThesisProposed" | "ThesisRevised" => &THESIS_RECORD,
        _ => return None,
    })
}

static MANDATE_VERSION_APPLIED: Ty = Ty::Record(&[
    ("agent_id", Ty::Ident),
    ("old_version", Ty::DigestRef),
    ("new_version", Ty::DigestRef),
    (
        "classification",
        Ty::OneOf(&["risk_increasing", "risk_reducing", "neutral"]),
    ),
    (
        "step_up",
        Ty::Nullable(&Ty::Record(&[
            ("assertion_id", Ty::Str),
            ("authenticated_at", Ty::Timestamp),
            ("method", Ty::Str),
        ])),
    ),
    ("result", Ty::OneOf(&["applied", "rejected"])),
    (
        "reason",
        Ty::Nullable(&Ty::OneOf(&[
            "increase_blocked_while_latched",
            "equity_below_exposure",
            "would_trigger_limit",
            "not_loosening",
            "waiting_period",
            "still_below_new_floor",
        ])),
    ),
    ("allocation_change", Ty::Nullable(&Ty::Decimal)),
    ("max_loss_from_allocation", Ty::Nullable(&Ty::Decimal)),
    ("risk_clock", Ty::RiskClock),
]);

static UNIVERSE_CHANGED: Ty = Ty::Record(&[
    ("agent_id", Ty::Ident),
    ("instrument", Ty::AssetId),
    ("change", Ty::OneOf(&["admitted", "removed"])),
    (
        "reason",
        Ty::OneOf(&[
            "thesis_admitted",
            "thesis_expired",
            "thesis_invalidated",
            "lineage_retired",
            "eligibility_lost",
            "operator_halt",
            "version_applied",
        ]),
    ),
    ("thesis_id", Ty::Nullable(&Ty::Ident)),
    ("lineage_id", Ty::Nullable(&Ty::Ident)),
    ("universe_size_after", Ty::Int),
    ("risk_clock", Ty::RiskClock),
]);

/// Journal spec §9.4's one schema for both thesis records, in the spec's member order (DEC-413).
/// `instrument_id` is an asset ID (v0.11, DEC-413 item 7).
static THESIS_RECORD: Ty = Ty::Record(&[
    ("model_id", Ty::Str),
    ("model_version", Ty::Str),
    ("content_hash", Ty::DigestRef),
    ("thesis_id", Ty::Ident),
    ("lineage_id", Ty::Ident),
    ("revision", Ty::Int),
    ("predecessor_thesis_id", Ty::Nullable(&Ty::Ident)),
    ("autopsy_ref", Ty::Nullable(&Ty::DigestRef)),
    ("instrument_id", Ty::AssetId),
    ("asset_class", Ty::OneOf(&["us_equity", "crypto"])),
    ("direction", Ty::Str),
    ("as_of", Ty::Timestamp),
    ("expires_at", Ty::Timestamp),
    ("horizon_s", Ty::Int),
    ("conviction", Ty::Decimal),
    ("confidence", Ty::Decimal),
    ("evidence_ref", Ty::Nullable(&Ty::DigestRef)),
    ("evidence_sources", Ty::List(&Ty::Str)),
    (
        "corroboration",
        Ty::Nullable(&Ty::OneOf(&["independent_source", "market_data"])),
    ),
    ("invalidation", Ty::Str),
    ("allowlist_version", Ty::Int),
    ("prompt_ref", Ty::DigestRef),
    ("response_ref", Ty::DigestRef),
    ("admitted", Ty::Bool),
    ("reason", Ty::Nullable(&Ty::OneOf(&THESIS_REFUSALS))),
]);

static ACCOUNT_SNAPSHOT_RECORDED: Ty = Ty::Record(&[
    ("status", Ty::Str),
    ("crypto_status", Ty::Str),
    ("trading_blocked", Ty::Bool),
    ("account_blocked", Ty::Bool),
    ("trade_suspended_by_user", Ty::Bool),
    ("multiplier", Ty::Int),
    ("equity", Ty::Decimal),
    ("cash", Ty::Decimal),
    ("buying_power", Ty::Decimal),
    ("non_marginable_buying_power", Ty::Decimal),
    ("accrued_fees", Ty::Decimal),
    ("model_cash", Ty::Nullable(&Ty::Decimal)),
    ("cash_band", Ty::Nullable(&Ty::Decimal)),
    ("cash_in_band", Ty::Nullable(&Ty::Bool)),
    ("risk_clock", Ty::RiskClock),
]);

static STREAM_OPENED: Ty = Ty::Record(&[
    ("stream_type", Ty::OneOf(&["control"])),
    ("workspace_id", Ty::Ident),
]);

static CONNECTION_ESTABLISHED: Ty = Ty::Record(&[
    ("connection_id", Ty::Ident),
    ("broker", Ty::Str),
    ("environment", Ty::OneOf(&["paper", "live"])),
    ("scopes", Ty::List(&Ty::Str)),
]);

static CONNECTION_REVOKED: Ty = Ty::Record(&[("connection_id", Ty::Ident)]);

static DISCLOSURE_ACCEPTED: Ty = Ty::Record(&[
    ("document", Ty::Ident),
    ("version", Ty::DigestRef),
    ("user", Ty::Str),
    (
        "step_up",
        Ty::Record(&[
            ("assertion_id", Ty::Str),
            ("authenticated_at", Ty::Timestamp),
            ("method", Ty::Str),
        ]),
    ),
]);

static CONFIG_SNAPSHOT_REGISTERED: Ty = Ty::Record(&[
    (
        "kind",
        Ty::OneOf(&[
            "fee_config",
            "trading_calendar",
            "settlement_calendar",
            "instrument_snapshot",
            "rule_set",
            "mandate_version",
            "model_version",
        ]),
    ),
    ("content_hash", Ty::DigestRef),
    ("model_id", Ty::Nullable(&Ty::Str)),
    ("model_version", Ty::Nullable(&Ty::Str)),
    ("params", Ty::List(&Ty::Str)),
    ("admits_instruments", Ty::Nullable(&Ty::Bool)),
]);

static MANDATE_VERSION_CREATED: Ty = Ty::Record(&[
    ("mandate_version", Ty::DigestRef),
    (
        "provenance",
        Ty::List(&Ty::Record(&[
            ("path", Ty::Pointer),
            (
                "source",
                Ty::OneOf(&[
                    "user_stated",
                    "user_entered",
                    "template_structure",
                    "platform_proposed",
                    "platform_default",
                ]),
            ),
        ])),
    ),
    ("record_ref", Ty::DigestRef),
]);

static MANDATE_CONFIRMED: Ty = Ty::Record(&[
    ("mandate_version", Ty::DigestRef),
    ("confirmed_paths", Ty::List(&Ty::Pointer)),
    ("record_ref", Ty::DigestRef),
]);

static AGENT_DEPLOYED: Ty = Ty::Record(&[
    ("agent_id", Ty::Ident),
    ("mandate_version", Ty::DigestRef),
    ("record_ref", Ty::DigestRef),
]);

static AGENT_STOPPED: Ty = Ty::Record(&[
    ("agent_id", Ty::Ident),
    ("connection_id", Ty::Ident),
    (
        "reason",
        Ty::OneOf(&[
            "goal_complete",
            "profit_stop_reached",
            "end_date",
            "owner_stop",
        ]),
    ),
    ("retired_on", Ty::Date),
    ("loss_added", Ty::Decimal),
]);

static OWNER_COMMAND_REFUSED: Ty = Ty::Record(&[
    ("command", Ty::OneOf(&["resume", "stop", "acknowledge"])),
    (
        "reason",
        Ty::OneOf(&[
            "step_up_missing",
            "step_up_stale",
            "step_up_reused",
            "step_up_method",
            NOT_INDEPENDENT,
        ]),
    ),
    ("effective_at", Ty::Timestamp),
]);

/// The vectors' `control_stream` section, read inside this crate so the mutation gate, which runs
/// only the mutated crate's own tests, judges §9.2's rules here as well as in `mandate-refcases`.
/// A draft is checked by `Draft::parse` alone.
#[cfg(test)]
mod tests {
    use std::path::Path;

    use mandate_canon::{Int, Key, Object, Value, parse, to_canonical};

    use crate::{Draft, StreamId};

    use super::subject_and_copy;

    fn section() -> Result<Value, String> {
        named_section("control_stream")
    }

    fn named_section(name: &str) -> Result<Value, String> {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let fixture = parse(&bytes).map_err(|e| format!("{e:?}"))?;
        fixture
            .get(name)
            .cloned()
            .ok_or_else(|| format!("no {name}"))
    }

    fn list<'a>(value: &'a Value, name: &str) -> &'a [Value] {
        value
            .get(name)
            .and_then(Value::as_array)
            .unwrap_or_default()
    }

    fn text<'a>(value: &'a Value, name: &str) -> &'a str {
        value.get(name).and_then(Value::as_str).unwrap_or_default()
    }

    /// A case's base as a writer's draft: a chain event's body without the journal's fields, or
    /// one of the section's named drafts.
    fn base(section: &Value, case: &Value) -> Result<Object, String> {
        let body = match case.get("base_seq").and_then(Value::as_int) {
            Some(seq) => list(section, "chain")
                .iter()
                .find(|e| e.get("seq").and_then(Value::as_int) == Some(seq))
                .and_then(|e| e.get("body"))
                .ok_or_else(|| format!("no chain seq {seq}"))?,
            None => section
                .get("drafts")
                .and_then(|d| d.get(text(case, "base_draft")))
                .ok_or_else(|| format!("no draft {}", text(case, "base_draft")))?,
        };
        let mut body = body.as_object().cloned().ok_or("a body is an object")?;
        for field in ["seq", "prev_hash", "recorded_at"] {
            body.remove(field);
        }
        Ok(body)
    }

    /// Applies a `{path, value}` or `{path, delete: true}` change, dotted from the envelope.
    fn apply(draft: &mut Object, change: &Value) -> Result<(), String> {
        let path = text(change, "path");
        let mut names: Vec<&str> = path.split('.').collect();
        let last = names.pop().ok_or("empty path")?;
        let mut node = draft;
        for name in names {
            node = match node.get_mut(name) {
                Some(Value::Object(inner)) => inner,
                _ => return Err(format!("`{path}` has no object `{name}`")),
            };
        }
        if change.get("delete") == Some(&Value::Bool(true)) {
            node.remove(last)
                .map(|_| ())
                .ok_or(format!("`{path}` is absent"))
        } else {
            let key = Key::new(last).map_err(|_| format!("bad key `{last}`"))?;
            let value = change.get("value").cloned().ok_or("no value")?;
            node.insert(key, value);
            Ok(())
        }
    }

    /// The case's draft: its base with its changes applied.
    fn draft(section: &Value, case: &Value) -> Result<Vec<u8>, String> {
        let mut body = base(section, case)?;
        for change in list(case, "changes") {
            apply(&mut body, change)?;
        }
        Ok(to_canonical(&Value::Object(body)))
    }

    #[test]
    fn every_chain_event_and_valid_draft_parses() -> Result<(), String> {
        let section = section()?;
        let chain = list(&section, "chain");
        assert_eq!(chain.len(), 10);
        for entry in chain {
            let case = Value::Object(
                [(
                    Key::new("base_seq").map_err(|_| "key")?,
                    entry.get("seq").cloned().ok_or("seq")?,
                )]
                .into_iter()
                .collect(),
            );
            let bytes = draft(&section, &case)?;
            let parsed = Draft::parse(&bytes).map(|d| d.event_type().to_owned());
            assert_eq!(parsed, Ok(text(entry, "event_type").to_owned()));
        }
        let mut parsed = 0;
        for case in list(&section, "valid_drafts") {
            let bytes = draft(&section, case)?;
            assert_eq!(
                Draft::parse(&bytes).map(|_| ()),
                Ok(()),
                "{}",
                text(case, "name")
            );
            parsed += 1;
        }
        assert!(
            parsed >= 8,
            "{parsed} valid drafts; a vectors change may add more, never fewer"
        );
        for name in [
            "refused_stop",
            "refused_acknowledgment",
            "snapshot_reconciled",
            "snapshot_fees",
        ] {
            let case = Value::Object(
                [(
                    Key::new("base_draft").map_err(|_| "key")?,
                    Value::Str(name.to_owned()),
                )]
                .into_iter()
                .collect(),
            );
            let bytes = draft(&section, &case)?;
            assert_eq!(Draft::parse(&bytes).map(|_| ()), Ok(()), "{name}");
        }
        Ok(())
    }

    #[test]
    fn every_invalid_draft_is_refused_with_its_reason_at_its_path() -> Result<(), String> {
        let section = section()?;
        let mut checked = 0;
        for case in list(&section, "invalid_drafts") {
            let bytes = draft(&section, case)?;
            let expect = case.get("expect").ok_or("no expect")?;
            let refused = Draft::parse(&bytes)
                .err()
                .map(|e| (e.reason.code().to_owned(), e.path));
            let wanted = (
                text(expect, "reason").to_owned(),
                text(expect, "path").to_owned(),
            );
            assert_eq!(refused, Some(wanted), "{}", text(case, "name"));
            checked += 1;
        }
        assert!(
            checked >= 75,
            "{checked} invalid drafts; a vectors change may add more, never fewer"
        );
        Ok(())
    }

    /// The members §9.2 lists without `?`, at every depth, as paths below `payload`: written out
    /// from the spec's tables rather than read from this module's schemas, so a schema loosened to
    /// nullable cannot also drop its member from this list (#445 round 1, B2; round 2, C1). A list
    /// is named by its first and second elements, so a check that stops after the first is seen
    /// (#445 round 3, D1).
    const REQUIRED: [(&str, &[&str]); 11] = [
        ("StreamOpened", &["stream_type", "workspace_id"]),
        (
            "ConnectionEstablished",
            &[
                "connection_id",
                "broker",
                "environment",
                "scopes",
                "scopes[0]",
                "scopes[1]",
            ],
        ),
        ("ConnectionRevoked", &["connection_id"]),
        (
            "DisclosureAccepted",
            &[
                "document",
                "version",
                "user",
                "step_up",
                "step_up.assertion_id",
                "step_up.authenticated_at",
                "step_up.method",
            ],
        ),
        (
            "ConfigSnapshotRegistered",
            &["kind", "content_hash", "params", "params[0]", "params[1]"],
        ),
        (
            "MandateVersionCreated",
            &[
                "mandate_version",
                "provenance",
                "provenance[0].path",
                "provenance[0].source",
                "provenance[1].path",
                "provenance[1].source",
                "record_ref",
            ],
        ),
        (
            "MandateConfirmed",
            &[
                "mandate_version",
                "confirmed_paths",
                "confirmed_paths[0]",
                "confirmed_paths[1]",
                "record_ref",
            ],
        ),
        (
            "AgentDeployed",
            &["agent_id", "mandate_version", "record_ref"],
        ),
        (
            "AgentStopped",
            &[
                "agent_id",
                "connection_id",
                "reason",
                "retired_on",
                "loss_added",
            ],
        ),
        (
            "OwnerCommandRefused",
            &["command", "reason", "effective_at"],
        ),
        (
            "AccountSnapshotRecorded",
            &[
                "status",
                "crypto_status",
                "trading_blocked",
                "account_blocked",
                "trade_suspended_by_user",
                "multiplier",
                "equity",
                "cash",
                "buying_power",
                "non_marginable_buying_power",
                "accrued_fees",
                "risk_clock",
            ],
        ),
    ];

    /// The first base of `event_type`: a chain event, the agent-stream refusal draft, or the
    /// compared snapshot draft.
    fn base_of(section: &Value, event_type: &str) -> Result<Object, String> {
        let drafted = match event_type {
            "OwnerCommandRefused" => Some("refused_stop"),
            "AccountSnapshotRecorded" => Some("snapshot_reconciled"),
            _ => None,
        };
        let case = if let Some(name) = drafted {
            named("base_draft", Value::Str(name.to_owned()))?
        } else {
            let seq = list(section, "chain")
                .iter()
                .find(|e| e.get("event_type").and_then(Value::as_str) == Some(event_type))
                .and_then(|e| e.get("seq").cloned())
                .ok_or_else(|| format!("no chain event of type {event_type}"))?;
            named("base_seq", seq)?
        };
        base(section, &case)
    }

    fn named(key: &str, value: Value) -> Result<Value, String> {
        let key = Key::new(key).map_err(|_| "key")?;
        Ok(Value::Object([(key, value)].into_iter().collect()))
    }

    fn refusal(body: Object) -> Option<(String, String)> {
        Draft::parse(&to_canonical(&Value::Object(body)))
            .err()
            .map(|e| (e.reason.code().to_owned(), e.path))
    }

    fn with_payload(mut body: Object, member: &str, value: Value) -> Result<Object, String> {
        let key = Key::new(member).map_err(|_| "key")?;
        match body.get_mut("payload") {
            Some(Value::Object(payload)) => {
                payload.insert(key, value);
                Ok(body)
            }
            _ => Err("no payload".to_owned()),
        }
    }

    /// The value at `path` below `at`, a `.`-separated path whose steps may end in one `[i]`.
    fn value_at<'a>(at: &'a mut Value, path: &str) -> Result<&'a mut Value, String> {
        let mut here = at;
        for step in path.split('.') {
            let (name, index) = match step.split_once('[') {
                Some((name, rest)) => {
                    let i = rest.trim_end_matches(']').parse::<usize>();
                    (name, Some(i.map_err(|_| format!("index in {path}"))?))
                }
                None => (step, None),
            };
            here = match here {
                Value::Object(members) => members
                    .get_mut(name)
                    .ok_or_else(|| format!("no {name} in {path}"))?,
                _ => return Err(format!("{name} in {path} is not in a record")),
            };
            if let Some(i) = index {
                here = match here {
                    Value::Array(items) => items
                        .get_mut(i)
                        .ok_or_else(|| format!("no element {i} in {path}"))?,
                    _ => return Err(format!("{name} in {path} is not a list")),
                };
            }
        }
        Ok(here)
    }

    /// Every member §9.2 requires, at every depth, is refused as `schema` at that member when it
    /// is `null`: an absent value is never a valid one (§4.2), even on a reference a writer must
    /// also drop from `artifact_refs` (#445 round 1, B2), and even inside the step-up evidence,
    /// a provenance entry, or a list (#445 round 2, C1).
    #[test]
    fn a_required_member_is_never_null() -> Result<(), String> {
        let section = section()?;
        let mut checked = 0;
        for (event_type, members) in REQUIRED {
            let control = refusal(base_of(&section, event_type)?);
            assert_eq!(control, None, "the {event_type} base appends");
            for member in members {
                let mut body = Value::Object(base_of(&section, event_type)?);
                *value_at(&mut body, &format!("payload.{member}"))? = Value::Null;
                let Value::Object(body) = body else {
                    return Err("the base is a record".to_owned());
                };
                assert_eq!(
                    refusal(body),
                    Some(("schema".to_owned(), format!("payload.{member}"))),
                    "{event_type}.{member} = null"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 56);
        let mut body = Value::Object(base_of(&section, "MandateVersionCreated")?);
        let Value::Object(entry) = value_at(&mut body, "payload.provenance[1]")? else {
            return Err("a provenance entry is a record".to_owned());
        };
        let span = Key::new("quoted_span").map_err(|_| "key")?;
        entry.insert(span, Value::Str("the owner's words".to_owned()));
        let Value::Object(body) = body else {
            return Err("the base is a record".to_owned());
        };
        assert_eq!(
            refusal(body),
            Some((
                "schema".to_owned(),
                "payload.provenance[1].quoted_span".to_owned()
            )),
            "a second provenance entry is closed too (#445 round 3, D1)"
        );
        Ok(())
    }

    /// `OwnerCommandRefused.effective_at` is a §4.7 timestamp: other text is `non_canonical`, as
    /// its sibling `step_up.authenticated_at` is (#445 round 1, B1).
    #[test]
    fn a_refusal_time_is_a_timestamp() -> Result<(), String> {
        let section = section()?;
        let body = with_payload(
            base_of(&section, "OwnerCommandRefused")?,
            "effective_at",
            Value::Str("2026-09-21 20:30:00".to_owned()),
        )?;
        assert_eq!(
            refusal(body),
            Some((
                "non_canonical".to_owned(),
                "payload.effective_at".to_owned()
            ))
        );
        Ok(())
    }

    /// A payload that breaks two rules is refused with the lower-numbered one (§9.1's report
    /// order, which §9.2 keeps): rule 20 before 21 on one snapshot registration, and subject rule 26
    /// before copy rule 27 on one refusal (#445 round 1, B3). Within rule 21, the first offending
    /// member in the spec's order: `model_id`, `model_version`, `admits_instruments`, then
    /// `params` (#445 round 2, C2; round 3, D2).
    #[test]
    fn the_lower_numbered_rule_is_reported_first() -> Result<(), String> {
        let section = section()?;
        let fee = list(&section, "chain")
            .iter()
            .find(|e| {
                e.get("body")
                    .and_then(|b| b.get("payload"))
                    .and_then(|p| p.get("kind"))
                    .and_then(Value::as_str)
                    == Some("fee_config")
            })
            .and_then(|e| e.get("seq").cloned())
            .ok_or("a fee_config registration")?;
        let fee = base(&section, &named("base_seq", fee)?)?;
        let unsorted = Value::Array(vec![
            Value::Str("z_entry".to_owned()),
            Value::Str("lookback_bars".to_owned()),
        ]);
        let named_model = with_payload(fee.clone(), "model_id", Value::Str("pairs".to_owned()))?;
        let both = with_payload(
            named_model.clone(),
            "model_version",
            Value::Str("1".to_owned()),
        )?;
        assert_eq!(
            refusal(both),
            Some(("schema".to_owned(), "payload.model_id".to_owned())),
            "rule 21 reports model_id before model_version (#445 round 2, C2)"
        );
        let sorted = Value::Array(vec![Value::Str("lookback_bars".to_owned())]);
        let with_params = with_payload(named_model, "params", sorted)?;
        assert_eq!(
            refusal(with_params),
            Some(("schema".to_owned(), "payload.model_id".to_owned())),
            "rule 21 reports a model member before params (#445 round 2, C2)"
        );
        let versioned = with_payload(fee.clone(), "model_version", Value::Str("1".to_owned()))?;
        let admitting = with_payload(versioned, "admits_instruments", Value::Bool(false))?;
        assert_eq!(
            refusal(admitting),
            Some(("schema".to_owned(), "payload.model_version".to_owned())),
            "rule 21 reports model_version before admits_instruments (#445 round 3, D2)"
        );
        let body = with_payload(fee, "params", unsorted)?;
        assert_eq!(
            refusal(body),
            Some(("non_canonical".to_owned(), "payload.params".to_owned())),
            "rule 20 (order) before rule 21 (params only on a model)"
        );
        let mut refused = with_payload(
            base_of(&section, "OwnerCommandRefused")?,
            "command",
            Value::Str("acknowledge".to_owned()),
        )?;
        let cause = Key::new("causation_id").map_err(|_| "key")?;
        refused.insert(cause, Value::Null);
        assert_eq!(
            refusal(refused),
            Some(("stream_mismatch".to_owned(), "payload.command".to_owned())),
            "rule 26 (stream) before rule 27 (cause)"
        );
        Ok(())
    }

    /// Rule 28: `not_independent` is the executor's refusal of an acknowledgment, so it is accepted on
    /// the account stream and refused at `payload.reason` on the agent stream, where rule 26 has
    /// already let a resume or Stop through; a step-up reason stays accepted on both. The report
    /// order is pinned both ways: a command on the wrong stream is rule 26's (`payload.command`)
    /// even with `not_independent`, and an uncaused `not_independent` on the agent stream is rule
    /// 28's (`payload.reason`), not rule 27's (`causation_id`).
    #[test]
    fn not_independent_is_an_account_stream_refusal_only() -> Result<(), String> {
        let payload = |command: &str, reason: &str| -> Result<Value, String> {
            Ok(Value::Object(
                [
                    ("command", command),
                    ("reason", reason),
                    ("effective_at", "2026-09-22T13:00:00.000000000Z"),
                ]
                .into_iter()
                .map(|(k, v)| Ok((Key::new(k).map_err(|_| "key")?, Value::Str(v.to_owned()))))
                .collect::<Result<_, String>>()?,
            ))
        };
        let cause = Value::Str("01J8ZNB00000000000000000C6".to_owned());
        let at_reason = Some(("stream_mismatch", "payload.reason"));
        let at_command = Some(("stream_mismatch", "payload.command"));
        for (stream, command, reason, caused, verdict) in [
            ("acct:ws_1:a1", "acknowledge", "not_independent", true, None),
            (
                "agent:ws_1:agent_a",
                "stop",
                "not_independent",
                true,
                at_reason,
            ),
            (
                "agent:ws_1:agent_a",
                "resume",
                "not_independent",
                true,
                at_reason,
            ),
            ("agent:ws_1:agent_a", "stop", "step_up_stale", true, None),
            ("acct:ws_1:a1", "acknowledge", "step_up_reused", true, None),
            ("acct:ws_1:a1", "stop", "not_independent", true, at_command),
            (
                "agent:ws_1:agent_a",
                "acknowledge",
                "not_independent",
                true,
                at_command,
            ),
            (
                "agent:ws_1:agent_a",
                "stop",
                "not_independent",
                false,
                at_reason,
            ),
        ] {
            let stream = StreamId::parse(stream).ok_or("a stream id")?;
            let got = subject_and_copy(
                "OwnerCommandRefused",
                &stream,
                &payload(command, reason)?,
                caused.then_some(&cause),
            )
            .err()
            .map(|e| (e.reason.code(), e.path));
            assert_eq!(
                got,
                verdict.map(|(code, path)| (code, path.to_owned())),
                "{stream} {command} {reason} caused={caused}"
            );
        }
        Ok(())
    }

    /// Rule 26 names the account and agent streams only; on any other a refused command matches
    /// nothing, so it is refused (DEC-303 item 12). No other stream routes the type today.
    #[test]
    fn a_refusal_off_its_two_streams_is_refused() -> Result<(), String> {
        let payload = Value::Object(
            [
                ("command", "stop"),
                ("reason", "step_up_stale"),
                ("effective_at", "2026-09-21T20:30:00.000000000Z"),
            ]
            .into_iter()
            .map(|(k, v)| Ok((Key::new(k).map_err(|_| "key")?, Value::Str(v.to_owned()))))
            .collect::<Result<_, String>>()?,
        );
        let cause = Value::Str("01J8ZNB00000000000000000C6".to_owned());
        for (stream, verdict) in [
            ("agent:ws_1:agent_a", None),
            ("acct:ws_1:a1", Some("payload.command")),
            ("ctl:ws_1", Some("payload.command")),
            ("clock:ws_1", Some("payload.command")),
        ] {
            let stream = StreamId::parse(stream).ok_or("a stream id")?;
            let got = subject_and_copy("OwnerCommandRefused", &stream, &payload, Some(&cause))
                .err()
                .map(|e| (e.reason.code(), e.path));
            assert_eq!(
                got,
                verdict.map(|path| ("stream_mismatch", path.to_owned())),
                "{stream}"
            );
        }
        Ok(())
    }

    /// A snapshot at any `schema_version` but 1 is refused as `unknown_schema`, as every other §9.2
    /// type is: the snapshot reaches its schema through `schema`'s version gate, never around it
    /// (#467 round 1, m3).
    #[test]
    fn a_snapshot_at_another_schema_version_is_an_unknown_schema() -> Result<(), String> {
        let section = section()?;
        let mut body = base_of(&section, "AccountSnapshotRecorded")?;
        let version = Key::new("schema_version").map_err(|_| "key")?;
        let two = Int::new(2).ok_or("an integer")?;
        body.insert(version, Value::Int(two));
        assert_eq!(
            refusal(body),
            Some(("unknown_schema".to_owned(), "payload".to_owned()))
        );
        Ok(())
    }

    /// Journal spec §9.3's vectors, judged by `Draft::parse` alone so the mutation gate sees rules 29
    /// to 33 here: every base and valid draft of `MandateVersionApplied` and `UniverseChanged` is
    /// accepted, and every invalid one is refused with its reason at its path (DEC-403, DEC-404).
    #[test]
    fn every_risk_state_draft_is_judged_as_its_vectors_say() -> Result<(), String> {
        let section = named_section("risk_state")?;
        let mut failures = Vec::new();
        let drafts = section
            .get("drafts")
            .and_then(Value::as_object)
            .ok_or("no drafts")?;
        let mut accepted: Vec<(String, Value)> = Vec::new();
        for name in drafts.keys() {
            accepted.push((
                format!("base {}", name.as_str()),
                named("base_draft", Value::Str(name.as_str().to_owned()))?,
            ));
        }
        for case in list(&section, "valid_drafts") {
            accepted.push((format!("valid {}", text(case, "name")), case.clone()));
        }
        for (name, case) in &accepted {
            let parsed = Draft::parse(&draft(&section, case)?).map(|_| ());
            if parsed.is_err() {
                failures.push(format!("{name}: {parsed:?}"));
            }
        }
        let invalid = list(&section, "invalid_drafts");
        for case in invalid {
            let expect = case.get("expect").ok_or("no expect")?;
            let refused = Draft::parse(&draft(&section, case)?)
                .err()
                .map(|e| (e.reason.code().to_owned(), e.path));
            let wanted = (
                text(expect, "reason").to_owned(),
                text(expect, "path").to_owned(),
            );
            if refused.as_ref() != Some(&wanted) {
                failures.push(format!("invalid {}: {refused:?}", text(case, "name")));
            }
        }
        assert!(
            accepted.len() >= 12 && invalid.len() >= 38,
            "the vectors may add drafts, never drop them"
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        Ok(())
    }

    /// Rule 24 reports at the first cash member that disagrees with `model_cash`, in the schema's
    /// order: when both `cash_band` and `cash_in_band` disagree, at `cash_band` (#473 round 1, M1).
    #[test]
    fn rule_24_reports_the_band_before_the_flag() -> Result<(), String> {
        let section = section()?;
        let compared_alone = with_payload(
            with_payload(
                base_of(&section, "AccountSnapshotRecorded")?,
                "cash_band",
                Value::Null,
            )?,
            "cash_in_band",
            Value::Null,
        )?;
        let flagged_alone = with_payload(
            base_of(&section, "AccountSnapshotRecorded")?,
            "model_cash",
            Value::Null,
        )?;
        for (name, body) in [
            ("model_cash alone", compared_alone),
            ("band and flag alone", flagged_alone),
        ] {
            assert_eq!(
                refusal(body),
                Some(("schema".to_owned(), "payload.cash_band".to_owned())),
                "{name}"
            );
        }
        Ok(())
    }

    /// Rule 24 compares exact `Usd`: an amount `Usd` cannot hold is refused as `schema` at that
    /// member, and a difference of two that fit but overflows at `model_cash`. None is ever compared
    /// approximately or accepted (DEC-402 item 6; #473 round 1, M2).
    #[test]
    fn an_amount_rule_24_cannot_compare_exactly_is_refused() -> Result<(), String> {
        let section = section()?;
        let near = "78999999999999999999999999999";
        let cases: [(&[(&str, &str)], &str); 3] = [
            (
                &[("cash_band", "9.9999999999999999999999999999")],
                "payload.cash_band",
            ),
            (
                &[("cash", "10000000000000000000000000000.1")],
                "payload.cash",
            ),
            (
                &[("cash", near), ("model_cash", &format!("-{near}"))],
                "payload.model_cash",
            ),
        ];
        for (members, path) in cases {
            let mut body = base_of(&section, "AccountSnapshotRecorded")?;
            for (member, value) in members {
                body = with_payload(body, member, Value::Str((*value).to_owned()))?;
            }
            assert_eq!(
                refusal(body),
                Some(("schema".to_owned(), path.to_owned())),
                "{members:?}"
            );
        }
        Ok(())
    }

    /// The members §9.3 lists without `?`, at every depth, as paths below `payload`, written out
    /// from the spec's tables rather than read from this module's schemas: each is refused as
    /// `schema` when `null` (DEC-403, DEC-404). The vectors pin each member's type but not its
    /// nullability, so this is what keeps a schema loosened to nullable from passing.
    #[test]
    fn a_required_risk_state_member_is_never_null() -> Result<(), String> {
        const RISK_STATE_REQUIRED: [(&str, &[&str]); 2] = [
            (
                "version_applied",
                &[
                    "agent_id",
                    "old_version",
                    "new_version",
                    "classification",
                    "step_up.assertion_id",
                    "step_up.authenticated_at",
                    "step_up.method",
                    "result",
                    "risk_clock",
                ],
            ),
            (
                "universe_admitted",
                &[
                    "agent_id",
                    "instrument",
                    "change",
                    "reason",
                    "universe_size_after",
                    "risk_clock",
                ],
            ),
        ];
        let section = named_section("risk_state")?;
        let mut checked = 0;
        for (name, members) in RISK_STATE_REQUIRED {
            let case = named("base_draft", Value::Str(name.to_owned()))?;
            assert_eq!(
                refusal(base(&section, &case)?),
                None,
                "the {name} base appends"
            );
            for member in members {
                let mut body = Value::Object(base(&section, &case)?);
                *value_at(&mut body, &format!("payload.{member}"))? = Value::Null;
                let Value::Object(body) = body else {
                    return Err("the base is a record".to_owned());
                };
                assert_eq!(
                    refusal(body),
                    Some(("schema".to_owned(), format!("payload.{member}"))),
                    "{name}.{member} = null"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 15);
        let mut separated = with_payload(
            base(
                &section,
                &named("base_draft", Value::Str("version_rejected".to_owned()))?,
            )?,
            "reason",
            Value::Str("equity_below_exposure".to_owned()),
        )?;
        assert_eq!(
            refusal(separated.clone()),
            None,
            "a rejection rule 33 does not read appends"
        );
        separated = with_payload(separated, "classification", Value::Null)?;
        assert_eq!(
            refusal(separated),
            Some(("schema".to_owned(), "payload.classification".to_owned())),
            "a null classification is refused by the schema, where rule 33 cannot answer for it \
             (#482 round 1, m1)"
        );
        Ok(())
    }

    /// Rule 33 reads a raise only from what the record shows: an applied decrease or a zero change,
    /// and a rejection the fold reaches without a raise (`equity_below_exposure`,
    /// `would_trigger_limit`, `not_loosening`), are accepted under any classification, so a
    /// reducing or neutral version is never refused for them (DEC-403 item 4).
    #[test]
    fn rule_33_refuses_nothing_that_shows_no_raise() -> Result<(), String> {
        let section = named_section("risk_state")?;
        let text = |t: &str| Value::Str(t.to_owned());
        let cases: [(&str, &[(&str, Value)]); 5] = [
            (
                "version_applied",
                &[
                    ("classification", text("risk_reducing")),
                    ("step_up", Value::Null),
                    ("allocation_change", text("-2500")),
                ],
            ),
            (
                "version_applied",
                &[
                    ("classification", text("neutral")),
                    ("step_up", Value::Null),
                    ("allocation_change", text("0")),
                ],
            ),
            (
                "version_rejected",
                &[
                    ("classification", text("neutral")),
                    ("step_up", Value::Null),
                    ("reason", text("equity_below_exposure")),
                ],
            ),
            (
                "version_rejected",
                &[
                    ("classification", text("risk_reducing")),
                    ("reason", text("would_trigger_limit")),
                ],
            ),
            (
                "version_rejected",
                &[
                    ("classification", text("neutral")),
                    ("reason", text("not_loosening")),
                ],
            ),
        ];
        for (name, members) in cases {
            let mut body = base(&section, &named("base_draft", Value::Str(name.to_owned()))?)?;
            for (member, value) in members {
                body = with_payload(body, member, value.clone())?;
            }
            assert_eq!(refusal(body), None, "{name} with {members:?}");
        }
        Ok(())
    }

    /// `UniverseChanged.instrument` is journal spec v0.10's `asset_id`: mandate spec §3's lowercase
    /// `8-4-4-4-12` form. An `id` that is not one, or an asset ID in capitals, is refused as
    /// `non_canonical` at `payload.instrument`, so it can never append and then leave the stream's
    /// context unbuildable (DEC-404 item 9; #497 round 1, m3).
    #[test]
    fn an_instrument_that_is_not_an_asset_id_is_refused() -> Result<(), String> {
        let section = named_section("risk_state")?;
        let case = named("base_draft", Value::Str("universe_admitted".to_owned()))?;
        assert_eq!(refusal(base(&section, &case)?), None, "the base appends");
        for instrument in [
            "BTCUSD",
            "7B4A1C2E-2222-4A2B-9C3D-000000000002",
            "7b4a1c2e22224a2b9c3d000000000002",
            "7b4a1c2e-2222-4a2b-9c3d-00000000002",
            "7b4a1c2e-2222-4a2b-9c3d-0000000000g2",
            "7b4a1c2e_2222-4a2b-9c3d-000000000002",
        ] {
            let body = with_payload(
                base(&section, &case)?,
                "instrument",
                Value::Str(instrument.to_owned()),
            )?;
            assert_eq!(
                refusal(body),
                Some(("non_canonical".to_owned(), "payload.instrument".to_owned())),
                "{instrument}"
            );
        }
        Ok(())
    }

    /// Either risk-state record at any `schema_version` but 1 is refused as `unknown_schema`, as
    /// every §9.2 type is: each reaches its schema through `schema`'s version gate, never around it
    /// (#482 round 1, m2; #467 round 1, m3).
    #[test]
    fn a_risk_state_record_at_another_schema_version_is_an_unknown_schema() -> Result<(), String> {
        let section = named_section("risk_state")?;
        let two = Int::new(2).ok_or("an integer")?;
        for name in ["version_applied", "universe_admitted"] {
            let mut body = base(&section, &named("base_draft", Value::Str(name.to_owned()))?)?;
            assert_eq!(refusal(body.clone()), None, "the {name} base appends");
            body.insert(
                Key::new("schema_version").map_err(|_| "key")?,
                Value::Int(two),
            );
            assert_eq!(
                refusal(body),
                Some(("unknown_schema".to_owned(), "payload".to_owned())),
                "{name}"
            );
        }
        Ok(())
    }

    /// `/` is one empty reference token: RFC 6901 grammar admits it, and §9.2's "one or more
    /// `/`-prefixed reference tokens" does too, so it is accepted; only the empty pointer is not
    /// (DEC-303 item 13).
    #[test]
    fn a_pointer_of_one_empty_token_is_accepted() -> Result<(), String> {
        let section = section()?;
        let entry = |path: &str| -> Result<Value, String> {
            Ok(Value::Array(vec![Value::Object(
                [("path", path), ("source", "user_entered")]
                    .into_iter()
                    .map(|(k, v)| Ok((Key::new(k).map_err(|_| "key")?, Value::Str(v.to_owned()))))
                    .collect::<Result<_, String>>()?,
            )]))
        };
        let created = |path: &str| -> Result<Option<(String, String)>, String> {
            let body = base_of(&section, "MandateVersionCreated")?;
            Ok(refusal(with_payload(body, "provenance", entry(path)?)?))
        };
        assert_eq!(created("/")?, None);
        assert_eq!(
            created("")?,
            Some((
                "non_canonical".to_owned(),
                "payload.provenance[0].path".to_owned()
            ))
        );
        Ok(())
    }

    /// Journal spec §9.4's vectors, judged by `Draft::parse` alone so the mutation gate sees rules 34
    /// to 38 here: every base and valid draft of `ThesisProposed` and `ThesisRevised` is accepted,
    /// and every invalid one is refused with the first reason and path it expects (DEC-413,
    /// DEC-414). The order drafts pin which of two broken rules is reported first.
    #[test]
    fn every_thesis_draft_is_judged_as_its_vectors_say() -> Result<(), String> {
        let section = named_section("research")?;
        let mut failures = Vec::new();
        let drafts = section
            .get("drafts")
            .and_then(Value::as_object)
            .ok_or("no drafts")?;
        let mut accepted: Vec<(String, Value)> = Vec::new();
        for name in drafts.keys() {
            accepted.push((
                format!("base {}", name.as_str()),
                named("base_draft", Value::Str(name.as_str().to_owned()))?,
            ));
        }
        for case in list(&section, "valid_drafts") {
            accepted.push((format!("valid {}", text(case, "name")), case.clone()));
        }
        for (name, case) in &accepted {
            let parsed = Draft::parse(&draft(&section, case)?).map(|_| ());
            if parsed.is_err() {
                failures.push(format!("{name}: {parsed:?}"));
            }
        }
        let invalid = list(&section, "invalid_drafts");
        for case in invalid {
            let expect = case.get("expect").ok_or("no expect")?;
            let refused = Draft::parse(&draft(&section, case)?)
                .err()
                .map(|e| (e.reason.code().to_owned(), e.path));
            let wanted = (
                text(expect, "reason").to_owned(),
                text(expect, "path").to_owned(),
            );
            if refused.as_ref() != Some(&wanted) {
                failures.push(format!("invalid {}: {refused:?}", text(case, "name")));
            }
        }
        assert!(
            accepted.len() >= 16 && invalid.len() >= 60,
            "the vectors may add drafts, never drop them: {} valid drafts and bases, {} invalid drafts",
            accepted.len(),
            invalid.len()
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        Ok(())
    }

    /// The members §9.4 lists without `?`, as paths below `payload`, written out from the spec's
    /// table rather than read from this module's schemas: each is refused as `schema` when `null`
    /// (DEC-413, DEC-414). The vectors pin each member's type but not its nullability, so this is
    /// what keeps a member loosened to nullable from passing.
    #[test]
    fn a_required_thesis_member_is_never_null() -> Result<(), String> {
        const THESIS_REQUIRED: [&str; 22] = [
            "model_id",
            "model_version",
            "content_hash",
            "thesis_id",
            "lineage_id",
            "revision",
            "instrument_id",
            "asset_class",
            "direction",
            "as_of",
            "expires_at",
            "horizon_s",
            "conviction",
            "confidence",
            "evidence_sources",
            "evidence_sources[0]",
            "evidence_sources[1]",
            "invalidation",
            "allowlist_version",
            "prompt_ref",
            "response_ref",
            "admitted",
        ];
        let section = named_section("research")?;
        let mut checked = 0;
        for name in ["proposed_admitted", "revised_admitted"] {
            let case = named("base_draft", Value::Str(name.to_owned()))?;
            assert_eq!(
                refusal(base(&section, &case)?),
                None,
                "the {name} base appends"
            );
            for member in THESIS_REQUIRED {
                let mut body = Value::Object(base(&section, &case)?);
                *value_at(&mut body, &format!("payload.{member}"))? = Value::Null;
                let Value::Object(body) = body else {
                    return Err("the base is a record".to_owned());
                };
                assert_eq!(
                    refusal(body),
                    Some(("schema".to_owned(), format!("payload.{member}"))),
                    "{name}.{member} = null"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 44);
        Ok(())
    }

    /// Either thesis record at any `schema_version` but 1 is refused as `unknown_schema`: each
    /// reaches its schema through `schema`'s version gate, never around it (DEC-414).
    #[test]
    fn a_thesis_record_at_another_schema_version_is_an_unknown_schema() -> Result<(), String> {
        let section = named_section("research")?;
        let two = Int::new(2).ok_or("an integer")?;
        for name in ["proposed_admitted", "revised_admitted"] {
            let mut body = base(&section, &named("base_draft", Value::Str(name.to_owned()))?)?;
            assert_eq!(refusal(body.clone()), None, "the {name} base appends");
            body.insert(
                Key::new("schema_version").map_err(|_| "key")?,
                Value::Int(two),
            );
            assert_eq!(
                refusal(body),
                Some(("unknown_schema".to_owned(), "payload".to_owned())),
                "{name}"
            );
        }
        Ok(())
    }
}
