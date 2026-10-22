"""Journal spec v0.75 rules 258 to 260's reference vectors (DEC-2337): §9.5's
`AccountCloseRequested`, one request of one instrument's close-position under an account kill
switch, journaled before it is sent.

Rules 258 and 259 live in `control.py`, beside §9.5's other account-stream records, so one validator
judges every closed schema. Rule 260 is a stream rule and lives here. This module builds the
`account_closes` section: the base close, an invalid draft for each clause of rules 258 and 259 and
for the closed schema, valid drafts the rules might be misread to refuse, and account streams for
rule 260. Its own oracles read the members directly, without the validator, and every seeded bug is
shown caught.
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

SPEC = "docs/specs/journal.md v0.75 §9.5 rules 258 to 260 (DEC-2337)"
ACCOUNT = f"acct:{WORKSPACE}:{ACCOUNT_STREAM_REF}"
AT = "2026-10-22T15:00:00.250000000Z"
RISK_CLOCK = "2026-10-22T15:00:01.000000000Z"
SWITCH = "01JAD2K0S000000000000000K1"
OTHER_SWITCH = "01JAD2K0S000000000000000K2"
CLOSE = "close"
EVENT_ID = "01JAD2K0A000000000000000E1"
RULE_258 = "rule 258"
RULE_259 = "rule 259"
RULE_260 = "rule 260"


def base_drafts() -> dict[str, dict]:
    """The first request to close ten shares under the account switch that named them."""
    close = {
        "instrument_id": "AAPL",
        "qty": "10",
        "kill_switch_event": SWITCH,
        "attempt": 1,
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
    """Each draft breaks rule 258, rule 259, or the closed schema once, at the member it names."""

    def bad(name, clause, changes, path, reason="schema"):
        return control_invalid(name, clause, CLOSE, changes, reason, path)

    qty = "payload.qty"
    return [
        bad("zero_qty", f"{RULE_258}: `qty` is positive", [change(qty, "0")], qty),
        bad("negative_qty", f"{RULE_258}: `qty` is positive", [change(qty, "-10")], qty),
        bad("negative_zero_qty", f"{RULE_258}: `-0` normalizes to 0 (§4.6)", [change(qty, "-0")], qty),
        bad("zero_qty_with_an_exponent", f"{RULE_258}: `0e5` normalizes to 0 (§4.6)", [change(qty, "0e5")], qty),
        bad("zero_qty_written_otherwise", f"{RULE_258}: `qty` compared as a decimal", [change(qty, "0.000")], qty),
        bad("zero_attempt", f"{RULE_258}: `attempt` is at least 1", [change("payload.attempt", 0)], "payload.attempt"),
        bad(
            "zero_qty_and_attempt",
            f"{RULE_258}: reported at the first that fails, `qty` before `attempt`",
            [change(qty, "0"), change("payload.attempt", 0)],
            qty,
        ),
        bad("another_cause", f"{RULE_259}: `causation_id` is the switch", [change("causation_id", OTHER_SWITCH)], "causation_id"),
        bad("no_cause", f"{RULE_259}: `causation_id` is the switch", [change("causation_id", None)], "causation_id"),
        bad("null_qty", "§9.5: `qty` is a decimal, never null", [change(qty, None)], qty),
        bad("qty_as_number", "§9.5: `qty` is a decimal string", [change(qty, 10)], qty),
        bad("qty_beyond_28_places", "§4.6: at most 28 fractional digits", [change(qty, "0." + "0" * 28 + "1")], qty, "non_canonical"),
        bad("no_qty", "§9.5: every member present", [delete(qty)], qty),
        bad("no_instrument", "§9.5: every member present", [delete("payload.instrument_id")], "payload.instrument_id"),
        bad("null_instrument", "§9.5: `instrument_id` is text, never null", [change("payload.instrument_id", None)], "payload.instrument_id"),
        bad("empty_instrument", "§9.5: `instrument_id` is non-empty text", [change("payload.instrument_id", "")], "payload.instrument_id", "non_canonical"),
        bad("switch_not_ulid", "§9.5: `kill_switch_event` is a ulid", [change("payload.kill_switch_event", "switch-1")], "payload.kill_switch_event", "non_canonical"),
        bad("no_switch", "§9.5: every member present", [delete("payload.kill_switch_event")], "payload.kill_switch_event"),
        bad("attempt_as_text", "§9.5: `attempt` is an integer", [change("payload.attempt", "1")], "payload.attempt"),
        bad("no_attempt", "§9.5: every member present", [delete("payload.attempt")], "payload.attempt"),
        bad("no_risk_clock", "§9.5: every member present", [delete("payload.risk_clock")], "payload.risk_clock"),
        bad("extra_client_order_id", "§9.5: the schema is closed", [change("payload.client_order_id", "md-k-1")], "payload.client_order_id"),
        bad("version_2", "§9.5: registered at `schema_version` 1 only", [change("schema_version", 2)], "payload", "unknown_schema"),
        bad("on_the_control_stream", "§9.5: an account-stream record", [change("stream_id", f"ctl:{WORKSPACE}")], "event_type", "wrong_stream"),
    ]


def valid_drafts() -> list[dict]:
    """What rules 258 and 259 might be misread to refuse."""
    qty = "payload.qty"
    return [
        control_valid("a_fraction_of_a_share", f"{RULE_258}: positive, however small", CLOSE, [change(qty, "0.0001")]),
        control_valid("twelve_places", f"{RULE_258}: positive at 12 places (§4.6 allows 28)", CLOSE, [change(qty, "0.000000000001")]),
        control_valid("trailing_zeros", f"{RULE_258}: compared as a decimal", CLOSE, [change(qty, "10.500")]),
        control_valid("an_exponent", f"{RULE_258}: `1e2` normalizes to 100 (§4.6)", CLOSE, [change(qty, "1e2")]),
        control_valid("a_negative_exponent", f"{RULE_258}: `5E-1` normalizes to 0.5 (§4.6)", CLOSE, [change(qty, "5E-1")]),
        control_valid("a_later_attempt", f"{RULE_258}: a re-request's attempt", CLOSE, [change("payload.attempt", 3)]),
        control_valid(
            "crypto",
            f"{RULE_258}: any instrument the switch named, as it names it",
            CLOSE,
            [change("payload.instrument_id", "BTC/USD"), change(qty, "0.25")],
        ),
    ]


# --------------------------------------------------------------------------- rule 260


def close(event_id: str, instrument: str, switch: str, attempt: int) -> dict:
    """One request in a stream: the base close with its identity, instrument, switch and attempt."""
    out = copy.deepcopy(base_drafts()[CLOSE])
    out["event_id"] = event_id
    out["causation_id"] = switch
    out["payload"].update(instrument_id=instrument, kill_switch_event=switch, attempt=attempt)
    return out


def stream_failure(records: list[dict], skip: frozenset[str] = frozenset()) -> dict | None:
    """Rule 260 over an account stream: the first `AccountCloseRequested` whose switch, instrument
    and attempt an earlier one of the stream already names, reported at its `payload.attempt`."""
    seen: set = set()
    for i, record in enumerate(records):
        if record["event_type"] != "AccountCloseRequested":
            continue
        p = record["payload"]
        key = [p["kill_switch_event"], p["instrument_id"], p["attempt"]]
        if "stream.260.instrument" in skip:
            key[1] = None
        if "stream.260.switch" in skip:
            key[0] = None
        if "stream.260.attempt" in skip:
            key[2] = None
        if tuple(key) in seen and "stream.260.once" not in skip:
            return {"index": i, "reason": "schema", "path": "payload.attempt"}
        seen.add(tuple(key))
    return None


def stream_cases() -> list[dict]:
    def case(name, clause, records, index):
        expect = {"outcome": "Valid"} if index is None else {"outcome": "Invalid", "index": index, "reason": "schema", "path": "payload.attempt"}
        return {"name": name, "clause": clause, "records": records, "expect": expect}

    def e(n: int) -> str:
        return f"01JAD2K0A0000000000000000{n}"

    return [
        case(
            "each_instrument_once",
            f"{RULE_260}: one request per instrument per attempt",
            [close(e(1), "AAPL", SWITCH, 1), close(e(2), "BTC/USD", SWITCH, 1)],
            None,
        ),
        case(
            "a_re_request",
            f"{RULE_260}: a re-request takes the next attempt",
            [close(e(1), "AAPL", SWITCH, 1), close(e(2), "AAPL", SWITCH, 2)],
            None,
        ),
        case(
            "under_a_later_switch",
            f"{RULE_260}: a later switch closes the instrument again from attempt 1",
            [close(e(1), "AAPL", SWITCH, 1), close(e(2), "AAPL", OTHER_SWITCH, 1)],
            None,
        ),
        case(
            "duplicate",
            f"{RULE_260}: the same switch, instrument and attempt twice",
            [close(e(1), "AAPL", SWITCH, 1), close(e(2), "AAPL", SWITCH, 1)],
            1,
        ),
        case(
            "duplicate_after_another",
            f"{RULE_260}: earlier records of the stream count, not only the last",
            [close(e(1), "AAPL", SWITCH, 1), close(e(2), "BTC/USD", SWITCH, 1), close(e(3), "AAPL", SWITCH, 2), close(e(4), "AAPL", SWITCH, 1)],
            3,
        ),
    ]


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = (
    "drafts.valid",
    "drafts.closes",
    "invalid_drafts",
    "invalid_drafts.oracle",
    "valid_drafts",
    "valid_drafts.oracle",
    "streams.records",
    "streams.rule",
    "streams.oracle",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def oracle_258_259(draft: dict) -> str | None:
    """Rules 258 and 259 from the members alone: the first path that fails, or None."""
    if draft["event_type"] != "AccountCloseRequested":
        return None
    p = draft["payload"]
    value = p.get("qty")
    if isinstance(value, str) and Decimal(value) <= 0:
        return "payload.qty"
    attempt = p.get("attempt")
    if isinstance(attempt, int) and not isinstance(attempt, bool) and attempt < 1:
        return "payload.attempt"
    if draft.get("causation_id") != p.get("kill_switch_event"):
        return "causation_id"
    return None


def oracle_260(records: list[dict]) -> int | None:
    """Rule 260 counted apart from `stream_failure`: each record against every earlier one."""
    for i, record in enumerate(records):
        p = record["payload"]
        for earlier in records[:i]:
            q = earlier["payload"]
            if (q["kill_switch_event"], q["instrument_id"], q["attempt"]) == (p["kill_switch_event"], p["instrument_id"], p["attempt"]):
                return i
    return None


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, draft in section["drafts"].items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    base = section["drafts"].get(CLOSE)
    if not (base and base["payload"]["attempt"] == 1 and base["causation_id"] == base["payload"]["kill_switch_event"]):
        problems.append(found("drafts.closes", "the base close is not a first request caused by the switch it names"))
    for case in section["invalid_drafts"]:
        draft = draft_for(section, case)
        got = violations(draft)
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
        if case["clause"].startswith((RULE_258, RULE_259)) and oracle_258_259(draft) != case["expect"]["path"]:
            problems.append(found("invalid_drafts.oracle", f"{case['name']}: the oracle disagrees on rules 258 and 259"))
    for case in section["valid_drafts"]:
        draft = draft_for(section, case)
        got = violations(draft)
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
        if oracle_258_259(draft) is not None:
            problems.append(found("valid_drafts.oracle", f"{case['name']}: the oracle refuses it by rule 258 or 259"))
    for case in section["streams"]:
        for i, record in enumerate(case["records"]):
            got = violations(record)
            if got:
                problems.append(found("streams.records", f"{case['name']} record {i}: {got}"))
        want = None if case["expect"]["outcome"] == "Valid" else {k: case["expect"][k] for k in ("index", "reason", "path")}
        if stream_failure(case["records"]) != want:
            problems.append(found("streams.rule", f"{case['name']}: expected {case['expect']}"))
        if oracle_260(case["records"]) != (None if want is None else want["index"]):
            problems.append(found("streams.oracle", f"{case['name']}: the oracle disagrees on rule 260"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.258",
    "rule.258.qty",
    "rule.258.attempt",
    "boundary.rule_258_zero",
    "boundary.rule_258_attempt",
    "order.rule_258_attempt_first",
    "rule.259",
)
STREAM_MUTANTS = (
    "stream.260.once",
    "stream.260.instrument",
    "stream.260.switch",
    "stream.260.attempt",
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
        ("the base close is a second attempt", "drafts.closes", mutated(lambda s: s["drafts"][CLOSE]["payload"].update(attempt=2))),
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
            "a valid draft is moved to a zero attempt",
            "valid_drafts.oracle",
            mutated(lambda s: case(s, "valid_drafts", "a_later_attempt")["changes"].append(change("payload.attempt", 0))),
        ),
        (
            "a stream record has a zero quantity",
            "streams.records",
            mutated(lambda s: case(s, "streams", "a_re_request")["records"][1]["payload"].update(qty="0")),
        ),
        (
            "a stream's expectation differs",
            "streams.rule",
            mutated(lambda s: case(s, "streams", "duplicate")["expect"].update(outcome="Valid")),
        ),
        (
            "a stream's record is renumbered into a duplicate",
            "streams.oracle",
            mutated(lambda s: case(s, "streams", "a_re_request")["records"][1]["payload"].update(attempt=1)),
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
    for mutant in STREAM_MUTANTS:
        skip = frozenset([mutant])
        caught = False
        for case in section["streams"]:
            want = None if case["expect"]["outcome"] == "Valid" else {k: case["expect"][k] for k in ("index", "reason", "path")}
            caught |= stream_failure(case["records"], skip) != want
        if not caught:
            escaped.append(f"account-close stream mutant {mutant}")
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
        "streams": stream_cases(),
    }
