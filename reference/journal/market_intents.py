"""Journal spec v0.64 rule 245's reference vectors (DEC-2037): §9.5 receives an exit proposed at
the market as `IntentReceived` `type` `market` with a null `limit_price`, at schema version 1 and 2.

Rule 245 lives in `control.py`, beside §9.5's other account-stream records, so one validator judges
every closed schema. This module builds the `market_intents` section: a version-2 and a version-1
base draft of a risk exit at the market, an invalid draft for each clause of rule 245, each reported
at the member the rule names and in the rule's order, and valid drafts for every exit purpose at
the market and for a limit exit and a limit opening. Its own oracle reads the drafts' members
directly, without the validator, and every seeded bug is shown caught.
"""

from __future__ import annotations

import copy

from common import change
from control import (
    ACCOUNT_STREAM_REF,
    AGENT,
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

SPEC = "docs/specs/journal.md v0.65 §9.5 rule 245 (DEC-2037)"
ACCOUNT = f"acct:{WORKSPACE}:{ACCOUNT_STREAM_REF}"
AT = "2026-10-19T14:30:00.250000000Z"
RISK_CLOCK = "2026-10-19T14:30:01.000000000Z"
INTENT = "01JAB0N0P000000000000000X1"
EVENT_IDS = {"exit_v2": "01JAB0N0A000000000000000E1", "exit_v1": "01JAB0N0A000000000000000E2"}
V2 = "exit_v2"
V1 = "exit_v1"
PAYLOAD = {
    "intent_id": INTENT,
    "agent_id": AGENT,
    "instrument_id": "AAPL",
    "side": "sell",
    "type": "market",
    "tif": "day",
    "qty": "10",
    "limit_price": None,
    "purpose": "risk_exit",
}
OPENINGS = ("open", "increase")
RULE = "rule 245"


def intent(name: str) -> dict:
    version = 2 if name == V2 else 1
    payload = {**PAYLOAD, **({"risk_clock": RISK_CLOCK} if version == 2 else {})}
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": EVENT_IDS[name],
        "stream_id": ACCOUNT,
        "event_type": "IntentReceived",
        "schema_version": version,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": INTENT,
        "correlation_id": None,
        "actor": dict(EXECUTOR),
        "config_refs": {"mandate_version": "sha256:" + "5" * 64},
        "payload": payload,
        "artifact_refs": [],
        "pii_refs": [],
    }


def base_drafts() -> dict[str, dict]:
    return {V2: intent(V2), V1: intent(V1)}


def limit(price: str | None = "187.40") -> list[dict]:
    return [change("payload.type", "limit"), change("payload.limit_price", price)]


def market_opening(purpose: str, price: str | None = None) -> list[dict]:
    return [change("payload.side", "buy"), change("payload.purpose", purpose), change("payload.limit_price", price)]


def invalid_drafts() -> list[dict]:
    """Each draft breaks rule 245 once, at the member the rule names first, at both versions."""
    out = []
    for base in (V2, V1):
        tag = "" if base == V2 else ".v1"

        def bad(name, clause, changes, path):
            out.append(control_invalid(f"{name}{tag}", f"{RULE}: {clause}", base, changes, "schema", path))

        bad("limit_without_price", "`limit_price` is non-null when `type` is `limit`", limit(None), "payload.limit_price")
        bad("market_with_price", "`limit_price` is null when `type` is `market`", [change("payload.limit_price", "187.40")], "payload.limit_price")
        bad("market_open", "`market` only for an exit: not `open`", market_opening("open"), "payload.type")
        bad("market_increase", "`market` only for an exit: not `increase`", market_opening("increase"), "payload.type")
        bad("stop_type", "`type` is `limit` or `market`", [change("payload.type", "stop")], "payload.type")
        bad(
            "market_open_with_price",
            "reported at the first that fails: `payload.type` before `payload.limit_price`",
            market_opening("open", "187.40"),
            "payload.type",
        )
        bad("limit_open_without_price", "an opening is a limit order with a limit", [*market_opening("open"), *limit(None)], "payload.limit_price")
    return out


def valid_drafts() -> list[dict]:
    """Every exit purpose at the market, and the limit orders rule 245 might be misread to refuse."""
    out = []
    for base in (V2, V1):
        tag = "" if base == V2 else ".v1"
        out.append(control_valid(f"market_risk_exit{tag}", f"{RULE}: an exit at the market carries no limit", base, []))
        for purpose in ("discretionary_exit", "owner_exit"):
            out.append(control_valid(f"market_{purpose}{tag}", f"{RULE}: `market` for any exit", base, [change("payload.purpose", purpose)]))
        out.append(control_valid(f"limit_exit{tag}", f"{RULE}: an exit may carry a limit", base, limit()))
        out.append(control_valid(f"limit_open{tag}", f"{RULE}: an opening is a limit order", base, [*market_opening("open"), *limit()]))
        out.append(control_valid(f"limit_increase{tag}", f"{RULE}: an increase is a limit order", base, [*market_opening("increase"), *limit()]))
    return out


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = (
    "drafts.valid",
    "drafts.market_exit",
    "invalid_drafts",
    "invalid_drafts.oracle",
    "valid_drafts",
    "valid_drafts.oracle",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def oracle_245(draft: dict) -> str | None:
    """Rule 245 read from the members alone: the member it reports, or None when it holds."""
    p = draft["payload"]
    kind, purpose, price = p.get("type"), p.get("purpose"), p.get("limit_price")
    if kind == "limit":
        return None if price is not None else "payload.limit_price"
    if kind != "market" or purpose in OPENINGS:
        return "payload.type"
    return None if price is None else "payload.limit_price"


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, draft in section["drafts"].items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
        p = draft["payload"]
        if not (p["type"] == "market" and p["limit_price"] is None and p["purpose"] not in OPENINGS):
            problems.append(found("drafts.market_exit", f"base draft {name} is not an exit at the market"))
    for case in section["invalid_drafts"]:
        draft = draft_for(section, case)
        got = violations(draft)
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
        names_245 = case["clause"].startswith(RULE)
        if not names_245 or oracle_245(draft) != case["expect"]["path"]:
            problems.append(found("invalid_drafts.oracle", f"{case['name']}: the oracle disagrees on rule 245"))
    for case in section["valid_drafts"]:
        draft = draft_for(section, case)
        got = violations(draft)
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
        if oracle_245(draft) is not None:
            problems.append(found("valid_drafts.oracle", f"{case['name']}: the oracle refuses it by rule 245"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.245",
    "rule.245.increase",
    "rule.245.other_type",
    "rule.245.market_opening",
    "rule.245.limit_null",
    "rule.245.market_priced",
    "order.rule_245_price_first",
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
        ("the base draft is at version 3", "drafts.valid", mutated(lambda s: s["drafts"][V2].update(schema_version=3))),
        (
            "the version-1 base draft is a limit exit",
            "drafts.market_exit",
            mutated(lambda s: s["drafts"][V1]["payload"].update(type="limit", limit_price="187.40")),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "market_increase")["expect"].update(path="payload.purpose")),
        ),
        (
            "the ordering draft is relabelled as a type case",
            "invalid_drafts.oracle",
            mutated(lambda s: case(s, "invalid_drafts", "market_open_with_price").update(clause="§9.5 types")),
        ),
        (
            "a valid draft is a market opening",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "limit_open")["changes"].append(change("payload.type", "market"))),
        ),
        (
            "a valid draft is a market owner exit with a limit",
            "valid_drafts.oracle",
            mutated(lambda s: case(s, "valid_drafts", "market_owner_exit.v1")["changes"].append(change("payload.limit_price", "1"))),
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
            escaped.append(f"market-intent validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"market-intent check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"market-intent vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
