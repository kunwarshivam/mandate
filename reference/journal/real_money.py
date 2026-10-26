"""Journal spec v0.88 §9.35's reference vectors for the customer real-money gate (DEC-2134):
`RealMoneyGateChanged`, a platform operator's setting of whether a workspace may reach an
environment other than `paper`, and `RealMoneyRefused`, the workspace API's record of a request it
refused while that setting was off.

The schemas and rules 273 and 274 live in `control.py`, beside §9.2's other control-stream records,
so one validator judges every closed schema. This module builds the `real_money` section: a base
draft of each record, an invalid draft for every member type and each clause of rules 273 and 274,
valid drafts for the cases a rule might be misread to refuse, and control streams with the setting
the gate's fold must read from each. Its own oracle reads the drafts' members directly, without the
validator, and every seeded bug is shown caught.
"""

from __future__ import annotations

import copy

from common import change, delete, digest_strings
from control import (
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

SPEC = "docs/specs/journal.md v0.88 §9.35 (DEC-2134)"
AT = "2026-10-26T09:00:00.000000000Z"
GATE_EVENT = "01JAA3G4A000000000000000G1"
REFUSED_EVENT = "01JAA3G4A000000000000000R1"
LATER = "01JAA3G4A000000000000000G2"
THIRD = "01JAA3G4A000000000000000G3"
FOURTH = "01JAA3G4A000000000000000G4"
OPERATOR = "01JAA3G0A000000000000000P1"
OTHER_OPERATOR = "01JAA3G0A000000000000000P2"
PRINCIPAL = "01JAA3G0A000000000000000M1"
OTHER_PRINCIPAL = "01JAA3G0A000000000000000M2"
DECISION = "DEC-2134"
GATE = "real_money_gate_changed"
REFUSED = "real_money_refused"
OPERATOR_ACTOR = {"kind": "platform_operator", "id": OPERATOR, "version": "1", "build": "sha256:" + "d" * 64}


def envelope(event_type: str, event_id: str, actor: dict, payload: dict) -> dict:
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": event_id,
        "stream_id": STREAM,
        "event_type": event_type,
        "schema_version": 1,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": None,
        "correlation_id": None,
        "actor": actor,
        "config_refs": {},
        "payload": payload,
        "artifact_refs": sorted(digest_strings(payload)),
        "pii_refs": [],
    }


def gate(enabled: bool = False, event_id: str = GATE_EVENT, decision: str | None = None) -> dict:
    payload = {"enabled": enabled, "operator": OPERATOR, "decision": decision}
    return envelope("RealMoneyGateChanged", event_id, dict(OPERATOR_ACTOR), payload)


def refused(event_id: str = REFUSED_EVENT, action: str = "deploy", principal: str = PRINCIPAL) -> dict:
    payload = {"action": action, "requested": "live", "principal": principal, "reason": "gate_off"}
    return envelope("RealMoneyRefused", event_id, {**USER, "id": principal}, payload)


def base_drafts() -> dict[str, dict]:
    """The off setting is the base: a draft that turns the gate on is one of the valid drafts."""
    return {GATE: gate(), REFUSED: refused()}


def invalid(base, name, clause, changes, reason, path):
    return control_invalid(name, clause, base, changes, reason, path, ())


def valid(base, name, clause, changes):
    return control_valid(name, clause, base, changes)


GATE_NULLED = ("enabled", "operator")
REFUSED_NULLED = ("action", "requested", "principal", "reason")
ONE_OF = {
    "action": ("live", "Deploy", "deploy ", "approve"),
    "requested": ("paper", "simulated", "Live"),
    "reason": ("gate_on", "Gate_off"),
}


def member_drafts() -> list[dict]:
    out = [
        invalid(GATE, "gate.enabled.kind", "§9.35 types", [change("payload.enabled", "true")], "schema", "payload.enabled"),
        invalid(GATE, "gate.enabled.int", "§9.35 types", [change("payload.enabled", 1)], "schema", "payload.enabled"),
        invalid(GATE, "gate.operator.kind", "§9.35 types", [change("payload.operator", 7)], "schema", "payload.operator"),
        invalid(GATE, "gate.operator.form", "§9.35 types", [change("payload.operator", "operator_01")], "non_canonical", "payload.operator"),
        invalid(GATE, "gate.decision.kind", "§9.35 types", [change("payload.decision", 2134)], "schema", "payload.decision"),
        invalid(GATE, "gate.missing", "§9.35 closed", [delete("payload.decision")], "schema", "payload.decision"),
        invalid(GATE, "gate.extra", "§9.35 closed", [change("payload.workspace", PRINCIPAL)], "schema", "payload.workspace"),
        invalid(REFUSED, "refused.principal.kind", "§9.35 types", [change("payload.principal", 7)], "schema", "payload.principal"),
        invalid(REFUSED, "refused.principal.form", "§9.35 types", [change("payload.principal", "user_owner_01")], "non_canonical", "payload.principal"),
        invalid(REFUSED, "refused.missing", "§9.35 closed", [delete("payload.reason")], "schema", "payload.reason"),
        invalid(REFUSED, "refused.extra", "§9.35 closed", [change("payload.mandate_version", None)], "schema", "payload.mandate_version"),
    ]
    for member in GATE_NULLED:
        path = f"payload.{member}"
        out.append(invalid(GATE, f"gate.{member}.null", "§9.35 types", [change(path, None)], "schema", path))
    for member in REFUSED_NULLED:
        path = f"payload.{member}"
        out.append(invalid(REFUSED, f"refused.{member}.null", "§9.35 types", [change(path, None)], "schema", path))
    for member, values in ONE_OF.items():
        path = f"payload.{member}"
        for value in values:
            out.append(invalid(REFUSED, f"refused.{member}.{value.strip()}{'_padded' if value != value.strip() else ''}", "§9.35 vocabulary", [change(path, value)], "non_canonical", path))
    for decision in ("DEC-0", "DEC-02134", "dec-2134", "DEC2134", "DEC-123456", "DEC-2134 "):
        out.append(
            invalid(
                GATE,
                f"gate.decision.form.{decision.strip()}{'_padded' if decision != decision.strip() else ''}",
                "§9.35 `decision_id`",
                [change("payload.enabled", True), change("payload.decision", decision)],
                "non_canonical",
                "payload.decision",
            )
        )
    return out


def rule_drafts() -> list[dict]:
    user = {**USER, "id": OPERATOR}
    system = {"kind": "system", "id": OPERATOR, "version": "1", "build": "sha256:" + "c" * 64}
    return [
        invalid(GATE, "set_by_a_user", "rule 273: a platform operator sets the gate", [change("actor", user)], "schema", "actor.kind"),
        invalid(GATE, "set_by_a_service", "rule 273: no service sets it", [change("actor", system)], "schema", "actor.kind"),
        invalid(GATE, "set_for_another_operator", "rule 273: the actor's id is the operator", [change("actor.id", OTHER_OPERATOR)], "schema", "actor.id"),
        invalid(GATE, "on_without_a_decision", "rule 273: on names its decision", [change("payload.enabled", True)], "schema", "payload.decision"),
        invalid(
            REFUSED,
            "refused_by_an_operator",
            "rule 274: the refusal is the principal's",
            [change("actor", dict(OPERATOR_ACTOR, id=PRINCIPAL))],
            "schema",
            "actor.kind",
        ),
        invalid(REFUSED, "refused_for_another_principal", "rule 274: the actor's id is the principal", [change("actor.id", OTHER_PRINCIPAL)], "schema", "actor.id"),
    ]


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule; together they cover every member type and rules 273 and 274."""
    return [
        *member_drafts(),
        *rule_drafts(),
        invalid(GATE, "gate.at_version_2", "§9.35: closed at schema version 1 only", [change("schema_version", 2)], "unknown_schema", "payload"),
        invalid(REFUSED, "refused.at_version_2", "§9.35: closed at schema version 1 only", [change("schema_version", 2)], "unknown_schema", "payload"),
    ]


def valid_drafts() -> list[dict]:
    """The cases a rule might be misread to refuse."""
    out = [
        valid(GATE, "on_with_a_decision", "rule 273: on with its decision", [change("payload.enabled", True), change("payload.decision", DECISION)]),
        valid(GATE, "off_with_a_decision", "rule 273: off may cite one", [change("payload.decision", "DEC-1")]),
        valid(GATE, "on_with_the_largest_number", "§9.35 `decision_id`: five digits", [change("payload.enabled", True), change("payload.decision", "DEC-99999")]),
        valid(
            GATE,
            "another_operator_as_itself",
            "rule 273: any operator, as itself",
            [change("payload.operator", OTHER_OPERATOR), change("actor.id", OTHER_OPERATOR)],
        ),
        valid(
            REFUSED,
            "another_principal_as_itself",
            "rule 274: any principal, as itself",
            [change("payload.principal", OTHER_PRINCIPAL), change("actor.id", OTHER_PRINCIPAL)],
        ),
    ]
    for action in ("create_version", "connect", "go_live"):
        out.append(valid(REFUSED, f"refused_{action}", "§9.35: every action", [change("payload.action", action)]))
    return out


# --------------------------------------------------------------------------- the gate's fold

def setting(records: list[dict], skip: frozenset[str] = frozenset()) -> bool:
    """§9.35: the latest `RealMoneyGateChanged`'s `enabled`, by place in the stream; none is off."""
    on = False
    seen = False
    for record in records:
        if record["event_type"] != "RealMoneyGateChanged":
            continue
        if "fold.first" in skip and seen:
            continue
        on = record["payload"]["enabled"] is True
        seen = True
    if "fold.default_on" in skip and not seen:
        return True
    return on


def stream_cases() -> list[dict]:
    def case(name, clause, records, enabled):
        return {"name": name, "clause": clause, "records": records, "expect": {"enabled": enabled}}

    on = gate(True, LATER, DECISION)
    return [
        case("none", "§9.35: a stream with no setting is off", [refused()], False),
        case("off", "§9.35: an off setting", [gate()], False),
        case("on", "§9.35: an on setting", [gate(), on], True),
        case("on_then_off", "§9.35: the latest setting, not the first", [gate(True, GATE_EVENT, DECISION), gate(False, LATER)], False),
        case("off_on_off_on", "§9.35: the latest of several", [gate(), on, gate(False, THIRD), gate(True, FOURTH, DECISION)], True),
        case("refused_after_on", "§9.35: a refusal changes no setting", [on, refused()], True),
    ]


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = (
    "drafts.valid",
    "drafts.base",
    "invalid_drafts",
    "valid_drafts",
    "streams.records",
    "streams.setting",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def oracle_setting(records: list[dict]) -> bool:
    """The setting read apart from `setting`: the records walked from the end, the first setting
    met answers, and none answers off."""
    for record in reversed(records):
        if record["event_type"] == "RealMoneyGateChanged":
            return bool(record["payload"]["enabled"])
    return False


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, draft in section["drafts"].items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    base = section["drafts"][GATE]
    if base["payload"]["enabled"] is not False or base["actor"]["kind"] != "platform_operator" or base["actor"]["id"] != base["payload"]["operator"]:
        problems.append(found("drafts.base", "the base RealMoneyGateChanged is not an operator's own off setting"))
    base = section["drafts"][REFUSED]
    if base["actor"]["kind"] != "user" or base["actor"]["id"] != base["payload"]["principal"] or base["payload"]["reason"] != "gate_off":
        problems.append(found("drafts.base", "the base RealMoneyRefused is not the principal's own gate_off refusal"))
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case))
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        got = violations(draft_for(section, case))
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
    for case in section["streams"]:
        for i, record in enumerate(case["records"]):
            got = violations(record)
            if got:
                problems.append(found("streams.records", f"{case['name']} record {i}: {got}"))
        want = case["expect"]["enabled"]
        if setting(case["records"]) != want or oracle_setting(case["records"]) != want:
            problems.append(found("streams.setting", f"{case['name']}: expected {case['expect']}"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "record.extra",
    "record.missing",
    "loose.payload.enabled",
    "loose.payload.operator",
    "loose.payload.principal",
    "loose.payload.decision",
    "nullable.payload.enabled",
    "nullable.payload.operator",
    "nullable.payload.principal",
    "types.decision_id",
    "types.decision_id_zero",
    "rule.273.operator",
    "rule.273.self",
    "rule.273.decision",
    "rule.274.user",
    "rule.274.self",
)
FOLD_MUTANTS = ("fold.first", "fold.default_on")


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, kind, name):
        return next(c for c in s[kind] if c["name"] == name)

    return [
        ("the base setting is written by a user", "drafts.valid", mutated(lambda s: s["drafts"][GATE].update(actor={**USER, "id": OPERATOR}))),
        ("the base setting is on", "drafts.base", mutated(lambda s: s["drafts"][GATE]["payload"].update(enabled=True, decision=DECISION))),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "on_without_a_decision")["expect"].update(path="payload.enabled")),
        ),
        (
            "a valid draft turns the gate on without a decision",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "on_with_a_decision")["changes"].append(change("payload.decision", None))),
        ),
        (
            "a stream record is set by a user",
            "streams.records",
            mutated(lambda s: case(s, "streams", "off")["records"][0].update(actor={**USER, "id": OPERATOR})),
        ),
        (
            "a stream case expects the first setting",
            "streams.setting",
            mutated(lambda s: case(s, "streams", "on_then_off").update(expect={"enabled": True})),
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
            escaped.append(f"real-money validator mutant {mutant}")
    for mutant in FOLD_MUTANTS:
        skip = frozenset([mutant])
        if not any(setting(c["records"], skip) != c["expect"]["enabled"] for c in section["streams"]):
            escaped.append(f"real-money fold mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"real-money check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"real-money vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
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
