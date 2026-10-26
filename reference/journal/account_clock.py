"""Journal spec §9.5's reference vectors for the executor's `ClockAdvanced` copy on the account
stream (DEC-2808): the scheduler's tick, journaled when it returns a risk record or crosses New York
midnight (DEC-2804 item 5), so the account risk fold can derive `RiskDayStarted` from it.

The schema and rules 301 and 302 live in `control.py`, beside §9.5's other account-stream records,
so one validator judges every closed schema. This module builds the `account_clock` section: a base
draft of an ordinary tick and of one crossing New York midnight, an invalid draft for every member
type and each rule, and valid drafts for what the rules might be misread to refuse. Its own oracle
reads the members directly, without the validator, and every seeded bug is shown caught.
"""

from __future__ import annotations

import copy
import itertools

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

SPEC = "docs/specs/journal.md §9.5 rules 301 and 302 (DEC-2808)"
ACCOUNT = f"acct:{WORKSPACE}:{ACCOUNT_STREAM_REF}"
TICK = "tick"
MIDNIGHT = "midnight"
RULE_301 = "rule 301"
RULE_302 = "rule 302"
CLOCK = "2026-10-26T15:00:01.000000000Z"
# 2026-10-27 00:00:00 in New York, still on daylight time (UTC-4).
NEW_YORK_MIDNIGHT = "2026-10-27T04:00:00.000000000Z"
# The scheduler stream's `ClockAdvanced` each copy names: the journal checks only that one is named.
SCHEDULER_TICK = "01JAE3A0B000000000000000C1"
SCHEDULER_MIDNIGHT = "01JAE7F00000000000000000C2"


def record(event_id: str, event_time: str, clock: str, causation: str | None) -> dict:
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": event_id,
        "stream_id": ACCOUNT,
        "event_type": "ClockAdvanced",
        "schema_version": 1,
        "event_time": event_time,
        "clock_source": "local",
        "causation_id": causation,
        "correlation_id": None,
        "actor": dict(EXECUTOR),
        "config_refs": {},
        "payload": {"clock": clock, "risk_clock": clock},
        "artifact_refs": [],
        "pii_refs": [],
    }


def base_drafts() -> dict[str, dict]:
    """A tick copied because the fold returned a record on it, and the tick that crosses New York
    midnight, each naming the scheduler's tick it copies."""
    return {
        TICK: record("01JAE3A0B000000000000000T1", "2026-10-26T15:00:01.250000000Z", CLOCK, SCHEDULER_TICK),
        MIDNIGHT: record(
            "01JAE7F00000000000000000T2", "2026-10-27T04:00:00.125000000Z", NEW_YORK_MIDNIGHT, SCHEDULER_MIDNIGHT
        ),
    }


def invalid_drafts() -> list[dict]:
    """Each draft breaks one rule or the closed schema once, at the member it names."""

    def bad(name, clause, base, changes, path, reason="schema"):
        return control_invalid(name, clause, base, changes, reason, path)

    return [
        bad("clock_missing", "§9.5: every member present", TICK, [delete("payload.clock")], "payload.clock"),
        bad("risk_clock_missing", "§2, §9.5: every account-stream row the fold reads carries `risk_clock`", TICK,
            [delete("payload.risk_clock")], "payload.risk_clock"),
        bad("extra_date", "§9.5: the schema is closed; the date is `TradingDayStarted`'s", MIDNIGHT,
            [change("payload.date", "2026-10-27")], "payload.date"),
        bad("clock_with_nanoseconds", "§9.2: a `risk_clock` is a whole second", TICK,
            [change("payload.clock", "2026-10-26T15:00:01.250000000Z")], "payload.clock", "non_canonical"),
        bad("risk_clock_with_nanoseconds", "§9.2: a `risk_clock` is a whole second", TICK,
            [change("payload.risk_clock", "2026-10-26T15:00:01.000000001Z")], "payload.risk_clock",
            "non_canonical"),
        bad("clock_as_seconds", "§9.2: a `risk_clock` is a timestamp, never integer seconds", TICK,
            [change("payload.clock", 1793026801)], "payload.clock"),
        bad("clock_null", "§9.5: `clock` is never null", TICK, [change("payload.clock", None)], "payload.clock"),
        bad("risk_clock_null", "§9.5: `risk_clock` is never null", TICK, [change("payload.risk_clock", None)],
            "payload.risk_clock"),
        bad("clock_with_offset", "§4: a timestamp is UTC, written with `Z`", TICK,
            [change("payload.clock", "2026-10-26T11:00:01.000000000-04:00")], "payload.clock", "non_canonical"),
        bad("risk_clock_after_clock", f"{RULE_301}: `risk_clock` equals `clock`", TICK,
            [change("payload.risk_clock", "2026-10-26T15:00:02.000000000Z")], "payload.risk_clock"),
        bad("risk_clock_before_clock", f"{RULE_301}: `risk_clock` equals `clock`", TICK,
            [change("payload.risk_clock", "2026-10-26T15:00:00.000000000Z")], "payload.risk_clock"),
        bad("risk_clock_on_the_eve", f"{RULE_301}: the midnight copy's `risk_clock` is its own tick", MIDNIGHT,
            [change("payload.risk_clock", "2026-10-27T03:59:59.000000000Z")], "payload.risk_clock"),
        bad("no_causation", f"{RULE_302}: the copy names the scheduler's tick", TICK,
            [change("causation_id", None)], "causation_id"),
        bad("midnight_no_causation", f"{RULE_302}: the copy names the scheduler's tick", MIDNIGHT,
            [change("causation_id", None)], "causation_id"),
        bad("risk_clock_reported_before_causation", f"{RULE_301} before {RULE_302}", TICK,
            [change("causation_id", None), change("payload.risk_clock", "2026-10-26T15:00:02.000000000Z")],
            "payload.risk_clock"),
        bad("version_2", "§9.5: registered at `schema_version` 1 only", TICK, [change("schema_version", 2)],
            "payload", "unknown_schema"),
        bad("on_the_control_stream", "§9.5: an account-stream record", TICK,
            [change("stream_id", f"ctl:{WORKSPACE}")], "event_type", "wrong_stream"),
        bad("on_the_agent_stream", "§9.5: an account-stream record", TICK,
            [change("stream_id", f"agent:{WORKSPACE}:agent_a")], "event_type", "wrong_stream"),
    ]


def valid_drafts() -> list[dict]:
    """What the rules might be misread to refuse: any scheduler tick named, a later tick, either
    environment, and a copy with a correlation."""

    def ok(name, clause, base, changes):
        return control_valid(name, clause, base, changes)

    return [
        ok("causation_not_on_this_stream", f"{RULE_302}: the journal checks presence, not the named event",
           TICK, [change("causation_id", "01JAE3A0B000000000000000C9")]),
        ok("later_tick", f"{RULE_301}: any whole second, the two equal", TICK,
           [change("payload.clock", "2026-10-26T19:59:59.000000000Z"),
            change("payload.risk_clock", "2026-10-26T19:59:59.000000000Z")]),
        ok("first_second_of_the_year", f"{RULE_301}: any whole second, the two equal", TICK,
           [change("payload.clock", "2027-01-01T05:00:00.000000000Z"),
            change("payload.risk_clock", "2027-01-01T05:00:00.000000000Z")]),
        ok("live", "§9.5: either environment", MIDNIGHT, [change("environment", "live")]),
        ok("with_correlation", "§9.5: no rule reads `correlation_id`", TICK,
           [change("correlation_id", "01JAE3A0B000000000000000R1")]),
    ]


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = ("drafts.valid", "invalid_drafts", "valid_drafts", "clocks.oracle")


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def expected(clock: str, risk_clock: str, causation: str | None) -> list[tuple[str, str]]:
    """§9.5's two rules read directly: the tick's own clock as `risk_clock`, then the scheduler's
    tick named, the first that fails reported."""
    if risk_clock != clock:
        return [("schema", "payload.risk_clock")]
    if causation is None:
        return [("schema", "causation_id")]
    return []


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
    seconds = ("2026-10-26T15:00:00.000000000Z", CLOCK, "2026-10-26T15:00:02.000000000Z")
    for clock, risk_clock, causation in itertools.product(seconds, seconds, (None, SCHEDULER_TICK)):
        draft = record("01JAE3A0B000000000000000T9", "2026-10-26T15:00:02.500000000Z", clock, causation)
        draft["payload"]["risk_clock"] = risk_clock
        got = [(v.reason, v.path) for v in violations(draft)]
        if got != expected(clock, risk_clock, causation):
            problems.append(found("clocks.oracle", f"{clock} {risk_clock} {causation}: the rules and §9.5 disagree"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.301",
    "rule.302",
    "loose.301.later",
    "order.rule_302_first",
    "types.risk_clock_whole",
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
        ("the midnight base names no scheduler tick", "drafts.valid",
         mutated(lambda s: s["drafts"][MIDNIGHT].update(causation_id=None))),
        ("an invalid draft's expectation differs", "invalid_drafts",
         mutated(lambda s: case(s, "invalid_drafts", "risk_clock_after_clock")["expect"].update(path="payload.clock"))),
        ("a valid draft moves `clock` alone", "valid_drafts",
         mutated(lambda s: case(s, "valid_drafts", "later_tick")["changes"].pop())),
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
            escaped.append(f"account clock validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if check.endswith(".oracle"):
            continue
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"account clock check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"account clock vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
