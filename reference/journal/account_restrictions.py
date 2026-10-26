"""Journal spec §9.5's reference vectors for `AccountRestrictionChanged` (DEC-2395): an account
restriction the executor detected (trading spec §7.3), with its closed `cause` (DEC-441 item 23),
journaled before the modes it applies.

The schema and rules 279 and 280 live in `control.py`, beside §9.5's other account-stream records,
so one validator judges every closed schema. This module builds the `account_restrictions` section:
a base draft of each restriction, an invalid draft for every member type and each clause of rules
279 and 280, and valid drafts for every pairing the rules admit. Its own oracle reads the members
directly, without the validator, and every seeded bug is shown caught.
"""

from __future__ import annotations

import copy

from common import change, delete
from control import (
    ACCOUNT_STREAM_REF,
    EXECUTOR,
    WORKSPACE,
    check_of,
    draft_for,
    reported,
    violations,
)
from control import (
    invalid as control_invalid,
)
from control import (
    valid as control_valid,
)

SPEC = "docs/specs/journal.md §9.5 rules 279 and 280 (DEC-2395)"
ACCOUNT = f"acct:{WORKSPACE}:{ACCOUNT_STREAM_REF}"
AT = "2026-10-26T15:00:00.250000000Z"
RISK_CLOCK = "2026-10-26T15:00:01.000000000Z"
BLOCKED = "blocked"
CLOSING = "closing_only"
RESTRICTIONS = (CLOSING, BLOCKED)
CAUSES = ("broker_reject", "broker_notice", "connection_unavailable")
CODES = ("account_trading_blocked", "account_restricted")
RULE_279 = "rule 279"
RULE_280 = "rule 280"


def restricted(event_id: str, restriction: str, cause: str, code: str) -> dict:
    payload = {"restriction": restriction, "cause": cause, "reason_code": code, "risk_clock": RISK_CLOCK}
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": event_id,
        "stream_id": ACCOUNT,
        "event_type": "AccountRestrictionChanged",
        "schema_version": 1,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": None,
        "correlation_id": None,
        "actor": dict(EXECUTOR),
        "config_refs": {},
        "payload": payload,
        "artifact_refs": [],
        "pii_refs": [],
    }


def base_drafts() -> dict[str, dict]:
    """Trading spec §7.3's two restrictions: a blocked account (row 1, a broker notice), and an
    account the broker's rejects restrict to closing only (rows 2 and 3)."""
    return {
        BLOCKED: restricted("01JAE2R0A000000000000000R1", BLOCKED, "broker_notice", "account_trading_blocked"),
        CLOSING: restricted("01JAE2R0A000000000000000R2", CLOSING, "broker_reject", "account_restricted"),
    }


def invalid_drafts() -> list[dict]:
    """Each draft breaks one rule or the closed schema once, at the member it names."""

    def bad(name, clause, base, changes, path, reason="schema"):
        return control_invalid(name, clause, base, changes, reason, path)

    code, cause = "payload.reason_code", "payload.cause"
    out = [
        bad("blocked_restricted", f"{RULE_279}: `blocked` exactly with `account_trading_blocked`", BLOCKED,
            [change(code, "account_restricted")], code),
        bad("closing_blocked_code", f"{RULE_279}: `account_trading_blocked` only for `blocked`", CLOSING,
            [change(code, "account_trading_blocked")], code),
        bad("blocked_by_a_reject", f"{RULE_280}: a blocked account is a broker notice (§7.3 row 1)", BLOCKED,
            [change(cause, "broker_reject")], cause),
        bad("blocked_by_the_connection", f"{RULE_280}: the connection never blocks (§7.3 row 5)", BLOCKED,
            [change(cause, "connection_unavailable")], cause),
        bad("both_rules", f"{RULE_279} reports first", BLOCKED,
            [change(code, "account_restricted"), change(cause, "broker_reject")], code),
        bad("no_risk_clock", "§9.5: every member present", CLOSING, [delete("payload.risk_clock")], "payload.risk_clock"),
        bad("extra_message", "§9.5: the schema is closed", CLOSING, [change("payload.message", "x")], "payload.message"),
        bad("version_2", "§9.5: registered at `schema_version` 1 only", CLOSING, [change("schema_version", 2)], "payload",
            "unknown_schema"),
        bad("on_the_control_stream", "§9.5: an account-stream record", CLOSING, [change("stream_id", f"ctl:{WORKSPACE}")],
            "event_type", "wrong_stream"),
    ]
    for member, other in (("restriction", "restricted"), ("cause", "broker"), ("reason_code", "restricted")):
        path = f"payload.{member}"
        out += [
            bad(f"{member}_unknown", f"§9.5: `{member}` is one of its values", CLOSING, [change(path, other)], path,
                "non_canonical"),
            bad(f"{member}_null", f"§9.5: `{member}` is never null", CLOSING, [change(path, None)], path),
            bad(f"{member}_number", f"§9.5: `{member}` is text", CLOSING, [change(path, 1)], path),
            bad(f"{member}_missing", "§9.5: every member present", CLOSING, [delete(path)], path),
        ]
    return out


def valid_drafts() -> list[dict]:
    """Every pairing rules 279 and 280 admit: a closing-only restriction of each cause."""
    return [
        control_valid(f"closing_by_{cause}", f"{RULE_280}: any cause restricts to closing only", CLOSING,
                      [change("payload.cause", cause)])
        for cause in CAUSES
    ] + [
        control_valid("live", "§9.5: either environment", BLOCKED, [change("environment", "live")]),
    ]


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = ("drafts.valid", "invalid_drafts", "valid_drafts", "pairs.oracle")


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def admitted(restriction: str, cause: str, code: str) -> bool:
    """Trading spec §7.3's table read directly: row 1 blocks on a broker notice and codes
    `account_trading_blocked`; rows 2 to 5 restrict to closing only and code `account_restricted`."""
    if restriction == BLOCKED:
        return (cause, code) == ("broker_notice", "account_trading_blocked")
    return code == "account_restricted"


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, draft in section["drafts"].items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case))
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        got = violations(draft_for(section, case))
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
    for restriction in RESTRICTIONS:
        for cause in CAUSES:
            for code in CODES:
                draft = restricted("01JAE2R0A000000000000000R9", restriction, cause, code)
                if (not violations(draft)) != admitted(restriction, cause, code):
                    problems.append(found("pairs.oracle", f"{restriction}/{cause}/{code}: the rules and §7.3 disagree"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = ("rule.279", "rule.280", "rule.279.closing_only_any_code", "rule.280.any_reject")


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, kind, name):
        return next(c for c in s[kind] if c["name"] == name)

    return [
        ("the blocked base names a reject", "drafts.valid",
         mutated(lambda s: s["drafts"][BLOCKED]["payload"].update(cause="broker_reject"))),
        ("an invalid draft's expectation differs", "invalid_drafts",
         mutated(lambda s: case(s, "invalid_drafts", "blocked_by_a_reject")["expect"].update(path="payload.reason_code"))),
        ("a valid draft blocks on a reject", "valid_drafts",
         mutated(lambda s: case(s, "valid_drafts", "live")["changes"].append(change("payload.cause", "broker_reject")))),
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
            escaped.append(f"restriction validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if check != "pairs.oracle" and not any(c == check for _, c, _ in registered):
            escaped.append(f"restriction check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"restriction vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
