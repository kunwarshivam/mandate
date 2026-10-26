"""Journal spec §9.5's reference vectors for `ExternalActivityIngested`, `RejectObserved` and
`OrderAbandoned` (DEC-2803): the broker's activity the executor did not originate, a reject it
answered with, and an intent the executor gave up on.

The schemas and rules 297 and 298 live in `control.py`, beside §9.5's other account-stream records,
so one validator judges every closed schema. This module builds the `account_activity` section: a
base draft of each record and each source, an invalid draft for every member type and each clause
of the rules, and valid drafts for what the rules might be misread to refuse. Its own oracle reads
the members directly, without the validator, and every seeded bug is shown caught.
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

SPEC = "docs/specs/journal.md §9.5 rules 297 and 298 (DEC-2803)"
ACCOUNT = f"acct:{WORKSPACE}:{ACCOUNT_STREAM_REF}"
AT = "2026-10-26T15:00:00.250000000Z"
RISK_CLOCK = "2026-10-26T15:00:01.000000000Z"
INTENT = "01JAE3A0A000000000000000N1"
EXTERNAL_ORDER = "external_order"
EXTERNAL_FILL = "external_fill"
REJECT = "reject"
ABANDONED = "abandoned"
RULE_297 = "rule 297"
RULE_298 = "rule 298"
REASONS = ("intent_too_old", "gate_recheck", "released_to_owner", "unprotected_interval_limit")
# The members whose presence each source decides, in the table's order.
SOURCED = ("broker_order_id", "fill_id", "client_order_id", "instrument", "side", "qty")


def record(event_id: str, event_type: str, payload: dict) -> dict:
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": event_id,
        "stream_id": ACCOUNT,
        "event_type": event_type,
        "schema_version": 1,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": None,
        "correlation_id": None,
        "actor": dict(EXECUTOR),
        "config_refs": {},
        "payload": {**payload, "risk_clock": RISK_CLOCK},
        "artifact_refs": [],
        "pii_refs": [],
    }


def external(event_id: str, source: str, members: dict) -> dict:
    payload = {"source": source, **dict.fromkeys(SOURCED), **members}
    return record(event_id, "ExternalActivityIngested", payload)


def abandoned(event_id: str, intent: str, reason: str, client_order_id: str | None = None) -> dict:
    payload = {
        "client_order_id": client_order_id if client_order_id is not None else f"md-{intent}",
        "intent_id": intent,
        "reason": reason,
    }
    return record(event_id, "OrderAbandoned", payload)


def base_drafts() -> dict[str, dict]:
    """An external open order and an external fill as the reconciliation journals them, a 403
    reject naming one of our orders, and an intent abandoned for its age."""
    order = {"broker_order_id": "b7c4e2", "instrument": "SPY", "side": "buy", "qty": "5"}
    reject = {"http_status": 403, "code": "40310000", "message": "insufficient buying power",
              "client_order_id": f"md-{INTENT}"}
    return {
        EXTERNAL_ORDER: external("01JAE3A0A000000000000000X1", "open_order", order),
        EXTERNAL_FILL: external("01JAE3A0A000000000000000X2", "fill", {"fill_id": "f-20261026-1"}),
        REJECT: record("01JAE3A0A000000000000000X3", "RejectObserved", reject),
        ABANDONED: abandoned("01JAE3A0A000000000000000X4", INTENT, "intent_too_old"),
    }


def invalid_drafts() -> list[dict]:
    """Each draft breaks one rule or the closed schema once, at the member it names."""

    def bad(name, clause, base, changes, path, reason="schema"):
        return control_invalid(name, clause, base, changes, reason, path)

    def at(member: str) -> str:
        return f"payload.{member}"

    out = []
    for member, value in (("broker_order_id", "b7c4e2"), ("client_order_id", "theirs-1"), ("instrument", "SPY"),
                          ("side", "sell"), ("qty", "1")):
        out.append(bad(f"fill_with_{member}", f"{RULE_297}: a fill names no `{member}`", EXTERNAL_FILL,
                       [change(at(member), value)], at(member)))
    out.append(bad("fill_reports_broker_order_id_first", f"{RULE_297}: the members in the table's order",
                   EXTERNAL_FILL, [change(at("qty"), "1"), change(at("broker_order_id"), "b7c4e2")],
                   at("broker_order_id")))
    out.append(bad("order_with_fill_id", f"{RULE_298}: an open order names no fill", EXTERNAL_ORDER,
                   [change(at("fill_id"), "f-1")], at("fill_id")))
    for member in ("instrument", "side", "qty"):
        out.append(bad(f"order_without_{member}", f"{RULE_298}: an open order has its `{member}`", EXTERNAL_ORDER,
                       [change(at(member), None)], at(member)))
    out.append(bad("order_reports_fill_id_first", f"{RULE_298}: `fill_id` before the open order's members",
                   EXTERNAL_ORDER, [change(at("qty"), None), change(at("fill_id"), "f-1")], at("fill_id")))
    out.append(bad("order_reports_instrument_before_side", f"{RULE_298}: the members in the table's order",
                   EXTERNAL_ORDER, [change(at("side"), None), change(at("instrument"), None)], at("instrument")))
    out.append(bad("fill_reports_client_order_id_before_instrument", f"{RULE_297}: the members in the table's order",
                   EXTERNAL_FILL, [change(at("instrument"), "SPY"), change(at("client_order_id"), "theirs-1")],
                   at("client_order_id")))
    for member in ("client_order_id", "instrument", "fill_id"):
        base = EXTERNAL_FILL if member == "fill_id" else EXTERNAL_ORDER
        out.append(bad(f"{member}_empty", "§9.5: an empty broker field is written `null`", base,
                       [change(at(member), "")], at(member), "non_canonical"))
    out.append(bad("order_reports_side_before_qty", f"{RULE_298}: the members in the table's order",
                   EXTERNAL_ORDER, [change(at("qty"), None), change(at("side"), None)], at("side")))
    out += [
        bad("source_unknown", "§9.5: `source` is one of its values", EXTERNAL_ORDER,
            [change(at("source"), "activity")], at("source"), "non_canonical"),
        bad("source_null", "§9.5: `source` is never null", EXTERNAL_ORDER, [change(at("source"), None)], at("source")),
        bad("side_short", "§9.5: `side` is `buy` or `sell`", EXTERNAL_ORDER, [change(at("side"), "short")], at("side"),
            "non_canonical"),
        bad("qty_number", "§9.1: a decimal is text", EXTERNAL_ORDER, [change(at("qty"), 5)], at("qty")),
        bad("qty_not_decimal", "§9.1: a decimal", EXTERNAL_ORDER, [change(at("qty"), "five")], at("qty"),
            "non_canonical"),
        bad("broker_order_id_empty", "§9.5: an empty broker field is written `null`", EXTERNAL_ORDER,
            [change(at("broker_order_id"), "")], at("broker_order_id"), "non_canonical"),
        bad("fill_id_missing", "§9.5: every member present, `null` when absent", EXTERNAL_FILL,
            [delete(at("fill_id"))], at("fill_id")),
        bad("external_extra_account", "§9.5: the schema is closed", EXTERNAL_ORDER,
            [change(at("account"), "x")], at("account")),
        bad("external_no_risk_clock", "§9.5: every member present", EXTERNAL_FILL, [delete(at("risk_clock"))],
            at("risk_clock")),
        bad("http_status_text", "§9.5: `http_status` is an integer", REJECT, [change(at("http_status"), "403")],
            at("http_status")),
        bad("http_status_null", "§9.5: `http_status` is never null", REJECT, [change(at("http_status"), None)],
            at("http_status")),
        bad("message_empty", "§9.5: an empty broker message is written `null`", REJECT,
            [change(at("message"), "")], at("message"), "non_canonical"),
        bad("code_number", "§9.5: `code` is text", REJECT, [change(at("code"), 40310000)], at("code")),
        bad("reject_without_client_order_id", "§9.5: every member present, `null` when absent", REJECT,
            [delete(at("client_order_id"))], at("client_order_id")),
        bad("reject_extra_account_number", "§9.5: the schema is closed", REJECT,
            [change(at("account_number"), "x")], at("account_number")),
        bad("abandoned_reason_unknown", "§9.5: `reason` is one of the four", ABANDONED,
            [change(at("reason"), "max_intent_age")], at("reason"), "non_canonical"),
        bad("abandoned_reason_empty", "§9.5: `reason` is one of the four", ABANDONED, [change(at("reason"), "")],
            at("reason"), "non_canonical"),
        bad("abandoned_reason_null", "§9.5: `reason` is never null", ABANDONED, [change(at("reason"), None)],
            at("reason")),
        bad("abandoned_intent_not_an_id", "§9.1: an id", ABANDONED, [change(at("intent_id"), "an intent")],
            at("intent_id"), "non_canonical"),
        bad("abandoned_client_order_id_null", "§9.5: the derived id is never null", ABANDONED,
            [change(at("client_order_id"), None)], at("client_order_id")),
        bad("version_2", "§9.5: registered at `schema_version` 1 only", ABANDONED, [change("schema_version", 2)],
            "payload", "unknown_schema"),
        bad("on_the_control_stream", "§9.5: an account-stream record", REJECT,
            [change("stream_id", f"ctl:{WORKSPACE}")], "event_type", "wrong_stream"),
    ]
    return out


def valid_drafts() -> list[dict]:
    """What the rules might be misread to refuse: a broker's empty ids, a foreign client order id, a
    reject with nothing but its status, each reason, and the executor's own exits' intent ids."""

    def ok(name, clause, base, changes):
        return control_valid(name, clause, base, changes)

    def at(member: str) -> str:
        return f"payload.{member}"

    out = [
        ok("order_without_broker_order_id", f"{RULE_298}: a broker may send an empty order id", EXTERNAL_ORDER,
           [change(at("broker_order_id"), None)]),
        ok("order_with_their_client_order_id", f"{RULE_298}: another system's client order id", EXTERNAL_ORDER,
           [change(at("client_order_id"), "theirs-1")]),
        ok("order_with_an_md_id_we_do_not_hold", f"{RULE_298}: an id of our form the executor does not know",
           EXTERNAL_ORDER, [change(at("client_order_id"), "md-01JAE3A0A000000000000000N9")]),
        ok("abandoned_any_client_order_id", "§9.5: no rule binds the id to the intent", ABANDONED,
           [change(at("client_order_id"), "md-01JAE3A0A000000000000000N2")]),
        ok("order_sell_fractional", "§9.5: either side, a decimal quantity", EXTERNAL_ORDER,
           [change(at("side"), "sell"), change(at("qty"), "0.5")]),
        ok("fill_without_fill_id", f"{RULE_297}: a broker may send an empty fill id", EXTERNAL_FILL,
           [change(at("fill_id"), None)]),
        ok("reject_status_only", "§9.5: every broker member may be empty", REJECT,
           [change(at("code"), None), change(at("message"), None), change(at("client_order_id"), None)]),
        ok("reject_server_error", "§9.5: any status", REJECT, [change(at("http_status"), 500)]),
        ok("reject_naming_another_order", "§9.5: the reject's id may not be ours", REJECT,
           [change(at("client_order_id"), "theirs-1")]),
        ok("live", "§9.5: either environment", REJECT, [change("environment", "live")]),
    ]
    for reason in REASONS:
        out.append(ok(f"abandoned_{reason}", "§9.5: each reason", ABANDONED, [change(at("reason"), reason)]))
    for intent in ("w-01JAE3A0A000000000000000W1", "k-01JAE3A0A000000000000000K1-2"):
        out.append(ok(f"abandoned_{intent[0]}_intent", "§9.5: the executor's own exit's intent is an `id`", ABANDONED,
                      [change(at("intent_id"), intent), change(at("client_order_id"), f"md-{intent}")]))
    return out


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = ("drafts.valid", "invalid_drafts", "valid_drafts", "sources.oracle")


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def source_admits(source: str, present: dict[str, bool]) -> bool:
    """§9.5's two sources read directly: a fill names only its fill id; an open order names no
    fill id, and always its instrument, side and quantity."""
    if source == "fill":
        return not any(present[m] for m in SOURCED if m != "fill_id")
    return not present["fill_id"] and all(present[m] for m in ("instrument", "side", "qty"))


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
    values = {"broker_order_id": "b1", "fill_id": "f1", "client_order_id": "c1", "instrument": "SPY",
              "side": "buy", "qty": "1"}
    for source in ("open_order", "fill"):
        for mask in itertools.product((False, True), repeat=len(SOURCED)):
            present = dict(zip(SOURCED, mask, strict=True))
            members = {m: values[m] if on else None for m, on in present.items()}
            draft = external("01JAE3A0A000000000000000X9", source, members)
            if (not violations(draft)) != source_admits(source, present):
                problems.append(found("sources.oracle", f"{source} {present}: the rules and §9.5 disagree"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.297",
    "rule.298",
    "rule.297.broker_order_id",
    "rule.297.order",
    "rule.298.side",
    "rule.298.fill_id",
    "rule.298.order",
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
        ("the fill base names an instrument", "drafts.valid",
         mutated(lambda s: s["drafts"][EXTERNAL_FILL]["payload"].update(instrument="SPY"))),
        ("an invalid draft's expectation differs", "invalid_drafts",
         mutated(lambda s: case(s, "invalid_drafts", "order_with_fill_id")["expect"].update(path="payload.qty"))),
        ("a valid draft abandons for an unknown reason", "valid_drafts",
         mutated(lambda s: case(s, "valid_drafts", "abandoned_gate_recheck")["changes"].append(
             change("payload.reason", "max_intent_age")))),
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
            escaped.append(f"account activity validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if check.endswith(".oracle"):
            continue
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"account activity check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"account activity vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
