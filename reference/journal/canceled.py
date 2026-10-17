"""Journal spec v0.43 §9.7's reference vectors for `ApprovalCanceled` (DEC-950): the runtime's
record that a pending ask was canceled by a tightening before any terminal answer (mandate spec
§2.2, §5.9, §6.4), so nothing it bound is acted on.

The schema lives in `control.py`, beside §9.7's other records, so one validator judges every
closed schema. This module builds the `approval_cancellations` section: a base draft, an invalid
draft for every member type and the record's catalogue entry, and valid drafts for each cancel
reason and for the envelope members no rule reads. Its own oracle reads the base draft's payload
directly, without the validator, and every seeded bug is shown caught.
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

SPEC = "docs/specs/journal.md v0.43 §9.7 (DEC-950)"
AT = "2026-09-21T13:05:00.000000000Z"
APPROVAL = "01J8Z3P0A000000000000000Q1"
CANCELED = "01J8Z3P4A000000000000000Q6"
OTHER = "01J8Z3P2A000000000000000Q3"
BOUND = "sha256:" + "1" * 64
BASE = "canceled"
REASONS = ("version_applied", "mode_tightened", "owner_pause", "owner_stop", "kill_switch")


def base_drafts() -> dict[str, dict]:
    payload = {"approval": APPROVAL, "reason": "version_applied"}
    return {
        BASE: {
            "envelope_version": 1,
            "environment": "paper",
            "event_id": CANCELED,
            "stream_id": AGENT_STREAM,
            "event_type": "ApprovalCanceled",
            "schema_version": 1,
            "event_time": AT,
            "clock_source": "local",
            "causation_id": None,
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
# (`non_canonical`). The legacy `rebound` names no reason this version records (DEC-950 item 2).
MEMBER_CASES = (
    ("approval", 7, "not-a-ulid"),
    ("reason", 7, "rebound"),
)
NULLED = ("approval", "reason")


def member_drafts() -> list[dict]:
    out = []
    for member, wrong_kind, wrong_form in MEMBER_CASES:
        path = f"payload.{member}"
        out.append(invalid(f"{member}.kind", "§9.7 types", [change(path, wrong_kind)], "schema", path))
        out.append(invalid(f"{member}.form", "§9.7 types", [change(path, wrong_form)], "non_canonical", path))
    for member in NULLED:
        path = f"payload.{member}"
        out.append(invalid(f"{member}.null", "§9.7 types", [change(path, None)], "schema", path))
    out.append(invalid("missing", "§9.7 closed", [delete("payload.reason")], "schema", "payload.reason"))
    out.append(invalid("extra", "§9.7 closed", [change("payload.on_timeout", "skip")], "schema", "payload.on_timeout"))
    return out


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule; together they cover every member type and the catalogue
    entry."""
    return [
        *member_drafts(),
        invalid(
            "without_its_mandate",
            "§9 row: `ApprovalCanceled` names `mandate_version`",
            [change("config_refs", {})],
            "missing_config_ref",
            "config_refs.mandate_version",
        ),
        invalid(
            "on_the_control_stream",
            "§9 row: `ApprovalCanceled` is an agent-stream record",
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
    """Each cancel reason, and the cases a rule might be misread to refuse; each must be accepted."""
    return [
        *(
            valid(f"reason.{reason}", "§9.7: every cancel reason the runtime writes", [change("payload.reason", reason)])
            for reason in REASONS
            if reason != "version_applied"
        ),
        valid(
            "caused",
            "§9.7: no rule reads the cause; a tightening that names its event is recorded",
            [change("causation_id", OTHER)],
        ),
        valid(
            "correlated",
            "§9.7: no rule reads the correlation",
            [change("correlation_id", OTHER)],
        ),
        valid(
            "another_approval",
            "§9.7: any ask the runtime held pending",
            [change("payload.approval", OTHER)],
        ),
    ]


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = ("drafts.valid", "drafts.cancel", "invalid_drafts", "valid_drafts")


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
    if base["payload"] != {"approval": APPROVAL, "reason": "version_applied"}:
        problems.append(found("drafts.cancel", "the base ApprovalCanceled is not its ask's version cancel"))
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
            "the base draft names no mandate version",
            "drafts.valid",
            mutated(lambda s: s["drafts"][BASE].update(config_refs={})),
        ),
        (
            "the base draft cancels for another reason",
            "drafts.cancel",
            mutated(lambda s: s["drafts"][BASE]["payload"].update(reason="kill_switch")),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "reason.form")["expect"].update(path="payload.approval")),
        ),
        (
            "a valid draft records the legacy reason",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "caused")["changes"].append(change("payload.reason", "rebound"))),
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
            escaped.append(f"cancel validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"cancel check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"cancel vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
