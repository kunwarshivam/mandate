"""Journal spec §9.5's reference vectors for the account stream's `KillSwitchActivated` (DEC-2139):
a kill switch the executor applied on its account, closed as the executor writes it.

The schema and rules 281, 282 and 293 to 296 live in `control.py`, beside §9.5's other
account-stream records, so one validator judges every closed schema. This module builds the
`account_switches` section: a base draft of each scope, an invalid draft for every member type and
each clause of the rules, and valid drafts for what the rules might be misread to refuse. Its own
oracle reads the members directly, without the validator, and every seeded bug is shown caught.
"""

from __future__ import annotations

import copy

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

SPEC = "docs/specs/journal.md §9.5 rules 281, 282 and 293 to 296 (DEC-2139)"
ACCOUNT = f"acct:{WORKSPACE}:{ACCOUNT_STREAM_REF}"
AT = "2026-10-26T15:00:00.250000000Z"
RISK_CLOCK = "2026-10-26T15:00:01.000000000Z"
MODE = "01JAF2M0A000000000000000M1"
COPIED = "01JAF2K0A000000000000000C1"
AGENT_SWITCH = "agent"
ACCOUNT_SWITCH = "account"


def switch(event_id: str, payload: dict) -> dict:
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": event_id,
        "stream_id": ACCOUNT,
        "event_type": "KillSwitchActivated",
        "schema_version": 1,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": MODE,
        "correlation_id": None,
        "actor": dict(EXECUTOR),
        "config_refs": {},
        "payload": payload,
        "artifact_refs": [],
        "pii_refs": [],
    }


def base_drafts() -> dict[str, dict]:
    """An agent switch the executor applied from the agent stream's (a risk limit's), cancelling
    one working order and closing one instrument now and one later; and the owner's account switch,
    confirmed at a displayed bid, closing everything held through the cancel-all."""
    agent = {
        "scope": "agent",
        "subject": "agent_a",
        "initiator": "risk_limit",
        "purpose": "risk_exit",
        "cancels": ["md-a-1"],
        "closes": ["AAPL"],
        "deferred": ["MSFT"],
        "floor": None,
        "bid": None,
        "bid_size": None,
        "copied_from": COPIED,
        "risk_clock": RISK_CLOCK,
    }
    account = {
        "scope": "account",
        "subject": ACCOUNT_STREAM_REF,
        "initiator": "owner",
        "purpose": "owner_exit",
        "cancels": [],
        "closes": ["AAPL", "MSFT"],
        "deferred": [],
        "floor": "180.00",
        "bid": "190.10",
        "bid_size": "300",
        "copied_from": None,
        "risk_clock": RISK_CLOCK,
    }
    return {
        AGENT_SWITCH: switch("01JAF2K0A000000000000000K1", agent),
        ACCOUNT_SWITCH: switch("01JAF2K0A000000000000000K2", account),
    }


def invalid_drafts() -> list[dict]:
    """Each draft breaks one rule or the closed schema once, at the member it names."""

    def bad(name, clause, base, changes, path, reason="schema"):
        return control_invalid(name, clause, base, changes, reason, path)

    a, acct = AGENT_SWITCH, ACCOUNT_SWITCH
    out = [
        bad("risk_limit_owner_exit", "rule 281: a risk limit's switch sells as `risk_exit`", a,
            [change("payload.purpose", "owner_exit")], "payload.purpose"),
        bad("owner_risk_exit", "rule 281: the owner's switch sells as `owner_exit`", acct,
            [change("payload.purpose", "risk_exit")], "payload.purpose"),
        bad("operator_owner_exit", "rule 281: an operator's switch sells as `risk_exit`", a,
            [change("payload.initiator", "platform_operator"), change("payload.purpose", "owner_exit")], "payload.purpose"),
        bad("bid_without_floor", "rule 282: the confirmation's members are null together", acct,
            [change("payload.floor", None)], "payload.bid"),
        bad("no_bid", "rule 282: the confirmation's members are null together", acct,
            [change("payload.bid", None)], "payload.bid"),
        bad("no_bid_size", "rule 282: the confirmation's members are null together", acct,
            [change("payload.bid_size", None)], "payload.bid_size"),
        bad("confirmed_risk_limit", "rule 293: only the owner's switch is confirmed", a,
            [change("payload.floor", "180"), change("payload.bid", "190"), change("payload.bid_size", "300")], "payload.floor"),
        bad("account_cancels", "rule 294: the cancel-all cancels, not ids", acct,
            [change("payload.cancels", ["md-a-1"])], "payload.cancels"),
        bad("account_deferred", "rule 294: an account switch defers nothing", acct,
            [change("payload.deferred", ["TSLA"])], "payload.deferred"),
        bad("uncaused", "rule 295: caused by the mode it applied", a, [change("causation_id", None)], "causation_id"),
        bad("closed_and_deferred", "rule 296: an instrument closes now or later, not both", a,
            [change("payload.deferred", ["AAPL"])], "payload.deferred"),
        bad("rules_281_and_295", "rule 281 reports first", a,
            [change("payload.purpose", "owner_exit"), change("causation_id", None)], "payload.purpose"),
        bad("connection_scope", "§9.5: the executor's switch is an agent's or its account's", a,
            [change("payload.scope", "connection")], "payload.scope", "non_canonical"),
        bad("other_purpose", "§9.5: a switch sells to exit", a, [change("payload.purpose", "flatten")], "payload.purpose",
            "non_canonical"),
        bad("unknown_initiator", "§9.5: the three initiators", a, [change("payload.initiator", "user")], "payload.initiator",
            "non_canonical"),
        bad("copied_not_ulid", "§9.5: `copied_from` is a ulid", a, [change("payload.copied_from", "switch-1")],
            "payload.copied_from", "non_canonical"),
        bad("floor_as_number", "§9.5: `floor` is a decimal", acct, [change("payload.floor", 180)], "payload.floor"),
        bad("closes_not_a_list", "§9.5: `closes` is a list", a, [change("payload.closes", "AAPL")], "payload.closes"),
        bad("no_risk_clock", "§9.5: every member present", a, [delete("payload.risk_clock")], "payload.risk_clock"),
        bad("no_copied_from", "§9.5: every member present", a, [delete("payload.copied_from")], "payload.copied_from"),
        bad("extra_mode_event", "§9.5: closed; the agent stream's `mode_event` is not the account's", a,
            [change("payload.mode_event", MODE)], "payload.mode_event"),
        bad("version_2", "§9.5: registered at `schema_version` 1 only", a, [change("schema_version", 2)], "payload",
            "unknown_schema"),
    ]
    return out


def valid_drafts() -> list[dict]:
    """What the rules might be misread to refuse."""
    a, acct = AGENT_SWITCH, ACCOUNT_SWITCH
    return [
        control_valid("originated", "the executor's own switch copies nothing", a, [change("payload.copied_from", None)]),
        control_valid("operator", "rule 281: an operator's switch sells as `risk_exit`", a,
                      [change("payload.initiator", "platform_operator")]),
        control_valid("owner_unconfirmed", "rule 293: the owner's switch may be unconfirmed", a,
                      [change("payload.initiator", "owner"), change("payload.purpose", "owner_exit")]),
        control_valid("nothing_held", "a switch with nothing to cancel or close", a,
                      [change("payload.cancels", []), change("payload.closes", []), change("payload.deferred", [])]),
        control_valid("account_risk_limit", "rule 294: an account switch of a risk limit", acct,
                      [change("payload.initiator", "risk_limit"), change("payload.purpose", "risk_exit"),
                       change("payload.floor", None), change("payload.bid", None), change("payload.bid_size", None)]),
        control_valid("live", "either environment", acct, [change("environment", "live")]),
    ]


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = ("drafts.valid", "invalid_drafts", "valid_drafts", "drafts.oracle")


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def admitted(draft: dict) -> bool:
    """Trading spec §5.5 and the executor's writer read directly: an owner sells as `owner_exit`
    and anyone else as `risk_exit`; a displayed bid is the owner's confirmation, all three members or
    none; the account scope's cancel-all leaves nothing cancelled by id or deferred; the switch is
    caused by its mode; and nothing closes both now and later."""
    p = draft["payload"]
    owner = p["initiator"] == "owner"
    if p["purpose"] != ("owner_exit" if owner else "risk_exit"):
        return False
    confirmation = {p["floor"] is None, p["bid"] is None, p["bid_size"] is None}
    if len(confirmation) > 1 or (not owner and confirmation == {False}):
        return False
    if p["scope"] == "account" and (p["cancels"] or p["deferred"]):
        return False
    return draft["causation_id"] is not None and not set(p["closes"]) & set(p["deferred"])


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, draft in section["drafts"].items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    for case in section["invalid_drafts"]:
        draft = draft_for(section, case)
        got = violations(draft)
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
        rule_case = case["clause"].startswith("rule")
        if rule_case and admitted(draft):
            problems.append(found("drafts.oracle", f"{case['name']}: the oracle admits what the rule refuses"))
    for case in section["valid_drafts"]:
        draft = draft_for(section, case)
        got = violations(draft)
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
        if not admitted(draft):
            problems.append(found("drafts.oracle", f"{case['name']}: the oracle refuses a valid draft"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.281",
    "rule.281.any_exit",
    "rule.282",
    "order.rule_282_bid_first",
    "rule.293",
    "rule.293.any_initiator",
    "rule.294.cancels",
    "rule.294.deferred",
    "rule.295",
    "rule.296",
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
        ("the agent base is uncaused", "drafts.valid",
         mutated(lambda s: s["drafts"][AGENT_SWITCH].update(causation_id=None))),
        ("an invalid draft's expectation differs", "invalid_drafts",
         mutated(lambda s: case(s, "invalid_drafts", "account_cancels")["expect"].update(path="payload.deferred"))),
        ("a valid draft defers on an account switch", "valid_drafts",
         mutated(lambda s: case(s, "valid_drafts", "live")["changes"].append(change("payload.deferred", ["TSLA"])))),
        ("a rule case is relabelled onto an admitted draft", "drafts.oracle",
         mutated(lambda s: case(s, "invalid_drafts", "version_2").update(clause="rule 281: relabelled"))),
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
            escaped.append(f"switch validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"switch check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"switch vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
