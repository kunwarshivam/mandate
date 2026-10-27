"""Journal spec v0.109's reference vectors (DEC-2848): `GateDecided` version 3, which admits the
executor's own gate members `sized_qty`, `parked` and `held_long`, each only where it writes them.

The executor journaled them on version 2, whose schema is closed, so a real journal refused the
record: a sized discretionary exit's whole batch, a parked ladder step's hold and a long hold's
record. Version 3 adds the three as nullable members, and rules 313 to 315 bind each to the decision
it belongs to:

- 313: `sized_qty` only on an allow, binding or the executor's own, and above zero;
- 314: `held_long` is `true` or `null`, and `true` only on a hold;
- 315: `parked` is `true` or `null`, and `true` only on a hold for `session_closed` or
  `session_unknown` that is also `held_long`.

The schema and the rules live in `control.py`. This module builds the `gate_v3` section: a base of
each kind of version-3 decision from the `account_stream` chain's own `GateDecided`, valid drafts
for what the rules might be misread to refuse, and an invalid draft for each clause and member type.
Its oracle reads the rules directly over a grid of every combination, without the validator, and
every seeded bug is shown caught.
"""

from __future__ import annotations

import copy
import itertools
from decimal import Decimal

from account import chain_bodies, draft_of_body
from common import change, delete
from control import check_of, draft_for, reported, violations
from control import invalid as control_invalid
from control import valid as control_valid

SPEC = "docs/specs/journal.md v0.109 §9.5 `GateDecided` version 3 and rules 313 to 315 (DEC-2848)"
BINDING = "binding_allow"
SIZED = "sized_allow"
HELD_LONG = "held_long"
PARKED = "parked"
LOCAL = "account_stream_only"
SESSIONS = ("session_closed", "session_unknown")


def at(member: str) -> str:
    return f"payload.{member}"


def version_3(draft: dict, **members) -> dict:
    """`draft`, a version-2 `GateDecided`, at version 3 with the three members, `null` unless
    given, before `risk_clock` as the table orders them."""
    out = copy.deepcopy(draft)
    out["schema_version"] = 3
    payload = out["payload"]
    clock = payload.pop("risk_clock")
    for member in ("sized_qty", "parked", "held_long"):
        payload[member] = members.get(member)
    payload["risk_clock"] = clock
    return out


def base_drafts() -> dict[str, dict]:
    """The chain's binding allow at version 3; a discretionary exit's local allow sized beside its
    ladder's rungs; a long hold; and a ladder step parked while no session is open, as the executor
    writes a park: its own decision's members and one failed `session` check."""
    gate = next(draft_of_body(b) for b in chain_bodies() if b["event_type"] == "GateDecided")
    local = copy.deepcopy(gate)
    local["payload"].update(data_profile=LOCAL, quotes_used=[], marks_used=[])
    hold = copy.deepcopy(local)
    hold["payload"].update(verdict="hold", reason_code="exit_unpriced")
    parked = copy.deepcopy(hold)
    parked["payload"].update(
        reason_code="session_closed",
        checks=[{"id": "session", "result": "fail", "inputs": {}, "computed": {}}],
    )
    return {
        BINDING: version_3(gate),
        SIZED: version_3(local, sized_qty="6"),
        HELD_LONG: version_3(hold, held_long=True),
        PARKED: version_3(parked, parked=True, held_long=True),
    }


def valid_drafts() -> list[dict]:
    """What the rules might be misread to refuse."""

    def ok(name, clause, base, changes):
        return control_valid(name, clause, base, changes)

    return [
        ok("parked_session_unknown", "rule 315: either session reason", PARKED,
           [change(at("reason_code"), "session_unknown")]),
        ok("sized_fraction", "rule 313: any quantity above zero", SIZED, [change(at("sized_qty"), "0.5")]),
        ok("sized_on_a_binding_allow", "rule 313: a binding allow the executor sized", BINDING,
           [change(at("sized_qty"), "6")]),
        ok("held_long_on_any_hold_reason", "rule 314: a hold for any reason", HELD_LONG,
           [change(at("reason_code"), "mark_freshness")]),
        ok("hold_with_nothing_of_its_own", "§9.5: each member `null` when not written", HELD_LONG,
           [change(at("held_long"), None)]),
        ok("local_deny", "§9.5: a local denial names none of the three", SIZED,
           [change(at("verdict"), "deny"), change(at("reason_code"), "sell_exceeds_available"),
            change(at("sized_qty"), None)]),
        ok("defer", "§9.5: a deferral names none of the three", BINDING,
           [change(at("verdict"), "defer"), change(at("reason_code"), "awaiting_cancel")]),
    ]


def invalid_drafts() -> list[dict]:
    """Each clause of rules 313 to 315 and each member type, refused at its member."""

    def bad(name, clause, base, changes, path, reason="schema"):
        return control_invalid(name, clause, base, changes, reason, path)

    rule_313, rule_314, rule_315 = "rule 313", "rule 314", "rule 315"
    return [
        bad("sized_on_a_hold", f"{rule_313}: only on an allow", HELD_LONG,
            [change(at("sized_qty"), "6")], at("sized_qty")),
        bad("sized_on_a_deny", f"{rule_313}: only on an allow", SIZED,
            [change(at("verdict"), "deny"), change(at("reason_code"), "sell_exceeds_available")],
            at("sized_qty")),
        bad("sized_zero", f"{rule_313}: above zero", SIZED, [change(at("sized_qty"), "0")], at("sized_qty")),
        bad("sized_negative", f"{rule_313}: above zero", SIZED, [change(at("sized_qty"), "-1")], at("sized_qty")),
        bad("sized_number", "§9.1: a decimal is text", SIZED, [change(at("sized_qty"), 6)], at("sized_qty")),
        bad("sized_not_decimal", "§9.1: a decimal", SIZED, [change(at("sized_qty"), "six")], at("sized_qty"),
            "non_canonical"),
        bad("held_long_false", f"{rule_314}: `true` or `null`", HELD_LONG, [change(at("held_long"), False)],
            at("held_long")),
        bad("held_long_on_an_allow", f"{rule_314}: only on a hold", SIZED, [change(at("held_long"), True)],
            at("held_long")),
        bad("held_long_on_a_deny", f"{rule_314}: only on a hold", HELD_LONG,
            [change(at("verdict"), "deny")], at("held_long")),
        bad("held_long_text", "§9.1: a boolean", HELD_LONG, [change(at("held_long"), "true")], at("held_long")),
        bad("parked_false", f"{rule_315}: `true` or `null`", PARKED, [change(at("parked"), False)], at("parked")),
        bad("parked_on_an_allow", f"{rule_315}: only on a hold", SIZED, [change(at("parked"), True)],
            at("parked")),
        bad("parked_unpriced", f"{rule_315}: only for a session", HELD_LONG, [change(at("parked"), True)],
            at("parked")),
        bad("parked_without_held_long", f"{rule_315}: a park is a long hold", PARKED,
            [change(at("held_long"), None)], at("parked")),
        bad("parked_text", "§9.1: a boolean", PARKED, [change(at("parked"), "true")], at("parked")),
        bad("sized_missing", "§9.5: every member present", SIZED, [delete(at("sized_qty"))], at("sized_qty")),
        bad("parked_missing", "§9.5: every member present", PARKED, [delete(at("parked"))], at("parked")),
        bad("held_long_missing", "§9.5: every member present", HELD_LONG, [delete(at("held_long"))],
            at("held_long")),
        bad("carries_the_legacy_evaluation", "§9.5: the schema is closed; `data_profile` says it", PARKED,
            [change(at("evaluation"), LOCAL)], at("evaluation")),
        bad("version_2_sized", "§9.5: version 2 is unchanged and closed", SIZED,
            [change("schema_version", 2), delete(at("parked")), delete(at("held_long"))], at("sized_qty")),
        bad("version_4", "§9.5: registered at versions 1 to 3", BINDING, [change("schema_version", 4)],
            "payload", "unknown_schema"),
    ]


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = ("drafts.valid", "invalid_drafts", "valid_drafts", "rules.oracle")
VERDICTS = ("allow", "deny", "hold", "defer")
REASONS = (None, "exit_unpriced", "session_closed", "session_unknown")
PROFILES = ("iex", LOCAL)
SIZES = (None, "6", "0")
FLAGS = (None, True, False)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def admits(verdict, reason, profile, sized, held_long, parked) -> bool:
    """Rules 313 to 315 read directly from the spec's sentences."""
    sized_ok = sized is None or (verdict == "allow" and Decimal(sized) > 0)
    held_ok = held_long is None or (held_long is True and verdict == "hold")
    parked_ok = parked is None or (parked is True and verdict == "hold" and reason in SESSIONS and held_long is True)
    return sized_ok and held_ok and parked_ok


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
    base = section["drafts"][BINDING]
    for verdict, reason, profile, sized, held_long, parked in itertools.product(
        VERDICTS, REASONS, PROFILES, SIZES, FLAGS, FLAGS
    ):
        probe = copy.deepcopy(base)
        probe["payload"].update(verdict=verdict, reason_code=reason, data_profile=profile,
                                sized_qty=sized, held_long=held_long, parked=parked)
        got = not violations(probe, skip)
        if got != admits(verdict, reason, profile, sized, held_long, parked):
            problems.append(found("rules.oracle",
                                  f"{verdict} {reason} {profile} {sized} {held_long} {parked}: the rules disagree"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

# Rule 315's own `hold` clause seeds no mutant: a park must be `held_long`, which rule 314 admits
# only on a hold, so dropping that clause alone changes no verdict.
VALIDATOR_MUTANTS = (
    "rule.313",
    "rule.313.allow",
    "rule.313.positive",
    "rule.314",
    "rule.314.true",
    "rule.314.hold",
    "rule.315",
    "rule.315.true",
    "rule.315.reason",
    "rule.315.closed_only",
    "rule.315.held_long",
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
        ("the parked base is not a long hold", "drafts.valid",
         mutated(lambda s: s["drafts"][PARKED]["payload"].update(held_long=None))),
        ("an invalid draft's expectation differs", "invalid_drafts",
         mutated(lambda s: case(s, "invalid_drafts", "sized_zero")["expect"].update(path="payload.verdict"))),
        ("a valid draft parks an unpriced hold", "valid_drafts",
         mutated(lambda s: case(s, "valid_drafts", "parked_session_unknown")["changes"].append(
             change(at("reason_code"), "exit_unpriced")))),
    ]


def run_mutants(section: dict) -> list[str]:
    """Every seeded bug caught by the cases and by the oracle; a vector mutant only by the check
    it is registered against."""
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
            escaped.append(f"gate v3 validator mutant {mutant}")
        if not any(check_of(p) == "rules.oracle" for p in check_section(section, skip)):
            escaped.append(f"gate v3 validator mutant {mutant} escapes the rules oracle")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if check.endswith(".oracle"):
            continue
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"gate v3 check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"gate v3 vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
