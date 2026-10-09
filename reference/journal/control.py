"""The control-stream vectors of journal.yaml (journal spec §9.2, DEC-261).

Journal spec §9.2 closes the control stream's payload schemas that `ValidationContext::from_journal`
reads (DEC-169), the account stream's `AccountSnapshotRecorded`, and `OwnerCommandRefused` on both
streams that write it. This module is their reference implementation, called by `generate.py`. It:

1. builds the artifacts, including the mandate document of the mandate reference cases' base
   `btc_accumulator` (its version recomputed from that file's own canonical text first), and the
   control-stream chain, hashing each body onto the previous one;
2. validates every draft against the closed schemas and the §9.2 rules: each chain event and base
   draft passes every rule, and each invalid draft breaks exactly the one rule it names;
3. recomputes, by its own path, the `JournaledFact` each record maps to, the version each record
   binds, and the cross-record facts (V-001, V-007) the chain must show;
4. seeds bugs into the validator and into the vectors, and requires every one to be caught by the
   one check it is registered against.
"""

import calendar
import copy
import functools
import hashlib
import re
from decimal import Decimal
from pathlib import Path

import yaml
from common import (
    BOOL,
    DEC,
    ENVELOPE,
    ULID,
    INT,
    REF,
    STEP_UP,
    STR,
    TS,
    T,
    Violation,
    apply_change,
    artifact_ref,
    ascending,
    canon,
    change,
    delete,
    digest_strings,
    draft_of,
    hash_chain,
    is_timestamp,
    is_ulid,
    list_of,
    normalize_decimal,
    one_of,
    opt,
    parse_instant,
    rec,
    rechain,
    sha256_hex,
    type_violations,
)
from common import (
    CONFIG_REF_KINDS as CONFIG_KINDS,
)
from common import (
    DIGEST as DIGEST_REF,
)
from common import (
    ID as IDENT_T,
)
from common import (
    IDENT as IDENT_RE,
)

ROOT = Path(__file__).resolve().parents[2]
MANDATE_CASES = ROOT / "docs/specs/reference-cases/mandate.yaml"
SPEC = "docs/specs/journal.md v0.7 §9.2 (DEC-261)"

# --------------------------------------------------------------------------- closed schemas (§9.2)

DATE = T("date")
RISK_CLOCK = T("risk_clock")
POINTER = T("pointer")
ASSET_ID = T("asset_id")
ASSET_ID_FORM = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\Z")
STREAM_ID = T("stream_id")
DIGEST_HEX = T("digest")
DIGEST_HEX_FORM = re.compile(r"^[0-9a-f]{64}\Z")
STREAM_SEGMENTS = {"acct": 3, "agent": 3, "ctl": 2, "clock": 2, "ntf": 2}
SOURCES = ("user_stated", "user_entered", "template_structure", "platform_proposed", "platform_default")
STOP_REASONS = ("goal_complete", "profit_stop_reached", "end_date", "owner_stop")
REFUSED_COMMANDS = {"agent": ("resume", "stop", "lift_hold"), "acct": ("acknowledge",)}
REFUSAL_REASONS = ("step_up_missing", "step_up_stale", "step_up_reused", "step_up_method", "not_independent")
MODEL_KIND = "model_version"
MODEL_MEMBERS = ("model_id", "model_version", "admits_instruments")
CASH_MEMBERS = ("cash_band", "cash_in_band")
RULE_21_REORDERINGS = {
    "order.rule_21_version_first": ("model_id", "model_version"),
    "order.rule_21_admits_first": ("model_version", "admits_instruments"),
}
RULE_21_PARAMS_FIRST = "order.rule_21_params_first"

SCHEMAS: dict[tuple[str, str], T] = {
    ("ctl", "StreamOpened"): rec(("stream_type", one_of("control")), ("workspace_id", IDENT_T)),
    ("ctl", "ConnectionEstablished"): rec(
        ("connection_id", IDENT_T),
        ("broker", STR),
        ("environment", one_of("paper", "live")),
        ("scopes", list_of(STR)),
    ),
    ("ctl", "ConnectionRevoked"): rec(("connection_id", IDENT_T)),
    ("ctl", "DisclosureAccepted"): rec(
        ("document", IDENT_T), ("version", REF), ("user", STR), ("step_up", STEP_UP)
    ),
    ("ctl", "ConfigSnapshotRegistered"): rec(
        ("kind", one_of(*CONFIG_KINDS)),
        ("content_hash", REF),
        ("model_id", opt(STR)),
        ("model_version", opt(STR)),
        ("params", list_of(STR)),
        ("admits_instruments", opt(BOOL)),
    ),
    ("ctl", "MandateVersionCreated"): rec(
        ("mandate_version", REF),
        ("provenance", list_of(rec(("path", POINTER), ("source", one_of(*SOURCES))))),
        ("record_ref", REF),
    ),
    ("ctl", "MandateConfirmed"): rec(
        ("mandate_version", REF), ("confirmed_paths", list_of(POINTER)), ("record_ref", REF)
    ),
    ("ctl", "AgentDeployed"): rec(("agent_id", IDENT_T), ("mandate_version", REF), ("record_ref", REF)),
    ("ctl", "AgentStopped"): rec(
        ("agent_id", IDENT_T),
        ("connection_id", IDENT_T),
        ("reason", one_of(*STOP_REASONS)),
        ("retired_on", DATE),
        ("loss_added", DEC),
    ),
    ("acct", "AccountSnapshotRecorded"): rec(
        ("status", STR),
        ("crypto_status", STR),
        ("trading_blocked", BOOL),
        ("account_blocked", BOOL),
        ("trade_suspended_by_user", BOOL),
        ("multiplier", INT),
        ("equity", DEC),
        ("cash", DEC),
        ("buying_power", DEC),
        ("non_marginable_buying_power", DEC),
        ("accrued_fees", DEC),
        ("model_cash", opt(DEC)),
        ("cash_band", opt(DEC)),
        ("cash_in_band", opt(BOOL)),
        ("risk_clock", RISK_CLOCK),
    ),
}
REFUSAL = rec(
    ("command", one_of("resume", "stop", "acknowledge")),
    ("reason", one_of(*REFUSAL_REASONS)),
    ("effective_at", TS),
)
SCHEMAS[("acct", "OwnerCommandRefused")] = REFUSAL
SCHEMAS[("agent", "OwnerCommandRefused")] = REFUSAL

REQUIRED_REFS = {
    "ApprovalResponded": ("mandate_version",),
    "ApprovalRevalidated": ("mandate_version",),
    "AgentDeployed": ("mandate_version",),
    "AgentStopped": ("mandate_version",),
    "MandateVersionApplied": ("mandate_version",),
    "UniverseChanged": ("mandate_version",),
    "ThesisProposed": ("mandate_version", "model_version"),
    "ThesisRevised": ("mandate_version", "model_version"),
}

# §9.3 (DEC-403): the account stream's risk-state records.
CLASSIFICATIONS = ("risk_increasing", "risk_reducing", "neutral")
VERSION_REJECTIONS = (
    "increase_blocked_while_latched",
    "equity_below_exposure",
    "would_trigger_limit",
    "not_loosening",
    "waiting_period",
    "still_below_new_floor",
)
UNIVERSE_REASONS = (
    "thesis_admitted",
    "thesis_expired",
    "thesis_invalidated",
    "lineage_retired",
    "eligibility_lost",
    "operator_halt",
    "version_applied",
)
ADMITTING = ("thesis_admitted", "version_applied")
REMOVING = tuple(r for r in UNIVERSE_REASONS if r != "thesis_admitted")
FROM_A_THESIS = ("thesis_admitted", "thesis_expired", "thesis_invalidated", "lineage_retired", "operator_halt")
# Rule 33: the rejections the risk fold reaches only on a risk-increasing version (`owner.rs`):
# a latched allocation increase, and a refused floor raise.
INCREASING_REJECTIONS = ("increase_blocked_while_latched", "waiting_period", "still_below_new_floor")
SCHEMAS[("acct", "MandateVersionApplied")] = rec(
    ("agent_id", IDENT_T),
    ("old_version", REF),
    ("new_version", REF),
    ("classification", one_of(*CLASSIFICATIONS)),
    ("step_up", opt(STEP_UP)),
    ("result", one_of("applied", "rejected")),
    ("reason", opt(one_of(*VERSION_REJECTIONS))),
    ("allocation_change", opt(DEC)),
    ("max_loss_from_allocation", opt(DEC)),
    ("risk_clock", RISK_CLOCK),
)
SCHEMAS[("acct", "UniverseChanged")] = rec(
    ("agent_id", IDENT_T),
    ("instrument", ASSET_ID),
    ("change", one_of("admitted", "removed")),
    ("reason", one_of(*UNIVERSE_REASONS)),
    ("thesis_id", opt(IDENT_T)),
    ("lineage_id", opt(IDENT_T)),
    ("universe_size_after", INT),
    ("risk_clock", RISK_CLOCK),
)

# §9.4 (DEC-413): the research agent's thesis records on the agent stream.
THESIS_REFUSALS = (
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
)
# Mandate spec §8.5's check number of each reason is its position in that order, from 1.
CHECK_NUMBER = {reason: number for number, reason in enumerate(THESIS_REFUSALS, start=1)}
CORROBORATION_CHECK = CHECK_NUMBER["no_corroboration"]
THESIS = rec(
    ("model_id", STR),
    ("model_version", STR),
    ("content_hash", REF),
    ("thesis_id", IDENT_T),
    ("lineage_id", IDENT_T),
    ("revision", INT),
    ("predecessor_thesis_id", opt(IDENT_T)),
    ("autopsy_ref", opt(REF)),
    ("instrument_id", ASSET_ID),
    ("asset_class", one_of("us_equity", "crypto")),
    ("direction", STR),
    ("as_of", TS),
    ("expires_at", TS),
    ("horizon_s", INT),
    ("conviction", DEC),
    ("confidence", DEC),
    ("evidence_ref", opt(REF)),
    ("evidence_sources", list_of(STR)),
    ("corroboration", opt(one_of("independent_source", "market_data"))),
    ("invalidation", STR),
    ("allowlist_version", INT),
    ("prompt_ref", REF),
    ("response_ref", REF),
    ("admitted", BOOL),
    ("reason", opt(one_of(*THESIS_REFUSALS))),
)
SCHEMAS[("agent", "ThesisProposed")] = THESIS
SCHEMAS[("agent", "ThesisRevised")] = THESIS

# §9.7 (DEC-533): the owner's approval answer and the runtime's two records of it. Times are integer
# risk-clock seconds, as §9.6's `ApprovalRequested.deadline` is, so their step-up evidence is not
# §9.2's `STEP_UP`, whose `authenticated_at` is a timestamp (DEC-533 item 2).
NULL = T("null")
ANSWER_STEP_UP = rec(("assertion_id", STR), ("authenticated_at", INT), ("method", STR))
ADMISSION_REASONS = (
    "not_pending",
    "late",
    "not_an_approver",
    "not_delivered",
    "content_mismatch",
    "step_up_missing",
    "step_up_stale",
    "step_up_reused",
    "step_up_method",
    "duplicate_approver",
    "not_independent",
)
# Mandate spec §6.4: a skip runs checks 1 to 5 only.
SKIP_REASONS = ADMISSION_REASONS[:5]
QUORUM_REASONS = ("duplicate_approver", "not_independent")
DRIFT_BANDS = (100, 200)
SCHEMAS[("ctl", "ApprovalResponseSubmitted")] = rec(
    ("agent", IDENT_T),
    ("approval", ULID),
    ("verdict", one_of("approved", "skipped")),
    ("content_hash", REF),
    ("submitted_at", INT),
    ("step_up", opt(ANSWER_STEP_UP)),
    ("responder", STR),
    ("role", one_of("approver")),
)
SCHEMAS[("agent", "ApprovalResponded")] = rec(
    ("approval", ULID),
    ("verdict", one_of("approved", "skipped")),
    ("responder", STR),
    ("role", one_of("approver")),
    ("result", one_of("admitted", "counted", "refused")),
    ("reason", opt(one_of(*ADMISSION_REASONS))),
    ("effective_at", INT),
    ("step_up", opt(ANSWER_STEP_UP)),
    ("quorum", opt(rec(("independent", BOOL), ("required", INT)))),
    ("separation_of_duties", NULL),
    ("delegation", NULL),
)
SCHEMAS[("agent", "ApprovalRevalidated")] = rec(
    ("approval", ULID),
    ("result", one_of("act", "skip")),
    ("reason", opt(STR)),
    ("mandate_version_bound", REF),
    ("mandate_version_now", REF),
    ("mode", one_of("normal", "exits_only", "paused", "stopped")),
    ("instrument_restricted", BOOL),
    ("decided_by_bound", STR),
    ("decided_by_now", opt(STR)),
    ("dry_run", one_of("allow", "deny")),
    ("dry_run_reason", opt(STR)),
    ("m_req", opt(DEC)),
    ("m_now", opt(DEC)),
    ("band_bp", INT),
)

# §9.8 (DEC-800): a connection's history. The control stream's three records and the account
# stream's three, with `ConnectionEstablished` at version 2 below, and the executor's copies of the
# establishment and a rotation on the account stream.
CHECK_REASONS = {
    "scope": ("scope_mismatch", "fund_movement", "permissions_unreadable"),
    "environment": ("wrong_environment", "reaches_both"),
    "account": ("account_unreadable", "account_mismatch", "not_dedicated"),
    "uniqueness": ("already_connected",),
    "contract": ("tools_missing", "contract_drift"),
}
# A teardown refuses a connect that no check refused (connections spec §5.2 step 6): `check` is null.
TEARDOWN_REASONS = ("timeout", "restart_past_deadline", "executor_stopped", "start_failed")
# The control services' fingerprint comparison: never an executor's result (rule 58).
SERVICES_ONLY_REASONS = ("account_mismatch",)
ALL_CHECK_REASONS = tuple(r for reasons in CHECK_REASONS.values() for r in reasons)
REFUSING_CHECKS = tuple(CHECK_REASONS)
EXECUTOR_CHECKS = ("account", "contract", "environment", "scope")
REQUIRED_CHECKS = ("account", "environment", "scope")
# Brokers whose connections are MCP (connections spec §3, `mcp_oauth`): check 7 is always listed.
MCP_BROKERS = ("robinhood",)
MARGIN_ATTESTATIONS = ("cash_account", "margin_disabled")
CONNECTION_STATES = ("active", "degraded", "suspended")
DEGRADING = ("network_errors", "rate_headroom", "contract_drift")
SUSPENDING = ("authorization_failed", "credential_expired", "refresh_failed", "check_failed", "lease_expired")
STATE_REASONS = (*DEGRADING, *SUSPENDING, "condition_cleared", "acknowledged")
PII_REF = T("pii_ref")
SCHEMAS[("ctl", "ConnectionRefused")] = rec(
    ("connection_id", IDENT_T),
    ("broker", STR),
    ("environment", one_of("paper", "live")),
    ("occasion", one_of("connect", "reconnect", "reauthorize")),
    ("check", opt(one_of(*REFUSING_CHECKS))),
    ("reason", one_of(*(r for c in REFUSING_CHECKS for r in CHECK_REASONS[c]), *TEARDOWN_REASONS)),
    ("existing_connection_id", opt(IDENT_T)),
    ("user", STR),
    ("step_up", STEP_UP),
)
# §9.8 v0.32 (DEC-694 item 4, DEC-699): the pending connection a connect starts with.
SCHEMAS[("ctl", "ConnectionRequested")] = rec(
    ("connection_id", IDENT_T),
    ("account_ref", ULID),
    ("broker", STR),
    ("environment", one_of("paper", "live")),
    ("user", STR),
    ("step_up", STEP_UP),
)
ROTATED_FIELDS = (("connection_id", IDENT_T), ("scopes", list_of(STR)), ("user", STR), ("step_up", STEP_UP))
SCHEMAS[("ctl", "ConnectionCredentialRotated")] = rec(*ROTATED_FIELDS)
SCHEMAS[("acct", "ConnectionCredentialRotated")] = rec(*ROTATED_FIELDS, ("risk_clock", RISK_CLOCK))
SCHEMAS[("acct", "ConnectionChecked")] = rec(
    ("connection_id", IDENT_T),
    ("occasion", one_of("connect", "reconnect", "reauthorize", "executor_start", "daily")),
    (
        "results",
        list_of(
            rec(
                ("check", one_of(*EXECUTOR_CHECKS)),
                ("result", one_of("passed", "failed")),
                ("reason", opt(one_of(*ALL_CHECK_REASONS))),
            )
        ),
    ),
    ("account_pii_ref", opt(PII_REF)),
    ("risk_clock", RISK_CLOCK),
)
SCHEMAS[("acct", "ConnectionStateChanged")] = rec(
    ("connection_id", IDENT_T),
    ("from", one_of(*CONNECTION_STATES)),
    ("to", one_of(*CONNECTION_STATES)),
    ("reason", one_of(*STATE_REASONS)),
    ("risk_clock", RISK_CLOCK),
)
SCHEMAS[("acct", "ConnectionCredentialRefreshed")] = rec(
    ("connection_id", IDENT_T), ("scopes", list_of(STR)), ("risk_clock", RISK_CLOCK)
)

# §9.9 (DEC-670): the records the workspace API commits. `MandateConfirmed` gains its agent link at
# `schema_version` 2 (registered with the §9.5 versions below); the others close at 1.
DRAFT_ORIGINS = ("description", "goal_answers", "template", "version", "edit", "compile")
NEW_DRAFT_ORIGINS = DRAFT_ORIGINS[:4]
CALL_REFUSALS = (
    "policy_denied",
    "budget_exhausted",
    "rate_limited_local",
    "meter_unavailable",
    "input_rejected",
    "model_withdrawn",
)
CALL_OUTCOMES = (
    "ok",
    *CALL_REFUSALS,
    "deadline_exceeded",
    "provider_unavailable",
    "rate_limited_provider",
    "credential_invalid",
    "content_refused",
    "schema_invalid",
    "identity_mismatch",
)
SENT_NOTHING = ("response_ref", "reported_identity", "provider_request_id", "attempts", "tokens", "cost_usd")
TOKENS = rec(("input", INT), ("output", INT), ("cached", INT))
SCHEMAS[("ctl", "MandateDraftSaved")] = rec(
    ("draft_id", IDENT_T),
    ("draft", REF),
    ("origin", one_of(*DRAFT_ORIGINS)),
    ("base_draft", opt(REF)),
    ("base_version", opt(REF)),
)
SCHEMAS[("ctl", "ModelInvocationRecorded")] = rec(
    ("call_id", ULID),
    ("purpose", one_of("compiler")),
    ("draft_id", IDENT_T),
    ("draft", REF),
    ("model", rec(("model_id", STR), ("model_version", STR), ("content_hash", REF))),
    ("endpoint", STR),
    ("request_digest", REF),
    ("sampling", rec(("temperature", DEC), ("seed", opt(INT)))),
    ("prompt_ref", REF),
    ("response_ref", opt(REF)),
    ("reported_identity", opt(STR)),
    ("provider_request_id", opt(STR)),
    ("outcome", one_of(*CALL_OUTCOMES)),
    ("attempts", INT),
    ("tokens", TOKENS),
    ("cost_usd", DEC),
    ("price_table_ref", REF),
    ("cache_hit", BOOL),
    ("deadline", TS),
    ("completed_at", TS),
)
SCHEMAS[("ctl", "OwnerRequestSubmitted")] = rec(
    ("agent_id", IDENT_T),
    ("instrument_id", T("asset_id")),
    ("side", one_of("buy", "sell")),
    ("quantity", opt(DEC)),
    ("requested_by", one_of("owner", "client")),
    ("client_id", opt(IDENT_T)),
)
CONFIRMED_V2 = rec(
    ("mandate_version", REF),
    ("confirmed_paths", list_of(POINTER)),
    ("record_ref", REF),
    ("agent_id", opt(IDENT_T)),
    ("base_version", opt(REF)),
)
REQUIRED_REFS["ModelInvocationRecorded"] = ("model_version",)

# §3 and §9.10 (DEC-671): the `client` actor, `ConnectionRevoked`'s reason, and the client records.
ACTOR_KINDS = ("system", "agent", "user", "broker", "platform_operator", "client")
CONTROL_ENVELOPE = rec(
    *(
        (name, rec(("kind", one_of(*ACTOR_KINDS)), *ty.fields[1:])) if name == "actor" else (name, ty)
        for name, ty in ENVELOPE.fields
    )
)
CLIENT_EVENTS = ("MandateDraftSaved", "OwnerRequestSubmitted", "RecordsAccessed")
CLIENT_SCOPES = ("dry_run", "hold", "propose", "read", "request")
REVOKED_V2 = rec(("connection_id", IDENT_T), ("reason", one_of("owner", "compromised")), ("step_up", STEP_UP))
SCHEMAS[("ctl", "ClientConnected")] = rec(
    ("client_id", IDENT_T),
    ("user", STR),
    ("scopes", list_of(one_of(*CLIENT_SCOPES))),
    ("agents", list_of(IDENT_T)),
    ("step_up", STEP_UP),
)
REVOCATION_ACTORS = {
    "owner": ("user",),
    "admin": ("user",),
    "member_deactivated": ("user", "system"),
    "deprovisioned": ("system",),
    "compromised": ("user", "system"),
}
SCHEMAS[("ctl", "ClientRevoked")] = rec(
    ("client_id", IDENT_T), ("user", STR), ("reason", one_of(*REVOCATION_ACTORS))
)

# §9.11 (DEC-672): the hold on new openings. `OwnerCommandIssued` is closed for its two hold commands
# only; the agent stream's two copies gain the hold at `schema_version` 2. `AgentModeChanged` version 1
# is `generate.py`'s (§9.1), so only version 2 is registered here.
HOLD_COMMANDS = ("hold_openings", "lift_hold")
MODE_ORDER = ("normal", "exits_only", "paused", "stopped")
OWNER_MODE_REASONS = ("owner_pause", "owner_resume", "owner_stop", "owner_hold", "owner_lift_hold")
# The members, and their order, are the CLI's `OwnerCommandIssued` for every command
# (`mandate-cli`'s `agent::issued`), so M7 closes the other commands without renaming any.
SCHEMAS[("ctl", "OwnerCommandIssued")] = rec(
    ("agent", IDENT_T),
    ("command", one_of(*HOLD_COMMANDS)),
    ("scope", one_of("agent")),
    ("subject", IDENT_T),
    ("release", NULL),
    ("warning_shown", NULL),
    ("bid", NULL),
    ("bid_size", NULL),
    ("floor", NULL),
    ("user", STR),
    ("submitted_at", INT),
    ("step_up", opt(rec(("assertion_id", STR), ("authenticated_at", INT), ("method", STR)))),
)
MODE_CHANGED_V2 = rec(
    ("from", one_of(*MODE_ORDER)),
    ("to", one_of(*MODE_ORDER)),
    ("reason", one_of("restriction_changed", "awaiting_reconciliation", "kill_switch", *OWNER_MODE_REASONS)),
    ("lifecycle", one_of("normal", "paused", "stopped")),
    ("held", BOOL),
)
REFUSAL_V2 = rec(
    ("command", one_of("resume", "stop", "acknowledge", "lift_hold")),
    ("reason", one_of(*REFUSAL_REASONS)),
    ("effective_at", TS),
)
CLIENT_EVENTS = (*CLIENT_EVENTS, "OwnerCommandIssued")

# §9.12 (DEC-437 item 9, DEC-648): the membership records. Instants are timestamps and step-up evidence
# is §9.2's `STEP_UP`; a member is a user principal's ULID.
ROLE = one_of("approver", "auditor", "operator", "viewer", "workspace_admin")
COOLING_ROLES = ("approver", "operator")
COOL_OFF_SECONDS = 86400
WRITERS = {
    "MemberInvited": "invited_by",
    "MemberInvitationRevoked": "revoked_by",
    "MemberRoleChanged": "changed_by",
    "MemberDeactivated": "by",
    "MemberReactivated": "by",
    "MemberRemoved": "by",
}
STEP_UP_WINDOW_SECONDS = 300
INVITATION_DAYS = 7
OWN_INSTANT = {
    "MemberInvited": "invited_at",
    "MemberActivated": "activated_at",
    "MemberRoleChanged": "changed_at",
    "MemberReactivated": "reactivated_at",
}
SYSTEM_REASONS = ("founding", "deprovisioned", "group_removed", "org_deleted")
SELF_REASONS = ("admin",)
SCHEMAS[("ctl", "MemberInvited")] = rec(
    ("invitation", ULID),
    ("roles", list_of(ROLE)),
    ("invited_by", STR),
    ("step_up", STEP_UP),
    ("invited_at", TS),
    ("expires_at", TS),
    ("session_ref", opt(STR)),
)
SCHEMAS[("ctl", "MemberInvitationRevoked")] = rec(
    ("invitation", ULID), ("revoked_by", STR), ("session_ref", opt(STR))
)
SCHEMAS[("ctl", "MemberActivated")] = rec(
    ("member", ULID),
    ("invitation", opt(ULID)),
    ("reason", one_of("invitation_accepted", "founding")),
    ("roles", list_of(ROLE)),
    ("method", one_of("passkey", "oidc", "email_link")),
    ("activated_at", TS),
    ("independent_approval_required", BOOL),
    ("cool_off_ends_at", TS),
    ("session_ref", opt(STR)),
)
SCHEMAS[("ctl", "MemberRoleChanged")] = rec(
    ("member", ULID),
    ("changed_by", STR),
    ("added", list_of(rec(("role", ROLE), ("cool_off_ends_at", TS)))),
    ("removed", list_of(ROLE)),
    ("changed_at", TS),
    ("independent_approval_required", BOOL),
    ("step_up", opt(STEP_UP)),
    ("session_ref", opt(STR)),
)
SCHEMAS[("ctl", "MemberDeactivated")] = rec(
    ("member", ULID),
    ("by", STR),
    ("reason", one_of("admin", "left", "deprovisioned", "group_removed")),
    ("session_ref", opt(STR)),
)
SCHEMAS[("ctl", "MemberReactivated")] = rec(
    ("member", ULID),
    ("by", STR),
    ("step_up", STEP_UP),
    ("roles", list_of(ROLE)),
    ("reactivated_at", TS),
    ("independent_approval_required", BOOL),
    ("cool_off_ends_at", TS),
    ("session_ref", opt(STR)),
)
SCHEMAS[("ctl", "MemberRemoved")] = rec(
    ("member", ULID),
    ("by", STR),
    ("reason", one_of("admin", "org_deleted")),
    ("session_ref", opt(STR)),
)

# §9.13 (DEC-780): the records-access, export, and verification records. Each names what it covers
# as stream ranges of its own workspace, bounded by event hashes, never by instrument or content.
# A `digest` is bare hex, so it names no stored artifact and stays out of `artifact_refs`.
GENESIS = "0" * 64
RANGE = rec(
    ("stream_id", STREAM_ID),
    ("from_seq", INT),
    ("to_seq", INT),
    ("prev_hash", DIGEST_HEX),
    ("to_hash", DIGEST_HEX),
)
# §11's codes: those reported at an event (checks 1 to 6, the anchored head, and every range check
# reported at the event that breaks it), then those reported for the range as a whole.
EVENT_CHECKS = (
    "non_canonical",
    "column_mismatch",
    "seq_gap",
    "rehash_mismatch",
    "prev_hash_mismatch",
    "artifact_missing",
    "artifact_mismatch",
    "anchor_head_mismatch",
    "anchor_self_mismatch",
    "break_glass_cause_mismatch",
    "intent_action_mismatch",
    "mode_event_mismatch",
    "held_mismatch",
    "connection_lifecycle_mismatch",
    "connection_cause_mismatch",
)
RANGE_CHECKS = ("anchor_root_mismatch", "tsa_token_invalid", "segment_manifest_mismatch", "segment_gap")
CHECKED_RANGE = rec(
    *RANGE.fields[:4],
    ("to_hash", opt(DIGEST_HEX)),
    ("failure", opt(rec(("check", one_of(*EVENT_CHECKS, *RANGE_CHECKS)), ("seq", opt(INT))))),
)
VIEW_FORMS = ("json", "csv")
TRIGGERS = ("startup", "segment_export", "weekly", "request", "restore_drill")
SCHEMAS[("ctl", "RecordsAccessed")] = rec(
    ("accessor", STR),
    ("operation", IDENT_T),
    ("ranges", list_of(RANGE)),
    ("resources", list_of(IDENT_T)),
    ("result", opt(REF)),
)
SCHEMAS[("ctl", "ExportCreated")] = rec(
    ("form", one_of("canonical", *VIEW_FORMS)),
    ("ranges", list_of(RANGE)),
    ("verifier_digest", DIGEST_HEX),
    ("view", opt(DIGEST_HEX)),
)
SCHEMAS[("ctl", "VerificationRun")] = rec(
    ("trigger", one_of(*TRIGGERS)),
    ("ranges", list_of(CHECKED_RANGE)),
    ("result", one_of("pass", "fail")),
)
# v0.36 (DEC-788 item 1, DEC-789): `VerificationRun` version 2 adds each range's trusted start, its
# count of events walked, and the check it could not finish, and `incomplete` as a third result.
# Version 1 is not edited (§8) and stays registered.
START_KINDS = ("genesis", "manifest", "anchor")
INCOMPLETE_CHECKS = ("tsa_token_invalid",)
CHECKED_RANGE_V2 = rec(
    *CHECKED_RANGE.fields[:5],
    ("start", rec(("kind", one_of(*START_KINDS)), ("manifest_hash", opt(DIGEST_HEX)), ("anchor_event_id", opt(ULID)))),
    ("checked", INT),
    CHECKED_RANGE.fields[5],
    ("incomplete", opt(one_of(*INCOMPLETE_CHECKS))),
)
VERIFICATION_RUN_V2 = rec(
    ("trigger", one_of(*TRIGGERS)),
    ("ranges", list_of(CHECKED_RANGE_V2)),
    ("result", one_of("pass", "incomplete", "fail")),
)

# §9.14 (DEC-783): the anchor and segment records, in the shapes the code already reads (the anchor
# file of DEC-115 item 6, DEC-263's manifest). Every hash is a bare-hex `digest`.
SCHEMAS[("ctl", "AnchorComputed")] = rec(
    ("leaves", list_of(rec(("hash", DIGEST_HEX), ("seq", INT), ("stream_id", STREAM_ID)))),
    ("root", DIGEST_HEX),
    ("token", opt(REF)),
)
SCHEMAS[("ctl", "SegmentExported")] = rec(
    ("stream_id", STREAM_ID),
    ("first_seq", INT),
    ("last_seq", INT),
    ("first_prev_hash", DIGEST_HEX),
    ("last_hash", DIGEST_HEX),
    ("file_sha256", DIGEST_HEX),
    ("manifest_hash", DIGEST_HEX),
)

# §9.5 (DEC-446, DEC-447): the account stream's executor records. The three records §9.5 closes at
# `schema_version` 2 carry both versions here; the companion and `ProtectionChanged` close at 1.
ACCOUNT_STREAM_REF_ALT = "01J8Z2ACCT00000000000000A2"
EXECUTOR_CLOSED = (
    "IntentReceived",
    "GateDecided",
    "OrderSubmitted",
    "OrderRequestRecorded",
    "ProtectionChanged",
)
GATE_CHECK_IDS = tuple(
    "account_status agent_mode eligibility concentration order_size session halt order_constraints "
    "mark_freshness collar conduct buying_power gross_exposure day_trade_budget".split()
)
INTENT_V1 = rec(
    ("intent_id", ULID),
    ("agent_id", IDENT_T),
    ("instrument_id", STR),
    ("side", STR),
    ("type", STR),
    ("tif", STR),
    ("qty", DEC),
    ("limit_price", opt(DEC)),
    ("purpose", STR),
)
GATE_V1 = rec(
    ("intent_id", ULID),
    ("verdict", STR),
    ("reason_code", opt(STR)),
    ("data_profile", STR),
    ("quotes_used", list_of(rec(("instrument_id", STR), ("bid", DEC), ("ask", DEC), ("as_of", TS), ("feed", STR)))),
    ("marks_used", list_of(rec(("instrument_id", STR), ("price", DEC), ("source", STR), ("kind", STR)))),
    ("checks", list_of(rec(("id", one_of(*GATE_CHECK_IDS)), ("result", STR), ("inputs", T("object")), ("computed", T("object"))))),
)
SUBMITTED_V1 = rec(
    ("client_order_id", STR),
    ("attempt", INT),
    ("instrument_id", STR),
    ("side", STR),
    ("type", STR),
    ("tif", STR),
    ("qty", DEC),
    ("limit_price", opt(DEC)),
)
COMPANION = rec(
    ("agent_id", IDENT_T),
    ("intent_id", opt(ULID)),
    ("purpose", one_of("open", "increase", "discretionary_exit", "risk_exit", "owner_exit", "protective", "flatten")),
    ("extended_hours", BOOL),
    ("stop_price", opt(DEC)),
    ("order_class", opt(one_of("bracket", "oco"))),
    ("take_profit", opt(DEC)),
    ("stop", opt(DEC)),
    ("rung", opt(INT)),
    ("at_floor", opt(BOOL)),
    ("risk_clock", RISK_CLOCK),
)
PROTECTION_CHANGED = rec(
    ("instrument_id", STR),
    ("action", one_of("intended", "placed", "cancelled", "passive_start", "unprotected_start", "watchdog", "exit_unpriced", "ladder_floor", "expiry_unreplaceable", "rung_short", "interval_limit", "unprotected_end")),
    ("orders", list_of(STR)),
    ("awaiting", list_of(STR)),
    ("qty", opt(DEC)),
    ("stop", opt(DEC)),
    ("take_profit", opt(DEC)),
    ("intent_id", opt(ULID)),
    ("bracket", opt(STR)),
    ("entry", opt(STR)),
    ("agent_id", opt(IDENT_T)),
    ("replacing", opt(BOOL)),
    ("created_on", opt(DATE)),
    ("sent", opt(DEC)),
    ("uncovered", opt(BOOL)),
    ("acknowledged", opt(BOOL)),
    ("risk_clock", RISK_CLOCK),
)
ESTABLISHED_V2 = rec(
    *SCHEMAS[("ctl", "ConnectionEstablished")].fields,
    ("account_ref", ULID),
    ("user", STR),
    ("step_up", STEP_UP),
    ("margin_attestation", opt(one_of(*MARGIN_ATTESTATIONS))),
)
VERSIONED_VERSIONS: dict[tuple[str, str], tuple[int, ...]] = {
    ("acct", "StreamOpened"): (1,),
    ("acct", "IntentReceived"): (1, 2),
    ("acct", "GateDecided"): (1, 2),
    ("acct", "OrderSubmitted"): (1, 2),
    ("acct", "OrderRequestRecorded"): (1,),
    ("acct", "ProtectionChanged"): (1,),
    ("ctl", "ConnectionEstablished"): (1, 2),
    ("acct", "ConnectionEstablished"): (2,),
    ("ctl", "MandateConfirmed"): (1, 2),
    ("ctl", "ConnectionRevoked"): (1, 2),
    ("agent", "AgentModeChanged"): (1, 2),
    ("agent", "OwnerCommandRefused"): (1, 2),
    ("ctl", "VerificationRun"): (1, 2),
}
VERSIONED_SCHEMAS: dict[tuple[str, str, int], T] = {
    ("acct", "StreamOpened", 1): rec(
        ("stream_type", one_of("account")),
        ("workspace_id", IDENT_T),
        ("broker", STR),
        ("account_ref", IDENT_T),
    ),
    ("acct", "IntentReceived", 1): INTENT_V1,
    ("acct", "IntentReceived", 2): rec(*INTENT_V1.fields, ("risk_clock", RISK_CLOCK)),
    ("acct", "GateDecided", 1): GATE_V1,
    ("acct", "GateDecided", 2): rec(*GATE_V1.fields, ("risk_clock", RISK_CLOCK)),
    ("acct", "OrderSubmitted", 1): SUBMITTED_V1,
    ("acct", "OrderSubmitted", 2): rec(*SUBMITTED_V1.fields, ("risk_clock", RISK_CLOCK)),
    ("acct", "OrderRequestRecorded", 1): COMPANION,
    ("acct", "ProtectionChanged", 1): PROTECTION_CHANGED,
    ("ctl", "ConnectionEstablished", 2): ESTABLISHED_V2,
    ("acct", "ConnectionEstablished", 2): rec(*ESTABLISHED_V2.fields, ("risk_clock", RISK_CLOCK)),
    ("ctl", "MandateConfirmed", 2): CONFIRMED_V2,
    ("ctl", "ConnectionRevoked", 2): REVOKED_V2,
    ("agent", "AgentModeChanged", 1): rec(
        ("from", one_of(*MODE_ORDER)),
        ("to", one_of(*MODE_ORDER)),
        ("reason", one_of("restriction_changed", "awaiting_reconciliation", "kill_switch", *OWNER_MODE_REASONS[:3])),
        ("lifecycle", one_of("normal", "paused", "stopped")),
    ),
    ("agent", "AgentModeChanged", 2): MODE_CHANGED_V2,
    ("agent", "OwnerCommandRefused", 2): REFUSAL_V2,
    ("ctl", "VerificationRun", 2): VERIFICATION_RUN_V2,
}


def is_pointer(text: str) -> bool:
    """A non-empty RFC 6901 pointer: `/`-prefixed tokens, `~` only as `~0` or `~1`."""
    if not text.startswith("/"):
        return False
    for token in text[1:].split("/"):
        rest = token.replace("~0", "").replace("~1", "")
        if "~" in rest:
            return False
    return True


def is_stream_id(text: str) -> bool:
    """§2's form: a known stream type and its segment count, each segment an identifier."""
    parts = text.split(":")
    return STREAM_SEGMENTS.get(parts[0]) == len(parts) and all(IDENT_RE.match(s) for s in parts[1:])


def is_date(text: str) -> bool:
    if len(text) != 10 or text[4] != "-" or text[7] != "-":
        return False
    year, month, day = text[:4], text[5:7], text[8:]
    if not (year + month + day).isdigit() or not 1970 <= int(year) <= 9999:
        return False
    days = (
        31,
        29 if int(year) % 4 == 0 and (int(year) % 100 != 0 or int(year) % 400 == 0) else 28,
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    )
    return 1 <= int(month) <= 12 and 1 <= int(day) <= days[int(month) - 1]


def loose_date(text: str) -> bool:
    """The seeded bug `types.date_length`: a date gate that checks the separators and the calendar
    but not the 10-character form, so `2026-09-0005` reads as 5 September."""
    parts = text.split("-")
    if len(parts) != 3 or not all(p.isdigit() for p in parts):
        return False
    return is_date(f"{int(parts[0]):04d}-{int(parts[1]):02d}-{int(parts[2]):02d}")


def nested_record(path: str) -> str:
    """The seeded-bug key of a nested record: `step_up`, a `provenance` entry, or the payload itself."""
    if path == "payload.step_up":
        return "step_up"
    if path.startswith("payload.ranges[") and path.endswith(".failure"):
        return "failure"
    if path.startswith("payload.ranges[") and path.endswith(".start"):
        return "start"
    if path.startswith("payload.ranges["):
        return "range"
    if path.startswith("payload.leaves["):
        return "leaf"
    if path.startswith("payload.provenance["):
        return "provenance_entry"
    return "payload"


def payload_type_violations(ty: T, value, path: str, skip: frozenset[str]) -> list[Violation]:
    """`type_violations` with §9.2's two new types: `pointer` and `date` (refused as `id` is), and
    §9.7's `null`."""
    if ty.kind == "null":
        if value is None or "types.null" in skip:
            return []
        return [Violation("types", "schema", path)]
    if ty.kind == "risk_clock":
        if value is None and "types.risk_clock_nullable" in skip:
            return []
        if not isinstance(value, str):
            return [Violation("types", "schema", path)]
        whole = value.endswith(".000000000Z") or "types.risk_clock_whole" in skip
        ok = is_timestamp(value) and whole
        if not ok and "types.risk_clock" not in skip:
            return [Violation("types", "non_canonical", path)]
        return []
    if ty.kind == "pii_ref":
        if not isinstance(value, str):
            return [Violation("types", "schema", path)]
        ok = (value.startswith("pii_") and is_ulid(value[4:])) or "types.pii_ref" in skip
        return [] if ok else [Violation("types", "non_canonical", path)]
    if ty.kind in ("pointer", "date", "asset_id", "stream_id", "digest"):
        if not isinstance(value, str):
            return [Violation("types", "schema", path)]
        if ty.kind == "pointer":
            ok = is_pointer(value)
        elif ty.kind == "stream_id":
            ok = is_stream_id(value)
        elif ty.kind == "digest":
            ok = bool(DIGEST_HEX_FORM.match(value))
        elif ty.kind == "asset_id":
            ok = bool(ASSET_ID_FORM.match(value)) or (
                "types.asset_id_trailing_newline" in skip and bool(ASSET_ID_FORM.match(value.removesuffix("\n")))
            ) or (
                "types.asset_id_case" in skip and bool(ASSET_ID_FORM.match(value.lower()))
            ) or ("types.asset_id_ident" in skip and bool(IDENT_RE.match(value)))
        elif "types.date_length" in skip:
            ok = loose_date(value)
        else:
            ok = is_date(value)
        if not ok and f"types.{ty.kind}" not in skip:
            return [Violation("types", "non_canonical", path)]
        return []
    if ty.kind == "record" and isinstance(value, dict):
        names = [name for name, _ in ty.fields]
        out = []
        depth = nested_record(path)
        past_first = path.endswith("]") and not path.endswith("[0]")
        if past_first and f"open_after_first.{depth}" in skip:
            pass
        elif "record.extra" not in skip and f"open.{depth}" not in skip:
            out += [
                Violation("record.extra", "schema", f"{path}.{k}".lstrip("."))
                for k in sorted(value)
                if k not in names
            ]
        for name, inner in ty.fields:
            member = f"{path}.{name}".lstrip(".")
            if name not in value:
                read_as_null = inner.kind == "nullable" and "record.missing.nullable" in skip
                if "record.missing" not in skip and not read_as_null:
                    out.append(Violation("record.missing", "schema", member))
            elif f"loose.{depth}.{name}" in skip:
                pass
            elif f"text.{depth}.{name}" in skip:
                out += payload_type_violations(STR, value[name], member, skip)
            elif value[name] is None and f"nullable.{depth}.{name}" in skip:
                pass
            else:
                out += payload_type_violations(inner, value[name], member, skip)
        return out
    if ty.kind == "list" and isinstance(value, list):
        checked = value[:1] if f"first_only.{path.rsplit('.', 1)[-1]}" in skip else value
        return [
            v
            for i, item in enumerate(checked)
            for v in payload_type_violations(ty.inner, item, f"{path}[{i}]", skip)
        ]
    if ty.kind == "nullable":
        return [] if value is None else payload_type_violations(ty.inner, value, path, skip)
    return type_violations(ty, value, path, skip)


def encoded(texts: list) -> list[bytes]:
    """A list's elements as bytes to order; a seeded bug can let a `null` element through, which
    reads as empty text, as the journal's own reader does."""
    return [(t if isinstance(t, str) else "").encode() for t in texts]


def consistency_violations(event_type: str, draft: dict, skip: frozenset[str]) -> list[Violation]:
    """The §9.2 consistency rules 17 to 24, §9.3's 29 to 33, and §9.4's 34 to 38, on a well-typed
    payload, each reported once."""
    p = draft["payload"]
    out: list[Violation] = []

    def rule(name: str, holds: bool, reason: str, path: str) -> None:
        if not holds and f"rule.{name}" not in skip:
            out.append(Violation(f"rule.{name}", reason, path))

    if event_type == "MandateVersionCreated":
        paths = [(entry["path"] or "").encode() for entry in p["provenance"]]
        rule("17", ascending(paths), "non_canonical", "payload.provenance")
    if event_type == "MandateConfirmed":
        paths = encoded(p["confirmed_paths"])
        rule("18", ascending(paths), "non_canonical", "payload.confirmed_paths")
    if event_type == "ConnectionEstablished":
        rule("19", ascending(encoded(p["scopes"])), "non_canonical", "payload.scopes")
    if event_type == "ConfigSnapshotRegistered":
        rule_20_first = "order.rule_21_first" not in skip
        if rule_20_first:
            rule("20", ascending(encoded(p["params"])), "non_canonical", "payload.params")
        model = p["kind"] == MODEL_KIND
        order = [*MODEL_MEMBERS, "params"]
        for bug, (a, b) in RULE_21_REORDERINGS.items():
            if bug in skip:
                i, j = order.index(a), order.index(b)
                order[i], order[j] = order[j], order[i]
        if RULE_21_PARAMS_FIRST in skip:
            order = ["params", *MODEL_MEMBERS]
        params_wrong = not model and p["params"] and "rule.21.params" not in skip
        wrong = [
            m
            for m in order
            if (m == "params" and params_wrong) or (m != "params" and (p[m] is not None) != model)
        ]
        rule("21", not wrong, "schema", f"payload.{wrong[0]}" if wrong else "")
        if not rule_20_first:
            rule("20", ascending(encoded(p["params"])), "non_canonical", "payload.params")
    if event_type == "AgentDeployed" and "mandate_version" in draft["config_refs"]:
        theirs = draft["config_refs"]["mandate_version"]
        if "rule.22.prefix" in skip:
            bound = p["mandate_version"][:8] == theirs[:8]
        else:
            bound = p["mandate_version"] == theirs
        rule("22", bound, "schema", "payload.mandate_version")
    if event_type == "AgentStopped":
        floor = (
            Decimal(p["loss_added"]) > 0
            if "boundary.rule_23_strict" in skip
            else Decimal(p["loss_added"]) >= 0
        )
        rule("23", floor, "schema", "payload.loss_added")
    if event_type == "AccountSnapshotRecorded":
        compared = p["model_cash"] is not None
        wrong = [m for m in CASH_MEMBERS if (p[m] is not None) != compared]
        rule("24", not wrong, "schema", f"payload.{wrong[0]}" if wrong else "")
        if compared and not wrong:
            band = Decimal(p["cash_band"])
            rule("24.band", band >= 0, "schema", "payload.cash_band")
            drift = abs(Decimal(p["cash"]) - Decimal(p["model_cash"]))
            inside = drift < band if "boundary.rule_24_strict" in skip else drift <= band
            rule("24.in_band", p["cash_in_band"] == inside, "schema", "payload.cash_in_band")
    if event_type == "MandateVersionApplied":
        increasing = p["classification"] == "risk_increasing"
        rejected = p["result"] == "rejected"

        def rule_29() -> None:
            rule("29", not increasing or p["step_up"] is not None, "schema", "payload.step_up")

        def rule_30() -> None:
            wrong = []
            if (p["reason"] is not None) != rejected and "rule.30.reason" not in skip:
                wrong.append("reason")
            wrong += [m for m in ("allocation_change", "max_loss_from_allocation") if rejected and p[m] is not None]
            rule("30", not wrong, "schema", f"payload.{wrong[0]}" if wrong else "")

        def rule_33() -> None:
            applied = p["result"] == "applied"
            implied = [
                applied
                and p["allocation_change"] is not None
                and Decimal(p["allocation_change"]) > 0
                and "rule.33.allocation" not in skip,
                applied and p["max_loss_from_allocation"] is not None and "rule.33.floor" not in skip,
                rejected and p["reason"] in INCREASING_REJECTIONS and "rule.33.reason" not in skip,
            ]
            rule("33", not any(implied) or increasing, "schema", "payload.classification")

        order = [rule_29, rule_30, rule_33]
        if "order.rule_30_first" in skip:
            order = [rule_30, rule_29, rule_33]
        if "order.rule_33_first" in skip:
            order = [rule_29, rule_33, rule_30]
        for check in order:
            check()
    if event_type == "UniverseChanged":

        def rule_31() -> None:
            allowed = ADMITTING if p["change"] == "admitted" else REMOVING
            rule("31", p["reason"] in allowed, "schema", "payload.reason")

        def rule_32() -> None:
            thesis, lineage = p["thesis_id"] is not None, p["lineage_id"] is not None
            if thesis != lineage:
                rule("32", False, "schema", "payload.thesis_id" if not thesis else "payload.lineage_id")
            elif p["reason"] in FROM_A_THESIS:
                rule("32.thesis", thesis, "schema", "payload.thesis_id")
            elif p["reason"] == "version_applied" and p["change"] == "admitted":
                rule("32.pinned", not thesis, "schema", "payload.thesis_id")

        for check in [rule_32, rule_31] if "order.rule_32_first" in skip else [rule_31, rule_32]:
            check()
    if event_type in ("ThesisProposed", "ThesisRevised"):
        out += thesis_violations(event_type, draft, skip)
    if event_type == "OrderRequestRecorded":
        flags = [p["order_class"] is None, p["take_profit"] is None, p["stop"] is None]
        scan = list(zip(("order_class", "take_profit"), flags, flags[1:]))
        if "order.rule_39_last" in skip:
            scan = list(reversed(scan))
        wrong = next((m for m, a, b in scan if a != b), None)
        rule("39", wrong is None or "rule.39" in skip, "schema", f"payload.{wrong}" if wrong else "")
        rule("40", (p["at_floor"] is None) == (p["rung"] is None) or "rule.40" in skip, "schema", "payload.at_floor")
    if event_type == "ProtectionChanged":
        action = p["action"]
        priced = action in ("intended", "placed", "passive_start", "unprotected_start")
        starts = action in ("passive_start", "unprotected_start")
        if "tighten.41.starts" in skip:
            starts = False
        names_orders = action in ("placed", "cancelled", "passive_start", "unprotected_start")
        orders_fit = starts or (len(p["orders"]) == 0) != names_orders
        rule("41", orders_fit or "rule.41" in skip, "schema", "payload.orders")
        may_await = action in ("interval_limit", "unprotected_end")
        awaiting_fit = may_await or len(p["awaiting"]) == 0
        if "tighten.41.awaiting" in skip:
            awaiting_fit = (len(p["awaiting"]) == 0) != may_await
        rule("41.awaiting", awaiting_fit or "rule.41" in skip, "schema", "payload.awaiting")
        if action == "placed":
            rule("42", p["qty"] is not None or "rule.42" in skip, "schema", "payload.qty")
        elif action != "cancelled":
            rule("42", p["qty"] is None or "rule.42" in skip, "schema", "payload.qty")
        rule("43", priced or (p["stop"] is None and p["take_profit"] is None) or "rule.43" in skip, "schema", "payload.stop")
        rule("43.tp", p["stop"] is not None or p["take_profit"] is None or "rule.43" in skip, "schema", "payload.take_profit")
        intents = action in ("intended", "rung_short")
        if action in ("passive_start", "unprotected_start"):
            replaces = (p["replacing"] is True or "rule.44.replacing_only" in skip) and "tighten.44.trio" not in skip
            if p["intent_id"] is not None:
                set_ = True
            elif replaces:
                set_ = p["entry"] is not None
            else:
                set_ = False
            wrong = [m for m in ("entry", "agent_id") if (p[m] is not None) != set_]
            if replaces and p["intent_id"] is None and "rule.44.pair" in skip:
                wrong = []
            rule("44.trio", not wrong or "rule.44.trio" in skip, "schema", f"payload.{wrong[0]}" if wrong else "")
        else:
            if (p["intent_id"] is not None) != intents:
                rule("44.intent", False, "schema", "payload.intent_id")
            for member in ("entry", "agent_id"):
                if p[member] is not None:
                    rule(f"44.{member}", False, "schema", f"payload.{member}")
        if p["replacing"] is not None and action not in ("passive_start", "unprotected_start"):
            rule("44.replacing", False, "schema", "payload.replacing")
        if p["bracket"] is not None and action not in ("placed", "passive_start", "unprotected_start", "unprotected_end"):
            rule("44.bracket", False, "schema", "payload.bracket")
        if p["created_on"] is not None and action != "placed":
            rule("44.created_on", False, "schema", "payload.created_on")
        if p["sent"] is not None and action != "rung_short":
            rule("44.sent", False, "schema", "payload.sent")
        if p["uncovered"] is not None and action not in ("interval_limit", "unprotected_end"):
            rule("44.uncovered", False, "schema", "payload.uncovered")
        if p["acknowledged"] is not None and action != "unprotected_end":
            rule("44.acknowledged", False, "schema", "payload.acknowledged")
    out += answer_violations(event_type, draft, skip)
    out += connection_violations(event_type, draft, skip)
    out += workspace_violations(event_type, draft, skip)
    out += membership_violations(event_type, draft, skip)
    out += audit_violations(event_type, draft, skip)
    out += cold_violations(event_type, draft, skip)
    return out


def connection_violations(event_type: str, draft: dict, skip: frozenset[str]) -> list[Violation]:
    """§9.8's consistency rules 54 to 65 but the copy rules 61 and 63, each reported once."""
    p = draft["payload"]
    out: list[Violation] = []

    def rule(name: str, holds: bool, path: str, reason: str = "schema") -> None:
        if not holds and f"rule.{name}" not in skip:
            out.append(Violation(f"rule.{name}", reason, path))

    if event_type == "ConnectionRefused":
        if p["check"] is None:
            fits = p["reason"] in TEARDOWN_REASONS or "rule.54.teardown" in skip
        else:
            fits = p["reason"] in CHECK_REASONS.get(p["check"], ())
        rule("54", fits, "payload.reason")
        existing = p["existing_connection_id"]
        if p["check"] == "uniqueness":
            named = existing is not None and (existing != p["connection_id"] or "rule.55.self" in skip)
        else:
            named = existing is None
        rule("55", named, "payload.existing_connection_id")
        rule("65", p["check"] is not None or draft["causation_id"] is None, "causation_id")
    if event_type in ("ConnectionCredentialRotated", "ConnectionCredentialRefreshed"):
        scopes = p["scopes"]
        rule("56", isinstance(scopes, list) and ascending(encoded(scopes)), "payload.scopes", "non_canonical")
    if event_type == "ConnectionChecked" and isinstance(p["results"], list):
        checks = encoded([r["check"] for r in p["results"]])
        rule("57.order", ascending(checks), "payload.results", "non_canonical")
        listed = {r["check"] for r in p["results"]}
        rule("57.required", all(c in listed for c in REQUIRED_CHECKS), "payload.results")
        for i, r in enumerate(p["results"]):
            failed = r["result"] == "failed"
            belongs = r["reason"] in CHECK_REASONS.get(r["check"], ()) or "rule.58.belongs" in skip
            executors = r["reason"] not in SERVICES_ONLY_REASONS or "rule.58.services" in skip
            fits = (r["reason"] is not None) == failed and (r["reason"] is None or (belongs and executors))
            rule("58", fits, f"payload.results[{i}].reason")
            if not fits:
                break
        unread = any(r["check"] == "account" and r["reason"] == "account_unreadable" for r in p["results"])
        reference = p["account_pii_ref"]
        read_matches = (reference is None) == unread or "rule.62.null" in skip
        listed = reference is None or reference in draft["pii_refs"] or "rule.62.listed" in skip
        rule("62", read_matches and listed, "payload.account_pii_ref")
    if event_type == "ConnectionStateChanged":
        reason, to, frm = p["reason"], p["to"], p["from"]
        if reason in DEGRADING:
            fits = to == "degraded"
        elif reason in SUSPENDING:
            fits = to == "suspended"
        elif reason == "condition_cleared":
            fits = to == frm and to != "active"
        else:
            fits = to == "active"
        rule("59", fits, "payload.reason")
        if fits:
            allowed = {"active": ("degraded", "suspended"), "degraded": ("active", "degraded")}
            rule("60", frm in allowed.get(to, CONNECTION_STATES), "payload.from")
    if event_type == "ConnectionEstablished" and draft["schema_version"] == 2:
        attested = p["margin_attestation"] is not None
        rule("64", attested == (p["environment"] == "live"), "payload.margin_attestation")
    return out


def answer_violations(event_type: str, draft: dict, skip: frozenset[str]) -> list[Violation]:
    """§9.7's rules 46 to 49 and 51 to 53, on a well-typed payload (rule 50 is a copy rule)."""
    p = draft["payload"]
    out: list[Violation] = []

    def rule(name: str, holds: bool, path: str) -> None:
        if not holds and f"rule.{name}" not in skip:
            out.append(Violation(f"rule.{name}", "schema", path))

    if event_type == "ApprovalResponseSubmitted":
        rule("46", p["responder"] == draft["actor"]["id"], "payload.responder")
    if event_type == "ApprovalResponded":
        refused = p["result"] == "refused"
        rule("47", (p["reason"] is not None) == refused, "payload.reason")
        judged = p["verdict"] == "approved" and (
            p["result"] in ("admitted", "counted") or p["reason"] in QUORUM_REASONS
        )
        rule("48", (p["quorum"] is not None) == judged, "payload.quorum")
        if p["verdict"] == "skipped":
            rule("49.counted", p["result"] != "counted", "payload.result")
            if p["reason"] is not None:
                rule("49.reason", p["reason"] in SKIP_REASONS, "payload.reason")
    if event_type == "ApprovalRevalidated":
        rule("51", (p["reason"] is not None) == (p["result"] == "skip"), "payload.reason")
        rule("52", (p["dry_run_reason"] is not None) == (p["dry_run"] == "deny"), "payload.dry_run_reason")
        band = p["band_bp"] in DRIFT_BANDS
        rule("53.band", band, "payload.band_bp")
        if p["result"] == "act" and band:
            failed = act_failure(p, skip)
            if failed:
                rule(f"53.{failed}", False, f"payload.{failed}")
    return out


def workspace_violations(event_type: str, draft: dict, skip: frozenset[str]) -> list[Violation]:
    """§9.9's rules 69 to 80, on a well-typed payload, in rule order. A seeded `loose` bug can let a
    member of the wrong kind through; the rules that read it then stop, as the journal's own reader
    would refuse it before them."""
    out: list[Violation] = []
    try:
        workspace_rules(event_type, draft, skip, out)
    except (TypeError, ValueError, ArithmeticError, AttributeError, KeyError):
        pass
    return out


def workspace_rules(event_type: str, draft: dict, skip: frozenset[str], out: list[Violation]) -> None:
    p = draft["payload"]
    actor = draft["actor"]

    def rule(name: str, holds: bool, path: str, reason: str = "schema") -> bool:
        if not holds and f"rule.{name}" not in skip:
            out.append(Violation(f"rule.{name}", reason, path))
            return False
        return True

    if event_type == "MandateDraftSaved":
        new = p["origin"] in NEW_DRAFT_ORIGINS
        (
            rule("69.base_draft", (p["base_draft"] is None) == new, "payload.base_draft")
            and rule("69.base_version", (p["base_version"] is not None) == (p["origin"] == "version"), "payload.base_version")
            and rule("69.compile", p["origin"] != "compile" or draft["causation_id"] is not None, "causation_id")
        )
        client = actor["kind"] == "client" and (p["origin"] == "version" or "rule.70.client_origin" in skip)
        person = actor["kind"] != "system" if "rule.70.not_system_only" in skip else actor["kind"] == "user"
        rule("70", person or client, "actor.kind")
    if event_type == "ModelInvocationRecorded":
        bound = draft["config_refs"].get("model_version")
        if bound is not None:
            rule("71", p["model"]["content_hash"] == bound, "payload.model.content_hash")
        ok = p["outcome"] == "ok"
        if ok:
            missing = next((m for m in ("response_ref", "reported_identity") if p[m] is None), None)
            rule("72", missing is None, f"payload.{missing}")
        if p["outcome"] in CALL_REFUSALS:
            sent = {
                "response_ref": p["response_ref"] is not None,
                "reported_identity": p["reported_identity"] is not None,
                "provider_request_id": p["provider_request_id"] is not None,
                "attempts": p["attempts"] != 0,
                "tokens": any(v != 0 for v in p["tokens"].values()) and "rule.73.tokens" not in skip,
                "cost_usd": Decimal(p["cost_usd"]) != 0,
            }
            first = next((m for m in SENT_NOTHING if sent[m]), None)
            rule("73", first is None, f"payload.{first}")
        if p["cache_hit"]:
            if rule("74.hit", ok, "payload.cache_hit"):
                spent = {
                    "attempts": p["attempts"] != 0,
                    "provider_request_id": p["provider_request_id"] is not None,
                    "cost_usd": Decimal(p["cost_usd"]) != 0,
                }
                first = next((m for m in spent if spent[m]), None)
                rule("74.free", first is None, f"payload.{first}")
        elif p["outcome"] not in CALL_REFUSALS:
            floor = 2 if "boundary.rule_74_attempts" in skip else 1
            rule("74.attempts", p["attempts"] >= floor, "payload.attempts")
        rule("75.cost", Decimal(p["cost_usd"]) >= 0, "payload.cost_usd")
        if ok:
            late = instant_nanos(p["completed_at"]) > instant_nanos(p["deadline"])
            if "boundary.rule_75_strict" in skip:
                late = instant_nanos(p["completed_at"]) >= instant_nanos(p["deadline"])
            rule("75.late", not late, "payload.completed_at")
        services = actor["kind"] != "user" if "rule.76.not_user_only" in skip else actor["kind"] == "system"
        rule("76", services, "actor.kind")
    if event_type == "MandateConfirmed" and draft["schema_version"] == 2:
        (
            rule("77.paired", (p["base_version"] is None) == (p["agent_id"] is None), "payload.base_version")
            and rule("77.moved", p["base_version"] != p["mandate_version"], "payload.base_version")
        )
        confirmer = actor["kind"] != "system" if "rule.78.not_system_only" in skip else actor["kind"] == "user"
        rule("78", confirmer, "actor.kind")
    if event_type == "OwnerRequestSubmitted":
        expected = {"user": "owner", "client": "client"}.get(actor["kind"])
        if "rule.79.not_system_only" in skip and actor["kind"] != "system":
            expected = expected or "owner"
        if rule("79.actor", expected is not None, "actor.kind") and rule(
            "79.requested_by", p["requested_by"] == expected, "payload.requested_by"
        ):
            client_id = actor["id"] if expected == "client" else None
            rule("79.client_id", p["client_id"] == client_id, "payload.client_id")
        if p["quantity"] is not None:
            positive = Decimal(p["quantity"]) >= 0 if "boundary.rule_80_zero" in skip else Decimal(p["quantity"]) > 0
            rule("80", positive, "payload.quantity")
    if event_type == "ConnectionRevoked" and draft["schema_version"] == 2:
        compromised = p["reason"] == "compromised"
        rule("84", (draft["causation_id"] is not None) == compromised, "causation_id")
        rule("85", actor["kind"] == "user", "actor.kind")
    if event_type == "ClientConnected":
        rule("86", bool(p["scopes"]) and bool(p["agents"]), "payload.scopes" if not p["scopes"] else "payload.agents")
        rule("87.scopes", ascending(encoded(p["scopes"])), "payload.scopes", "non_canonical")
        rule("87.agents", ascending(encoded(p["agents"])), "payload.agents", "non_canonical")
        if rule("88.actor", actor["kind"] == "user", "actor.kind"):
            rule("88.user", p["user"] == human(actor), "payload.user")
    if event_type == "ClientRevoked":
        allowed = REVOCATION_ACTORS[p["reason"]]
        if "rule.89.user_any" in skip:
            allowed = (*allowed, "user")
        if rule("89", actor["kind"] in allowed, "actor.kind"):
            if p["reason"] == "owner":
                rule("89.owner", p["user"] == actor["id"], "payload.user")
            if p["reason"] == "admin":
                rule("89.admin", p["user"] != actor["id"], "payload.user")
    if event_type == "OwnerCommandIssued":
        hold = p["command"] == "hold_openings"
        allowed = ("user", "client") if hold else ("user",)
        if "rule.90.client_lifts" in skip:
            allowed = ("user", "client")
        if rule("90", actor["kind"] in allowed, "actor.kind"):
            rule("91", p["user"] == human(actor), "payload.user")
        rule("91.subject", p["subject"] == p["agent"], "payload.subject")
        if hold:
            rule("92", p["step_up"] is None, "payload.step_up")
    if event_type == "AgentModeChanged":
        reason = p["reason"]
        held = p.get("held") is True
        if reason in ("owner_hold", "owner_lift_hold"):
            rule("93", held == (reason == "owner_hold"), "payload.held")
        floor = max(MODE_ORDER.index(p["lifecycle"]), 1 if held and "rule.94.held" not in skip else 0)
        rule("94", MODE_ORDER.index(p["to"]) >= floor, "payload.to")


def membership_violations(event_type: str, draft: dict, skip: frozenset[str]) -> list[Violation]:
    """§9.12's rules 96 to 106, on a well-typed payload, in number order and each rule's clauses in
    the order the spec gives them."""
    if event_type not in WRITERS and event_type != "MemberActivated":
        return []
    p = draft["payload"]
    actor = draft["actor"]
    reason = p.get("reason")
    out: list[Violation] = []

    def rule(name: str, holds: bool, path: str, why: str = "schema") -> None:
        if not holds and f"rule.{name}" not in skip:
            out.append(Violation(f"rule.{name}", why, path))

    for member in ("roles", "removed"):
        if member in p:
            rule(f"96.{member}", ascending(encoded(listed(p[member]))), f"payload.{member}", "non_canonical")
    if "added" in p:
        added_roles = encoded([a.get("role") for a in record_items(p["added"])])
        rule("96.added", ascending(added_roles), "payload.added", "non_canonical")
    writer = WRITERS.get(event_type)
    if writer:
        rule("97", p[writer] == actor["id"], f"payload.{writer}")
    system = reason in SYSTEM_REASONS
    rule("98", actor["kind"] == ("system" if system else "user"), "actor.kind")
    if event_type == "MemberRoleChanged":
        rule("99.self", p["changed_by"] != p["member"], "payload.changed_by")
    if event_type == "MemberReactivated" or (
        event_type in ("MemberDeactivated", "MemberRemoved") and reason in SELF_REASONS
    ):
        rule("99.self", p["by"] != p["member"], "payload.by")
    if event_type == "MemberDeactivated" and reason == "left":
        rule("99.left", p["by"] == p["member"], "payload.by")
    if event_type == "MemberActivated":
        rule("99.invitation", (p["invitation"] is None) == (reason == "founding"), "payload.invitation")
        if reason == "invitation_accepted":
            rule("99.invitee", p["member"] == actor["id"], "payload.member")
    if event_type in ("MemberInvited", "MemberActivated", "MemberReactivated"):
        rule("100.empty", bool(listed(p["roles"])), "payload.roles")
    if event_type == "MemberActivated" and reason == "founding" and listed(p["roles"]):
        rule("100.founding_admin", "workspace_admin" in listed(p["roles"]), "payload.roles")
    if event_type == "MemberRoleChanged":
        rule("100.no_change", bool(listed(p["added"])) or bool(listed(p["removed"])), "payload.added")
        added = {a.get("role") for a in record_items(p["added"])}
        removed = {r for r in listed(p["removed"]) if isinstance(r, str)}
        rule("100.disjoint", not added & removed, "payload.removed")
        rule("101", (p["step_up"] is not None) == bool(listed(p["added"])), "payload.step_up")
    step_up = p.get("step_up")
    if isinstance(step_up, dict):
        allowed = ("passkey",) if draft["environment"] == "live" else ("passkey", "cli_confirm")
        rule("102.method", step_up.get("method") in allowed, "payload.step_up.method")
        at = draft["event_time"]
        rule("102.window", step_up_fresh(step_up.get("authenticated_at"), at, skip), "payload.step_up.authenticated_at")
    for end, cooling, path in cool_offs(event_type, p, skip):
        rule(f"103.{event_type}", cool_off_exact(draft["event_time"], end, cooling, skip), path)
    if event_type == "MemberInvited":
        days = gap_nanos(p["invited_at"], p["expires_at"])
        week = INVITATION_DAYS * 86400 * 10**9
        lasts = days is not None and days >= week if "boundary.rule_104_at_least" in skip else days == week
        rule("104", lasts, "payload.expires_at")
    rule("105", (p["session_ref"] is not None) == (actor["kind"] == "user"), "payload.session_ref")
    instant = OWN_INSTANT.get(event_type)
    if instant:
        rule("106", p[instant] == draft["event_time"], f"payload.{instant}")
    return out


def gap_nanos(start, end) -> int | None:
    """`end` − `start` in nanoseconds, or `None` for an instant a seeded type bug let through."""
    if not all(isinstance(v, str) and is_timestamp(v) for v in (start, end)):
        return None
    return instant_nanos(end) - instant_nanos(start)


def step_up_fresh(authenticated_at, at, skip: frozenset[str]) -> bool:
    """Mandate spec §6.1: valid at `at` when 0 ≤ `at` − `authenticated_at` ≤ 300 s."""
    gap = gap_nanos(authenticated_at, at)
    if gap is None:
        return False
    if "boundary.rule_102_after" in skip:
        gap = abs(gap)
    limit = STEP_UP_WINDOW_SECONDS * 10**9 + (1 if "boundary.rule_102_window" in skip else 0)
    return 0 <= gap <= limit


def listed(value) -> list:
    """A list as rules read it: anything else, which only a seeded type bug lets through, is empty."""
    return value if isinstance(value, list) else []


def record_items(value) -> list[dict]:
    """A list of records as rules read it, so a seeded `loose` bug that lets another kind through
    leaves the rule nothing to check rather than failing."""
    return [v for v in listed(value) if isinstance(v, dict)]


END = ".cool_off_ends_at"


def cool_offs(event_type: str, p: dict, skip: frozenset[str]) -> list[tuple[str, bool, str]]:
    """Each cool-off a record states: its end, whether identity spec §8.3's 24 hours apply to it, and
    its path (rule 103). They apply exactly when independence is required and the grant adds operator
    or approver to an existing workspace."""
    independent = p.get("independent_approval_required") is True or "boundary.rule_103_ignores_independence" in skip
    roles = COOLING_ROLES + (("workspace_admin",) if "boundary.rule_103_admin_cools" in skip else ())
    if event_type == "MemberActivated":
        cooling = independent and p["reason"] != "founding" and any(r in roles for r in listed(p["roles"]))
        if "boundary.rule_103_founding_cools" in skip and p["reason"] == "founding":
            cooling = independent and any(r in roles for r in listed(p["roles"]))
        return [(p["cool_off_ends_at"], cooling, "payload.cool_off_ends_at")]
    if event_type == "MemberReactivated":
        cooling = independent and any(r in roles for r in listed(p["roles"]))
        return [(p["cool_off_ends_at"], cooling, "payload.cool_off_ends_at")]
    if event_type == "MemberRoleChanged":
        return [
            (a.get(END[1:]), independent and a.get("role") in roles, f"payload.added[{i}]{END}")
            for i, a in enumerate(record_items(p["added"]))
        ]
    return []


def cool_off_exact(start: str, end, cooling: bool, skip: frozenset[str]) -> bool:
    """Rule 103: the end is exactly a day after `start` when the 24 hours apply, and `start` itself
    otherwise."""
    gap = gap_nanos(start, end)
    if gap is None:
        return False
    if "boundary.rule_103_either" in skip:
        return gap in (0, COOL_OFF_SECONDS * 10**9)
    if "boundary.rule_103_at_least" in skip and cooling:
        return gap >= COOL_OFF_SECONDS * 10**9
    return gap == (COOL_OFF_SECONDS * 10**9 if cooling else 0)


def audit_violations(event_type: str, draft: dict, skip: frozenset[str]) -> list[Violation]:
    """§9.13's rules 107 to 112, on a well-typed payload, in number order. A seeded `loose` or
    `nullable` bug can let a mistyped member through; a clause whose inputs are mistyped is not
    judged, so the bug shows as the type violation it hid rather than as a rule at the same path."""
    if event_type not in ("RecordsAccessed", "ExportCreated", "VerificationRun"):
        return []
    p = draft["payload"]
    kind = draft["actor"]["kind"]
    out: list[Violation] = []

    def rule(name: str, holds: bool, path: str) -> None:
        if not holds and f"rule.{name}" not in skip:
            out.append(Violation(f"rule.{name}", "schema", path))

    ranges = p["ranges"] if isinstance(p["ranges"], list) else None
    items = [r if isinstance(r, dict) else {} for r in ranges or []]
    rule("107.empty", ranges is None or bool(ranges), "payload.ranges")
    workspace = draft["stream_id"].split(":")[1]
    previous = None
    for i, r in enumerate(items):
        at = f"payload.ranges[{i}]"
        stream, first, last, prev = r.get("stream_id"), r.get("from_seq"), r.get("to_seq"), r.get("prev_hash")
        named = isinstance(stream, str)
        rule("107.workspace", not named or stream.split(":")[1:2] == [workspace], f"{at}.stream_id")
        least = 0 if "boundary.rule_107_from_seq" in skip else 1
        rule("107.from_seq", not is_integer(first) or first >= least, f"{at}.from_seq")
        seqs = is_integer(first) and is_integer(last)
        rule("107.to_seq", not seqs or last >= first, f"{at}.to_seq")
        typed = isinstance(prev, str) and is_integer(first)
        rule("107.genesis", not typed or (prev == GENESIS) == (first == 1), f"{at}.prev_hash")
        if previous is not None and named and isinstance(previous.get("stream_id"), str):
            here, before = stream.encode(), previous["stream_id"].encode()
            gap = 0 if "boundary.rule_107_overlap" in skip else 1
            ends = is_integer(first) and is_integer(previous.get("to_seq"))
            disjoint = not ends or first >= previous["to_seq"] + gap
            rule("107.order", here > before or (here == before and disjoint), f"{at}.stream_id")
        previous = r
    if event_type == "RecordsAccessed":
        accessor = p["accessor"]
        rule("108.accessor", not isinstance(accessor, str) or accessor == draft["actor"]["id"], "payload.accessor")
        refused = tuple(k for k in ("agent", "broker") if f"kinds.108.{k}" not in skip)
        rule("108.actor", kind not in refused, "actor.kind")
        approved = kind != "platform_operator" or draft["causation_id"] is not None
        rule("108.break_glass", approved, "causation_id")
        resources = p["resources"] if isinstance(p["resources"], list) else []
        named = [r.encode() for r in resources if isinstance(r, str)]
        rule("108.resources", all(a < b for a, b in zip(named, named[1:])), "payload.resources")
    if event_type == "ExportCreated":
        widened = tuple(k for k in ("broker", "platform_operator") if f"kinds.109.{k}" in skip)
        rule("109.actor", kind in ("user", "system", *widened), "actor.kind")
        if p["form"] in ("canonical", *VIEW_FORMS) and (p["view"] is None or isinstance(p["view"], str)):
            rule("109.view", (p["view"] is not None) == (p["form"] in VIEW_FORMS), "payload.view")
    if event_type == "VerificationRun":
        if p["trigger"] in TRIGGERS:
            allowed = ("user", "system") if p["trigger"] == "request" else ("system",)
            widened = tuple(k for k in ("broker", "platform_operator") if f"kinds.110.{k}" in skip)
            rule("110", kind in (*allowed, *widened), "actor.kind")
        for i, r in enumerate(items):
            at = f"payload.ranges[{i}]"
            failure = r.get("failure")
            if isinstance(failure, dict):
                seq, check = failure.get("seq"), failure.get("check")
                if check in EVENT_CHECKS or check in RANGE_CHECKS:
                    at_event = check in EVENT_CHECKS and f"classify.{check}" not in skip
                    rule("111.seq", (seq is not None) == at_event, f"{at}.failure.seq")
                bounds = (r.get("from_seq"), seq, r.get("to_seq"))
                if all(is_integer(b) for b in bounds):
                    rule("111.inside", bounds[0] <= bounds[1] <= bounds[2], f"{at}.failure.seq")
            rule("111.to_hash", failure is not None or r.get("to_hash") is not None, f"{at}.to_hash")
        if draft["schema_version"] == 1 and p["result"] in ("pass", "fail"):
            passed = all(r.get("failure") is None for r in items)
            rule("112", (p["result"] == "pass") == passed, "payload.result")
        if draft["schema_version"] == 2:
            out += run_v2_violations(p, items, rule, skip)
    return out


def run_v2_violations(p: dict, items: list[dict], rule, skip: frozenset[str]) -> list[Violation]:
    """Rules 132 and 133 (v0.36, DEC-788 item 1, DEC-789): `VerificationRun` version 2's start,
    count, and incomplete check per range, then its three-way result. `rule` appends to the caller's
    list; nothing is returned of its own."""
    for i, r in enumerate(items):
        at = f"payload.ranges[{i}]"
        start = r.get("start") if isinstance(r.get("start"), dict) else {}
        kind = start.get("kind")
        if kind in START_KINDS:
            rule("132.manifest", (start.get("manifest_hash") is not None) == (kind == "manifest"), f"{at}.start.manifest_hash")
            rule("132.anchor", (start.get("anchor_event_id") is not None) == (kind == "anchor"), f"{at}.start.anchor_event_id")
            first = r.get("from_seq")
            if is_integer(first):
                least = 1 if "boundary.rule_132_anchor_seq" in skip else 2
                fits = (kind != "genesis" or first == 1) and (kind != "anchor" or first >= least)
                rule("132.start_seq", fits, f"{at}.start.kind")
        first, last, checked = r.get("from_seq"), r.get("to_seq"), r.get("checked")
        if all(is_integer(v) for v in (first, last, checked)):
            span = last - first + 1
            ceiling = span + 1 if "boundary.rule_132_checked_ceiling" in skip else span
            rule("132.checked", checked <= ceiling, f"{at}.checked")
            rule("132.walked", r.get("failure") is not None or checked == span, f"{at}.checked")
        rule("132.one_outcome", r.get("failure") is None or r.get("incomplete") is None, f"{at}.incomplete")
        unproven = kind != "anchor" or r.get("failure") is not None or r.get("incomplete") is not None
        rule("132.anchor_unproven", unproven, f"{at}.incomplete")
    if p["result"] in ("pass", "incomplete", "fail"):
        failed = any(r.get("failure") is not None for r in items)
        unfinished = any(r.get("incomplete") is not None for r in items)
        if "rule.133.incomplete_as_pass" in skip:
            unfinished = False
        want = "fail" if failed else "incomplete" if unfinished else "pass"
        rule("133", p["result"] == want, "payload.result")
    return []


def merkle_root(leaves: list[dict], skip: frozenset[str] = frozenset()) -> str:
    """§10's root over `leaves`, as 64 hex: leaf = SHA-256(0x00 ‖ canonical(leaf)), node =
    SHA-256(0x01 ‖ left ‖ right), split at the largest power of two below the count."""
    tag = b"\x01" if "boundary.leaf_prefix" in skip else b"\x00"
    hashes = [hashlib.sha256(tag + canon(leaf).encode()).digest() for leaf in leaves]

    def node(items: list[bytes]) -> bytes:
        if len(items) == 1:
            return items[0]
        split = 1
        while split * 2 < len(items):
            split *= 2
        if "boundary.merkle_split" in skip:
            split = len(items) // 2
        return hashlib.sha256(b"\x01" + node(items[:split]) + node(items[split:])).digest()

    return node(hashes).hex()


def manifest_hash(p: dict) -> str:
    """DEC-263 item 3: SHA-256 of the canonical JSON of the six manifest fields, `stream` named so."""
    manifest = {
        "stream": p["stream_id"],
        "first_seq": p["first_seq"],
        "last_seq": p["last_seq"],
        "first_prev_hash": p["first_prev_hash"],
        "last_hash": p["last_hash"],
        "file_sha256": p["file_sha256"],
    }
    return hashlib.sha256(canon(manifest).encode()).hexdigest()


def cold_violations(event_type: str, draft: dict, skip: frozenset[str]) -> list[Violation]:
    """§9.14's rules 113 to 118, on a well-typed payload, in number order. A clause whose inputs a
    seeded `loose` or `nullable` bug let through mistyped is not judged, so the bug shows."""
    if event_type not in ("AnchorComputed", "SegmentExported"):
        return []
    p = draft["payload"]
    kind = draft["actor"]["kind"]
    workspace = draft["stream_id"].split(":")[1]
    out: list[Violation] = []

    def rule(name: str, holds: bool, path: str) -> None:
        if not holds and f"rule.{name}" not in skip:
            out.append(Violation(f"rule.{name}", "schema", path))

    def mine(stream) -> bool:
        return not isinstance(stream, str) or stream.split(":")[1:2] == [workspace]

    if event_type == "AnchorComputed":
        leaves = p["leaves"] if isinstance(p["leaves"], list) else None
        items = [leaf if isinstance(leaf, dict) else {} for leaf in leaves or []]
        rule("113.empty", leaves is None or bool(leaves), "payload.leaves")
        previous = None
        for i, leaf in enumerate(items):
            at = f"payload.leaves[{i}]"
            stream, seq = leaf.get("stream_id"), leaf.get("seq")
            rule("113.workspace", mine(stream), f"{at}.stream_id")
            rule("113.seq", not is_integer(seq) or seq >= 1, f"{at}.seq")
            if previous is not None and isinstance(stream, str) and isinstance(previous, str):
                rule("113.order", stream.encode() > previous.encode(), f"{at}.stream_id")
            previous = stream
        streams = [leaf.get("stream_id") for leaf in items]
        rule("113.self", not leaves or draft["stream_id"] in streams, "payload.leaves")
        typed = items and all(
            isinstance(leaf.get("hash"), str) and is_integer(leaf.get("seq")) and isinstance(leaf.get("stream_id"), str)
            for leaf in items
        )
        if typed and isinstance(p["root"], str):
            rule("114", p["root"] == merkle_root(items, skip), "payload.root")
        rule("115", kind == "system", "actor.kind")
    if event_type == "SegmentExported":
        first, last = p["first_seq"], p["last_seq"]
        rule("116.workspace", mine(p["stream_id"]), "payload.stream_id")
        least = 0 if "boundary.rule_116_first_seq" in skip else 1
        rule("116.first_seq", not is_integer(first) or first >= least, "payload.first_seq")
        rule("116.last_seq", not (is_integer(first) and is_integer(last)) or last >= first, "payload.last_seq")
        if isinstance(p["first_prev_hash"], str) and is_integer(first):
            genesis = (p["first_prev_hash"] == GENESIS) == (first == 1)
            rule("116.genesis", genesis, "payload.first_prev_hash")
        members = ("stream_id", "first_seq", "last_seq", "first_prev_hash", "last_hash", "file_sha256")
        typed = all(p[m] is not None for m in members) and is_integer(first) and is_integer(last)
        if typed and isinstance(p["manifest_hash"], str):
            rule("117", p["manifest_hash"] == manifest_hash(p), "payload.manifest_hash")
        rule("118", kind == "system", "actor.kind")
    return out


def anchor_self_failure(entries: list[dict], skip: frozenset[str] = frozenset()) -> dict | None:
    """§11's `anchor_self_mismatch` over one control stream's entries in `seq` order: each
    `AnchorComputed` has a leaf for its own stream naming the event just before it, by `seq` and by
    `hash`. Returns the first failure, `{seq, check}`, or `None`."""
    for i, entry in enumerate(entries):
        body = entry["body"]
        if body["event_type"] != "AnchorComputed" or i == 0:
            continue
        before = entries[i - 1]
        own = [leaf for leaf in body["payload"]["leaves"] if leaf["stream_id"] == body["stream_id"]]
        if not own:
            if "self.missing" in skip:
                continue
            return {"seq": entry["seq"], "check": "anchor_self_mismatch"}
        leaf = own[0]
        seq_ok = leaf["seq"] == before["seq"] or "self.seq" in skip
        hash_ok = leaf["hash"] == before["hash"] or "self.hash" in skip
        if not (seq_ok and hash_ok):
            return {"seq": entry["seq"], "check": "anchor_self_mismatch"}
    return None


def break_glass_failure(entries: list[dict], skip: frozenset[str] = frozenset()) -> dict | None:
    """§11's `break_glass_cause_mismatch` over one control-stream range's entries in `seq` order: a
    `platform_operator`'s `RecordsAccessed` cites, by `causation_id`, an earlier
    `PlatformOperatorAction` on this stream. A cause named by no entry is judged only when the range
    starts at seq 1, since it may lie before a later trusted start. Returns the first failure,
    `{seq, check}`, or `None`."""
    if not entries:
        return None
    from_seq = entries[0]["seq"]
    position = {entry["body"]["event_id"]: entry for entry in entries}
    for entry in entries:
        body = entry["body"]
        operator = body["actor"]["kind"] == "platform_operator" or "cause.actor" in skip
        if body["event_type"] != "RecordsAccessed" or not operator:
            continue
        named = position.get(body["causation_id"])
        if named is None:
            judged = from_seq == 1 or "cause.tail" in skip
            if judged and "cause.unresolved" not in skip:
                return {"seq": entry["seq"], "check": "break_glass_cause_mismatch"}
            continue
        typed = named["body"]["event_type"] == "PlatformOperatorAction" or "cause.type" in skip
        earlier = named["seq"] < entry["seq"] or "cause.earlier" in skip
        if not (typed and earlier):
            return {"seq": entry["seq"], "check": "break_glass_cause_mismatch"}
    return None


def trusted_start(
    records: list[dict], stream: str, from_seq: int, request: dict, skip: frozenset[str] = frozenset()
) -> dict | None:
    """§9.14's "What a verifier reads": the trusted start of a range of `stream` entered at
    `from_seq`, resolved from `request` among the control-stream `records` of `stream`'s own
    workspace, or `None` when the request names no usable start."""
    workspace = stream.split(":")[1]
    own = [
        r
        for r in records
        if r["stream_id"].split(":")[1] == workspace or "start.workspace" in skip
    ]
    if request["kind"] == "genesis":
        if from_seq == 1 or "start.genesis_seq" in skip:
            return {"from_seq": from_seq, "prev_hash": GENESIS}
        return None
    if request["kind"] == "manifest":
        for r in own:
            p = r["payload"]
            if r["event_type"] != "SegmentExported" or p["manifest_hash"] != request["manifest_hash"]:
                continue
            fits = p["stream_id"] == stream and (p["first_seq"] == from_seq or "start.first_seq" in skip)
            if fits:
                return {"from_seq": from_seq, "prev_hash": p["first_prev_hash"]}
        return None
    for r in own:
        if r["event_type"] != "AnchorComputed" or r["event_id"] != request["anchor_event_id"]:
            continue
        if r["payload"]["token"] is None and "start.null_token" not in skip:
            return None
        for leaf in r["payload"]["leaves"]:
            fits = leaf["seq"] == from_seq - 1 or "start.anchor_seq" in skip
            if leaf["stream_id"] == stream and fits:
                return {"from_seq": from_seq, "prev_hash": leaf["hash"]}
    return None


def act_failure(p: dict, skip: frozenset[str]) -> str | None:
    """Rule 53: the first member of an `act` that shows a check failed, in the rule's order."""
    marks = p["m_req"] is not None and p["m_now"] is not None
    inside = False
    if marks:
        m_req, m_now = Decimal(p["m_req"]), Decimal(p["m_now"])
        drift = abs(m_now - m_req) * 10000
        bound = p["band_bp"] * m_req
        inside = drift < bound if "boundary.rule_53_strict" in skip else drift <= bound
    checks = (
        ("mandate_version_now", p["mandate_version_now"] == p["mandate_version_bound"]),
        ("mode", p["mode"] == "normal"),
        ("instrument_restricted", p["instrument_restricted"] is False),
        ("decided_by_now", p["decided_by_now"] in (None, p["decided_by_bound"])),
        ("dry_run", p["dry_run"] == "allow"),
        ("m_req", p["m_req"] is not None),
        ("m_now", p["m_now"] is not None),
    )
    for member, holds in checks:
        if not holds and f"rule.53.{member}" not in skip:
            return member
    if marks and not inside and "rule.53.m_now_drift" not in skip:
        return "m_now"
    return None


def instant_nanos(text: str) -> int:
    """A timestamp as nanoseconds since the epoch, so §8.2's horizon is compared exactly."""
    instant, nanos = parse_instant(text)
    return calendar.timegm(instant.timetuple()) * 10**9 + nanos


def is_integer(value) -> bool:
    """An integer the type check passed, which a seeded `loose` bug may not have."""
    return isinstance(value, int) and not isinstance(value, bool)


# §9.4's report order, by group: rule 34, its autopsy clause, rules 35 to 37 (all at
# `payload.reason`), then rule 38. Each `order.<a>.<b>` seeded bug swaps two groups, so a fixture that
# breaks both shows which is reported first.
THESIS_ORDER = ("revision", "autopsy", "reason", "model")
THESIS_ORDER_BUGS = tuple(
    f"order.{a}.{b}" for i, a in enumerate(THESIS_ORDER) for b in THESIS_ORDER[i + 1 :]
)


def thesis_violations(event_type: str, draft: dict, skip: frozenset[str]) -> list[Violation]:
    """§9.4's rules 34 to 38 on a well-typed thesis record, each reported once, in number order.
    The guards on each member's type matter only under a seeded `loose` bug, which lets an
    ill-typed member reach the rules."""
    p = draft["payload"]
    groups: dict[str, list[Violation]] = {group: [] for group in THESIS_ORDER}

    def rule(group: str, name: str, holds: bool, reason: str, path: str) -> None:
        if not holds and f"rule.{name}" not in skip:
            groups[group].append(Violation(f"rule.{name}", reason, path))

    first_thesis = event_type == "ThesisProposed"
    revised = is_integer(p["revision"]) and p["revision"] > 0
    rule("revision", "34", first_thesis != revised, "schema", "payload.revision")
    rule("autopsy", "34.autopsy", not first_thesis or p["autopsy_ref"] is None, "schema", "payload.autopsy_ref")
    rule("reason", "35", (p["reason"] is None) == (p["admitted"] is True), "schema", "payload.reason")
    failing = []
    if p["direction"] != "long" and "rule.36.direction" not in skip:
        failing.append("direction_not_allowed")
    instants = all(isinstance(p[m], str) and is_timestamp(p[m]) for m in ("as_of", "expires_at"))
    if instants and is_integer(p["horizon_s"]):
        horizon_end = instant_nanos(p["as_of"]) + p["horizon_s"] * 10**9
        if instant_nanos(p["expires_at"]) != horizon_end and "rule.36.horizon" not in skip:
            failing.append("horizon_mismatch")
    predecessor = p["predecessor_thesis_id"] is not None
    if predecessor != revised and "rule.36.predecessor" not in skip:
        failing.append("revision_without_predecessor")
    if failing:
        decides = failing[-1] if "rule.36.order" in skip else failing[0]
        rule("reason", "36", p["reason"] == decides, "schema", "payload.reason")
    else:
        rule("reason", "36.unfailed", p["reason"] not in THESIS_REFUSALS[:3], "schema", "payload.reason")
    if p["corroboration"] is None:
        number = CHECK_NUMBER.get(p["reason"]) if isinstance(p["reason"], str) else None
        rule("reason", "37", number is not None and number <= CORROBORATION_CHECK, "schema", "payload.reason")
    else:
        rule("reason", "37.corroborated", p["reason"] != "no_corroboration", "schema", "payload.reason")
    sources = p["evidence_sources"] if isinstance(p["evidence_sources"], list) else []
    if "tighten.sorted_sources" in skip and not ascending(encoded(sources)):
        groups["reason"].append(Violation("tighten.sorted_sources", "non_canonical", "payload.evidence_sources"))
    model_ref = draft["config_refs"].get("model_version")
    rule("model", "38", model_ref is None or model_ref == p["content_hash"], "schema", "payload.content_hash")
    order = list(THESIS_ORDER)
    for bug in THESIS_ORDER_BUGS:
        if bug in skip:
            _, first, second = bug.split(".")
            i, j = order.index(first), order.index(second)
            order[i], order[j] = order[j], order[i]
    return [v for group in order for v in groups[group]]


def subject_violations(event_type: str, draft: dict, skip: frozenset[str]) -> list[Violation]:
    """Rules 25, 26 and 28 (reason `stream_mismatch`), in that order: rule 28 is reported after rule 26, not
    suppressed by it, so a draft breaking both names both."""
    p = draft["payload"]
    kind = draft["stream_id"].split(":")[0]
    out = []
    if event_type == "StreamOpened" and "rule.25" not in skip:
        if kind == "acct":
            opened = f"acct:{p['workspace_id']}:{p['account_ref']}"
        else:
            opened = f"ctl:{p['workspace_id']}"
        if draft["stream_id"] != opened:
            out.append(Violation("rule.25", "stream_mismatch", "stream_id"))
    refused = event_type == "OwnerCommandRefused" and p["command"] not in REFUSED_COMMANDS[kind]
    if refused and f"rule.26.{kind}" not in skip:
        out.append(Violation(f"rule.26.{kind}", "stream_mismatch", "payload.command"))
    independent = event_type == "OwnerCommandRefused" and p["reason"] == "not_independent" and kind != "acct"
    if independent and "rule.28" not in skip:
        out.append(Violation("rule.28", "stream_mismatch", "payload.reason"))
    return out


def copy_violations(event_type: str, draft: dict, skip: frozenset[str]) -> list[Violation]:
    """Rule 27: an `OwnerCommandRefused` names the owner input it refused, and §9.7's rule 50: an
    `ApprovalResponded` names the answer it copies (reason `schema`)."""
    kind = draft["stream_id"].split(":")[0]
    if event_type == "ApprovalResponded" and draft["causation_id"] is None and "rule.50" not in skip:
        return [Violation("rule.50", "schema", "causation_id")]
    checked_first = event_type == "ConnectionCredentialRotated" or (
        event_type == "ConnectionEstablished" and draft["schema_version"] == 2
    )
    copied = kind == "acct" and event_type in ("ConnectionEstablished", "ConnectionCredentialRotated")
    if copied and "rule.63.copy" in skip:
        checked_first = False
    if checked_first and draft["causation_id"] is None and "rule.63" not in skip:
        return [Violation("rule.63", "schema", "causation_id")]
    acknowledged = event_type == "ConnectionStateChanged" and draft["payload"]["reason"] == "acknowledged"
    if acknowledged and draft["causation_id"] is None and "rule.61" not in skip:
        return [Violation("rule.61", "schema", "causation_id")]
    if (
        event_type == "OwnerCommandRefused"
        and draft["causation_id"] is None
        and f"rule.27.{kind}" not in skip
    ):
        return [Violation(f"rule.27.{kind}", "schema", "causation_id")]
    copied = event_type == "AgentModeChanged" and draft["payload"]["reason"] in OWNER_MODE_REASONS
    if copied and draft["causation_id"] is None and "rule.95" not in skip:
        return [Violation("rule.95", "schema", "causation_id")]
    return []


def envelope_violations(draft: dict, skip: frozenset[str]) -> list[Violation]:
    """§3's envelope with the `client` actor (rules 81 and 82): `on_behalf_of` is a member of a
    `client` actor only, so no other actor's canonical form changes."""
    actor = draft.get("actor")
    named = isinstance(actor, dict) and "on_behalf_of" in actor
    seen = {**draft, "actor": {k: v for k, v in actor.items() if k != "on_behalf_of"}} if named else draft
    out = type_violations(CONTROL_ENVELOPE, seen, "", skip)
    if out:
        return out
    client = actor["kind"] == "client"
    if named and not client and "rule.81.extra" not in skip:
        return [Violation("rule.81.extra", "schema", "actor.on_behalf_of")]
    if client and not named and "rule.81.missing" not in skip:
        return [Violation("rule.81.missing", "schema", "actor.on_behalf_of")]
    if client and named:
        out = type_violations(IDENT_T, actor["on_behalf_of"], "actor.on_behalf_of", skip)
        if out and "loose.actor.on_behalf_of" not in skip:
            return out
    if client and actor["build"] is not None and "rule.82" not in skip:
        return [Violation("rule.82", "schema", "actor.build")]
    return []


def open_command_violations(draft: dict, skip: frozenset[str]) -> list[Violation]:
    """§9.11: an `OwnerCommandIssued` whose command is not a hold command stays open until M7
    closes it. Its `command` is read, and a client may issue none of them (rule 90); nothing else
    about it is refused here, so a kill switch from any principal is always recorded."""
    out = []
    if not isinstance(draft["payload"].get("command"), str):
        out.append(Violation("types", "schema", "payload.command"))
    elif draft["actor"]["kind"] == "client" and "rule.90.client_commands" not in skip:
        out.append(Violation("rule.90", "schema", "actor.kind"))
    if "artifact_refs" not in skip and draft["artifact_refs"] != sorted(digest_strings(draft["payload"])):
        out.append(Violation("artifact_refs", "artifact_refs", "artifact_refs"))
    return out


def human(actor: dict) -> str:
    """The person behind an actor (§3): a client's `on_behalf_of`, anyone else's `id`."""
    return actor["on_behalf_of"] if actor["kind"] == "client" else actor["id"]


def violations(draft: dict, skip: frozenset[str] = frozenset()) -> list[Violation]:
    """Every rule a §9.2 draft breaks, in the journal's check order (spec §9.1, §9.2)."""
    out = envelope_violations(draft, skip)
    if out:
        return out
    if draft["envelope_version"] != 1:
        return [Violation("envelope", "schema", "envelope_version")]
    event_type = draft["event_type"]
    stream = draft["stream_id"].split(":")
    kinds = {k for k, e in SCHEMAS if e == event_type}
    kinds |= {k for k, e, _ in VERSIONED_SCHEMAS if e == event_type}
    if not kinds:
        return [Violation("catalogue", "unknown_event_type", "event_type")]
    shape = {"acct": 3, "agent": 3, "ctl": 2}.get(stream[0])
    if shape != len(stream) or not all(IDENT_RE.match(s) for s in stream[1:]):
        return [Violation("stream", "non_canonical", "stream_id")]
    if stream[0] not in kinds:
        return [Violation("stream", "wrong_stream", "event_type")]
    if draft["actor"]["kind"] == "client" and "rule.83" not in skip:
        if stream[0] != "ctl" or event_type not in CLIENT_EVENTS:
            return [Violation("rule.83", "schema", "actor.kind")]
    if draft["actor"]["kind"] in ("system", "agent") and draft["actor"]["build"] is None:
        out.append(Violation("actor", "schema", "actor.build"))
    refs = draft["config_refs"]
    for kind in sorted(refs):
        if kind not in CONFIG_KINDS:
            out.append(Violation("config_refs", "schema", f"config_refs.{kind}"))
        elif not (isinstance(refs[kind], str) and DIGEST_REF.match(refs[kind])):
            out.append(Violation("config_refs", "non_canonical", f"config_refs.{kind}"))
    if "config_refs.required" not in skip:
        missing = [k for k in REQUIRED_REFS.get(event_type, ()) if k not in refs]
        out += [
            Violation("config_refs.required", "missing_config_ref", f"config_refs.{k}") for k in missing[:1]
        ]
    versions = VERSIONED_VERSIONS.get((stream[0], event_type), (1,))
    if draft["schema_version"] not in versions:
        return [*out, Violation("catalogue", "unknown_schema", "payload")]
    command = draft["payload"].get("command") if isinstance(draft["payload"], dict) else None
    if event_type == "OwnerCommandIssued" and command not in HOLD_COMMANDS and "closed.owner_commands" not in skip:
        return out + open_command_violations(draft, skip)
    if (stream[0], event_type) in SCHEMAS and draft["schema_version"] == 1:
        schema = SCHEMAS[(stream[0], event_type)]
    else:
        schema = VERSIONED_SCHEMAS[(stream[0], event_type, draft["schema_version"])]
    payload_types = payload_type_violations(schema, draft["payload"], "payload", skip)
    out += payload_types
    schema_names = [name for name, _ in schema.fields]
    as_read = {**draft, "payload": {name: draft["payload"].get(name) for name in schema_names}}
    if not payload_types:
        out += consistency_violations(event_type, as_read, skip)
    if "artifact_refs" not in skip and draft["artifact_refs"] != sorted(digest_strings(draft["payload"])):
        out.append(Violation("artifact_refs", "artifact_refs", "artifact_refs"))
    if not ascending(draft["pii_refs"]):
        out.append(Violation("pii_refs", "pii_refs", "pii_refs"))
    if not payload_types:
        out += subject_violations(event_type, as_read, skip)
        out += copy_violations(event_type, as_read, skip)
    return out


# --------------------------------------------------------------------------- the control stream

WORKSPACE = "ws_01J8Z2"
AGENT = "agent_a"
STREAM = f"ctl:{WORKSPACE}"
AGENT_STREAM = f"agent:{WORKSPACE}:{AGENT}"
OWNER = "user_owner_01"
SERVICES = {"kind": "system", "id": "control_services", "version": "0.1.0", "build": "sha256:" + "d" * 64}
USER = {"kind": "user", "id": OWNER, "version": "1", "build": None}
EXECUTOR = {"kind": "system", "id": "executor", "version": "0.1.0", "build": "sha256:" + "3" * 64}
RUNTIME = {"kind": "agent", "id": AGENT, "version": "0.1.0", "build": "sha256:" + "c" * 64}
CHAIN = (
    ("opened", "01J8Y0A0A000000000000000S1"),
    ("connection", "01J8Y0A1A000000000000000S2"),
    ("model_registered", "01J8Y0A2A000000000000000S3"),
    ("fee_registered", "01J8Y0A2B000000000000000S4"),
    ("disclosure", "01J8Y0A3A000000000000000S5"),
    ("version_created", "01J8Y0A4A000000000000000S6"),
    ("confirmed", "01J8Y0A5A000000000000000S7"),
    ("deployed", "01J8Y0A6A000000000000000S8"),
    ("stopped", "01J8ZQA0A000000000000000S9"),
    ("revoked", "01J8ZQA1A000000000000000SA"),
)
SEQ = {name: seq for seq, (name, _) in enumerate(CHAIN, start=1)}
ID = dict(CHAIN)
BASE_IDS = {
    "snapshot_reconciled": "01J8Z3N0A000000000000000R1",
    "snapshot_fees": "01J8Z3N0B000000000000000R2",
    "refused_stop": "01J8ZNB0T000000000000000R3",
    "refused_acknowledgment": "01J8ZPA0A000000000000000R4",
}
OWNER_INPUT = {
    "stop": "01J8ZNB00000000000000000C6",
    "acknowledge": "01J8ZPA00000000000000000C7",
}
assert all(is_ulid(i) for i in (*ID.values(), *BASE_IDS.values(), *OWNER_INPUT.values())), "event IDs (§3)"
assert len({*ID.values(), *BASE_IDS.values(), *OWNER_INPUT.values()}) == len(ID) + len(BASE_IDS) + len(
    OWNER_INPUT
)


def base_mandate() -> tuple[dict, str]:
    """The mandate reference cases' `btc_accumulator` and its version, after reproducing that file's
    own canonical text and hash, so the canonicalizer is proven on a mandate before it is trusted."""
    mandate, name = proven_base_mandate()
    return copy.deepcopy(mandate), name


@functools.cache
def proven_base_mandate() -> tuple[dict, str]:
    cases = yaml.safe_load(MANDATE_CASES.read_text(encoding="utf-8"))
    vector = cases["version_vector"]
    base = cases["bases"][vector["base"]]
    mandate = base["mandate"]
    assert artifact_ref(mandate) == vector["mandate_version"] == base["canonical_sha256"], "mandate version"
    assert canon(mandate) == vector["canonical"], "the base mandate's canonical text differs"
    return mandate, vector["base"]


MODEL_CONTENT = {
    "kind": "signal_model",
    "model_id": "quant.mean_reversion",
    "model_version": "1.0.0",
    "authorship": "platform",
    "code": "reference fixture: stands for the model code, prompt, and parameter schema (mandate spec §8.1)",
    "parameter_schema": {"lookback_bars": "integer", "z_entry": "decimal"},
}
FEE_CONFIG = {
    "kind": "fee_config",
    "note": "reference fixture: stands for a fee configuration snapshot (trading spec §6)",
}
DISCLOSURE = {
    "kind": "disclosure",
    "document": "leveraged_etp",
    "note": "reference fixture: stands for the disclosure text the owner accepted (mandate spec V-005)",
}


def mandate_document() -> dict:
    """The base mandate, pinning the registered model's real content hash (V-007): the base's
    placeholder hash names no stored object, and a `ref` must re-hash (§11 check 6)."""
    mandate, _ = base_mandate()
    document = copy.deepcopy(mandate)
    (model,) = document["behavior"]["signal_models"]
    assert (model["id"], model["version"]) == (MODEL_CONTENT["model_id"], MODEL_CONTENT["model_version"])
    model["content_hash"] = artifact_ref(MODEL_CONTENT)
    return document


def records(version: str) -> dict:
    note = "reference fixture: stands for mandate spec §10's {} contents, which mandate spec §10 defines: {}"
    return {
        "version_record": {
            "kind": "mandate_version_record",
            "mandate_version": version,
            "note": note.format(
                "MandateVersionCreated",
                "source text, compiled fields, provenance with quoted spans, template, policy-set hashes, "
                "validation results, warnings and worst-case figures, classification, and diff",
            ),
        },
        "confirmation_record": {
            "kind": "mandate_confirmation_record",
            "mandate_version": version,
            "note": note.format(
                "MandateConfirmed",
                "the rendered confirmation screen and UI build, warnings acknowledged, step-up evidence, "
                "and the confirming user",
            ),
        },
        "deployment_record": {
            "kind": "deployment_record",
            "mandate_version": version,
            "note": note.format(
                "AgentDeployed",
                "the rendered go-live screen and UI build, the backtest and paper-run IDs shown, the "
                "performance legend and disclosure versions shown, the approving users, and step-up evidence",
            ),
        },
    }


def artifacts() -> dict[str, dict]:
    document = mandate_document()
    return {
        "mandate_document": document,
        "model_content": MODEL_CONTENT,
        "fee_config": FEE_CONFIG,
        "disclosure": DISCLOSURE,
        **records(artifact_ref(document)),
    }


TOP_LEVEL_SOURCES = {
    "/autonomy": "user_entered",
    "/behavior": "user_entered",
    "/capital": "user_entered",
    "/connection_id": "user_entered",
    "/environment": "user_entered",
    "/goal": "user_stated",
    "/name": "user_entered",
    "/notifications": "user_entered",
    "/protection": "user_entered",
    "/risk": "user_entered",
    "/universe": "user_entered",
}


def step_up(at: str) -> dict:
    return {"assertion_id": "assert_owner_01", "authenticated_at": at, "method": "webauthn"}


def event(
    name: str, event_type: str, at: str, payload: dict, actor: dict, version: str | None = None, **envelope
) -> dict:
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": ID[name],
        "stream_id": STREAM,
        "seq": SEQ[name],
        "event_type": event_type,
        "schema_version": 1,
        "event_time": at,
        "recorded_at": at[:-4] + "400Z",
        "clock_source": "local",
        "causation_id": envelope.get("causation_id"),
        "correlation_id": None,
        "actor": dict(actor),
        "config_refs": {"mandate_version": version} if event_type in REQUIRED_REFS else {},
        "payload": payload,
        "artifact_refs": sorted(digest_strings(payload)),
        "pii_refs": [],
        "prev_hash": None,
    }


def chain_bodies(arts: dict[str, dict]) -> list[dict]:
    ref = {name: artifact_ref(obj) for name, obj in arts.items()}
    document = arts["mandate_document"]
    version = ref["mandate_document"]
    (model,) = document["behavior"]["signal_models"]
    return [
        event(
            "opened",
            "StreamOpened",
            "2026-09-20T13:00:00.000000000Z",
            {"stream_type": "control", "workspace_id": WORKSPACE},
            SERVICES,
        ),
        event(
            "connection",
            "ConnectionEstablished",
            "2026-09-20T13:05:00.000000000Z",
            {
                "connection_id": document["connection_id"],
                "broker": "alpaca",
                "environment": document["environment"],
                "scopes": ["account:write", "trading"],
            },
            USER,
        ),
        event(
            "model_registered",
            "ConfigSnapshotRegistered",
            "2026-09-20T13:06:00.000000000Z",
            {
                "kind": MODEL_KIND,
                "content_hash": ref["model_content"],
                "model_id": MODEL_CONTENT["model_id"],
                "model_version": MODEL_CONTENT["model_version"],
                "params": sorted(MODEL_CONTENT["parameter_schema"]),
                "admits_instruments": model["admits_instruments"],
            },
            SERVICES,
        ),
        event(
            "fee_registered",
            "ConfigSnapshotRegistered",
            "2026-09-20T13:06:30.000000000Z",
            {
                "kind": "fee_config",
                "content_hash": ref["fee_config"],
                "model_id": None,
                "model_version": None,
                "params": [],
                "admits_instruments": None,
            },
            SERVICES,
        ),
        event(
            "disclosure",
            "DisclosureAccepted",
            "2026-09-20T13:07:00.000000000Z",
            {
                "document": DISCLOSURE["document"],
                "version": ref["disclosure"],
                "user": OWNER,
                "step_up": step_up("2026-09-20T13:06:50.000000000Z"),
            },
            USER,
        ),
        event(
            "version_created",
            "MandateVersionCreated",
            "2026-09-20T13:10:00.000000000Z",
            {
                "mandate_version": version,
                "provenance": [
                    {"path": path, "source": source} for path, source in sorted(TOP_LEVEL_SOURCES.items())
                ],
                "record_ref": ref["version_record"],
            },
            SERVICES,
        ),
        event(
            "confirmed",
            "MandateConfirmed",
            "2026-09-20T13:12:00.000000000Z",
            {
                "mandate_version": version,
                "confirmed_paths": sorted(TOP_LEVEL_SOURCES),
                "record_ref": ref["confirmation_record"],
            },
            USER,
        ),
        event(
            "deployed",
            "AgentDeployed",
            "2026-09-20T13:20:00.000000000Z",
            {
                "agent_id": AGENT,
                "mandate_version": version,
                "record_ref": ref["deployment_record"],
            },
            USER,
            version,
        ),
        event(
            "stopped",
            "AgentStopped",
            "2026-09-25T18:00:00.000000000Z",
            {
                "agent_id": AGENT,
                "connection_id": document["connection_id"],
                "reason": "owner_stop",
                "retired_on": "2026-09-25",
                "loss_added": "125.5",
            },
            SERVICES,
            version,
        ),
        event(
            "revoked",
            "ConnectionRevoked",
            "2026-09-25T18:30:00.000000000Z",
            {"connection_id": document["connection_id"]},
            USER,
        ),
    ]


ACCOUNT_STREAM_REF = "01J8Z2ACCT00000000000000A1"


def base_drafts(v3: dict) -> dict[str, dict]:
    """Drafts on the account and agent streams, which have no chain here; each must be valid."""
    account = v3["chain"][0]["body"]["stream_id"]
    assert account == f"acct:{WORKSPACE}:{ACCOUNT_STREAM_REF}", "the version-3 account stream"

    def draft(name, stream, event_type, at, payload, actor, causation=None):
        return {
            "envelope_version": 1,
            "environment": "paper",
            "event_id": BASE_IDS[name],
            "stream_id": stream,
            "event_type": event_type,
            "schema_version": 1,
            "event_time": at,
            "clock_source": "broker" if event_type == "AccountSnapshotRecorded" else "local",
            "causation_id": causation,
            "correlation_id": None,
            "actor": dict(actor),
            "config_refs": {},
            "payload": payload,
            "artifact_refs": [],
            "pii_refs": [],
        }

    account_values = {
        "status": "ACTIVE",
        "crypto_status": "ACTIVE",
        "trading_blocked": False,
        "account_blocked": False,
        "trade_suspended_by_user": False,
        "multiplier": 1,
        "equity": "25000",
        "cash": "10000",
        "buying_power": "20000",
        "non_marginable_buying_power": "10000",
        "accrued_fees": "0",
    }
    return {
        "snapshot_reconciled": draft(
            "snapshot_reconciled",
            account,
            "AccountSnapshotRecorded",
            "2026-09-21T14:05:00.000000000Z",
            {
                **account_values,
                "model_cash": "10000.5",
                "cash_band": "1.25",
                "cash_in_band": True,
                "risk_clock": "2026-09-21T14:05:00.000000000Z",
            },
            EXECUTOR,
        ),
        "snapshot_fees": draft(
            "snapshot_fees",
            account,
            "AccountSnapshotRecorded",
            "2026-09-21T21:00:00.000000000Z",
            {
                **account_values,
                "model_cash": None,
                "cash_band": None,
                "cash_in_band": None,
                "risk_clock": "2026-09-21T21:00:00.000000000Z",
            },
            EXECUTOR,
        ),
        "refused_stop": draft(
            "refused_stop",
            AGENT_STREAM,
            "OwnerCommandRefused",
            "2026-09-21T20:30:00.000000000Z",
            {"command": "stop", "reason": "step_up_stale", "effective_at": "2026-09-21T20:30:00.000000000Z"},
            RUNTIME,
            OWNER_INPUT["stop"],
        ),
        "refused_acknowledgment": draft(
            "refused_acknowledgment",
            account,
            "OwnerCommandRefused",
            "2026-09-22T13:00:00.000000000Z",
            {
                "command": "acknowledge",
                "reason": "step_up_missing",
                "effective_at": "2026-09-22T13:00:00.000000000Z",
            },
            EXECUTOR,
            OWNER_INPUT["acknowledge"],
        ),
    }


DERIVATION = {
    "note": (
        "The control stream from a connection to a retirement, for the mandate reference cases' base "
        "btc_accumulator (its signal model's content hash replaced by the stored model content's, V-007). "
        "The account stream is the version-3 vectors'; which connection it belongs to is journaled "
        "nowhere yet (DEC-261), so the mapping to JournaledFact takes it as an argument, given here. "
        "The facts are what each record maps to (DEC-169): a record that maps to none is not listed."
    ),
    "mandate_base": "btc_accumulator",
    "account_connection": {"stream_id": f"acct:{WORKSPACE}:{ACCOUNT_STREAM_REF}", "connection_id": None},
}


def derivation(document: dict) -> dict:
    d = copy.deepcopy(DERIVATION)
    d["account_connection"]["connection_id"] = document["connection_id"]
    return d


def expected_facts(arts: dict[str, dict]) -> list[dict]:
    """What each record maps to, written from the constants the chain was built from."""
    document = arts["mandate_document"]
    version = artifact_ref(document)
    connection = document["connection_id"]
    return [
        {
            "seq": SEQ["connection"],
            "fact": {"kind": "ConnectionEstablished", "connection_id": connection, "environment": "paper"},
        },
        {
            "seq": SEQ["model_registered"],
            "fact": {
                "kind": "ModelRegistered",
                "id": "quant.mean_reversion",
                "version": "1.0.0",
                "content_hash": artifact_ref(MODEL_CONTENT),
                "params": ["lookback_bars", "z_entry"],
                "admits_instruments": False,
            },
        },
        {
            "seq": SEQ["disclosure"],
            "fact": {"kind": "DisclosureAccepted", "version": artifact_ref(DISCLOSURE)},
        },
        {
            "seq": SEQ["version_created"],
            "fact": {
                "kind": "MandateVersionCreated",
                "version": version,
                "sources": [{"path": p, "source": s} for p, s in sorted(TOP_LEVEL_SOURCES.items())],
            },
        },
        {
            "seq": SEQ["confirmed"],
            "fact": {
                "kind": "MandateConfirmed",
                "version": version,
                "confirmed_paths": sorted(TOP_LEVEL_SOURCES),
            },
        },
        {
            "seq": SEQ["deployed"],
            "fact": {
                "kind": "AgentVersionActive",
                "agent": AGENT,
                "connection_id": connection,
                "environment": "paper",
                "allocation_usd": "10000",
                "pinned": ["7b4a1c2e-1111-4a2b-9c3d-000000000001"],
            },
        },
        {
            "seq": SEQ["stopped"],
            "fact": {
                "kind": "AgentStopped",
                "agent": AGENT,
                "connection_id": connection,
                "retired_on": "2026-09-25",
                "loss_added_usd": "125.5",
            },
        },
        {"seq": SEQ["revoked"], "fact": {"kind": "ConnectionRevoked", "connection_id": connection}},
        {
            "draft": "snapshot_reconciled",
            "fact": {"kind": "AccountSnapshot", "connection_id": connection, "equity_usd": "25000"},
        },
        {
            "draft": "snapshot_fees",
            "fact": {"kind": "AccountSnapshot", "connection_id": connection, "equity_usd": "25000"},
        },
    ]


# --------------------------------------------------------------------------- drafts


def invalid(name, clause, base, changes, reason, path, also=()):
    """`also` lists, in report order, the later rules a draft breaks on purpose: the journal
    reports only the first, and the generator checks the whole order."""
    where = {"base_seq": SEQ[base]} if base in SEQ else {"base_draft": base}
    expect = {"outcome": "Invalid", "reason": reason, "path": path}
    if also:
        expect["also"] = [{"reason": r, "path": at} for r, at in also]
    return {"name": name, "clause": clause, **where, "changes": changes, "expect": expect}


def reported(got: list[Violation], want: dict) -> bool:
    """The draft breaks exactly the rules its expectation lists, in that order."""
    listed = [(want["reason"], want["path"])] + [(a["reason"], a["path"]) for a in want.get("also", [])]
    return [(v.reason, v.path) for v in got] == listed


def valid(name, clause, base, changes):
    where = {"base_seq": SEQ[base]} if base in SEQ else {"base_draft": base}
    return {"name": name, "clause": clause, **where, "changes": changes, "expect": {"outcome": "Valid"}}


def provenance(*paths: str) -> list[dict]:
    return [{"path": p, "source": TOP_LEVEL_SOURCES.get(p, "user_entered")} for p in paths]


def refs(*names: str) -> dict:
    """`artifact_refs` once a change leaves only the named artifacts in the payload (§3)."""
    arts = artifacts()
    return change("artifact_refs", sorted(artifact_ref(arts[name]) for name in names))


def last_byte_flipped(ref: str) -> str:
    """The same hash but for its last hex digit: a near-collision a prefix comparison accepts."""
    return ref[:-1] + ("0" if ref[-1] != "0" else "1")


def chain_payload(name: str) -> dict:
    """A copy of the named chain event's payload, to change one element of a list in it."""
    return copy.deepcopy(chain_bodies(artifacts())[SEQ[name] - 1]["payload"])


def second_nulled(items: list) -> list:
    return [items[0], None, *items[2:]]


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule, or the rules its `also` lists; together they cover every
    §9.2 member type and rule."""
    version = artifact_ref(artifacts()["mandate_document"])
    scopes = chain_payload("connection")["scopes"]
    params = chain_payload("model_registered")["params"]
    confirmed = chain_payload("confirmed")["confirmed_paths"]
    entries = chain_payload("version_created")["provenance"]
    path_nulled = copy.deepcopy(entries)
    path_nulled[1]["path"] = None
    spanned = copy.deepcopy(entries)
    spanned[1]["quoted_span"] = "keep it under ten percent"
    return [
        invalid(
            "disclosure_step_up_assertion_null",
            "§9.2 DisclosureAccepted.step_up.assertion_id: never null",
            "disclosure",
            [change("payload.step_up.assertion_id", None)],
            "schema",
            "payload.step_up.assertion_id",
        ),
        invalid(
            "disclosure_step_up_time_null",
            "§9.2 DisclosureAccepted.step_up.authenticated_at: never null",
            "disclosure",
            [change("payload.step_up.authenticated_at", None)],
            "schema",
            "payload.step_up.authenticated_at",
        ),
        invalid(
            "provenance_second_path_null",
            "§9.2 MandateVersionCreated.provenance[].path: never null, in every entry",
            "version_created",
            [change("payload.provenance", path_nulled)],
            "schema",
            "payload.provenance[1].path",
        ),
        invalid(
            "provenance_second_entry_quoted_span",
            "§9.2 every schema is closed, in every provenance entry",
            "version_created",
            [change("payload.provenance", spanned)],
            "schema",
            "payload.provenance[1].quoted_span",
        ),
        invalid(
            "connection_second_scope_null",
            "§9.2 ConnectionEstablished.scopes: every element is text",
            "connection",
            [change("payload.scopes", second_nulled(scopes))],
            "schema",
            "payload.scopes[1]",
        ),
        invalid(
            "confirmed_second_path_null",
            "§9.2 MandateConfirmed.confirmed_paths: every element is a pointer",
            "confirmed",
            [change("payload.confirmed_paths", second_nulled(confirmed))],
            "schema",
            "payload.confirmed_paths[1]",
        ),
        invalid(
            "model_second_param_null",
            "§9.2 ConfigSnapshotRegistered.params: every element is text",
            "model_registered",
            [change("payload.params", second_nulled(params))],
            "schema",
            "payload.params[1]",
        ),
        invalid(
            "fee_named_and_versioned",
            "rule 21: the first offending member, model_id before model_version",
            "fee_registered",
            [change("payload.model_id", "pairs"), change("payload.model_version", "1")],
            "schema",
            "payload.model_id",
        ),
        invalid(
            "fee_versioned_and_admitting",
            "rule 21: the first offending member, model_version before admits_instruments",
            "fee_registered",
            [change("payload.model_version", "1"), change("payload.admits_instruments", False)],
            "schema",
            "payload.model_version",
        ),
        invalid(
            "fee_named_with_params",
            "rule 21: a model member before params",
            "fee_registered",
            [change("payload.model_id", "pairs"), change("payload.params", ["lookback_bars"])],
            "schema",
            "payload.model_id",
        ),
        invalid(
            "connection_carries_a_key",
            "§9.2 no credential member (AGENTS.md rules 6 and 7)",
            "connection",
            [change("payload.api_key", "reference-fixture-not-a-key")],
            "schema",
            "payload.api_key",
        ),
        invalid(
            "connection_live_scope_unsorted",
            "rule 19",
            "connection",
            [change("payload.scopes", ["trading", "account:write"])],
            "non_canonical",
            "payload.scopes",
        ),
        invalid(
            "connection_scope_repeated",
            "rule 19",
            "connection",
            [change("payload.scopes", ["trading", "trading"])],
            "non_canonical",
            "payload.scopes",
        ),
        invalid(
            "connection_environment_backtest",
            "§9.2 ConnectionEstablished.environment",
            "connection",
            [change("payload.environment", "backtest")],
            "non_canonical",
            "payload.environment",
        ),
        invalid(
            "connection_scope_empty_string",
            "§9.1 text",
            "connection",
            [change("payload.scopes", ["", "trading"])],
            "non_canonical",
            "payload.scopes[0]",
        ),
        invalid(
            "revoked_connection_not_an_id",
            "§9.1 id",
            "revoked",
            [change("payload.connection_id", "conn alpaca")],
            "non_canonical",
            "payload.connection_id",
        ),
        invalid(
            "disclosure_without_step_up",
            "§9.2 DisclosureAccepted.step_up (V-005)",
            "disclosure",
            [change("payload.step_up", None)],
            "schema",
            "payload.step_up",
        ),
        invalid(
            "disclosure_step_up_carries_a_token",
            "§9.2 every schema is closed at every depth: step_up (AGENTS.md rules 6 and 7)",
            "disclosure",
            [change("payload.step_up.token", "reference-fixture-not-a-token")],
            "schema",
            "payload.step_up.token",
        ),
        invalid(
            "disclosure_step_up_assertion_not_text",
            "§9.2 step_up.assertion_id: text",
            "disclosure",
            [change("payload.step_up.assertion_id", 7)],
            "schema",
            "payload.step_up.assertion_id",
        ),
        invalid(
            "disclosure_step_up_time_off_form",
            "§9.2 step_up.authenticated_at: a §4.7 timestamp",
            "disclosure",
            [change("payload.step_up.authenticated_at", "2026-09-20 13:06:50")],
            "non_canonical",
            "payload.step_up.authenticated_at",
        ),
        invalid(
            "disclosure_step_up_method_null",
            "§9.2 step_up.method: text, never null",
            "disclosure",
            [change("payload.step_up.method", None)],
            "schema",
            "payload.step_up.method",
        ),
        invalid(
            "provenance_entry_carries_a_span",
            "§9.2 every schema is closed at every depth: a provenance entry (quoted spans are in the record)",
            "version_created",
            [
                change(
                    "payload.provenance",
                    [{"path": "/autonomy", "source": "user_entered", "quoted_span": "buy dips"}],
                )
            ],
            "schema",
            "payload.provenance[0].quoted_span",
        ),
        invalid(
            "disclosure_version_bare_id",
            "§9.2 ref",
            "disclosure",
            [change("payload.version", "leveraged_etp_v3"), change("artifact_refs", [])],
            "non_canonical",
            "payload.version",
        ),
        invalid(
            "model_kind_unknown",
            "§9.2 ConfigSnapshotRegistered.kind",
            "fee_registered",
            [change("payload.kind", "calendar")],
            "non_canonical",
            "payload.kind",
        ),
        invalid(
            "model_without_id",
            "rule 21",
            "model_registered",
            [change("payload.model_id", None)],
            "schema",
            "payload.model_id",
        ),
        invalid(
            "model_without_admits",
            "rule 21",
            "model_registered",
            [change("payload.admits_instruments", None)],
            "schema",
            "payload.admits_instruments",
        ),
        invalid(
            "fee_with_model_version",
            "rule 21",
            "fee_registered",
            [change("payload.model_version", "1.0.0")],
            "schema",
            "payload.model_version",
        ),
        invalid(
            "fee_with_params",
            "rule 21",
            "fee_registered",
            [change("payload.params", ["lookback_bars"])],
            "schema",
            "payload.params",
        ),
        invalid(
            "fee_params_unsorted",
            "rules 20 and 21: the lower-numbered rule is reported first",
            "fee_registered",
            [change("payload.params", ["taker_bps", "maker_bps"])],
            "non_canonical",
            "payload.params",
            also=[("schema", "payload.params")],
        ),
        invalid(
            "model_params_unsorted",
            "rule 20",
            "model_registered",
            [change("payload.params", ["z_entry", "lookback_bars"])],
            "non_canonical",
            "payload.params",
        ),
        invalid(
            "version_created_bare_version",
            "§9.2 MandateVersionCreated.mandate_version: a bare id is not a version",
            "version_created",
            [change("payload.mandate_version", "btc-accumulator-v1"), refs("version_record")],
            "non_canonical",
            "payload.mandate_version",
        ),
        invalid(
            "version_created_without_version",
            "§9.2 MandateVersionCreated.mandate_version: never null",
            "version_created",
            [change("payload.mandate_version", None), refs("version_record")],
            "schema",
            "payload.mandate_version",
        ),
        invalid(
            "version_created_record_by_path",
            "§9.2 record_ref: a path is not a content address",
            "version_created",
            [change("payload.record_ref", "records/btc-accumulator/v1.json"), refs("mandate_document")],
            "non_canonical",
            "payload.record_ref",
        ),
        invalid(
            "version_created_record_absent",
            "§9.1 absent member",
            "version_created",
            [delete("payload.record_ref"), refs("mandate_document")],
            "schema",
            "payload.record_ref",
        ),
        invalid(
            "provenance_dotted_path",
            "§9.2 pointer",
            "version_created",
            [change("payload.provenance", [{"path": "autonomy", "source": "user_entered"}])],
            "non_canonical",
            "payload.provenance[0].path",
        ),
        invalid(
            "provenance_bad_escape",
            "§9.2 pointer",
            "version_created",
            [change("payload.provenance", [{"path": "/risk/a~2b", "source": "user_entered"}])],
            "non_canonical",
            "payload.provenance[0].path",
        ),
        invalid(
            "provenance_whole_document",
            "§9.2 pointer: never the empty pointer",
            "version_created",
            [change("payload.provenance", [{"path": "", "source": "user_entered"}])],
            "non_canonical",
            "payload.provenance[0].path",
        ),
        invalid(
            "provenance_unknown_source",
            "§9.2 MandateVersionCreated.provenance[].source",
            "version_created",
            [change("payload.provenance", [{"path": "/autonomy", "source": "compiler"}])],
            "non_canonical",
            "payload.provenance[0].source",
        ),
        invalid(
            "provenance_unsorted",
            "rule 17",
            "version_created",
            [change("payload.provenance", provenance("/risk", "/autonomy"))],
            "non_canonical",
            "payload.provenance",
        ),
        invalid(
            "provenance_repeated_path",
            "rule 17",
            "version_created",
            [change("payload.provenance", provenance("/risk", "/risk"))],
            "non_canonical",
            "payload.provenance",
        ),
        invalid(
            "confirmed_bare_version",
            "§9.2 MandateConfirmed.mandate_version",
            "confirmed",
            [change("payload.mandate_version", "v1"), refs("confirmation_record")],
            "non_canonical",
            "payload.mandate_version",
        ),
        invalid(
            "confirmed_paths_unsorted",
            "rule 18",
            "confirmed",
            [change("payload.confirmed_paths", ["/risk", "/autonomy"])],
            "non_canonical",
            "payload.confirmed_paths",
        ),
        invalid(
            "confirmed_path_not_a_pointer",
            "§9.2 pointer",
            "confirmed",
            [change("payload.confirmed_paths", ["risk"])],
            "non_canonical",
            "payload.confirmed_paths[0]",
        ),
        invalid(
            "confirmed_record_unlisted",
            "§3 artifact_refs",
            "confirmed",
            [change("artifact_refs", [])],
            "artifact_refs",
            "artifact_refs",
        ),
        invalid(
            "deployed_version_not_its_config_ref",
            "rule 22",
            "deployed",
            [change("config_refs.mandate_version", "sha256:" + "5" * 64)],
            "schema",
            "payload.mandate_version",
        ),
        invalid(
            "deployed_without_mandate_ref",
            "§9 required config_refs",
            "deployed",
            [change("config_refs", {})],
            "missing_config_ref",
            "config_refs.mandate_version",
        ),
        invalid(
            "deployed_on_the_agent_stream",
            "§9 catalogue",
            "deployed",
            [change("stream_id", AGENT_STREAM)],
            "wrong_stream",
            "event_type",
        ),
        invalid(
            "stopped_loss_negative",
            "rule 23",
            "stopped",
            [change("payload.loss_added", "-0.01")],
            "schema",
            "payload.loss_added",
        ),
        invalid(
            "stopped_reason_risk_limit",
            "§9.2 AgentStopped.reason",
            "stopped",
            [change("payload.reason", "risk_limit")],
            "non_canonical",
            "payload.reason",
        ),
        invalid(
            "stopped_retired_on_timestamp",
            "§9.2 date",
            "stopped",
            [change("payload.retired_on", "2026-09-25T18:00:00.000000000Z")],
            "non_canonical",
            "payload.retired_on",
        ),
        invalid(
            "deployed_version_differs_in_its_last_byte",
            "rule 22: the whole hash is compared",
            "deployed",
            [change("config_refs.mandate_version", last_byte_flipped(version))],
            "schema",
            "payload.mandate_version",
        ),
        invalid(
            "stopped_retired_on_long_day",
            "§9.2 date: exactly YYYY-MM-DD",
            "stopped",
            [change("payload.retired_on", "2026-09-0005")],
            "non_canonical",
            "payload.retired_on",
        ),
        invalid(
            "stopped_retired_on_impossible",
            "§9.2 date",
            "stopped",
            [change("payload.retired_on", "2026-02-29")],
            "non_canonical",
            "payload.retired_on",
        ),
        invalid(
            "opened_for_another_workspace",
            "rule 25",
            "opened",
            [change("payload.workspace_id", "ws_other")],
            "stream_mismatch",
            "stream_id",
        ),
        invalid(
            "snapshot_carries_account_number",
            "§9.2 no personal data (§6.4)",
            "snapshot_reconciled",
            [change("payload.account_number", "reference-fixture")],
            "schema",
            "payload.account_number",
        ),
        invalid(
            "snapshot_model_cash_absent",
            "§9.1 absent member: never read as null, even where null is valid (§4.2)",
            "snapshot_reconciled",
            [delete("payload.model_cash")],
            "schema",
            "payload.model_cash",
        ),
        invalid(
            "snapshot_risk_clock_absent",
            "§9.2 AccountSnapshotRecorded.risk_clock: the executor folds every event at its clock",
            "snapshot_fees",
            [delete("payload.risk_clock")],
            "schema",
            "payload.risk_clock",
        ),
        invalid(
            "snapshot_risk_clock_as_seconds",
            "§9.2 risk_clock: a whole-second timestamp, never integer seconds (§2)",
            "snapshot_fees",
            [change("payload.risk_clock", 1790024400)],
            "schema",
            "payload.risk_clock",
        ),
        invalid(
            "snapshot_risk_clock_not_a_timestamp",
            "§9.2 risk_clock: a §4.7 timestamp, whole second or not (month 13 here)",
            "snapshot_reconciled",
            [change("payload.risk_clock", "2026-13-21T14:05:00.000000000Z")],
            "non_canonical",
            "payload.risk_clock",
        ),
        invalid(
            "snapshot_risk_clock_null",
            "§9.2 risk_clock: required and never null",
            "snapshot_reconciled",
            [change("payload.risk_clock", None)],
            "schema",
            "payload.risk_clock",
        ),
        invalid(
            "snapshot_risk_clock_off_the_second",
            "§9.2 risk_clock: a whole second (§2)",
            "snapshot_reconciled",
            [change("payload.risk_clock", "2026-09-21T14:05:00.000000001Z")],
            "non_canonical",
            "payload.risk_clock",
        ),
        invalid(
            "snapshot_multiplier_text",
            "§9.1 integer",
            "snapshot_reconciled",
            [change("payload.multiplier", "1")],
            "schema",
            "payload.multiplier",
        ),
        invalid(
            "snapshot_band_without_model_cash",
            "rule 24",
            "snapshot_fees",
            [change("payload.cash_band", "1.25")],
            "schema",
            "payload.cash_band",
        ),
        invalid(
            "snapshot_comparison_without_flag",
            "rule 24",
            "snapshot_reconciled",
            [change("payload.cash_in_band", None)],
            "schema",
            "payload.cash_in_band",
        ),
        invalid(
            "snapshot_band_negative",
            "rule 24",
            "snapshot_reconciled",
            [change("payload.cash_band", "-1.25"), change("payload.cash_in_band", False)],
            "schema",
            "payload.cash_band",
        ),
        invalid(
            "snapshot_outside_band_flagged_inside",
            "rule 24",
            "snapshot_reconciled",
            [change("payload.model_cash", "10001.26")],
            "schema",
            "payload.cash_in_band",
        ),
        invalid(
            "snapshot_inside_band_flagged_outside",
            "rule 24",
            "snapshot_reconciled",
            [change("payload.cash_in_band", False)],
            "schema",
            "payload.cash_in_band",
        ),
        invalid(
            "refused_acknowledgment_on_the_agent_stream",
            "rule 26",
            "refused_stop",
            [change("payload.command", "acknowledge")],
            "stream_mismatch",
            "payload.command",
        ),
        invalid(
            "refused_stop_on_the_account_stream",
            "rule 26",
            "refused_acknowledgment",
            [change("payload.command", "stop")],
            "stream_mismatch",
            "payload.command",
        ),
        invalid(
            "refused_stop_not_independent",
            "rule 28",
            "refused_stop",
            [change("payload.reason", "not_independent")],
            "stream_mismatch",
            "payload.reason",
        ),
        invalid(
            "refused_stop_not_independent_without_cause",
            "rules 28 then 27",
            "refused_stop",
            [change("payload.reason", "not_independent"), change("causation_id", None)],
            "stream_mismatch",
            "payload.reason",
            also=[("schema", "causation_id")],
        ),
        invalid(
            "refused_acknowledgment_on_the_agent_stream_not_independent",
            "rules 26 then 28",
            "refused_stop",
            [change("payload.command", "acknowledge"), change("payload.reason", "not_independent")],
            "stream_mismatch",
            "payload.command",
            also=[("stream_mismatch", "payload.reason")],
        ),
        invalid(
            "refused_stop_without_cause",
            "rule 27 (rule 16 on the agent stream)",
            "refused_stop",
            [change("causation_id", None)],
            "schema",
            "causation_id",
        ),
        invalid(
            "refused_acknowledgment_without_cause",
            "rule 27",
            "refused_acknowledgment",
            [change("causation_id", None)],
            "schema",
            "causation_id",
        ),
        invalid(
            "refused_pause",
            "§9.2 OwnerCommandRefused.command",
            "refused_stop",
            [change("payload.command", "pause")],
            "non_canonical",
            "payload.command",
        ),
        invalid(
            "refused_reason_unknown",
            "§9.2 OwnerCommandRefused.reason",
            "refused_stop",
            [change("payload.reason", "step_up_expired")],
            "non_canonical",
            "payload.reason",
        ),
        invalid(
            "refused_at_a_date",
            "§9.1 timestamp: the §4.7 form, not any text",
            "refused_stop",
            [change("payload.effective_at", "2026-10-02")],
            "non_canonical",
            "payload.effective_at",
        ),
        invalid(
            "refused_at_risk_clock_seconds",
            "§9.1 timestamp",
            "refused_stop",
            [change("payload.effective_at", 1790022600)],
            "schema",
            "payload.effective_at",
        ),
    ]


def valid_drafts() -> list[dict]:
    """Cases a rule might be misread to refuse; each must be accepted."""
    return [
        valid(
            "snapshot_drift_equal_to_band",
            "rule 24: inside includes the band itself",
            "snapshot_reconciled",
            [change("payload.model_cash", "10001.25")],
        ),
        valid(
            "snapshot_outside_band",
            "rule 24",
            "snapshot_reconciled",
            [change("payload.model_cash", "10001.26"), change("payload.cash_in_band", False)],
        ),
        valid(
            "stopped_with_no_loss",
            "rule 23: max(0, N - E) may be 0",
            "stopped",
            [change("payload.loss_added", "0")],
        ),
        valid(
            "model_without_params",
            "rule 21: a model may declare no parameters",
            "model_registered",
            [change("payload.params", [])],
        ),
        valid("refused_resume", "rule 26", "refused_stop", [change("payload.command", "resume")]),
        valid(
            "refused_acknowledgment_for_its_method",
            "rule 26",
            "refused_acknowledgment",
            [change("payload.reason", "step_up_method")],
        ),
        valid(
            "refused_acknowledgment_not_independent",
            "rule 28",
            "refused_acknowledgment",
            [change("payload.reason", "not_independent")],
        ),
        valid(
            "provenance_escaped_token",
            "§9.2 pointer: ~0 and ~1",
            "version_created",
            [change("payload.provenance", [{"path": "/risk/a~0b~1c", "source": "user_entered"}])],
        ),
        valid(
            "live_connection",
            "§9.2 ConnectionEstablished.environment",
            "connection",
            [change("payload.environment", "live")],
        ),
    ]


def draft_for(section: dict, case: dict) -> dict:
    if "base_seq" in case:
        draft = draft_of(section["chain"][case["base_seq"] - 1]["body"])
    else:
        draft = copy.deepcopy(section["drafts"][case["base_draft"]])
    for item in case["changes"]:
        apply_change(draft, item)
    return draft


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = (
    *(f"chain.{c}" for c in ("seq", "prev_hash", "canonical", "hash", "stream", "opened", "valid")),
    "drafts.valid",
    "artifacts.rehash",
    "artifacts.missing",
    "document.base",
    *(f"version.{c}" for c in ("binds", "record")),
    *(f"order.{c}" for c in ("confirmed", "deployed", "stopped", "revoked")),
    "confirm.covers",
    "deploy.connection",
    "model.pinned",
    "facts.recompute",
    "invalid_drafts",
    "valid_drafts",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def check_of(problem: str) -> str:
    return problem.partition(": ")[0]


def covers(prefix: str, path: str) -> bool:
    return path == prefix or path.startswith(prefix + "/")


def recompute_facts(section: dict) -> list[dict]:
    """The `JournaledFact` each record maps to (DEC-169), from the chain's payloads and the stored
    artifacts alone: a deployment's connection, environment, allocation, and pinned instruments are
    read from the mandate document its version names."""
    stored = {a["ref"]: a["object"] for a in section["artifacts"]}
    binding = section["derivation"]["account_connection"]
    out = []
    for entry in section["chain"]:
        body, p = entry["body"], entry["body"]["payload"]
        fact = None
        match body["event_type"]:
            case "ConnectionEstablished":
                fact = {
                    "kind": "ConnectionEstablished",
                    "connection_id": p["connection_id"],
                    "environment": p["environment"],
                }
            case "ConnectionRevoked":
                fact = {"kind": "ConnectionRevoked", "connection_id": p["connection_id"]}
            case "DisclosureAccepted":
                fact = {"kind": "DisclosureAccepted", "version": p["version"]}
            case "ConfigSnapshotRegistered" if p["kind"] == MODEL_KIND:
                fact = {
                    "kind": "ModelRegistered",
                    "id": p["model_id"],
                    "version": p["model_version"],
                    "content_hash": p["content_hash"],
                    "params": p["params"],
                    "admits_instruments": p["admits_instruments"],
                }
            case "MandateVersionCreated":
                fact = {
                    "kind": "MandateVersionCreated",
                    "version": p["mandate_version"],
                    "sources": p["provenance"],
                }
            case "MandateConfirmed":
                fact = {
                    "kind": "MandateConfirmed",
                    "version": p["mandate_version"],
                    "confirmed_paths": p["confirmed_paths"],
                }
            case "AgentDeployed" if p["mandate_version"] not in stored:
                fact = {"kind": "unresolved", "mandate_version": p["mandate_version"]}
            case "AgentDeployed":
                document = stored[p["mandate_version"]]
                fact = {
                    "kind": "AgentVersionActive",
                    "agent": p["agent_id"],
                    "connection_id": document["connection_id"],
                    "environment": document["environment"],
                    "allocation_usd": normalize_decimal(document["capital"]["allocation_usd"]),
                    "pinned": sorted(i["asset_id"] for i in document["universe"]["pinned_instruments"]),
                }
            case "AgentStopped":
                fact = {
                    "kind": "AgentStopped",
                    "agent": p["agent_id"],
                    "connection_id": p["connection_id"],
                    "retired_on": p["retired_on"],
                    "loss_added_usd": p["loss_added"],
                }
        if fact is not None:
            out.append({"seq": entry["seq"], "fact": fact})
    for name, draft in section["drafts"].items():
        if draft["event_type"] == "AccountSnapshotRecorded" and draft["stream_id"] == binding["stream_id"]:
            fact = {
                "kind": "AccountSnapshot",
                "connection_id": binding["connection_id"],
                "equity_usd": draft["payload"]["equity"],
            }
            out.append({"draft": name, "fact": fact})
    return out


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    chain = section["chain"]
    prev = section["genesis_prev_hash"]
    for i, entry in enumerate(chain, start=1):
        body = entry["body"]
        if body["seq"] != i or entry["seq"] != i:
            problems.append(found("chain.seq", f"seq {i}: gap"))
        if body["prev_hash"] != prev:
            problems.append(found("chain.prev_hash", f"seq {i}: prev_hash does not chain"))
        if canonical_of(body) != entry["canonical"]:
            problems.append(found("chain.canonical", f"seq {i}: canonical differs"))
        if sha(entry["canonical"]) != entry["hash"]:
            problems.append(found("chain.hash", f"seq {i}: hash differs"))
        prev = entry["hash"]
        if body["stream_id"] != STREAM or body["event_type"] != entry["event_type"]:
            problems.append(found("chain.stream", f"seq {i}: stream or type differs"))
    if chain[0]["event_type"] != "StreamOpened":
        problems.append(found("chain.opened", "seq 1 is not StreamOpened"))
    stored = {a["ref"]: a for a in section["artifacts"]}
    for a in section["artifacts"]:
        if canonical_of(a["object"]) != a["canonical"] or "sha256:" + sha(a["canonical"]) != a["ref"]:
            problems.append(found("artifacts.rehash", f"artifact {a['name']}: does not re-hash"))
    for entry in chain:
        for ref in entry["body"]["artifact_refs"]:
            if ref not in stored:
                problems.append(found("artifacts.missing", f"seq {entry['seq']}: artifact {ref} missing"))
    named = named_records(chain)
    problems += check_versions(section, named)
    problems += check_order(named)
    created, confirmed = named["version_created"]["payload"], named["confirmed"]["payload"]
    for path in confirmed["confirmed_paths"]:
        if not any(covers(path, entry["path"]) for entry in created["provenance"]):
            problems.append(found("confirm.covers", f"confirmed path {path} covers no recorded path"))
    by_ref = {a["ref"]: a["object"] for a in section["artifacts"]}
    document = by_ref.get(named["deployed"]["payload"]["mandate_version"], {})
    connection = named["connection"]["payload"]
    if (document.get("connection_id"), document.get("environment")) != (
        connection["connection_id"],
        connection["environment"],
    ):
        problems.append(
            found(
                "deploy.connection",
                "the deployed mandate's connection and environment are not the connection's (V-001)",
            )
        )
    registered = named["model_registered"]["payload"]
    pinned = [
        (m["id"], m["version"], m["content_hash"])
        for m in document.get("behavior", {}).get("signal_models", [])
    ]
    if pinned != [(registered["model_id"], registered["model_version"], registered["content_hash"])]:
        problems.append(
            found(
                "model.pinned",
                "the mandate does not pin the registered model's id, version, and hash (V-007)",
            )
        )
    if recompute_facts(section) != section["journaled_facts"]:
        problems.append(found("facts.recompute", "the listed facts are not what the records map to"))
    for name, draft in section["drafts"].items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case))
        want = case["expect"]
        if not reported(got, want):
            problems.append(
                found(
                    "invalid_drafts",
                    f"{case['name']}: expected exactly {want['reason']} at {want['path']}"
                    f" and {want.get('also', [])}, got {got}",
                )
            )
    for case in section["valid_drafts"]:
        got = violations(draft_for(section, case))
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
    for entry in chain:
        got = violations(draft_of(entry["body"]))
        if got:
            problems.append(found("chain.valid", f"seq {entry['seq']}: valid event breaks {got}"))
    return problems


NAMED_TYPES = {
    "connection": "ConnectionEstablished",
    "disclosure": "DisclosureAccepted",
    "version_created": "MandateVersionCreated",
    "confirmed": "MandateConfirmed",
    "deployed": "AgentDeployed",
    "stopped": "AgentStopped",
    "revoked": "ConnectionRevoked",
}


def named_records(chain: list[dict]) -> dict[str, dict]:
    """Each lifecycle record, found by what it is rather than where the chain was built to put it,
    so the order checks see a record journaled out of place."""
    named = {}
    for entry in chain:
        body = entry["body"]
        for name, event_type in NAMED_TYPES.items():
            if body["event_type"] == event_type and name not in named:
                named[name] = body
        if body["event_type"] == "ConfigSnapshotRegistered" and body["payload"]["kind"] == MODEL_KIND:
            named.setdefault("model_registered", body)
    return named


def check_versions(section: dict, named: dict) -> list[str]:
    """Each record binds the version its mandate document hashes to, and each §10 record names it."""
    problems = []
    by_name = {a["name"]: a for a in section["artifacts"]}
    mandate, _ = base_mandate()
    document = by_name["mandate_document"]["object"]
    expected = copy.deepcopy(mandate)
    expected["behavior"]["signal_models"][0]["content_hash"] = by_name["model_content"]["ref"]
    if document != expected:
        problems.append(
            found("document.base", "the stored mandate is not the base with the registered model's hash")
        )
    version = by_name["mandate_document"]["ref"]
    bound = [
        named["version_created"]["payload"]["mandate_version"],
        named["confirmed"]["payload"]["mandate_version"],
        named["deployed"]["payload"]["mandate_version"],
        named["deployed"]["config_refs"].get("mandate_version"),
        named["stopped"]["config_refs"].get("mandate_version"),
    ]
    if any(v != version for v in bound):
        problems.append(found("version.binds", f"a record binds another version than {version}: {bound}"))
    by_ref = {a["ref"]: a["object"] for a in section["artifacts"]}
    for name in ("version_created", "confirmed", "deployed"):
        record = by_ref.get(named[name]["payload"]["record_ref"], {})
        if record.get("mandate_version") != version:
            problems.append(found("version.record", f"{name}'s §10 record names another version"))
    return problems


def check_order(named: dict) -> list[str]:
    """The lifecycle (mandate spec §2): created, confirmed, deployed, stopped, then the connection revoked."""
    problems = []
    seq = {name: body["seq"] for name, body in named.items()}
    for later, earlier, check in (
        ("confirmed", "version_created", "order.confirmed"),
        ("deployed", "confirmed", "order.deployed"),
        ("stopped", "deployed", "order.stopped"),
        ("revoked", "stopped", "order.revoked"),
    ):
        if not seq[earlier] < seq[later]:
            problems.append(found(check, f"{later} is journaled before {earlier}"))
    if named["deployed"]["payload"]["agent_id"] != named["stopped"]["payload"]["agent_id"]:
        problems.append(found("order.stopped", "the stopped agent is not the deployed one"))
    if named["revoked"]["payload"]["connection_id"] != named["stopped"]["payload"]["connection_id"]:
        problems.append(found("order.revoked", "the revoked connection is not the stopped agent's"))
    return problems


def canonical_of(value) -> str:
    return canon(value)


def sha(text: str) -> str:
    return sha256_hex(text.encode())


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "record.extra",
    "record.missing",
    "record.missing.nullable",
    "open.step_up",
    "open.provenance_entry",
    "loose.step_up.assertion_id",
    "loose.step_up.authenticated_at",
    "loose.step_up.method",
    "types.str_nonempty",
    "types.pointer",
    "types.date",
    "types.date_length",
    "types.risk_clock",
    "types.risk_clock_whole",
    "types.risk_clock_nullable",
    "text.payload.effective_at",
    "nullable.payload.mandate_version",
    "artifact_refs",
    "config_refs.required",
    *(f"rule.{n}" for n in range(17, 25)),
    "rule.21.params",
    "order.rule_21_first",
    *RULE_21_REORDERINGS,
    RULE_21_PARAMS_FIRST,
    "nullable.step_up.assertion_id",
    "nullable.step_up.authenticated_at",
    "nullable.provenance_entry.path",
    "open_after_first.provenance_entry",
    "first_only.scopes",
    "first_only.confirmed_paths",
    "first_only.params",
    "rule.22.prefix",
    "rule.24.band",
    "rule.24.in_band",
    "boundary.rule_23_strict",
    "boundary.rule_24_strict",
    "rule.25",
    "rule.26.agent",
    "rule.26.acct",
    "rule.27.agent",
    "rule.27.acct",
    "rule.28",
)


def body_named(section: dict, name: str) -> dict:
    return section["chain"][SEQ[name] - 1]["body"]


def payload_named(section: dict, name: str) -> dict:
    return body_named(section, name)["payload"]


def artifact_named(section: dict, name: str) -> dict:
    return next(a for a in section["artifacts"] if a["name"] == name)


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""
    unhashed = {"prev_hash not chained", "mandate document edited after it was stored"}

    def mutated(name: str, fn) -> dict:
        copy_ = copy.deepcopy(section)
        fn(copy_)
        if name not in unhashed:
            for entry in copy_["chain"]:
                entry["body"]["artifact_refs"] = sorted(digest_strings(entry["body"]["payload"]))
            rechain(copy_["chain"])
        return copy_

    def unchain(s: dict) -> None:
        entry = s["chain"][SEQ["disclosure"] - 1]
        entry["body"]["prev_hash"] = "0" * 64
        entry["canonical"] = canonical_of(entry["body"])
        entry["hash"] = sha(entry["canonical"])

    def restore_placeholder_hash(s: dict) -> None:
        """The base's placeholder model hash, stored honestly under its own new ref: the document is
        no longer the one the model registration pins."""
        art = artifact_named(s, "mandate_document")
        art["object"]["behavior"]["signal_models"][0]["content_hash"] = "sha256:" + "1" * 64
        art["canonical"] = canonical_of(art["object"])
        art["ref"] = "sha256:" + sha(art["canonical"])

    def record_for_another_version(s: dict) -> None:
        """The §10 record re-stored, honestly hashed, naming another version."""
        art = artifact_named(s, "version_record")
        art["object"]["mandate_version"] = "sha256:" + "5" * 64
        art["canonical"] = canonical_of(art["object"])
        art["ref"] = "sha256:" + sha(art["canonical"])
        payload_named(s, "version_created")["record_ref"] = art["ref"]

    def swap(first: str, second: str):
        def fn(s: dict) -> None:
            a, b = SEQ[first] - 1, SEQ[second] - 1
            s["chain"][a]["body"], s["chain"][b]["body"] = s["chain"][b]["body"], s["chain"][a]["body"]
            for i in (a, b):
                s["chain"][i]["body"]["seq"] = i + 1
                s["chain"][i]["event_type"] = s["chain"][i]["body"]["event_type"]

        return fn

    def fact(s: dict, kind: str) -> dict:
        return next(f["fact"] for f in s["journaled_facts"] if f["fact"]["kind"] == kind)

    out = []
    for name, check, fn in [
        ("prev_hash not chained", "chain.prev_hash", unchain),
        (
            "mandate document edited after it was stored",
            "artifacts.rehash",
            lambda s: artifact_named(s, "mandate_document")["object"].update(name="edited"),
        ),
        ("mandate stored with the base's placeholder model hash", "document.base", restore_placeholder_hash),
        (
            "confirmation of another version",
            "version.binds",
            lambda s: payload_named(s, "confirmed").update(
                mandate_version=artifact_named(s, "fee_config")["ref"]
            ),
        ),
        ("version record for another version", "version.record", record_for_another_version),
        ("confirmed before the version is created", "order.confirmed", swap("version_created", "confirmed")),
        (
            "stopped agent is another agent",
            "order.stopped",
            lambda s: payload_named(s, "stopped").update(agent_id="agent_b"),
        ),
        ("connection revoked while its agent runs", "order.revoked", swap("stopped", "revoked")),
        (
            "confirmation of a path nothing records",
            "confirm.covers",
            lambda s: payload_named(s, "confirmed").update(
                confirmed_paths=[*sorted(TOP_LEVEL_SOURCES), "/zz"]
            ),
        ),
        (
            "connection established as live",
            "deploy.connection",
            lambda s: payload_named(s, "connection").update(environment="live"),
        ),
        (
            "model registered at another version",
            "model.pinned",
            lambda s: payload_named(s, "model_registered").update(model_version="1.0.1"),
        ),
        (
            "deployment fact reads the allocation from nowhere",
            "facts.recompute",
            lambda s: fact(s, "AgentVersionActive").update(allocation_usd="9000"),
        ),
        (
            "stop fact drops the loss",
            "facts.recompute",
            lambda s: fact(s, "AgentStopped").update(loss_added_usd="0"),
        ),
        (
            "snapshot fact on another connection",
            "facts.recompute",
            lambda s: s["derivation"]["account_connection"].update(connection_id="conn_other"),
        ),
        (
            "fee registration mapped to a model",
            "facts.recompute",
            lambda s: s["journaled_facts"].insert(
                2, {"seq": SEQ["fee_registered"], "fact": fact(s, "ModelRegistered")}
            ),
        ),
        (
            "base snapshot inconsistent",
            "drafts.valid",
            lambda s: s["drafts"]["snapshot_fees"]["payload"].update(cash_band="1"),
        ),
        (
            "invalid draft expects the wrong reason",
            "invalid_drafts",
            lambda s: s["invalid_drafts"][0]["expect"].update(reason="non_canonical"),
        ),
        (
            "valid draft that a rule refuses",
            "valid_drafts",
            lambda s: s["valid_drafts"][0]["changes"].append(change("payload.cash_in_band", False)),
        ),
        (
            "chain event breaks a rule",
            "chain.valid",
            lambda s: payload_named(s, "stopped").update(loss_added="-1"),
        ),
    ]:
        out.append((name, check, mutated(name, fn)))
    return out


def run_mutants(section: dict) -> list[str]:
    """Every seeded bug caught; a vector mutant only by the check it is registered against, with no
    other check of that check's family also catching it."""
    escaped = []
    cases = [(draft_of(e["body"]), None) for e in section["chain"]]
    cases += [(copy.deepcopy(d), None) for d in section["drafts"].values()]
    cases += [(draft_for(section, c), None) for c in section["valid_drafts"]]
    cases += [(draft_for(section, c), c["expect"]) for c in section["invalid_drafts"]]
    for mutant in VALIDATOR_MUTANTS:
        skip = frozenset([mutant])
        caught = False
        for draft, want in cases:
            got = violations(draft, skip)
            if want is None:
                caught |= bool(got)
            else:
                caught |= not reported(got, want)
        if not caught:
            escaped.append(f"control validator mutant {mutant}")
    for name, check, mutated in vector_mutants(section):
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        family = check.partition(".")[0]
        masked = sorted(c for c in caught_by if c != check and c.partition(".")[0] == family)
        if check not in caught_by:
            escaped.append(
                f"control vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})"
            )
        elif masked:
            escaped.append(f"control vector mutant: {name} (caught by {check} and also by {masked})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section(v3: dict) -> dict:
    arts = artifacts()
    chain = hash_chain(chain_bodies(arts), v3["genesis_prev_hash"])
    return {
        "spec": SPEC,
        "stream_id": STREAM,
        "genesis_prev_hash": v3["genesis_prev_hash"],
        "artifacts": [
            {"name": name, "ref": artifact_ref(obj), "object": obj, "canonical": canonical_of(obj)}
            for name, obj in arts.items()
        ],
        "chain": chain,
        "drafts": base_drafts(v3),
        "derivation": derivation(arts["mandate_document"]),
        "journaled_facts": expected_facts(arts),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
