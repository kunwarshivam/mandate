"""Journal spec v0.41 §9.7's reference vectors for `ApprovalTimedOut` (DEC-944): the runtime's
record that an ask's deadline passed and its safe default, skip, applied (mandate spec §6.4).

The schema and rule 135 live in `control.py`, beside §9.7's other records, so one validator judges
every closed schema. This module builds the `approval_timeouts` section: a base draft, an invalid
draft for every member type and rule, and valid drafts for the cases a rule might be misread to
refuse. Its own oracle reads the base draft's cause and payload directly, without the validator,
and every seeded bug is shown caught.
"""

from __future__ import annotations

import copy

from common import change, delete, digest_strings
from control import (
    AGENT_STREAM,
    RUNTIME,
    STREAM,
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

SPEC = "docs/specs/journal.md v0.41 §9.7 (DEC-944)"
AT = "2026-09-21T13:05:00.000000000Z"
APPROVAL = "01J8Z3P0A000000000000000Q1"
TIMED_OUT = "01J8Z3P4A000000000000000Q5"
OTHER = "01J8Z3P2A000000000000000Q3"
BOUND = "sha256:" + "1" * 64
BASE = "timed_out"


def base_drafts() -> dict[str, dict]:
    payload = {"approval": APPROVAL, "on_timeout": "skip"}
    return {
        BASE: {
            "envelope_version": 1,
            "environment": "paper",
            "event_id": TIMED_OUT,
            "stream_id": AGENT_STREAM,
            "event_type": "ApprovalTimedOut",
            "schema_version": 1,
            "event_time": AT,
            "clock_source": "local",
            "causation_id": APPROVAL,
            "correlation_id": None,
            "actor": dict(RUNTIME),
            "config_refs": {"mandate_version": BOUND},
            "payload": payload,
            "artifact_refs": sorted(digest_strings(payload)),
            "pii_refs": [],
        }
    }


def invalid(name, clause, changes, reason, path):
    return control_invalid(name, clause, BASE, changes, reason, path)


def valid(name, clause, changes):
    return control_valid(name, clause, BASE, changes)


# Each member, a value of the wrong JSON kind (`schema`) and a string of the wrong form
# (`non_canonical`).
MEMBER_CASES = (
    ("approval", 7, "not-a-ulid"),
    ("on_timeout", 7, "act"),
)
NULLED = ("approval", "on_timeout")


def member_drafts() -> list[dict]:
    out = []
    for member, wrong_kind, wrong_form in MEMBER_CASES:
        path = f"payload.{member}"
        out.append(invalid(f"{member}.kind", "§9.7 types", [change(path, wrong_kind)], "schema", path))
        out.append(invalid(f"{member}.form", "§9.7 types", [change(path, wrong_form)], "non_canonical", path))
    for member in NULLED:
        path = f"payload.{member}"
        out.append(invalid(f"{member}.null", "§9.7 types", [change(path, None)], "schema", path))
    out.append(invalid("missing", "§9.7 closed", [delete("payload.on_timeout")], "schema", "payload.on_timeout"))
    out.append(invalid("extra", "§9.7 closed", [change("payload.deadline", 1790000300)], "schema", "payload.deadline"))
    return out


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule; together they cover every member type and rule 135."""
    return [
        *member_drafts(),
        invalid(
            "without_its_ask",
            "rule 135: the timeout is caused by the ask it ends",
            [change("causation_id", None)],
            "schema",
            "causation_id",
        ),
        invalid(
            "caused_by_another_event",
            "rule 135: the cause is the approval it names",
            [change("causation_id", OTHER)],
            "schema",
            "causation_id",
        ),
        invalid(
            "names_another_approval",
            "rule 135: the approval is the cause it names",
            [change("payload.approval", OTHER)],
            "schema",
            "causation_id",
        ),
        invalid(
            "without_its_mandate",
            "§9 row: `ApprovalTimedOut` names `mandate_version`",
            [change("config_refs", {})],
            "missing_config_ref",
            "config_refs.mandate_version",
        ),
        invalid(
            "on_the_control_stream",
            "§9 row: `ApprovalTimedOut` is an agent-stream record",
            [change("stream_id", STREAM)],
            "wrong_stream",
            "event_type",
        ),
        invalid(
            "at_version_2",
            "§9.7: closed at schema version 1 only",
            [change("schema_version", 2)],
            "unknown_schema",
            "payload",
        ),
    ]


def valid_drafts() -> list[dict]:
    """Cases a rule might be misread to refuse; each must be accepted."""
    return [
        valid(
            "correlated",
            "rule 135 reads the cause only: a correlation is free",
            [change("correlation_id", OTHER)],
        ),
        valid(
            "another_approval_and_its_cause",
            "rule 135: any ask, so long as the cause and the approval agree",
            [change("payload.approval", OTHER), change("causation_id", OTHER)],
        ),
    ]


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = ("drafts.valid", "drafts.skip", "invalid_drafts", "valid_drafts")


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, draft in section["drafts"].items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    base = section["drafts"][BASE]
    if base["causation_id"] != APPROVAL or base["payload"] != {"approval": APPROVAL, "on_timeout": "skip"}:
        problems.append(found("drafts.skip", "the base ApprovalTimedOut is not its ask's skip"))
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case))
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        got = violations(draft_for(section, case))
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.135.missing",
    "rule.135.approval",
    "record.extra",
    "record.missing",
    "config_refs.required",
    *(f"loose.payload.{member}" for member, _, _ in MEMBER_CASES),
    *(f"nullable.payload.{member}" for member in NULLED),
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
            "the base draft names no cause",
            "drafts.valid",
            mutated(lambda s: s["drafts"][BASE].update(causation_id=None)),
        ),
        (
            "the base draft is caused by and names another ask",
            "drafts.skip",
            mutated(lambda s: s["drafts"][BASE].update(causation_id=OTHER) or s["drafts"][BASE]["payload"].update(approval=OTHER)),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "without_its_ask")["expect"].update(path="payload.approval")),
        ),
        (
            "a valid draft breaks rule 135",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "correlated")["changes"].append(change("causation_id", None))),
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
            escaped.append(f"timeout validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"timeout check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"timeout vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
