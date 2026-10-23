"""Journal spec v0.48 §9.21's reference vectors for the deployment manager's records (DEC-1170), with
v0.70's token exchange (DEC-1175): the starts, stops, and crash loops of the processes it runs,
written by workspace control services on the control stream before each takes effect.

The schemas and rules 165 to 168 and 256 live in `control.py`, beside the other closed schemas, so
one validator judges every record. This module builds the `deployment_manager` section: a base draft of
each record, an invalid draft for every member type and every rule, and valid drafts for the cases
a rule might be misread to refuse. Its own oracle reads the base drafts' payloads directly, without
the validator, and every seeded bug is shown caught.
"""

from __future__ import annotations

import copy
import re

from common import change, delete
from control import (
    AGENT,
    AGENT_STREAM,
    DM_RECORDS,
    SCHEMAS,
    SERVICES,
    STREAM,
    USER,
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

SPEC = "docs/specs/journal.md v0.70 §9.21 (DEC-1170, DEC-1175)"
AT = "2026-10-17T15:00:00.000000000Z"
ACCOUNT = "01J8Z2ACCT00000000000000A1"
CAUSE = "01J8Z3P2A000000000000000Q3"
CONNECTION = "conn_01J8Z2CONN000000000000C1"
OPAQUE = re.compile(r"^[A-Za-z0-9_-]{1,64}\Z")

PAYLOADS: dict[str, dict] = {
    "ProcessStartIssued": {
        "process": "runtime",
        "subject": AGENT,
        "generation": 1,
        "cause": "deployed",
    },
    "ProcessStopIssued": {
        "process": "runtime",
        "subject": AGENT,
        "generation": 3,
        "cause": "agent_stopped",
    },
    "ProcessCrashLooped": {
        "process": "executor",
        "subject": ACCOUNT,
        "generation": 6,
        "restarts": 5,
        "window_seconds": 600,
    },
}
EVENT_IDS = {name: f"01J8Z4DM{i:02d}".ljust(26, "0") for i, name in enumerate(DM_RECORDS)}


def snake(record: str) -> str:
    """A record's base-draft key: §4.1's key grammar admits no capital letter."""
    return re.sub(r"(?<!^)(?=[A-Z])", "_", record).lower()


def base_drafts() -> dict[str, dict]:
    return {
        snake(name): {
            "envelope_version": 1,
            "environment": "paper",
            "event_id": EVENT_IDS[name],
            "stream_id": STREAM,
            "event_type": name,
            "schema_version": 1,
            "event_time": AT,
            "clock_source": "local",
            "causation_id": None,
            "correlation_id": None,
            "actor": dict(SERVICES),
            "config_refs": {},
            "payload": copy.deepcopy(PAYLOADS[name]),
            "artifact_refs": [],
            "pii_refs": [],
        }
        for name in DM_RECORDS
    }


def invalid(name, clause, base, changes, reason, path):
    return control_invalid(f"{base}.{name}", clause, snake(base), changes, reason, path)


def valid(name, clause, base, changes):
    return control_valid(f"{base}.{name}", clause, snake(base), changes)


def wrong_form(ty) -> object | None:
    """A string the member's type refuses as `non_canonical`, or `None` when a string is refused
    as `schema` (an integer)."""
    return {"id": "not an id", "one_of": "not_listed"}.get(ty.kind)


def member_drafts() -> list[dict]:
    """Per record, per member: a value of the wrong JSON kind (`schema`), a string of the wrong
    form (`non_canonical`), and `null` (`schema`), since no member is nullable; and per record one
    missing and one extra member."""
    out = []
    for record in DM_RECORDS:
        for member, ty in SCHEMAS[("ctl", record)].fields:
            path = f"payload.{member}"
            wrong_kind = "3" if ty.kind == "int" else 7
            out.append(
                invalid(f"{member}.kind", "§9.21 types", record, [change(path, wrong_kind)], "schema", path)
            )
            form = wrong_form(ty)
            if form is not None:
                out.append(
                    invalid(f"{member}.form", "§9.21 types", record, [change(path, form)], "non_canonical", path)
                )
            out.append(invalid(f"{member}.null", "§9.21 types", record, [change(path, None)], "schema", path))
        first = SCHEMAS[("ctl", record)].fields[0][0]
        out.append(
            invalid(
                "missing", "§9.21 closed", record, [delete(f"payload.{first}")], "schema", f"payload.{first}"
            )
        )
        out.append(
            invalid("extra", "§9.21 closed", record, [change("payload.note", "x")], "schema", "payload.note")
        )
    return out


def rule_drafts() -> list[dict]:
    out = []
    for record in DM_RECORDS:
        out += [
            invalid(
                "by_a_user",
                "rule 165: workspace control services write it as a system actor",
                record,
                [change("actor", dict(USER))],
                "schema",
                "actor.kind",
            ),
            invalid(
                "on_an_agent_stream",
                "§9 row: a control-stream record",
                record,
                [change("stream_id", AGENT_STREAM)],
                "wrong_stream",
                "event_type",
            ),
            invalid(
                "at_version_2",
                "§9.21: closed at schema version 1 only",
                record,
                [change("schema_version", 2)],
                "unknown_schema",
                "payload",
            ),
        ]
        zero = [change("payload.generation", 0)]
        if record == "ProcessStartIssued":
            zero.append(change("payload.cause", "restart"))
        out.append(
            invalid(
                "generation_zero",
                "rule 166: a generation is at least 1",
                record,
                zero,
                "schema",
                "payload.generation",
            )
        )
    start = "ProcessStartIssued"
    out += [
        invalid(
            "first_start_as_restart",
            "rule 167: generation 1 is the deployed start",
            start,
            [change("payload.cause", "restart")],
            "schema",
            "payload.cause",
        ),
        invalid(
            "first_start_for_kill_switch",
            "rule 167: generation 1 is the deployed start",
            start,
            [change("payload.cause", "kill_switch")],
            "schema",
            "payload.cause",
        ),
        invalid(
            "deployed_twice",
            "rule 167: only generation 1 is deployed",
            start,
            [change("payload.generation", 2)],
            "schema",
            "payload.cause",
        ),
        invalid(
            "no_restarts",
            "rule 168: a crash loop counted a restart",
            "ProcessCrashLooped",
            [change("payload.restarts", 0)],
            "schema",
            "payload.restarts",
        ),
        invalid(
            "no_window",
            "rule 168: the window is at least a second",
            "ProcessCrashLooped",
            [change("payload.window_seconds", 0)],
            "schema",
            "payload.window_seconds",
        ),
        invalid(
            "exchange_restarted",
            "rule 167: an exchange is requested at generation 1 only",
            start,
            [
                change("payload.process", "token_exchange"),
                change("payload.subject", CONNECTION),
                change("payload.generation", 2),
                change("payload.cause", "requested"),
            ],
            "schema",
            "payload.cause",
        ),
        invalid(
            "exchange_deployed",
            "rule 256: a token exchange is started as requested",
            start,
            [change("payload.process", "token_exchange"), change("payload.subject", CONNECTION)],
            "schema",
            "payload.cause",
        ),
        invalid(
            "runtime_requested",
            "rule 256: only a token exchange is requested",
            start,
            [change("payload.cause", "requested")],
            "schema",
            "payload.cause",
        ),
        invalid(
            "exchange_stopped_for_an_agent",
            "rule 256: a token exchange is stopped as its request closed",
            "ProcessStopIssued",
            [
                change("payload.process", "token_exchange"),
                change("payload.subject", CONNECTION),
                change("payload.generation", 1),
            ],
            "schema",
            "payload.cause",
        ),
        invalid(
            "runtime_stopped_as_closed",
            "rule 256: only a token exchange stops as its request closed",
            "ProcessStopIssued",
            [change("payload.cause", "request_closed")],
            "schema",
            "payload.cause",
        ),
        invalid(
            "executor_exchanged_twice",
            "rule 167: a connect's executor is exchanged at generation 1 only",
            start,
            [
                change("payload.process", "executor"),
                change("payload.subject", ACCOUNT),
                change("payload.generation", 2),
                change("payload.cause", "exchanged"),
            ],
            "schema",
            "payload.cause",
        ),
        invalid(
            "runtime_exchanged",
            "rule 257: only an executor is started on an exchange's success",
            start,
            [change("payload.cause", "exchanged")],
            "schema",
            "payload.cause",
        ),
        invalid(
            "runtime_stopped_as_connection_closed",
            "rule 257: only an executor stops as its connection closed",
            "ProcessStopIssued",
            [change("payload.cause", "connection_closed")],
            "schema",
            "payload.cause",
        ),
        invalid(
            "exchange_looped",
            "§9.21 types: a token exchange never crash-loops",
            "ProcessCrashLooped",
            [change("payload.process", "token_exchange"), change("payload.subject", CONNECTION)],
            "non_canonical",
            "payload.process",
        ),
    ]
    return out


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule; together they cover every member type and rule."""
    return [*member_drafts(), *rule_drafts()]


def valid_drafts() -> list[dict]:
    """The cases a rule might be misread to refuse; each must be accepted."""
    start, looped = "ProcessStartIssued", "ProcessCrashLooped"
    return [
        valid(
            "executor",
            "§9.21: an executor, named by its account",
            start,
            [change("payload.process", "executor"), change("payload.subject", ACCOUNT)],
        ),
        valid(
            "restart",
            "rule 167: a later generation restarts",
            start,
            [change("payload.generation", 2), change("payload.cause", "restart")],
        ),
        valid(
            "kill_switch",
            "rule 167: a kill switch starts a later generation",
            start,
            [change("payload.generation", 7), change("payload.cause", "kill_switch"), change("causation_id", CAUSE)],
        ),
        valid("first_stopped", "rule 166: generation 1 may be stopped", "ProcessStopIssued", [change("payload.generation", 1)]),
        valid(
            "executor_stopped",
            "§9.21: an executor stopped with its last agent",
            "ProcessStopIssued",
            [change("payload.process", "executor"), change("payload.subject", ACCOUNT)],
        ),
        valid("one_restart", "rule 168: one restart is enough to record", looped, [change("payload.restarts", 1)]),
        valid(
            "one_second_window",
            "rule 168: a window of one second is enough",
            looped,
            [change("payload.window_seconds", 1)],
        ),
        valid(
            "runtime",
            "§9.21: a runtime's crash loop",
            looped,
            [change("payload.process", "runtime"), change("payload.subject", AGENT)],
        ),
        valid("live", "§9.21: either environment", start, [change("environment", "live")]),
        valid(
            "token_exchange",
            "rule 256: an OAuth connect's exchange, named by its connection",
            start,
            [
                change("payload.process", "token_exchange"),
                change("payload.subject", CONNECTION),
                change("payload.cause", "requested"),
            ],
        ),
        valid(
            "executor_exchanged",
            "rule 257: a connect's executor, started on its exchange's success",
            start,
            [
                change("payload.process", "executor"),
                change("payload.subject", ACCOUNT),
                change("payload.cause", "exchanged"),
            ],
        ),
        valid(
            "executor_connection_closed",
            "rule 257: a connect's executor stopped as its connection closed",
            "ProcessStopIssued",
            [
                change("payload.process", "executor"),
                change("payload.subject", ACCOUNT),
                change("payload.generation", 1),
                change("payload.cause", "connection_closed"),
            ],
        ),
        valid(
            "token_exchange_closed",
            "rule 256: a running exchange stopped as its request closed",
            "ProcessStopIssued",
            [
                change("payload.process", "token_exchange"),
                change("payload.subject", CONNECTION),
                change("payload.generation", 1),
                change("payload.cause", "request_closed"),
            ],
        ),
    ]


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = (
    "drafts.valid",
    "drafts.records",
    "drafts.no_text",
    "drafts.generation",
    "invalid_drafts",
    "valid_drafts",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def plain_strings(value) -> list[str]:
    """Every string in a payload that is not an opaque ID or listed value: the oracle's own
    reading of "no content", done without the validator's types."""
    if isinstance(value, dict):
        return [s for v in value.values() for s in plain_strings(v)]
    if isinstance(value, list):
        return [s for v in value for s in plain_strings(v)]
    if isinstance(value, str) and not OPAQUE.match(value):
        return [value]
    return []


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    drafts = section["drafts"]
    if sorted(drafts) != sorted(snake(r) for r in DM_RECORDS):
        problems.append(found("drafts.records", f"base drafts {sorted(drafts)} are not the three §9.21 records"))
    records = {snake(r): r for r in DM_RECORDS}
    for name, draft in drafts.items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
        if draft.get("event_type") != records.get(name) or draft.get("actor", {}).get("kind") != "system":
            problems.append(found("drafts.records", f"base draft {name} is not that record by a system actor"))
        if plain_strings(draft.get("payload")):
            problems.append(found("drafts.no_text", f"base draft {name} carries text"))
        generation = (draft.get("payload") or {}).get("generation")
        if not (isinstance(generation, int) and not isinstance(generation, bool) and generation >= 1):
            problems.append(found("drafts.generation", f"base draft {name} has no generation of at least 1"))
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
    "rule.165",
    "rule.166",
    "boundary.rule_166",
    "rule.167",
    "rule.168.restarts",
    "rule.168.window",
    "rule.256",
    "rule.256.stop",
    "rule.257",
    "rule.257.stop",
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
            "a base draft is written by a user",
            "drafts.valid",
            mutated(lambda s: s["drafts"]["process_start_issued"].update(actor=dict(USER))),
        ),
        (
            "a base draft is filed as another record",
            "drafts.records",
            mutated(lambda s: s["drafts"]["process_stop_issued"].update(event_type="ProcessStartIssued")),
        ),
        (
            "a base draft carries free text in an id-shaped member",
            "drafts.no_text",
            mutated(lambda s: s["drafts"]["process_crash_looped"]["payload"].update(subject="x" * 65)),
        ),
        (
            "a base draft carries no generation",
            "drafts.generation",
            mutated(lambda s: s["drafts"]["process_stop_issued"]["payload"].update(generation=0)),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(
                lambda s: case(s, "invalid_drafts", "ProcessStartIssued.deployed_twice")["expect"].update(
                    path="payload.generation"
                )
            ),
        ),
        (
            "a valid exchange is started as deployed",
            "valid_drafts",
            mutated(
                lambda s: case(s, "valid_drafts", "ProcessStartIssued.token_exchange")["changes"].append(
                    change("payload.cause", "deployed")
                )
            ),
        ),
        (
            "a valid draft breaks rule 167",
            "valid_drafts",
            mutated(
                lambda s: case(s, "valid_drafts", "ProcessStartIssued.restart")["changes"].append(
                    change("payload.cause", "deployed")
                )
            ),
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
            escaped.append(f"deployment-manager validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"deployment-manager check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(
                f"deployment-manager vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})"
            )
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
