"""Journal spec v0.79 §9.34's reference vectors for `RetirementHeld` (DEC-2352): the runtime's
record that an agent's retirement has waited another 24 hours of risk clock for what the agent
still holds (DEC-2344 item 2), each in a batch with its `OwnerAlertSent` (`retirement_held`).

The schema and rule 264 live in `control.py`, beside §9.7's other agent-stream records, so one
validator judges every closed schema. This module builds the `retirements` section: a base draft,
an invalid draft for every member type and rule, valid drafts for the cases a rule might be misread
to refuse, and agent streams for stream rule 265. Its own oracle reads each stream's held periods
per cause without the stream fold, and every seeded bug is shown caught.
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

SPEC = "docs/specs/journal.md v0.79 §9.34 (DEC-2352)"
BOUND = "sha256:" + "1" * 64
NEWER = "sha256:" + "2" * 64
STOP = "01J8Z5RH0A000000000000000S"
LIMIT = "01J8Z5RH0B000000000000000M"
HELD = "01J8Z5RH10000000000000000H"
SINCE = "2026-09-21T15:00:00.000000000Z"
DAY = 86_400
BASE = "held"


def clock(seconds: int) -> str:
    """The risk clock `seconds` after `SINCE`, whole seconds as §9.2's `risk_clock`."""
    hours, rest = divmod(15 * 3600 + seconds, 3600)
    days, hour = divmod(hours, 24)
    minute, second = divmod(rest, 60)
    return f"2026-09-{21 + days:02d}T{hour:02d}:{minute:02d}:{second:02d}.000000000Z"


def held(
    period: int,
    *,
    cause: str = STOP,
    since: str = SINCE,
    after: int | None = None,
    waiting: tuple[str, ...] = ("position",),
    event_id: str | None = None,
    version: str = BOUND,
) -> dict:
    payload = {
        "held_since": since,
        "period": period,
        "waiting_on": list(waiting),
        "risk_clock": clock(DAY * period if after is None else after),
    }
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": event_id or f"01J8Z5RH2{period:01d}{cause[-1]}".ljust(26, "0"),
        "stream_id": AGENT_STREAM,
        "event_type": "RetirementHeld",
        "schema_version": 1,
        "event_time": payload["risk_clock"],
        "clock_source": "scheduler",
        "causation_id": cause,
        "correlation_id": None,
        "actor": dict(RUNTIME),
        "config_refs": {"mandate_version": version},
        "payload": payload,
        "artifact_refs": sorted(digest_strings(payload)),
        "pii_refs": [],
    }


def base_drafts() -> dict[str, dict]:
    return {BASE: held(1, event_id=HELD)}


def invalid(name, clause, changes, reason, path):
    return control_invalid(name, clause, BASE, changes, reason, path)


def valid(name, clause, changes):
    return control_valid(name, clause, BASE, changes)


# Each member, a value of the wrong JSON kind (`schema`) and one of the wrong form (`non_canonical`
# for a string, `schema` for a list item outside its enumeration).
MEMBER_CASES = (
    ("held_since", 7, "2026-09-21T15:00:00.5Z", "non_canonical"),
    ("period", "1", None, None),
    ("waiting_on", "position", ["orders"], "non_canonical"),
    ("risk_clock", 7, "2026-09-22T15:00:00Z", "non_canonical"),
)
NULLED = ("held_since", "period", "waiting_on", "risk_clock")


def member_drafts() -> list[dict]:
    out = []
    for member, wrong_kind, wrong_form, form_reason in MEMBER_CASES:
        path = f"payload.{member}"
        out.append(invalid(f"{member}.kind", "§9.34 types", [change(path, wrong_kind)], "schema", path))
        if wrong_form is not None:
            form_path = f"{path}[0]" if isinstance(wrong_form, list) else path
            out.append(invalid(f"{member}.form", "§9.34 types", [change(path, wrong_form)], form_reason, form_path))
    for member in NULLED:
        path = f"payload.{member}"
        out.append(invalid(f"{member}.null", "§9.34 types", [change(path, None)], "schema", path))
    out.append(invalid("missing", "§9.34 closed", [delete("payload.period")], "schema", "payload.period"))
    out.append(invalid("extra", "§9.34 closed", [change("payload.qty", "10")], "schema", "payload.qty"))
    return out


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule; together they cover every member type and rule 264."""
    return [
        *member_drafts(),
        invalid(
            "by_a_user",
            "rule 264: the runtime's own record",
            [change("actor.kind", "user")],
            "schema",
            "actor.kind",
        ),
        invalid("uncaused", "rule 264: caused by what began the retirement", [change("causation_id", None)], "schema", "causation_id"),
        invalid("period_zero", "rule 264: periods count from 1", [change("payload.period", 0)], "schema", "payload.period"),
        invalid(
            "too_soon",
            "rule 264: period n no sooner than n times 24 hours after it began",
            [change("payload.risk_clock", clock(DAY - 1)), change("event_time", clock(DAY - 1))],
            "schema",
            "payload.risk_clock",
        ),
        invalid(
            "second_period_too_soon",
            "rule 264: period 2 waits 48 hours",
            [change("payload.period", 2), change("payload.risk_clock", clock(2 * DAY - 1))],
            "schema",
            "payload.risk_clock",
        ),
        invalid("waiting_on_nothing", "rule 264: it waits on something", [change("payload.waiting_on", [])], "schema", "payload.waiting_on"),
        invalid(
            "waiting_on_twice",
            "rule 264: each thing once",
            [change("payload.waiting_on", ["position", "position"])],
            "schema",
            "payload.waiting_on",
        ),
        invalid(
            "waiting_on_out_of_order",
            "rule 264: in order",
            [change("payload.waiting_on", ["working_order", "position"])],
            "schema",
            "payload.waiting_on",
        ),
        invalid(
            "without_its_mandate",
            "§9 row: `RetirementHeld` names `mandate_version`",
            [change("config_refs", {})],
            "missing_config_ref",
            "config_refs.mandate_version",
        ),
        invalid(
            "on_the_control_stream",
            "§9 row: `RetirementHeld` is an agent-stream record",
            [change("stream_id", STREAM)],
            "wrong_stream",
            "event_type",
        ),
        invalid("at_version_2", "§9.34: closed at schema version 1 only", [change("schema_version", 2)], "unknown_schema", "payload"),
    ]


def valid_drafts() -> list[dict]:
    """Cases a rule might be misread to refuse; each must be accepted."""
    return [
        valid("exactly_due", "rule 264: at the 24-hour mark itself", []),
        valid("late", "rule 264: a late period is fine; no sooner, not no later", [change("payload.risk_clock", clock(DAY + 3600))]),
        valid(
            "both",
            "rule 264: a position and a working order",
            [change("payload.waiting_on", ["position", "working_order"])],
        ),
        valid("working_order_alone", "rule 264: a working order alone", [change("payload.waiting_on", ["working_order"])]),
        valid(
            "third_period",
            "rule 264: period 3 at 72 hours",
            [change("payload.period", 3), change("payload.risk_clock", clock(3 * DAY))],
        ),
    ]


# --------------------------------------------------------------------------- stream rule 265


def stream_failure(records: list[dict], skip: frozenset[str] = frozenset()) -> int | None:
    """Rule 265 over one agent stream's records in `seq` order, folded once: the index of the first
    `RetirementHeld` that does not continue its cause's run of periods, under one `held_since` and
    one mandate version, or `None`."""
    last: dict[str, tuple[int, str, str]] = {}
    for i, record in enumerate(records):
        if record["event_type"] != "RetirementHeld":
            continue
        cause = record["causation_id"]
        p = record["payload"]
        version = record["config_refs"]["mandate_version"]
        before = last.get(cause)
        expected = 1 if before is None else before[0] + 1
        if p["period"] != expected and "stream.265.period" not in skip:
            return i
        if before is not None and p["held_since"] != before[1] and "stream.265.since" not in skip:
            return i
        if before is not None and version != before[2] and "stream.265.version" not in skip:
            return i
        last[cause] = (p["period"], *(before[1:] if before is not None else (p["held_since"], version)))
    return None


def other(index: int) -> dict:
    """Another agent-stream record between the held periods, which rule 265 does not read."""
    record = held(1, event_id=f"01J8Z5RH3{index:01d}".ljust(26, "0"))
    record["event_type"] = "ApprovalTimedOut"
    return record


def stream_cases() -> list[dict]:
    def case(name, clause, records, index):
        expect = {"outcome": "Valid"} if index is None else {"outcome": "Invalid", "rule": "rule 265", "index": index}
        return {"name": name, "clause": clause, "records": records, "expect": expect}

    return [
        case("one_period", "rule 265: the first period is 1", [held(1)], None),
        case("three_periods", "rule 265: each next period is one more", [held(1), held(2), held(3)], None),
        case("between_others", "rule 265: other records between periods are not read", [held(1), other(1), held(2)], None),
        case(
            "two_causes",
            "rule 265: each cause counts its own periods",
            [held(1), held(1, cause=LIMIT), held(2), held(2, cause=LIMIT)],
            None,
        ),
        case(
            "what_it_waits_on_changes",
            "rule 265: what it waits on may change between periods",
            [held(1, waiting=("position", "working_order")), held(2, waiting=("position",))],
            None,
        ),
        case("starts_at_two", "rule 265: the first period is 1", [held(2)], 0),
        case("skips_a_period", "rule 265: each next period is one more", [held(1), held(3)], 1),
        case("repeats_a_period", "rule 265: each period once", [held(1), held(2), held(2)], 2),
        case(
            "another_since",
            "rule 265: one retirement began once",
            [held(1), held(2, since=clock(3600), after=2 * DAY + 3600)],
            1,
        ),
        case(
            "under_a_newer_version",
            "rule 265: a newer mandate version ends the retirement it was held under",
            [held(1), held(2, version=NEWER)],
            1,
        ),
        case(
            "retired_again_under_the_newer_version",
            "rule 265: a retirement the newer version begins is a new cause, from 1",
            [held(1), held(1, cause=LIMIT, since=clock(3600), after=DAY + 3600, version=NEWER),
             held(2, cause=LIMIT, since=clock(3600), after=2 * DAY + 3600, version=NEWER)],
            None,
        ),
        case(
            "another_cause_restarts_at_one",
            "rule 265: another cause starts from 1",
            [held(1), held(2), held(2, cause=LIMIT)],
            2,
        ),
    ]


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = ("drafts.valid", "invalid_drafts", "valid_drafts", "streams.rule", "streams.oracle")


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def oracle(records: list[dict]) -> int | None:
    """Rule 265 read another way: group each cause's held periods, then require them to be exactly
    1, 2, ... in stream order with the first one's `held_since` and mandate version, and report the
    earliest record that is not."""
    groups: dict[str, list[tuple[int, dict]]] = {}
    for i, record in enumerate(records):
        if record["event_type"] == "RetirementHeld":
            groups.setdefault(record["causation_id"], []).append((i, record))
    bad = []
    for runs in groups.values():
        first = runs[0][1]
        for n, (i, record) in enumerate(runs, start=1):
            same = (
                record["payload"]["held_since"] == first["payload"]["held_since"]
                and record["config_refs"] == first["config_refs"]
            )
            if record["payload"]["period"] != n or not same:
                bad.append(i)
                break
    return min(bad) if bad else None


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
    for case in section["streams"]:
        want = case["expect"].get("index")
        for record in case["records"]:
            if record["event_type"] == "RetirementHeld" and violations(record):
                problems.append(found("streams.rule", f"{case['name']}: a record breaks a draft rule"))
        if stream_failure(case["records"]) != want:
            problems.append(found("streams.rule", f"{case['name']}: expected {case['expect']}"))
        if oracle(case["records"]) != want:
            problems.append(found("streams.oracle", f"{case['name']}: expected {case['expect']}"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.264.agent",
    "rule.264.cause",
    "rule.264.period",
    "rule.264.due",
    "rule.264.waiting",
    "record.extra",
    "record.missing",
    "config_refs.required",
    *(f"loose.payload.{member}" for member, _, _, _ in MEMBER_CASES),
    *(f"nullable.payload.{member}" for member in NULLED),
)
STREAM_MUTANTS = ("stream.265.period", "stream.265.since", "stream.265.version")


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, kind, name):
        return next(c for c in s[kind] if c["name"] == name)

    return [
        ("the base draft names no cause", "drafts.valid", mutated(lambda s: s["drafts"][BASE].update(causation_id=None))),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "too_soon")["expect"].update(path="payload.held_since")),
        ),
        (
            "a valid draft breaks rule 264",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "late")["changes"].append(change("payload.period", 0))),
        ),
        (
            "a stream case's expectation is moved off the record the fold refuses",
            "streams.rule",
            mutated(lambda s: case(s, "streams", "skips_a_period")["expect"].update(index=0)),
        ),
        (
            "a stream case lets a repeated period pass",
            "streams.oracle",
            mutated(lambda s: case(s, "streams", "repeats_a_period").update(expect={"outcome": "Valid"})),
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
            escaped.append(f"retirement validator mutant {mutant}")
    for mutant in STREAM_MUTANTS:
        skip = frozenset([mutant])
        if all(stream_failure(c["records"], skip) == c["expect"].get("index") for c in section["streams"]):
            escaped.append(f"retirement stream mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"retirement check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"retirement vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
        "streams": stream_cases(),
    }
