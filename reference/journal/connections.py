"""Journal spec v0.21 §9.10's reference vectors (DEC-800): a connection's history.

The schemas and consistency rules 54 to 61 live in `control.py`, beside §9.2's to §9.7's, so one
validator judges every closed schema. This module builds the `connections` section: a base draft of
each record, an invalid draft for every member type and rule, valid drafts for the cases a rule
might be misread to refuse, and `sequences` for the stream rules 62 to 64, which need a stream's
earlier records. It folds each sequence by its own path, checks that no base draft carries a
secret-shaped member (CN-1) and that the account-stream drafts are on the stream the establishment
binds, and shows every seeded bug caught.
"""

from __future__ import annotations

import copy

from common import change, delete
from control import (
    ACCOUNT_STREAM_REF,
    EXECUTOR,
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

SPEC = "docs/specs/journal.md v0.21 §9.10 (DEC-800)"
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
}
# Member names a credential, a broker account number, or the account fingerprint would sit in
# (CN-1, connections spec §3.1); no draft may carry one at any depth.
SECRET_SHAPED = ("secret", "token", "password", "api_key", "key_id", "account_number", "fingerprint", "credential")


def envelope(name: str, event_type: str, payload: dict, causation=None, version: int = 1) -> dict:
    on_account = event_type in ("ConnectionChecked", "ConnectionStateChanged", "ConnectionCredentialRefreshed")
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
        "pii_refs": [],
    }


def base_drafts() -> dict[str, dict]:
    owner = {"user": OWNER, "step_up": dict(STEP_UP)}
    passed = [{"check": c, "result": "passed", "reason": None} for c in ("account", "environment", "scope")]
    return {
        "established_v2": envelope(
            "established_v2",
            "ConnectionEstablished",
            {
                "connection_id": CONNECTION,
                "broker": "alpaca",
                "environment": "paper",
                "scopes": list(SCOPES),
                "account_ref": ACCOUNT_STREAM_REF,
                **owner,
            },
            version=2,
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
        "rotated": envelope(
            "rotated", "ConnectionCredentialRotated", {"connection_id": CONNECTION, "scopes": list(SCOPES), **owner}
        ),
        "checked_start": envelope(
            "checked_start",
            "ConnectionChecked",
            {"occasion": "executor_start", "results": passed, "risk_clock": CLOCK},
        ),
        "state_degraded": envelope(
            "state_degraded",
            "ConnectionStateChanged",
            {"from": "active", "to": "degraded", "reason": "network_errors", "risk_clock": CLOCK},
        ),
        "refreshed": envelope(
            "refreshed", "ConnectionCredentialRefreshed", {"scopes": list(SCOPES), "risk_clock": CLOCK}
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
        ("occasion", 7, "hourly"),
        ("results", {}, None),
        ("risk_clock", 1790000000, "2026-09-22T13:00:00.500000000Z"),
    ),
    "state_degraded": (
        ("from", 7, "connecting"),
        ("to", 7, "revoked"),
        ("reason", 7, "outage"),
        ("risk_clock", 1790000000, "2026-09-22T13:00:00.500000000Z"),
    ),
    "refreshed": (("scopes", "trading", None), ("risk_clock", 1790000000, "2026-09-22")),
}
NULLED = {
    "established_v2": ("account_ref", "user", "step_up"),
    "refused_scope": ("connection_id", "occasion", "check", "reason", "user", "step_up"),
    "rotated": ("connection_id", "scopes", "user", "step_up"),
    "checked_start": ("occasion", "results", "risk_clock"),
    "state_degraded": ("from", "to", "reason", "risk_clock"),
    "refreshed": ("scopes", "risk_clock"),
}


def member_drafts() -> list[dict]:
    out = []
    for base, cases in MEMBER_CASES.items():
        for member, wrong_kind, wrong_form in cases:
            path = f"payload.{member}"
            out.append(invalid(f"{base}.{member}.kind", "§9.10 types", base, [change(path, wrong_kind)], "schema", path))
            if wrong_form is not None:
                form = [change(path, wrong_form)]
                out.append(invalid(f"{base}.{member}.form", "§9.10 types", base, form, "non_canonical", path))
        for member in NULLED[base]:
            path = f"payload.{member}"
            out.append(invalid(f"{base}.{member}.null", "§9.10 types", base, [change(path, None)], "schema", path))
        last = MEMBER_CASES[base][-1][0]
        out.append(invalid(f"{base}.missing", "§9.10 closed", base, [delete(f"payload.{last}")], "schema", f"payload.{last}"))
        out.append(invalid(f"{base}.extra", "§9.10 closed", base, [change("payload.note", "x")], "schema", "payload.note"))
    return out


def result(check: str, outcome: str = "passed", reason=None) -> dict:
    return {"check": check, "result": outcome, "reason": reason}


def state(frm: str, to: str, reason: str) -> list[dict]:
    return [change("payload.from", frm), change("payload.to", to), change("payload.reason", reason)]


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule; together they cover every §9.10 member type and rule."""
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
            "§9.10: version 1 stays closed with its four members",
            est,
            [change("schema_version", 1), delete("payload.user"), delete("payload.step_up")],
            "schema",
            "payload.account_ref",
        ),
        invalid("established_version_3", "§9.10: versions 1 and 2 only", est, [change("schema_version", 3)], "unknown_schema", "payload"),
        invalid(
            "established_carries_a_secret",
            "§9.10, CN-1: a credential has no member to sit in",
            est,
            [change("payload.api_secret", "canary-secret")],
            "schema",
            "payload.api_secret",
        ),
        invalid(
            "refused_carries_the_fingerprint",
            "§9.10, connections spec §3: the fingerprint is never journaled",
            ref,
            [change("payload.account_fingerprint", "hmac:" + "f" * 64)],
            "schema",
            "payload.account_fingerprint",
        ),
        invalid(
            "established_step_up_extra",
            "§9.10: the step-up evidence is closed",
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
        invalid("refused_on_the_account_stream", "§9.10: a control-stream record", ref, [change("stream_id", ACCOUNT_STREAM)], "wrong_stream", "event_type"),
        invalid("state_on_the_control_stream", "§9.10: an account-stream record", st, [change("stream_id", STREAM)], "wrong_stream", "event_type"),
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
            [change("payload.results", [result("account"), result("account"), result("environment"), result("scope")])],
            "non_canonical",
            "payload.results",
        ),
        invalid(
            "checked_without_scope",
            "rule 57: scope, environment, and account are always run",
            chk,
            [change("payload.results", [result("account"), result("contract"), result("environment")])],
            "schema",
            "payload.results",
        ),
        invalid(
            "checked_uniqueness",
            "§9.10: uniqueness is a connect-time check",
            chk,
            [change("payload.results", [result("account"), result("environment"), result("scope"), result("uniqueness")])],
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
            [change("payload.results", [result("account"), result("environment"), result("scope", "passed", "fund_movement")])],
            "schema",
            "payload.results[2].reason",
        ),
        invalid(
            "failed_with_another_checks_reason",
            "rule 58: the reason belongs to its check",
            chk,
            [change("payload.results", [result("account", "failed", "tools_missing"), result("environment"), result("scope")])],
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
    ]


def valid_drafts() -> list[dict]:
    """Cases a rule might be misread to refuse; each must be accepted."""
    ref, chk, st = "refused_scope", "checked_start", "state_degraded"
    return [
        valid(
            "established_version_1",
            "§9.10: version 1 stays registered",
            "established_v2",
            [
                change("schema_version", 1),
                delete("payload.account_ref"),
                delete("payload.user"),
                delete("payload.step_up"),
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
                    [result("account"), result("contract", "failed", "contract_drift"), result("environment"), result("scope")],
                ),
            ],
        ),
        valid("degraded_again", "rules 59 and 60: a cause returns before the acknowledgment", st, state("degraded", "degraded", "rate_headroom")),
        valid("suspended_while_degraded", "rule 60", st, state("degraded", "suspended", "authorization_failed")),
        valid("suspended_again", "rule 60", st, state("suspended", "suspended", "lease_expired")),
        valid("suspension_cleared", "rule 59: the state stays", st, state("suspended", "suspended", "condition_cleared")),
        valid(
            "acknowledged_back_to_active",
            "rules 59 to 61",
            st,
            [*state("suspended", "active", "acknowledged"), change("causation_id", OWNER_ACK)],
        ),
    ]


# --------------------------------------------------------------------------- stream rules 62 to 64


def record(base: str, index: int, *changes: dict) -> dict:
    """One record of a sequence: a base draft, its own event id, then the changes."""
    event_id = f"01J8Z4S{index:02d}A0000000000000000"[:26]
    return {"base_draft": base, "changes": [change("event_id", event_id), *changes]}


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


def moved(index: int, frm: str, to: str, reason: str) -> dict:
    extra = [change("causation_id", OWNER_ACK)] if reason == "acknowledged" else []
    return record("state_degraded", index, *state(frm, to, reason), *extra)


def sequence(name: str, clause: str, records: list[dict], mismatch: tuple[int, str] | None = None) -> dict:
    if mismatch is None:
        expect = {"outcome": "Valid"}
    else:
        expect = {"outcome": "Mismatch", "code": "connection_lifecycle_mismatch", "index": mismatch[0], "rule": mismatch[1]}
    return {"name": name, "clause": clause, "records": records, "expect": expect}


def sequences() -> list[dict]:
    other_ref = change("payload.account_ref", OTHER_ACCOUNT_REF)
    v1 = [change("schema_version", 1), delete("payload.account_ref"), delete("payload.user"), delete("payload.step_up")]
    return [
        sequence(
            "connect_revoke_reconnect",
            "rule 62: a reconnect after its revocation, same broker, environment, and account_ref",
            [established(1), record("rotated", 2), revoked(3), established(4)],
        ),
        sequence(
            "every_occasion_in_its_place",
            "rule 63",
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
            "§9.10: no rule refuses a ConnectionRevoked",
            [revoked(1), established(2), revoked(3), revoked(4), established(5)],
        ),
        sequence(
            "a_refused_id_connects_later",
            "rule 63: a refused connect leaves no connection",
            [refused(1, "connect"), established(2)],
        ),
        sequence("established_twice", "rule 62: not revoked", [established(1), established(2)], (1, "62")),
        sequence("reconnect_another_account", "rule 62", [established(1), revoked(2), established(3, other_ref)], (2, "62")),
        sequence(
            "reconnect_another_environment",
            "rule 62",
            [established(1), revoked(2), established(3, change("payload.environment", "live"), change("environment", "live"))],
            (2, "62"),
        ),
        sequence("reconnect_another_broker", "rule 62", [established(1), revoked(2), established(3, change("payload.broker", "kraken_derivatives_us"))], (2, "62")),
        sequence("version_1_never_reestablished", "rule 62", [established(1, *v1), revoked(2), established(3)], (2, "62")),
        sequence(
            "two_connections_one_account",
            "rule 62: one account_ref, one connection (CN-5)",
            [established(1), established(2, change("payload.connection_id", OTHER_CONNECTION))],
            (1, "62"),
        ),
        sequence("established_again_after_reconnect", "rule 62", [established(1), revoked(2), established(3), established(4)], (3, "62")),
        sequence("rotated_never_established", "rule 63", [record("rotated", 1)], (0, "63")),
        sequence("rotated_after_revocation", "rule 63", [established(1), revoked(2), record("rotated", 3)], (2, "63")),
        sequence("reauthorize_after_revocation", "rule 63", [established(1), revoked(2), refused(3, "reauthorize")], (2, "63")),
        sequence("reconnect_while_connected", "rule 63", [established(1), refused(2, "reconnect")], (1, "63")),
        sequence("connect_an_established_id", "rule 63", [established(1), revoked(2), refused(3, "connect")], (2, "63")),
        sequence(
            "reconnect_refused_for_another_environment",
            "rule 63",
            [
                established(1),
                revoked(2),
                refused(3, "reconnect", CONNECTION, change("payload.environment", "live"), change("environment", "live")),
            ],
            (2, "63"),
        ),
        sequence(
            "degraded_cleared_acknowledged_then_suspended",
            "rule 64",
            [
                moved(1, "active", "degraded", "network_errors"),
                moved(2, "degraded", "degraded", "condition_cleared"),
                moved(3, "degraded", "active", "acknowledged"),
                moved(4, "active", "suspended", "credential_expired"),
                moved(5, "suspended", "suspended", "refresh_failed"),
                moved(6, "suspended", "suspended", "condition_cleared"),
                moved(7, "suspended", "active", "acknowledged"),
            ],
        ),
        sequence("first_change_not_from_active", "rule 64", [moved(1, "degraded", "suspended", "check_failed")], (0, "64")),
        sequence(
            "from_not_the_last_state",
            "rule 64",
            [moved(1, "active", "suspended", "check_failed"), moved(2, "degraded", "degraded", "contract_drift")],
            (1, "64"),
        ),
        sequence(
            "acknowledged_before_cleared",
            "rule 64: the acknowledgment never lifts a cause that has not cleared",
            [moved(1, "active", "degraded", "contract_drift"), moved(2, "degraded", "active", "acknowledged")],
            (1, "64"),
        ),
        sequence(
            "acknowledged_after_the_cause_returned",
            "rule 64",
            [
                moved(1, "active", "degraded", "network_errors"),
                moved(2, "degraded", "degraded", "condition_cleared"),
                moved(3, "degraded", "degraded", "network_errors"),
                moved(4, "degraded", "active", "acknowledged"),
            ],
            (3, "64"),
        ),
    ]


def sequence_drafts(section: dict, case: dict) -> list[dict]:
    return [draft_for(section, r) for r in case["records"]]


def stream_mismatch(drafts: list[dict], skip: frozenset[str] = frozenset()) -> tuple[int, str] | None:
    """The first record that breaks rule 62, 63, or 64, folding the stream in order."""
    first: dict[str, dict] = {}
    latest: dict[str, str] = {}
    bound: dict[str, str] = {}
    current, cleared = "active", False
    for i, d in enumerate(drafts):
        p, kind = d["payload"], d["event_type"]
        cid = p.get("connection_id")
        if kind == "ConnectionEstablished":
            prior = first.get(cid)
            if prior is not None:
                same = all(
                    p[m] == prior[m] or f"stream.62.{m}" in skip for m in ("broker", "environment")
                ) and (p.get("account_ref") == prior.get("account_ref") or "stream.62.account_ref" in skip)
                if (latest[cid] != "ConnectionRevoked" and "stream.62.revoked" not in skip) or not same:
                    return i, "62"
            holder = bound.get(p.get("account_ref"))
            if holder is not None and holder != cid and "stream.62.unique" not in skip:
                return i, "62"
            first.setdefault(cid, p)
            if p.get("account_ref") is not None:
                bound.setdefault(p["account_ref"], cid)
            latest[cid] = kind
        elif kind == "ConnectionRevoked":
            latest[cid] = kind
        elif kind == "ConnectionCredentialRotated":
            if latest.get(cid) != "ConnectionEstablished" and "stream.63.rotated" not in skip:
                return i, "63"
        elif kind == "ConnectionRefused":
            want = {"connect": None, "reconnect": "ConnectionRevoked", "reauthorize": "ConnectionEstablished"}
            if latest.get(cid) != want[p["occasion"]] and f"stream.63.{p['occasion']}" not in skip:
                return i, "63"
            prior = first.get(cid)
            if prior is not None and p["occasion"] != "connect" and "stream.63.same" not in skip:
                if p["broker"] != prior["broker"] or p["environment"] != prior["environment"]:
                    return i, "63"
        elif kind == "ConnectionStateChanged":
            if p["from"] != current and "stream.64.from" not in skip:
                return i, "64"
            if p["reason"] == "acknowledged" and not cleared and "stream.64.cleared" not in skip:
                return i, "64"
            cleared = p["reason"] == "condition_cleared"
            current = p["to"]
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
    return problems


def sequence_problems(case: dict, drafts: list[dict], skip: frozenset[str]) -> list[str]:
    got = stream_mismatch(drafts, skip)
    want = case["expect"]
    expected = None if want["outcome"] == "Valid" else (want["index"], want["rule"])
    if got != expected:
        return [found("sequences", f"{case['name']}: expected {expected}, got {got}")]
    return []


# --------------------------------------------------------------------------- seeded bugs

# A `null` `reason` or `from` is refused by rule 54 or 59, or rule 60, at the member's own path with
# the same reason, so a draft is refused identically whether or not the type check reads `null`.
RULE_TYPED = ("nullable.payload.reason", "nullable.payload.from")
VALIDATOR_MUTANTS = (
    "rule.54",
    "rule.55",
    "rule.55.self",
    "rule.56",
    "rule.57.order",
    "rule.57.required",
    "rule.58",
    "rule.58.belongs",
    "rule.59",
    "rule.60",
    "rule.61",
    "record.extra",
    "record.missing",
    "types.risk_clock",
    *sorted({f"loose.payload.{m}" for cases in MEMBER_CASES.values() for m, _, _ in cases}),
    *sorted({f"nullable.payload.{m}" for members in NULLED.values() for m in members} - set(RULE_TYPED)),
)
STREAM_MUTANTS = (
    "stream.62.revoked",
    "stream.62.broker",
    "stream.62.environment",
    "stream.62.account_ref",
    "stream.62.unique",
    "stream.63.rotated",
    "stream.63.connect",
    "stream.63.reconnect",
    "stream.63.reauthorize",
    "stream.63.same",
    "stream.64.from",
    "stream.64.cleared",
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
    }
