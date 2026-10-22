"""Journal spec v0.75 rule 258's reference vectors (DEC-2337): §9.5's `AccountCloseRequested`, one
instrument's close-position under an account kill switch, journaled before it is sent.

Rule 258 lives in `control.py`, beside §9.5's other account-stream records, so one validator judges
every closed schema. This module builds the `account_closes` section: the base close, an invalid
draft for each clause of rule 258 and for the closed schema, and valid drafts the rule might be
misread to refuse. Its own oracle reads the quantity directly, without the validator, and every
seeded bug is shown caught.
"""

from __future__ import annotations

import copy
from decimal import Decimal

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

SPEC = "docs/specs/journal.md v0.75 §9.5 rule 258 (DEC-2337)"
ACCOUNT = f"acct:{WORKSPACE}:{ACCOUNT_STREAM_REF}"
AT = "2026-10-22T15:00:00.250000000Z"
RISK_CLOCK = "2026-10-22T15:00:01.000000000Z"
SWITCH = "01JAD2K0S000000000000000K1"
CLOSE = "close"
EVENT_ID = "01JAD2K0A000000000000000E1"
RULE_258 = "rule 258"


def base_drafts() -> dict[str, dict]:
    """A close of ten shares under the account switch that named them."""
    close = {
        "instrument_id": "AAPL",
        "qty": "10",
        "kill_switch_event": SWITCH,
        "risk_clock": RISK_CLOCK,
    }
    return {
        CLOSE: {
            "envelope_version": 1,
            "environment": "paper",
            "event_id": EVENT_ID,
            "stream_id": ACCOUNT,
            "event_type": "AccountCloseRequested",
            "schema_version": 1,
            "event_time": AT,
            "clock_source": "local",
            "causation_id": SWITCH,
            "correlation_id": None,
            "actor": dict(EXECUTOR),
            "config_refs": {},
            "payload": close,
            "artifact_refs": [],
            "pii_refs": [],
        }
    }


def invalid_drafts() -> list[dict]:
    """Each draft breaks rule 258 or the closed schema once, at the member it names."""

    def bad(name, clause, changes, path, reason="schema"):
        return control_invalid(name, clause, CLOSE, changes, reason, path)

    return [
        bad("zero_qty", f"{RULE_258}: `qty` is positive", [change("payload.qty", "0")], "payload.qty"),
        bad("negative_qty", f"{RULE_258}: `qty` is positive", [change("payload.qty", "-10")], "payload.qty"),
        bad("zero_qty_written_otherwise", f"{RULE_258}: `qty` compared as a decimal", [change("payload.qty", "0.000")], "payload.qty"),
        bad("null_qty", "§9.5: `qty` is a decimal, never null", [change("payload.qty", None)], "payload.qty"),
        bad("qty_as_number", "§9.5: `qty` is a decimal string", [change("payload.qty", 10)], "payload.qty"),
        bad("switch_not_ulid", "§9.5: `kill_switch_event` is a ulid", [change("payload.kill_switch_event", "switch-1")], "payload.kill_switch_event", "non_canonical"),
        bad("no_switch", "§9.5: every member present", [delete("payload.kill_switch_event")], "payload.kill_switch_event"),
        bad("no_risk_clock", "§9.5: every member present", [delete("payload.risk_clock")], "payload.risk_clock"),
        bad("extra_client_order_id", "§9.5: the schema is closed", [change("payload.client_order_id", "md-k-1")], "payload.client_order_id"),
    ]


def valid_drafts() -> list[dict]:
    """What rule 258 might be misread to refuse."""
    return [
        control_valid("a_fraction_of_a_share", f"{RULE_258}: positive, however small", CLOSE, [change("payload.qty", "0.0001")]),
        control_valid("trailing_zeros", f"{RULE_258}: compared as a decimal", CLOSE, [change("payload.qty", "10.500")]),
        control_valid("crypto", f"{RULE_258}: any instrument the switch named", CLOSE, [change("payload.instrument_id", "BTC/USD"), change("payload.qty", "0.25")]),
    ]


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = (
    "drafts.valid",
    "drafts.closes",
    "invalid_drafts",
    "invalid_drafts.oracle",
    "valid_drafts",
    "valid_drafts.oracle",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def oracle_258(draft: dict) -> str | None:
    """Rule 258 from the member alone: `payload.qty` when it is a decimal string not above zero."""
    if draft["event_type"] != "AccountCloseRequested":
        return None
    value = draft["payload"].get("qty")
    if isinstance(value, str) and Decimal(value) <= 0:
        return "payload.qty"
    return None


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, draft in section["drafts"].items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    close = section["drafts"].get(CLOSE)
    if not (close and close["causation_id"] == close["payload"]["kill_switch_event"]):
        problems.append(found("drafts.closes", "the base close is not caused by the switch it names"))
    for case in section["invalid_drafts"]:
        draft = draft_for(section, case)
        got = violations(draft)
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
        if case["clause"].startswith(RULE_258) and oracle_258(draft) != case["expect"]["path"]:
            problems.append(found("invalid_drafts.oracle", f"{case['name']}: the oracle disagrees on rule 258"))
    for case in section["valid_drafts"]:
        draft = draft_for(section, case)
        got = violations(draft)
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
        if oracle_258(draft) is not None:
            problems.append(found("valid_drafts.oracle", f"{case['name']}: the oracle refuses it by rule 258"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.258",
    "boundary.rule_258_zero",
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
        ("the base close has a zero quantity", "drafts.valid", mutated(lambda s: s["drafts"][CLOSE]["payload"].update(qty="0"))),
        ("the base close is caused by another event", "drafts.closes", mutated(lambda s: s["drafts"][CLOSE].update(causation_id=EVENT_ID))),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "zero_qty")["expect"].update(path="payload.instrument_id")),
        ),
        (
            "a schema draft is relabelled to rule 258",
            "invalid_drafts.oracle",
            mutated(lambda s: case(s, "invalid_drafts", "switch_not_ulid").update(clause=f"{RULE_258}: relabelled")),
        ),
        (
            "a valid draft has a negative quantity",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "trailing_zeros")["changes"].append(change("payload.qty", "-1"))),
        ),
        (
            "a valid draft is moved to a negative zero",
            "valid_drafts.oracle",
            mutated(lambda s: case(s, "valid_drafts", "crypto")["changes"].append(change("payload.qty", "-0"))),
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
            escaped.append(f"account-close validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"account-close check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"account-close vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
