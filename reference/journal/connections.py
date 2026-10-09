"""Journal spec v0.20 §9.8's reference vectors (DEC-800): a connection's history.

The schemas and consistency rules 54 to 65 live in `control.py`, beside §9.2's to §9.7's, so one
validator judges every closed schema. This module builds the `connections` section: a base draft of
each record, an invalid draft for every member type and rule, valid drafts for the cases a rule
might be misread to refuse, `sequences` for the stream rules 66 to 68, which need a stream's
earlier records, and `chains` for §11's `connection_cause_mismatch`, which follows a cause from one
stream to the other. It folds each sequence and chain by its own path, checks that no base draft
carries a secret-shaped member (CN-1) and that the account-stream drafts are on the stream the
establishment binds, and shows every seeded bug caught.
"""

from __future__ import annotations

import copy

from common import change, delete
from control import (
    ACCOUNT_STREAM_REF,
    EXECUTOR,
    MCP_BROKERS,
    SERVICES,
    STREAM,
    WORKSPACE,
    check_of,
    draft_for,
    reported,
    violations,
)
from control import invalid as control_invalid
from control import valid as control_valid

SPEC = "docs/specs/journal.md v0.20 §9.8 (DEC-800)"
AT = "2026-09-22T13:00:00.000000000Z"
CLOCK = "2026-09-22T13:00:00.000000000Z"
ACCOUNT_STREAM = f"acct:{WORKSPACE}:{ACCOUNT_STREAM_REF}"
OTHER_ACCOUNT_REF = "01J8Z2ACCT00000000000000B7"
CONNECTION = "conn_alpaca_paper"
OTHER_CONNECTION = "conn_alpaca_paper_2"
OWNER = "user_owner_01"
STEP_UP = {"assertion_id": "assert_owner_09", "authenticated_at": "2026-09-22T12:59:50.000000000Z", "method": "cli_confirm"}
SCOPES = ["data", "trading"]
OWNER_ACK = "01J8Z4C0A000000000000000K9"
IDS = {
    "established_v2": "01J8Z4C1A000000000000000C1",
    "refused_scope": "01J8Z4C2A000000000000000C2",
    "rotated": "01J8Z4C3A000000000000000C3",
    "checked_start": "01J8Z4C4A000000000000000C4",
    "state_degraded": "01J8Z4C5A000000000000000C5",
    "refreshed": "01J8Z4C6A000000000000000C6",
    "established_copy": "01J8Z4C7A000000000000000C7",
    "rotated_copy": "01J8Z4C8A000000000000000C8",
}
ON_ACCOUNT = ("checked_start", "state_degraded", "refreshed", "established_copy", "rotated_copy")
# Member names a credential, a broker account number, or the account fingerprint would sit in
# (CN-1, connections spec §3.1); no draft may carry one at any depth.
ACCOUNT_PII = "pii_01J8Z2PR0000000000000000A1"
SECRET_SHAPED = ("secret", "token", "password", "api_key", "key_id", "account_number", "fingerprint", "credential")


def envelope(name: str, event_type: str, payload: dict, causation=None, version: int = 1) -> dict:
    on_account = name in ON_ACCOUNT
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": IDS[name],
        "stream_id": ACCOUNT_STREAM if on_account else STREAM,
        "event_type": event_type,
        "schema_version": version,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": causation,
        "correlation_id": None,
        "actor": dict(EXECUTOR if on_account else SERVICES),
        "config_refs": {},
        "payload": payload,
        "artifact_refs": [],
        "pii_refs": [ACCOUNT_PII] if event_type == "ConnectionChecked" else [],
    }


def base_drafts() -> dict[str, dict]:
    owner = {"user": OWNER, "step_up": dict(STEP_UP)}
    passed = [
        {"check": c, "result": "passed", "reason": None}
        for c in ("account", "environment", "scope")
    ]
    established = {
        "connection_id": CONNECTION,
        "broker": "alpaca",
        "environment": "paper",
        "scopes": list(SCOPES),
        "account_ref": ACCOUNT_STREAM_REF,
        **owner,
        "margin_attestation": None,
    }
    rotated = {"connection_id": CONNECTION, "scopes": list(SCOPES), **owner}
    return {
        "established_v2": envelope(
            "established_v2", "ConnectionEstablished", established, causation=IDS["checked_start"], version=2
        ),
        "established_copy": envelope(
            "established_copy",
            "ConnectionEstablished",
            {**established, "risk_clock": CLOCK},
            causation=IDS["established_v2"],
            version=2,
        ),
        "rotated_copy": envelope(
            "rotated_copy", "ConnectionCredentialRotated", {**rotated, "risk_clock": CLOCK}, causation=IDS["rotated"]
        ),
        "refused_scope": envelope(
            "refused_scope",
            "ConnectionRefused",
            {
                "connection_id": OTHER_CONNECTION,
                "broker": "alpaca",
                "environment": "paper",
                "occasion": "connect",
                "check": "scope",
                "reason": "fund_movement",
                "existing_connection_id": None,
                **owner,
            },
        ),
        "rotated": envelope("rotated", "ConnectionCredentialRotated", rotated, causation=IDS["checked_start"]),
        "checked_start": envelope(
            "checked_start",
            "ConnectionChecked",
            {
                "connection_id": CONNECTION,
                "occasion": "connect",
                "results": passed,
                "account_pii_ref": ACCOUNT_PII,
                "risk_clock": CLOCK,
            },
        ),
        "state_degraded": envelope(
            "state_degraded",
            "ConnectionStateChanged",
            {"connection_id": CONNECTION, "from": "active", "to": "degraded", "reason": "network_errors", "risk_clock": CLOCK},
        ),
        "refreshed": envelope(
            "refreshed",
            "ConnectionCredentialRefreshed",
            {"connection_id": CONNECTION, "scopes": list(SCOPES), "risk_clock": CLOCK},
        ),
    }


BASES = base_drafts()
invalid = control_invalid
valid = control_valid

# Each member: a value of the wrong JSON kind (`schema`) and, where its type constrains a string, a
# string of the wrong form (`non_canonical`).
MEMBER_CASES = {
    "established_v2": (
        ("connection_id", 7, "conn:alpaca"),
        ("broker", 7, ""),
        ("environment", 7, "backtest"),
        ("scopes", "trading", None),
        ("account_ref", 7, "01J8Z2ACCT00000000000000AI"),
        ("user", 7, ""),
        ("step_up", "cli_confirm", None),
        ("margin_attestation", 7, "cash"),
    ),
    "established_copy": (
        ("margin_attestation", 7, "cash"),
        ("risk_clock", 1790000000, "2026-09-22T13:00:00.500000000Z"),
    ),
    "rotated_copy": (
        ("scopes", "trading", None),
        ("risk_clock", 1790000000, "2026-09-22T13:00:00.500000000Z"),
    ),
    "refused_scope": (
        ("connection_id", 7, "conn:alpaca"),
        ("broker", 7, ""),
        ("environment", 7, "backtest"),
        ("occasion", 7, "retry"),
        ("check", 7, "one_x"),
        ("reason", 7, "expired"),
        ("existing_connection_id", 7, "conn:other"),
        ("user", 7, ""),
        ("step_up", [], None),
    ),
    "rotated": (
        ("connection_id", 7, "conn:alpaca"),
        ("scopes", {}, None),
        ("user", 7, ""),
        ("step_up", "cli_confirm", None),
    ),
    "checked_start": (
        ("connection_id", 7, "conn:alpaca"),
        ("occasion", 7, "hourly"),
        ("results", {}, None),
        ("account_pii_ref", 7, "pii:acct"),
        ("risk_clock", 1790000000, "2026-09-22T13:00:00.500000000Z"),
    ),
    "state_degraded": (
        ("connection_id", 7, "conn:alpaca"),
        ("from", 7, "connecting"),
        ("to", 7, "revoked"),
        ("reason", 7, "outage"),
        ("risk_clock", 1790000000, "2026-09-22T13:00:00.500000000Z"),
    ),
    "refreshed": (
        ("connection_id", 7, "conn:alpaca"),
        ("scopes", "trading", None),
        ("risk_clock", 1790000000, "2026-09-22"),
    ),
}
NULLED = {
    "established_v2": ("account_ref", "user", "step_up"),
    "established_copy": ("account_ref", "risk_clock"),
    "rotated_copy": ("scopes", "risk_clock"),
    "refused_scope": ("connection_id", "occasion", "reason", "user", "step_up"),
    "rotated": ("connection_id", "scopes", "user", "step_up"),
    "checked_start": ("connection_id", "occasion", "results", "risk_clock"),
    "state_degraded": ("connection_id", "from", "to", "reason", "risk_clock"),
    "refreshed": ("connection_id", "scopes", "risk_clock"),
}


def member_drafts() -> list[dict]:
    out = []
    for base, cases in MEMBER_CASES.items():
        for member, wrong_kind, wrong_form in cases:
            path = f"payload.{member}"
            out.append(invalid(f"{base}.{member}.kind", "§9.8 types", base, [change(path, wrong_kind)], "schema", path))
            if wrong_form is not None:
                form = [change(path, wrong_form)]
                out.append(invalid(f"{base}.{member}.form", "§9.8 types", base, form, "non_canonical", path))
        for member in NULLED[base]:
            path = f"payload.{member}"
            out.append(invalid(f"{base}.{member}.null", "§9.8 types", base, [change(path, None)], "schema", path))
        last = MEMBER_CASES[base][-1][0]
        out.append(invalid(f"{base}.missing", "§9.8 closed", base, [delete(f"payload.{last}")], "schema", f"payload.{last}"))
        out.append(invalid(f"{base}.extra", "§9.8 closed", base, [change("payload.note", "x")], "schema", "payload.note"))
    return out


def result(check: str, outcome: str = "passed", reason=None) -> dict:
    return {"check": check, "result": outcome, "reason": reason}


def results(*extra: str, **failed: str) -> list[dict]:
    """Every check the executor always runs, plus `extra`, in byte order; `failed` maps a check to
    the reason it failed with."""
    checks = sorted({"account", "environment", "scope", *extra})
    return [result(c, "failed", failed[c]) if c in failed else result(c) for c in checks]


def state(frm: str, to: str, reason: str) -> list[dict]:
    return [change("payload.from", frm), change("payload.to", to), change("payload.reason", reason)]


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule; together they cover every §9.8 member type and rule."""
    est, ref, rot, chk, st, fresh = (
        "established_v2",
        "refused_scope",
        "rotated",
        "checked_start",
        "state_degraded",
        "refreshed",
    )
    return [
        *member_drafts(),
        invalid(
            "established_v1_with_account_ref",
            "§9.8: version 1 stays closed with its four members",
            est,
            [change("schema_version", 1), delete("payload.user"), delete("payload.step_up"), delete("payload.margin_attestation")],
            "schema",
            "payload.account_ref",
        ),
        invalid("established_version_3", "§9.8: versions 1 and 2 only", est, [change("schema_version", 3)], "unknown_schema", "payload"),
        invalid(
            "established_carries_a_secret",
            "§9.8, CN-1: a credential has no member to sit in",
            est,
            [change("payload.api_secret", "canary-secret")],
            "schema",
            "payload.api_secret",
        ),
        invalid(
            "refused_carries_the_fingerprint",
            "§9.8, connections spec §3: the fingerprint is never journaled",
            ref,
            [change("payload.account_fingerprint", "hmac:" + "f" * 64)],
            "schema",
            "payload.account_fingerprint",
        ),
        invalid(
            "established_step_up_extra",
            "§9.8: the step-up evidence is closed",
            est,
            [change("payload.step_up.token", "x")],
            "schema",
            "payload.step_up.token",
        ),
        invalid(
            "established_scopes_unsorted",
            "rule 19 applies to version 2",
            est,
            [change("payload.scopes", ["trading", "data"])],
            "non_canonical",
            "payload.scopes",
        ),
        invalid("refused_on_the_account_stream", "§9.8: a control-stream record", ref, [change("stream_id", ACCOUNT_STREAM)], "wrong_stream", "event_type"),
        invalid("state_on_the_control_stream", "§9.8: an account-stream record", st, [change("stream_id", STREAM)], "wrong_stream", "event_type"),
        invalid(
            "refused_reason_of_another_check",
            "rule 54",
            ref,
            [change("payload.reason", "reaches_both")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "uniqueness_without_the_existing_connection",
            "rule 55",
            ref,
            [change("payload.check", "uniqueness"), change("payload.reason", "already_connected")],
            "schema",
            "payload.existing_connection_id",
        ),
        invalid(
            "uniqueness_naming_itself",
            "rule 55: the existing connection is another",
            ref,
            [
                change("payload.check", "uniqueness"),
                change("payload.reason", "already_connected"),
                change("payload.existing_connection_id", OTHER_CONNECTION),
            ],
            "schema",
            "payload.existing_connection_id",
        ),
        invalid(
            "scope_refusal_naming_a_connection",
            "rule 55",
            ref,
            [change("payload.existing_connection_id", CONNECTION)],
            "schema",
            "payload.existing_connection_id",
        ),
        invalid("rotated_scopes_unsorted", "rule 56", rot, [change("payload.scopes", ["trading", "data"])], "non_canonical", "payload.scopes"),
        invalid("refreshed_scopes_repeated", "rule 56", fresh, [change("payload.scopes", ["data", "data"])], "non_canonical", "payload.scopes"),
        invalid(
            "checked_out_of_order",
            "rule 57: strictly ascending",
            chk,
            [change("payload.results", [result("environment"), result("account"), result("scope")])],
            "non_canonical",
            "payload.results",
        ),
        invalid(
            "checked_twice",
            "rule 57: no check listed twice",
            chk,
            [change("payload.results", [result("account"), *results()])],
            "non_canonical",
            "payload.results",
        ),
        invalid(
            "checked_without_scope",
            "rule 57: scope, environment, and account are always run",
            chk,
            [change("payload.results", results("contract")[:-1])],
            "schema",
            "payload.results",
        ),
        invalid(
            "checked_uniqueness",
            "§9.8: uniqueness is a connect-time check",
            chk,
            [change("payload.results", [*results(), result("uniqueness")])],
            "non_canonical",
            "payload.results[3].check",
        ),
        invalid(
            "failed_without_a_reason",
            "rule 58",
            chk,
            [change("payload.results", [result("account"), result("environment", "failed"), result("scope")])],
            "schema",
            "payload.results[1].reason",
        ),
        invalid(
            "passed_with_a_reason",
            "rule 58",
            chk,
            [change("payload.results", [*results()[:2], result("scope", "passed", "fund_movement")])],
            "schema",
            "payload.results[2].reason",
        ),
        invalid(
            "failed_with_another_checks_reason",
            "rule 58: the reason belongs to its check",
            chk,
            [change("payload.results", [result("account", "failed", "tools_missing"), *results()[1:]])],
            "schema",
            "payload.results[0].reason",
        ),
        invalid("degraded_by_a_credential", "rule 59", st, state("active", "degraded", "authorization_failed"), "schema", "payload.reason"),
        invalid("suspended_by_the_network", "rule 59", st, state("active", "suspended", "network_errors"), "schema", "payload.reason"),
        invalid("cleared_into_another_state", "rule 59", st, state("degraded", "active", "condition_cleared"), "schema", "payload.reason"),
        invalid("cleared_while_active", "rule 59", st, state("active", "active", "condition_cleared"), "schema", "payload.reason"),
        invalid(
            "acknowledged_into_degraded",
            "rule 59",
            st,
            [*state("suspended", "degraded", "acknowledged"), change("causation_id", OWNER_ACK)],
            "schema",
            "payload.reason",
        ),
        invalid(
            "active_from_active",
            "rule 60",
            st,
            [*state("active", "active", "acknowledged"), change("causation_id", OWNER_ACK)],
            "schema",
            "payload.from",
        ),
        invalid("degraded_from_suspended", "rule 60", st, state("suspended", "degraded", "contract_drift"), "schema", "payload.from"),
        invalid("acknowledged_without_the_owner", "rule 61", st, state("degraded", "active", "acknowledged"), "schema", "causation_id"),
        invalid(
            "account_read_without_its_reference",
            "rule 62: the account was read, so its personal-data reference is recorded",
            chk,
            [change("payload.account_pii_ref", None), change("pii_refs", [])],
            "schema",
            "payload.account_pii_ref",
        ),
        invalid(
            "unreadable_account_with_a_reference",
            "rule 62: an account that could not be read has no reference",
            chk,
            [change("payload.results", results(account="account_unreadable"))],
            "schema",
            "payload.account_pii_ref",
        ),
        invalid(
            "reference_not_in_pii_refs",
            "rule 62: the reference is listed in the envelope's pii_refs (§3)",
            chk,
            [change("pii_refs", [])],
            "schema",
            "payload.account_pii_ref",
        ),
        invalid(
            "refused_for_one_x",
            "§9.8: checks 5 and 6 come from AccountStateObserved and never refuse a connection",
            ref,
            [change("payload.check", "one_x")],
            "non_canonical",
            "payload.check",
        ),
        invalid(
            "established_without_its_check",
            "rule 63: names the passing ConnectionChecked",
            est,
            [change("causation_id", None)],
            "schema",
            "causation_id",
        ),
        invalid(
            "rotated_without_its_check",
            "rule 63: a replaced credential passed its checks first",
            rot,
            [change("causation_id", None)],
            "schema",
            "causation_id",
        ),
        invalid(
            "establishment_copied_without_its_original",
            "rule 63: the account stream's copy names the control stream's original",
            "established_copy",
            [change("causation_id", None)],
            "schema",
            "causation_id",
        ),
        invalid(
            "rotation_copied_without_its_original",
            "rule 63",
            "rotated_copy",
            [change("causation_id", None)],
            "schema",
            "causation_id",
        ),
        invalid(
            "establishment_copied_at_version_1",
            "§9.8: only version 2 names an account stream to copy into",
            "established_copy",
            [change("schema_version", 1)],
            "unknown_schema",
            "payload",
        ),
        invalid(
            "account_number_as_its_reference",
            "§9.8: account_pii_ref is a personal-data vault reference, pii_ and a ULID (§6.4)",
            chk,
            [change("payload.account_pii_ref", "4111111111111111"), change("pii_refs", ["4111111111111111"])],
            "non_canonical",
            "payload.account_pii_ref",
        ),
        invalid(
            "reference_without_a_ulid",
            "§9.8: pii_ and a ULID",
            chk,
            [change("payload.account_pii_ref", "pii_acct_7Q2M"), change("pii_refs", ["pii_acct_7Q2M"])],
            "non_canonical",
            "payload.account_pii_ref",
        ),
        invalid(
            "executor_reports_another_account",
            "rule 58: the fingerprint comparison is the control services', refused on the control stream",
            chk,
            [change("payload.results", results(account="account_mismatch"))],
            "schema",
            "payload.results[0].reason",
        ),
        invalid(
            "teardown_naming_a_check",
            "rule 54: a teardown names no check",
            ref,
            [change("payload.reason", "timeout")],
            "schema",
            "payload.reason",
        ),
        invalid(
            "no_check_but_a_checks_reason",
            "rule 54: with no check, the reason is a teardown's",
            ref,
            [change("payload.check", None)],
            "schema",
            "payload.reason",
        ),
        invalid(
            "teardown_with_a_cause",
            "rule 65: no check failed, so nothing caused it",
            ref,
            [change("payload.check", None), change("payload.reason", "timeout"), change("causation_id", IDS["checked_start"])],
            "schema",
            "causation_id",
        ),
        invalid(
            "live_without_the_attestation",
            "rule 64: a live connection carries the owner's no-margin attestation (DEC-529 item 11)",
            est,
            [change("payload.environment", "live"), change("environment", "live")],
            "schema",
            "payload.margin_attestation",
        ),
        invalid(
            "paper_with_an_attestation",
            "rule 64: only a live connection carries it",
            est,
            [change("payload.margin_attestation", "cash_account")],
            "schema",
            "payload.margin_attestation",
        ),
        invalid(
            "copy_live_without_the_attestation",
            "rule 64 holds on the copy",
            "established_copy",
            [change("payload.environment", "live"), change("environment", "live")],
            "schema",
            "payload.margin_attestation",
        ),
    ]


def valid_drafts() -> list[dict]:
    """Cases a rule might be misread to refuse; each must be accepted."""
    ref, chk, st = "refused_scope", "checked_start", "state_degraded"
    return [
        valid(
            "established_version_1",
            "§9.8: version 1 stays registered",
            "established_v2",
            [
                change("schema_version", 1),
                delete("payload.account_ref"),
                delete("payload.user"),
                delete("payload.step_up"),
                delete("payload.margin_attestation"),
                change("causation_id", None),
            ],
        ),
        valid(
            "uniqueness_names_the_holder",
            "rule 55",
            ref,
            [
                change("payload.check", "uniqueness"),
                change("payload.reason", "already_connected"),
                change("payload.existing_connection_id", CONNECTION),
            ],
        ),
        valid(
            "reaches_both_refused_live",
            "rule 54: DEC-441 item 21 refuses it for either environment",
            ref,
            [
                change("payload.environment", "live"),
                change("payload.check", "environment"),
                change("payload.reason", "reaches_both"),
                change("environment", "live"),
            ],
        ),
        valid(
            "checked_daily_with_contract_drift",
            "rules 57 and 58: contract is listed for an MCP connection",
            chk,
            [
                change("payload.occasion", "daily"),
                change(
                    "payload.results",
                    results("contract", contract="contract_drift"),
                ),
            ],
        ),
        valid(
            "checked_daily_with_tools_missing",
            "rules 57 and 58: a later contract check finds an allowlisted tool absent (DEC-674)",
            chk,
            [
                change("payload.occasion", "daily"),
                change("payload.results", results("contract", contract="tools_missing")),
            ],
        ),
        valid(
            "unreadable_account_without_a_reference",
            "rule 62",
            chk,
            [
                change("payload.results", results(account="account_unreadable")),
                change("payload.account_pii_ref", None),
                change("pii_refs", []),
            ],
        ),
        valid(
            "checked_for_a_reauthorization",
            "§9.8: the executor checks a replaced credential before it is accepted",
            chk,
            [change("payload.occasion", "reauthorize")],
        ),
        valid(
            "teardown_after_the_deadline",
            "rules 54 and 65: connections spec §5.2 step 6",
            ref,
            [change("payload.check", None), change("payload.reason", "timeout")],
        ),
        valid(
            "teardown_on_restart",
            "rule 54",
            ref,
            [change("payload.check", None), change("payload.reason", "restart_past_deadline"), change("payload.occasion", "reauthorize")],
        ),
        valid(
            "teardown_when_the_executor_never_started",
            "rule 54: connections spec §5.2 step 3",
            ref,
            [change("payload.check", None), change("payload.reason", "start_failed")],
        ),
        valid(
            "control_services_find_another_account",
            "rule 54: account_mismatch is the control services' refusal",
            ref,
            [change("payload.check", "account"), change("payload.reason", "account_mismatch")],
        ),
        valid(
            "live_robinhood_attested",
            "rule 64",
            "established_v2",
            [
                change("payload.broker", "robinhood"),
                change("payload.environment", "live"),
                change("environment", "live"),
                change("payload.margin_attestation", "margin_disabled"),
            ],
        ),
        valid("degraded_again", "rules 59 and 60: a cause returns before the acknowledgment", st, state("degraded", "degraded", "rate_headroom")),
        valid("suspended_while_degraded", "rule 60", st, state("degraded", "suspended", "authorization_failed")),
        valid("suspended_again", "rule 60", st, state("suspended", "suspended", "lease_expired")),
        valid(
            "suspended_on_a_failed_check",
            "rule 59: a later tools_missing suspends with check_failed (DEC-674)",
            st,
            state("active", "suspended", "check_failed"),
        ),
        valid("suspension_cleared", "rule 59: the state stays", st, state("suspended", "suspended", "condition_cleared")),
        valid(
            "acknowledged_back_to_active",
            "rules 59 to 61",
            st,
            [*state("suspended", "active", "acknowledged"), change("causation_id", OWNER_ACK)],
        ),
    ]


# --------------------------------------------------------------------------- stream rules 66 to 68


def record_id(index: int) -> str:
    return f"01J8Z4S{index:02d}A0000000000000000"[:26]


def record(base: str, index: int, *changes: dict) -> dict:
    """One record of a sequence: a base draft, its own event id, then the changes."""
    return {"base_draft": base, "changes": [change("event_id", record_id(index)), *changes]}


def revoked(index: int, connection: str = CONNECTION) -> dict:
    return record(
        "rotated",
        index,
        change("event_type", "ConnectionRevoked"),
        change("payload", {"connection_id": connection}),
    )


def refused(index: int, occasion: str, connection: str = CONNECTION, *more: dict) -> dict:
    return record("refused_scope", index, change("payload.occasion", occasion), change("payload.connection_id", connection), *more)


def established(index: int, *more: dict) -> dict:
    return record("established_v2", index, *more)


def moved(index: int, frm: str, to: str, reason: str, cause: str | None = None) -> dict:
    if reason == "acknowledged":
        cause = OWNER_ACK
    return record("state_degraded", index, *state(frm, to, reason), change("causation_id", cause))


def checked(index: int, occasion: str = "reauthorize", *listed_results: dict) -> dict:
    listed = list(listed_results) or results()
    return record("checked_start", index, change("payload.occasion", occasion), change("payload.results", listed))


def bound(index: int, cause: str | None = None, *more: dict) -> dict:
    """The executor's copy of the establishment, which binds the account stream (rule 68)."""
    return record("established_copy", index, change("causation_id", cause or IDS["established_v2"]), *more)


def rotation_copy(index: int, *more: dict) -> dict:
    return record("rotated_copy", index, *more)


# A passing connect check and the copy of the establishment it caused: the stream is bound.
BOUND = (checked(1, "connect"), bound(2))


def sequence(name: str, clause: str, records: list[dict], mismatch: tuple[int, str] | None = None) -> dict:
    if mismatch is None:
        expect = {"outcome": "Valid"}
    else:
        expect = {"outcome": "Mismatch", "code": "connection_lifecycle_mismatch", "index": mismatch[0], "rule": mismatch[1]}
    return {"name": name, "clause": clause, "records": records, "expect": expect}


def sequences() -> list[dict]:
    other_ref = change("payload.account_ref", OTHER_ACCOUNT_REF)
    v1 = [
        change("schema_version", 1),
        delete("payload.account_ref"),
        delete("payload.user"),
        delete("payload.step_up"),
        delete("payload.margin_attestation"),
    ]
    mcp = [change("payload.broker", "robinhood"), change("payload.environment", "live"), change("environment", "live"),
           change("payload.margin_attestation", "cash_account")]
    narrower = change("payload.scopes", ["trading"])
    suspended = [*BOUND, moved(3, "active", "suspended", "authorization_failed")]
    return [
        sequence(
            "connect_revoke_reconnect",
            "rule 66: a reconnect after its revocation, same broker, environment, and account_ref",
            [established(1), record("rotated", 2), revoked(3), established(4)],
        ),
        sequence(
            "every_occasion_in_its_place",
            "rule 67",
            [
                refused(1, "connect", OTHER_CONNECTION),
                established(2),
                refused(3, "reauthorize"),
                record("rotated", 4),
                revoked(5),
                refused(6, "reconnect"),
                established(7),
            ],
        ),
        sequence(
            "revocation_is_never_refused",
            "§9.8: no rule refuses a ConnectionRevoked",
            [revoked(1), established(2), revoked(3), revoked(4), established(5)],
        ),
        sequence(
            "a_refused_id_connects_later",
            "rule 67: a refused connect leaves no connection",
            [refused(1, "connect"), established(2)],
        ),
        sequence(
            "rotations_narrow",
            "rule 67: a rotation keeps or narrows the scopes",
            [established(1), record("rotated", 2, narrower), record("rotated", 3, narrower)],
        ),
        sequence("established_twice", "rule 66: not revoked", [established(1), established(2)], (1, "66")),
        sequence("reconnect_another_account", "rule 66", [established(1), revoked(2), established(3, other_ref)], (2, "66")),
        sequence(
            "reconnect_another_environment",
            "rule 66",
            [established(1), revoked(2), established(3, *mcp[1:]), ],
            (2, "66"),
        ),
        sequence("reconnect_another_broker", "rule 66", [established(1), revoked(2), established(3, change("payload.broker", "kraken_derivatives_us"))], (2, "66")),
        sequence("version_1_never_reestablished", "rule 66", [established(1, *v1), revoked(2), established(3)], (2, "66")),
        sequence(
            "two_connections_one_account",
            "rule 66: one account_ref, one connection (CN-5)",
            [established(1), established(2, change("payload.connection_id", OTHER_CONNECTION))],
            (1, "66"),
        ),
        sequence("established_again_after_reconnect", "rule 66", [established(1), revoked(2), established(3), established(4)], (3, "66")),
        sequence("rotated_never_established", "rule 67", [record("rotated", 1)], (0, "67")),
        sequence("rotated_after_revocation", "rule 67", [established(1), revoked(2), record("rotated", 3)], (2, "67")),
        sequence(
            "rotation_widens_the_scopes",
            "rule 67: a rotation never adds a scope",
            [established(1), record("rotated", 2, narrower), record("rotated", 3)],
            (2, "67"),
        ),
        sequence("reauthorize_after_revocation", "rule 67", [established(1), revoked(2), refused(3, "reauthorize")], (2, "67")),
        sequence("reconnect_while_connected", "rule 67", [established(1), refused(2, "reconnect")], (1, "67")),
        sequence("connect_an_established_id", "rule 67", [established(1), revoked(2), refused(3, "connect")], (2, "67")),
        sequence(
            "reconnect_refused_for_another_environment",
            "rule 67",
            [
                established(1),
                revoked(2),
                refused(3, "reconnect", CONNECTION, change("payload.environment", "live"), change("environment", "live")),
            ],
            (2, "67"),
        ),
        sequence(
            "degraded_cleared_acknowledged_then_suspended_and_reauthorized",
            "rule 68: the B1 path: a reauthorize after the suspension, accepted by a rotation, then cleared",
            [
                *BOUND,
                moved(3, "active", "degraded", "network_errors"),
                moved(4, "degraded", "degraded", "condition_cleared"),
                moved(5, "degraded", "active", "acknowledged"),
                moved(6, "active", "suspended", "credential_expired"),
                moved(7, "suspended", "suspended", "refresh_failed"),
                checked(8),
                rotation_copy(9),
                moved(10, "suspended", "suspended", "condition_cleared", record_id(9)),
                moved(11, "suspended", "active", "acknowledged"),
            ],
        ),
        sequence(
            "executor_start_after_binding",
            "rule 68: checks after the binding",
            [*BOUND, checked(3, "executor_start"), checked(4, "daily")],
        ),
        sequence(
            "mcp_lists_its_contract",
            "rule 68: an MCP connection's checks list check 7",
            [checked(1, "connect", *results("contract")), bound(2, None, *mcp), checked(3, "daily", *results("contract"))],
        ),
        sequence(
            "mcp_later_tools_missing_suspends",
            "rules 59, 60, and 68: a later contract check failing with tools_missing suspends with check_failed (DEC-674)",
            [
                checked(1, "connect", *results("contract")),
                bound(2, None, *mcp),
                checked(3, "daily", *results("contract", contract="tools_missing")),
                moved(4, "active", "suspended", "check_failed"),
            ],
        ),
        sequence(
            "state_on_an_unbound_stream",
            "rule 68: the stream is connecting until its establishment is copied",
            [checked(1, "connect"), moved(2, "active", "degraded", "network_errors")],
            (1, "68"),
        ),
        sequence(
            "refreshed_on_an_unbound_stream",
            "rule 68",
            [record("refreshed", 1)],
            (0, "68"),
        ),
        sequence(
            "daily_check_on_an_unbound_stream",
            "rule 68: an executor starts only on a bound stream",
            [checked(1, "daily")],
            (0, "68"),
        ),
        sequence(
            "bound_without_a_passing_connect_check",
            "rule 68: the copy follows the connect sequence's passing check",
            [checked(1, "connect", *results(scope="scope_mismatch")), bound(2)],
            (1, "68"),
        ),
        sequence(
            "bound_to_another_account",
            "rule 68: the copy names this stream's account_ref",
            [checked(1, "connect"), bound(2, None, other_ref)],
            (1, "68"),
        ),
        sequence(
            "mcp_bound_without_its_contract",
            "rule 68: an MCP connection's connect check lists check 7",
            [checked(1, "connect"), bound(2, None, *mcp)],
            (1, "68"),
        ),
        sequence(
            "mcp_daily_check_without_its_contract",
            "rule 68",
            [checked(1, "connect", *results("contract")), bound(2, None, *mcp), checked(3, "daily")],
            (2, "68"),
        ),
        sequence(
            "rotation_copied_without_a_passing_reauthorize",
            "rule 68: a rotation follows a passing reauthorize check",
            [*BOUND, checked(3, "reauthorize", *results(scope="scope_mismatch")), rotation_copy(4)],
            (3, "68"),
        ),
        sequence(
            "suspension_cleared_without_a_rotation",
            "rule 68: the clearing cause is the copied rotation",
            [*suspended, moved(4, "suspended", "suspended", "condition_cleared")],
            (3, "68"),
        ),
        sequence(
            "refused_reauth_check_clears",
            "rule 68: a passing reauthorize the control services refused never clears (no rotation names it)",
            [*suspended, checked(4), moved(5, "suspended", "suspended", "condition_cleared", record_id(4))],
            (4, "68"),
        ),
        sequence(
            "stale_check_clears",
            "rule 68: the reauthorize came after the suspension",
            [*BOUND, checked(3), moved(4, "active", "suspended", "authorization_failed"), rotation_copy(5),
             moved(6, "suspended", "suspended", "condition_cleared", record_id(5))],
            (5, "68"),
        ),
        sequence(
            "stale_rotation_clears",
            "rule 68: the rotation came after the suspension",
            [*BOUND, checked(3), rotation_copy(4), moved(5, "active", "suspended", "authorization_failed"),
             moved(6, "suspended", "suspended", "condition_cleared", record_id(4))],
            (5, "68"),
        ),
        sequence(
            "two_connections_on_one_account_stream",
            "rule 68: one account stream, one connection (CN-5)",
            [*BOUND, moved(3, "active", "degraded", "network_errors"), record("refreshed", 4, change("payload.connection_id", OTHER_CONNECTION))],
            (3, "68"),
        ),
        sequence("first_change_not_from_active", "rule 68", [*BOUND, moved(3, "degraded", "suspended", "check_failed")], (2, "68")),
        sequence(
            "from_not_the_last_state",
            "rule 68",
            [*BOUND, moved(3, "active", "suspended", "check_failed"), moved(4, "degraded", "degraded", "contract_drift")],
            (3, "68"),
        ),
        sequence(
            "acknowledged_before_cleared",
            "rule 68: the acknowledgment never lifts a cause that has not cleared",
            [*BOUND, moved(3, "active", "degraded", "contract_drift"), moved(4, "degraded", "active", "acknowledged")],
            (3, "68"),
        ),
        sequence(
            "acknowledged_after_the_cause_returned",
            "rule 68",
            [
                *BOUND,
                moved(3, "active", "degraded", "network_errors"),
                moved(4, "degraded", "degraded", "condition_cleared"),
                moved(5, "degraded", "degraded", "network_errors"),
                moved(6, "degraded", "active", "acknowledged"),
            ],
            (5, "68"),
        ),
    ]


def sequence_drafts(section: dict, case: dict) -> list[dict]:
    return [draft_for(section, r) for r in case["records"]]


def listed(p: dict, check: str) -> bool:
    return check in {r["check"] for r in p["results"]}


def all_passed(p: dict, mcp: bool) -> bool:
    listed = {r["check"] for r in p["results"]}
    return all(r["result"] == "passed" for r in p["results"]) and (not mcp or "contract" in listed)


def stream_mismatch(drafts: list[dict], skip: frozenset[str] = frozenset()) -> tuple[int, str] | None:
    """The first record that breaks rule 66, 68, or 69, folding each stream in order."""
    first: dict[str, dict] = {}
    latest: dict[str, str] = {}
    scopes: dict[str, list[str]] = {}
    holder_of: dict[str, str] = {}
    current, cleared, owner = "active", False, None
    binding: dict | None = None
    last_check: dict[str, tuple[int, bool]] = {}
    rotations: dict[str, int] = {}
    suspended_at = -1
    for i, d in enumerate(drafts):
        p, kind = d["payload"], d["event_type"]
        cid = p.get("connection_id")
        if d["stream_id"].startswith("ctl:"):
            if kind == "ConnectionEstablished":
                prior = first.get(cid)
                if prior is not None:
                    same = all(
                        p[m] == prior[m] or f"stream.66.{m}" in skip for m in ("broker", "environment")
                    ) and (p.get("account_ref") == prior.get("account_ref") or "stream.66.account_ref" in skip)
                    if (latest[cid] != "ConnectionRevoked" and "stream.66.revoked" not in skip) or not same:
                        return i, "66"
                holder = holder_of.get(p.get("account_ref"))
                if holder is not None and holder != cid and "stream.66.unique" not in skip:
                    return i, "66"
                first.setdefault(cid, p)
                if p.get("account_ref") is not None:
                    holder_of.setdefault(p["account_ref"], cid)
                latest[cid] = kind
                scopes[cid] = p["scopes"]
            elif kind == "ConnectionRevoked":
                latest[cid] = kind
            elif kind == "ConnectionCredentialRotated":
                if latest.get(cid) != "ConnectionEstablished" and "stream.67.rotated" not in skip:
                    return i, "67"
                if not set(p["scopes"]) <= set(scopes.get(cid, ())) and "stream.67.narrow" not in skip:
                    return i, "67"
                scopes[cid] = p["scopes"]
            elif kind == "ConnectionRefused":
                want = {"connect": None, "reconnect": "ConnectionRevoked", "reauthorize": "ConnectionEstablished"}
                if latest.get(cid) != want[p["occasion"]] and f"stream.67.{p['occasion']}" not in skip:
                    return i, "67"
                prior = first.get(cid)
                if prior is not None and p["occasion"] != "connect" and "stream.67.same" not in skip:
                    if p["broker"] != prior["broker"] or p["environment"] != prior["environment"]:
                        return i, "67"
            continue
        owner = cid if owner is None else owner
        if cid != owner and "stream.68.one_connection" not in skip:
            return i, "68"
        mcp = binding is not None and binding["broker"] in MCP_BROKERS
        if kind == "ConnectionChecked":
            if p["occasion"] in ("executor_start", "daily") and binding is None and "stream.68.bound" not in skip:
                return i, "68"
            if mcp and "contract" not in {r["check"] for r in p["results"]} and "stream.68.mcp" not in skip:
                return i, "68"
            last_check[p["occasion"]] = (i, all_passed(p, False))
            last_check[f"{p['occasion']}.listed"] = (i, "contract" in {r["check"] for r in p["results"]})
            continue
        if kind == "ConnectionEstablished":
            stream_ref = d["stream_id"].split(":")[2]
            if p["account_ref"] != stream_ref and "stream.68.account_ref" not in skip:
                return i, "68"
            occasion = "reconnect" if binding is not None else "connect"
            at, passed = last_check.get(occasion, (-1, False))
            if not passed and "stream.68.checked" not in skip:
                return i, "68"
            listed = last_check.get(f"{occasion}.listed", (-1, False))[1]
            if p["broker"] in MCP_BROKERS and not listed and "stream.68.mcp" not in skip:
                return i, "68"
            binding = p
            continue
        if binding is None and "stream.68.bound" not in skip:
            return i, "68"
        if kind == "ConnectionCredentialRotated":
            at, passed = last_check.get("reauthorize", (-1, False))
            listed = last_check.get("reauthorize.listed", (-1, False))[1]
            if (not passed or (mcp and not listed)) and "stream.68.reauthorized" not in skip:
                return i, "68"
            rotations[d["event_id"]] = at
            continue
        if kind != "ConnectionStateChanged":
            continue
        if p["from"] != current and "stream.68.from" not in skip:
            return i, "68"
        if p["reason"] == "acknowledged" and not cleared and "stream.68.cleared" not in skip:
            return i, "68"
        if p["reason"] == "condition_cleared" and p["from"] == "suspended":
            check_at = rotations.get(d["causation_id"])
            if check_at is None and "stream.68.rotation" not in skip:
                return i, "68"
            if check_at is not None and check_at < suspended_at and "stream.68.after" not in skip:
                return i, "68"
        if p["to"] == "suspended" and p["from"] != "suspended":
            suspended_at = i
        cleared = p["reason"] == "condition_cleared"
        current = p["to"]
    return None


# --------------------------------------------------------------------------- §11's cause chain


def chain(name: str, clause: str, records: list[dict], mismatch: int | None = None) -> dict:
    if mismatch is None:
        expect = {"outcome": "Valid"}
    else:
        expect = {"outcome": "Mismatch", "code": "connection_cause_mismatch", "index": mismatch}
    return {"name": name, "clause": clause, "records": records, "expect": expect}


def with_changes(base: dict, *more: dict) -> dict:
    return {**base, "changes": [*base["changes"], *more]}


def chains() -> list[dict]:
    """Both streams in one list, in commit order. A control-stream record names its cause by event
    id; the copies on the account stream name their original."""
    check_id = record_id(1)
    est = record("established_v2", 2, change("causation_id", check_id))
    copy_est = bound(3, record_id(2))
    reauth = checked(4)
    rot = record("rotated", 5, change("causation_id", record_id(4)))
    copy_rot = rotation_copy(6, change("causation_id", record_id(5)))
    good = [checked(1, "connect"), est, copy_est, reauth, rot, copy_rot]
    mcp = [change("payload.broker", "robinhood"), change("payload.environment", "live"), change("environment", "live"),
           change("payload.margin_attestation", "cash_account")]

    def swap(index: int, replacement: dict) -> list[dict]:
        return [replacement if i == index else r for i, r in enumerate(good)]

    return [
        chain("connect_then_reauthorize", "§11: every cause on its stream, for its connection, passing", good),
        chain(
            "established_on_a_failed_check",
            "§11: the establishment's check passed every result",
            swap(0, checked(1, "connect", *results(scope="scope_mismatch"))),
            1,
        ),
        chain("established_on_a_daily_check", "§11: occasion connect", swap(0, checked(1, "daily")), 1),
        chain(
            "established_on_another_connections_check",
            "§11: the same connection_id",
            swap(0, with_changes(checked(1, "connect"), change("payload.connection_id", OTHER_CONNECTION))),
            1,
        ),
        chain(
            "established_on_another_accounts_check",
            "§11: the check is on the stream account_ref names",
            swap(0, with_changes(checked(1, "connect"), change("stream_id", f"acct:{WORKSPACE}:{OTHER_ACCOUNT_REF}"))),
            1,
        ),
        chain(
            "mcp_established_without_its_contract",
            "§11: check 7 for an MCP connection",
            [checked(1, "connect"), record("established_v2", 2, change("causation_id", check_id), *mcp)],
            1,
        ),
        chain("rotated_on_a_connect_check", "§11: occasion reauthorize", swap(4, record("rotated", 5, change("causation_id", check_id))), 4),
        chain(
            "copy_differs_from_its_original",
            "§11: a copy carries its original's members",
            swap(2, bound(3, record_id(2), change("payload.scopes", ["trading"]))),
            2,
        ),
        chain("copy_of_nothing", "§11: a copy names its original", swap(5, rotation_copy(6, change("causation_id", record_id(4)))), 5),
    ]


def chain_mismatch(drafts: list[dict], skip: frozenset[str] = frozenset()) -> int | None:
    """§11's `connection_cause_mismatch`: the first record whose cause on the other stream is not
    the one §9.8 names."""
    by_id = {d["event_id"]: d for d in drafts}
    established_ids: set[str] = set()
    refs: dict[str, str] = {}
    for i, d in enumerate(drafts):
        p, kind = d["payload"], d["event_type"]
        cause = by_id.get(d["causation_id"]) if d["causation_id"] else None
        if d["stream_id"].startswith("ctl:") and kind in ("ConnectionEstablished", "ConnectionCredentialRotated"):
            if kind == "ConnectionEstablished" and d["schema_version"] != 2:
                continue
            cid = p["connection_id"]
            if kind == "ConnectionEstablished":
                refs[cid] = p["account_ref"]
                occasion = "reconnect" if cid in established_ids else "connect"
                established_ids.add(cid)
                broker = p["broker"]
            else:
                occasion = "reauthorize"
                broker = next((e["payload"]["broker"] for e in drafts if e["event_type"] == "ConnectionEstablished"
                               and e["payload"]["connection_id"] == cid), None)
            ok = (
                cause is not None
                and cause["event_type"] == "ConnectionChecked"
                and (cause["stream_id"] == f"acct:{WORKSPACE}:{refs.get(cid)}" or "chain.stream" in skip)
                and (cause["payload"]["connection_id"] == cid or "chain.connection" in skip)
                and (cause["payload"]["occasion"] == occasion or "chain.occasion" in skip)
                and (all_passed(cause["payload"], False) or "chain.passed" in skip)
                and (broker not in MCP_BROKERS or listed(cause["payload"], "contract") or "chain.mcp" in skip)
            )
            if not ok:
                return i
        if d["stream_id"].startswith("acct:") and kind in ("ConnectionEstablished", "ConnectionCredentialRotated"):
            original = {k: v for k, v in p.items() if k != "risk_clock"}
            ok = cause is not None and cause["stream_id"].startswith("ctl:") and cause["event_type"] == kind
            if ok and cause["payload"] != original and "chain.copy" not in skip:
                ok = False
            if not ok and "chain.original" not in skip:
                return i
    return None


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = (
    "drafts.valid",
    "drafts.secret_free",
    "drafts.binding",
    "invalid_drafts",
    "valid_drafts",
    "sequences.drafts",
    "sequences",
    "chains.drafts",
    "chains",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def member_names(value) -> list[str]:
    if isinstance(value, dict):
        return [k for k, v in value.items() for k in (k, *member_names(v))]
    if isinstance(value, list):
        return [k for item in value for k in member_names(item)]
    return []


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, draft in section["drafts"].items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
        shaped = [m for m in member_names(draft["payload"]) if any(s in m for s in SECRET_SHAPED)]
        if shaped:
            problems.append(found("drafts.secret_free", f"base draft {name} carries {shaped}"))
    binding = section["drafts"]["established_v2"]["payload"]["account_ref"]
    for name, draft in section["drafts"].items():
        if draft["stream_id"].startswith("acct:") and draft["stream_id"] != f"acct:{WORKSPACE}:{binding}":
            problems.append(found("drafts.binding", f"{name} is not on the stream the establishment binds"))
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case))
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        got = violations(draft_for(section, case))
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
    for case in section["sequences"]:
        drafts = sequence_drafts(section, case)
        for i, d in enumerate(drafts):
            if violations(d):
                problems.append(found("sequences.drafts", f"{case['name']}[{i}]: {violations(d)}"))
        problems += sequence_problems(case, drafts, frozenset())
    for case in section["chains"]:
        drafts = sequence_drafts(section, case)
        for i, d in enumerate(drafts):
            if violations(d):
                problems.append(found("chains.drafts", f"{case['name']}[{i}]: {violations(d)}"))
        problems += chain_problems(case, drafts, frozenset())
    return problems


def sequence_problems(case: dict, drafts: list[dict], skip: frozenset[str]) -> list[str]:
    got = stream_mismatch(drafts, skip)
    want = case["expect"]
    expected = None if want["outcome"] == "Valid" else (want["index"], want["rule"])
    if got != expected:
        return [found("sequences", f"{case['name']}: expected {expected}, got {got}")]
    return []


def chain_problems(case: dict, drafts: list[dict], skip: frozenset[str]) -> list[str]:
    got = chain_mismatch(drafts, skip)
    want = case["expect"]
    expected = None if want["outcome"] == "Valid" else want["index"]
    if got != expected:
        return [found("chains", f"{case['name']}: expected {expected}, got {got}")]
    return []


# --------------------------------------------------------------------------- seeded bugs

# A `null` `reason` or `from` is refused by rule 54 or 59, or rule 60, at the member's own path with
# the same reason, so a draft is refused identically whether or not the type check reads `null`.
RULE_TYPED = ("nullable.payload.reason", "nullable.payload.from")
VALIDATOR_MUTANTS = (
    "rule.54",
    "rule.54.teardown",
    "rule.55",
    "rule.55.self",
    "rule.56",
    "rule.57.order",
    "rule.57.required",
    "rule.58",
    "rule.58.belongs",
    "rule.58.services",
    "rule.59",
    "rule.60",
    "rule.61",
    "rule.62",
    "rule.62.null",
    "rule.62.listed",
    "rule.63",
    "rule.63.copy",
    "rule.64",
    "rule.65",
    "record.extra",
    "record.missing",
    "types.risk_clock",
    "types.pii_ref",
    *sorted({f"loose.payload.{m}" for cases in MEMBER_CASES.values() for m, _, _ in cases}),
    *sorted({f"nullable.payload.{m}" for members in NULLED.values() for m in members} - set(RULE_TYPED)),
)
STREAM_MUTANTS = (
    "stream.66.revoked",
    "stream.66.broker",
    "stream.66.environment",
    "stream.66.account_ref",
    "stream.66.unique",
    "stream.67.rotated",
    "stream.67.narrow",
    "stream.67.connect",
    "stream.67.reconnect",
    "stream.67.reauthorize",
    "stream.67.same",
    "stream.68.from",
    "stream.68.cleared",
    "stream.68.rotation",
    "stream.68.after",
    "stream.68.one_connection",
    "stream.68.bound",
    "stream.68.checked",
    "stream.68.account_ref",
    "stream.68.mcp",
    "stream.68.reauthorized",
)
CHAIN_MUTANTS = (
    "chain.stream",
    "chain.connection",
    "chain.occasion",
    "chain.passed",
    "chain.mcp",
    "chain.copy",
    "chain.original",
)


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, kind, name):
        return next(c for c in s[kind] if c["name"] == name)

    return [
        (
            "a base draft breaks rule 59",
            "drafts.valid",
            mutated(lambda s: s["drafts"]["state_degraded"]["payload"].update(reason="lease_expired")),
        ),
        (
            "a base draft's step-up carries a token",
            "drafts.secret_free",
            mutated(lambda s: s["drafts"]["rotated"]["payload"]["step_up"].update(refresh_token="canary")),
        ),
        (
            "an account-stream draft is on another account's stream",
            "drafts.binding",
            mutated(lambda s: s["drafts"]["established_v2"]["payload"].update(account_ref=OTHER_ACCOUNT_REF)),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "acknowledged_without_the_owner")["expect"].update(path="payload.reason")),
        ),
        (
            "a valid draft breaks a rule",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "suspension_cleared")["changes"].append(change("payload.to", "active"))),
        ),
        (
            "a sequence record breaks a per-draft rule",
            "sequences.drafts",
            mutated(lambda s: case(s, "sequences", "connect_revoke_reconnect")["records"][1]["changes"].append(
                change("payload.scopes", ["trading", "data"]))),
        ),
        (
            "a sequence's expected mismatch moves",
            "sequences",
            mutated(lambda s: case(s, "sequences", "established_twice")["expect"].update(index=0)),
        ),
        (
            "a chain record breaks a per-draft rule",
            "chains.drafts",
            mutated(lambda s: case(s, "chains", "connect_then_reauthorize")["records"][1]["changes"].append(
                change("payload.scopes", ["trading", "data"]))),
        ),
        (
            "a chain's expected mismatch moves",
            "chains",
            mutated(lambda s: case(s, "chains", "copy_of_nothing")["expect"].update(index=4)),
        ),
    ]


def run_mutants(section: dict) -> list[str]:
    """Every seeded bug caught; a vector mutant only by the check it is registered against."""
    escaped = []
    cases = [(copy.deepcopy(d), None) for d in section["drafts"].values()]
    cases += [(draft_for(section, c), None) for c in section["valid_drafts"]]
    cases += [(draft_for(section, c), c["expect"]) for c in section["invalid_drafts"]]
    for mutant in VALIDATOR_MUTANTS:
        skip = frozenset([mutant])
        caught = False
        for draft, want in cases:
            got = violations(draft, skip)
            caught |= bool(got) if want is None else not reported(got, want)
        if not caught:
            escaped.append(f"connections validator mutant {mutant}")
    for mutant in STREAM_MUTANTS:
        skip = frozenset([mutant])
        if not any(sequence_problems(c, sequence_drafts(section, c), skip) for c in section["sequences"]):
            escaped.append(f"connections stream mutant {mutant}")
    for mutant in CHAIN_MUTANTS:
        skip = frozenset([mutant])
        if not any(chain_problems(c, sequence_drafts(section, c), skip) for c in section["chains"]):
            escaped.append(f"connections chain mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"connections check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"connections vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
        "sequences": sequences(),
        "chains": chains(),
    }
