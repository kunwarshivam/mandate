"""Journal spec v0.108's reference vectors (DEC-2845): §9.1's `intent` type on every account-stream
record the executor's own exits write.

Trading spec §2.3 names a kill switch's flatten `k-{switch}-{n}` and the triggered-stop watchdog's
exit `w-{event}`. `IntentReceived` and `GateDecided` at versions 1 and 2, `OrderRequestRecorded`, and
`ProtectionChanged` at versions 1 and 2 type their `intent_id` as `intent`: a ULID, or exactly one of
those two forms, `n` a decimal from 0 to 999 with no leading zero. The schemas live in `control.py`,
so one validator judges every closed schema. This module builds the `exit_intents` section: a base
draft of each of the seven record versions, the `account_stream` chain's own payloads, a valid draft
of each with each form, and an invalid draft of each with every near miss. Its oracle reads the form
with its own expression, without the validator, over the cases and a generated set of candidates,
and every seeded bug is shown caught.
"""

from __future__ import annotations

import copy
import re

from account import chain_bodies, draft_of_body
from common import change
from control import check_of, draft_for, reported, violations
from control import invalid as control_invalid
from control import valid as control_valid

SPEC = "docs/specs/journal.md v0.108 §9.1's `intent` type (DEC-2845)"
SWITCH = "01JAE3A0A000000000000000K1"
EVENT = "01JAE3A0A000000000000000W1"
INTENT = "intent_id"
PATH = f"payload.{INTENT}"

# The seven record versions, each the chain draft it is built from and, for a version the chain
# does not hold, how it differs.
RECEIVED_V1 = "received_v1"
RECEIVED_V2 = "received_v2"
GATE_V1 = "gate_v1"
GATE_V2 = "gate_v2"
REQUEST = "request"
PROTECTION_V1 = "protection_v1"
PROTECTION_V2 = "protection_v2"
BASES = (RECEIVED_V1, RECEIVED_V2, GATE_V1, GATE_V2, REQUEST, PROTECTION_V1, PROTECTION_V2)

OWN = {
    "watchdog": f"w-{EVENT}",
    "flatten_first": f"k-{SWITCH}-0",
    "flatten_third": f"k-{SWITCH}-2",
    "flatten_last": f"k-{SWITCH}-999",
}
NEAR_MISSES = {
    "flatten_without_n": f"k-{SWITCH}",
    "flatten_without_dash": f"k-{SWITCH}1",
    "flatten_empty_n": f"k-{SWITCH}-",
    "flatten_leading_zero": f"k-{SWITCH}-01",
    "flatten_n_1000": f"k-{SWITCH}-1000",
    "flatten_negative_n": f"k-{SWITCH}--1",
    "flatten_n_letter": f"k-{SWITCH}-1a",
    "flatten_n_not_ascii": f"k-{SWITCH}-٣",
    "flatten_two_ns": f"k-{SWITCH}-1-2",
    "flatten_upper_case_prefix": f"K-{SWITCH}-1",
    "flatten_lower_case_switch": f"k-{SWITCH.lower()}-1",
    "flatten_switch_not_a_ulid": f"k-{SWITCH[:-1]}-1",
    "watchdog_upper_case_prefix": f"W-{EVENT}",
    "watchdog_with_n": f"w-{EVENT}-1",
    "watchdog_event_not_a_ulid": f"w-{EVENT}Z",
    "another_prefix": f"x-{EVENT}",
    "ulid_with_suffix": f"{EVENT}-1",
    "the_order_id": f"md-k-{SWITCH}-1",
    "a_random_id": "flatten-1",
    "empty": "",
    "watchdog_event_after_7": f"w-8{EVENT[1:]}",
    "flatten_switch_after_7": f"k-8{SWITCH[1:]}-1",
    "flatten_trailing_newline": f"k-{SWITCH}-1\n",
    "watchdog_leading_space": f" w-{EVENT}",
}


def base_drafts() -> dict[str, dict]:
    """The `account_stream` chain's `IntentReceived` version 2, `GateDecided` version 2, the
    companion, and the `unprotected_start`, each as a writer's draft; version 1 of the first two
    without `risk_clock`; and `ProtectionChanged` version 2 with no unprotected fraction."""
    chain = {body["event_id"]: draft_of_body(body) for body in chain_bodies()}
    by_type = {}
    for draft in chain.values():
        by_type.setdefault((draft["event_type"], draft["payload"].get("action")), draft)
    received = copy.deepcopy(by_type[("IntentReceived", None)])
    gate = copy.deepcopy(by_type[("GateDecided", None)])
    request = copy.deepcopy(by_type[("OrderRequestRecorded", None)])
    start = copy.deepcopy(by_type[("ProtectionChanged", "unprotected_start")])

    def version_1(draft: dict) -> dict:
        out = copy.deepcopy(draft)
        out["schema_version"] = 1
        del out["payload"]["risk_clock"]
        return out

    protection_v2 = copy.deepcopy(start)
    protection_v2["schema_version"] = 2
    clock = protection_v2["payload"].pop("risk_clock")
    protection_v2["payload"]["unprotected_fraction"] = None
    protection_v2["payload"]["risk_clock"] = clock
    return {
        RECEIVED_V1: version_1(received),
        RECEIVED_V2: received,
        GATE_V1: version_1(gate),
        GATE_V2: gate,
        REQUEST: request,
        PROTECTION_V1: start,
        PROTECTION_V2: protection_v2,
    }


def valid_drafts() -> list[dict]:
    """Each record version with each form of the executor's own exit's intent."""
    return [
        control_valid(
            f"{base}_{form}",
            "§9.1: an `intent` is a ULID or the executor's own exit's, trading spec §2.3",
            base,
            [change(PATH, value)],
        )
        for base in BASES
        for form, value in OWN.items()
    ]


def invalid_drafts() -> list[dict]:
    """Each record version with every near miss, and with a number, each refused at `intent_id`."""
    out = [
        control_invalid(
            f"{base}_{miss}",
            "§9.1: exactly `w-{ulid}` or `k-{ulid}-{n}`, `n` from 0 to 999 with no leading zero",
            base,
            [change(PATH, value)],
            "non_canonical",
            PATH,
        )
        for base in BASES
        for miss, value in NEAR_MISSES.items()
    ]
    out += [
        control_invalid(f"{base}_a_number", "§9.1: an `intent` is text", base, [change(PATH, 7)], "schema", PATH)
        for base in BASES
    ]
    return out


# --------------------------------------------------------------------------- independent oracle

ULID_FORM = "[0-7][0-9A-HJKMNP-TV-Z]{25}"
INTENT_FORM = re.compile(rf"(?:{ULID_FORM}|w-{ULID_FORM}|k-{ULID_FORM}-(?:0|[1-9][0-9]{{0,2}}))")
ORACLE_CHECKS = ("drafts.valid", "invalid_drafts", "valid_drafts", "forms.oracle")


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def candidates() -> list[str]:
    """Every form and near miss, and each ordinal around the bounds, with and without a prefix."""
    out = [EVENT, SWITCH, *OWN.values(), *NEAR_MISSES.values()]
    for n in (*range(0, 12), 99, 100, 101, 998, 999, 1000, 1001, 9999):
        out += [f"k-{SWITCH}-{n}", f"k-{SWITCH}-0{n}", f"w-{EVENT}-{n}", f"{SWITCH}-{n}"]
    for prefix in ("w", "k", "W", "K", "m", "md-w", "md-k", "ww", "kk"):
        out += [f"{prefix}-{EVENT}", f"{prefix}-{EVENT}-1"]
    return out


def check_section(section: dict, skip: frozenset = frozenset()) -> list[str]:
    """Every failure, so seeded bugs can be shown caught; `skip` seeds a validator bug, which the
    oracle must then report on its own."""
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
    for base, draft in section["drafts"].items():
        for value in candidates():
            probe = copy.deepcopy(draft)
            probe["payload"][INTENT] = value
            admitted = not violations(probe, skip)
            if admitted != bool(INTENT_FORM.fullmatch(value)):
                problems.append(found("forms.oracle", f"{base} {value!r}: the validator and §9.1 disagree"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "types.intent_own",
    "types.intent_any",
    "types.intent_bound",
    "types.intent_zero",
    "types.intent_ascii",
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
        ("the gate base names a flatten with no ordinal", "drafts.valid",
         mutated(lambda s: s["drafts"][GATE_V2]["payload"].update(intent_id=f"k-{SWITCH}"))),
        ("an invalid draft's expectation differs", "invalid_drafts",
         mutated(lambda s: case(s, "invalid_drafts", f"{REQUEST}_flatten_leading_zero")["expect"].update(
             path="payload.purpose"))),
        ("a valid draft's flatten has a leading zero", "valid_drafts",
         mutated(lambda s: case(s, "valid_drafts", f"{PROTECTION_V2}_flatten_third")["changes"].append(
             change(PATH, f"k-{SWITCH}-02")))),
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
            escaped.append(f"exit intent validator mutant {mutant}")
        oracle = [p for p in check_section(section, skip) if check_of(p) == "forms.oracle"]
        if not oracle:
            escaped.append(f"exit intent validator mutant {mutant} escapes the forms oracle")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if check.endswith(".oracle"):
            continue
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"exit intent check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"exit intent vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
