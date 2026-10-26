"""Journal spec §9.5's reference vectors for `FillApplied` version 2 (DEC-2812): one broker fill
applied to the account, whose `client_order_id` is `null` for a fill that names no order of ours.

The schemas and rule 299 live in `control.py`, beside §9.5's other account-stream records, so one
validator judges every closed schema. This module builds the `fill_applied` section: a base draft of
an attributed and an unattributed fill at version 2, of version 1, and of `LateFillApplied`, an
invalid draft for each member and each clause of the rule, and valid drafts for every form of our
id that trading spec §2.3 states. Its own oracle reads the id's form directly, without the
validator, and every seeded bug is shown caught.
"""

from __future__ import annotations

import copy
import string

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

SPEC = "docs/specs/journal.md §9.5 `FillApplied` version 2 and rules 299 and 300 (DEC-2812, DEC-2817)"
ACCOUNT = f"acct:{WORKSPACE}:{ACCOUNT_STREAM_REF}"
AT = "2026-10-26T15:00:00.250000000Z"
RISK_CLOCK = "2026-10-26T15:00:01.000000000Z"
INTENT = "01JAE3A0A000000000000000F1"
OURS = f"md-{INTENT}"
REFS = {
    "fee_config": "sha256:" + "a" * 64,
    "instrument_snapshot": "sha256:" + "b" * 64,
    "settlement_calendar": "sha256:" + "c" * 64,
    "trading_calendar": "sha256:" + "d" * 64,
}
ATTRIBUTED = "attributed"
UNATTRIBUTED = "unattributed"
VERSION_1 = "version_1"
LATE = "late"
RULE_299 = "rule 299"
RULE_300 = "rule 300"
LONGEST = 128


def record(event_id: str, event_type: str, version: int, client_order_id: str | None) -> dict:
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": event_id,
        "stream_id": ACCOUNT,
        "event_type": event_type,
        "schema_version": version,
        "event_time": AT,
        "clock_source": "broker",
        "causation_id": None,
        "correlation_id": None,
        "actor": dict(EXECUTOR),
        "config_refs": dict(REFS),
        "payload": {
            "fill_id": "f-20261026-1",
            "client_order_id": client_order_id,
            "instrument_id": "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415",
            "side": "buy",
            "qty_gross": "10",
            "price": "150",
            "trade_date": "2026-10-26",
            "risk_clock": RISK_CLOCK,
            "fees": [],
        },
        "artifact_refs": [],
        "pii_refs": [],
    }


def base_drafts() -> dict[str, dict]:
    """A fill of an order of ours and a fill that names none, both at version 2; a version-1 fill
    with the id form version 1 has always admitted; and a late fill, which stays at version 1."""
    return {
        ATTRIBUTED: record("01JAE3A0A000000000000000Y1", "FillApplied", 2, OURS),
        UNATTRIBUTED: record("01JAE3A0A000000000000000Y2", "FillApplied", 2, None),
        VERSION_1: record("01JAE3A0A000000000000000Y3", "FillApplied", 1, "01J8Z3M1P0000000000000000X-1"),
        LATE: record("01JAE3A0A000000000000000Y4", "LateFillApplied", 1, OURS),
    }


def invalid_drafts() -> list[dict]:
    """Each draft breaks rule 299, the closed schema, or the registration once, at what it names."""

    def bad(name, clause, base, changes, path, reason="schema"):
        return control_invalid(name, clause, base, changes, reason, path)

    def at(member: str) -> str:
        return f"payload.{member}"

    coid = at("client_order_id")
    out = []
    for name, theirs in (
        ("another_systems_id", "theirs-1"),
        ("prefix_only", "md-"),
        ("upper_case_prefix", f"MD-{INTENT}"),
        ("no_hyphen_after_md", f"md{INTENT}"),
        ("an_underscore", "md-01JAE3A0A_00000000000000F1"),
        ("a_space", "md-01JAE3A0A 00000000000000F1"),
        ("a_dot", "md-01JAE3A0A.00000000000000F1"),
        ("a_non_ascii_letter", "md-01JAE3A0Aé0000000000000F1"),
        ("one_past_the_longest", "md-" + "A" * (LONGEST - 2)),
        ("the_intent_alone", INTENT),
    ):
        out.append(bad(f"v2_{name}", f"{RULE_299}: a named order is one of ours (trading spec §2.3)", ATTRIBUTED,
                       [change(coid, theirs)], coid))
    out += [
        bad("v2_client_order_id_empty", "§9.1: `text?` is never empty", ATTRIBUTED, [change(coid, "")], coid,
            "non_canonical"),
        bad("v2_client_order_id_number", "§9.1: `text?` is text", ATTRIBUTED, [change(coid, 7)], coid),
        bad("v2_client_order_id_missing", "§9.5: every member present, `null` when absent", UNATTRIBUTED,
            [delete(coid)], coid),
        bad("v1_unattributed", "§9.5: version 1's `client_order_id` is never null", VERSION_1, [change(coid, None)],
            coid),
        bad("late_unattributed", "§9.5: a late fill is always of an order of ours", LATE, [change(coid, None)],
            coid),
        bad("v2_fill_id_null", "§9.5: `fill_id` is never null", UNATTRIBUTED, [change(at("fill_id"), None)],
            at("fill_id")),
        bad("v2_fill_id_empty", "§9.1: `text` is never empty", UNATTRIBUTED, [change(at("fill_id"), "")],
            at("fill_id"), "non_canonical"),
        bad("v2_qty_number", "§9.1: a decimal is text", UNATTRIBUTED, [change(at("qty_gross"), 10)],
            at("qty_gross")),
        bad("v2_price_not_decimal", "§9.1: a decimal", ATTRIBUTED, [change(at("price"), "one fifty")], at("price"),
            "non_canonical"),
        bad("v2_trade_date_time", "§9.1: a date", ATTRIBUTED, [change(at("trade_date"), AT)], at("trade_date"),
            "non_canonical"),
        bad("v2_no_risk_clock", "§9.5: every member present", UNATTRIBUTED, [delete(at("risk_clock"))],
            at("risk_clock")),
        bad("v2_fee_without_status", "§9.5: a fee's members are closed", ATTRIBUTED,
            [change(at("fees"), [{"kind": "commission", "amount": "0.01", "asset": "USD"}])],
            "payload.fees[0].status"),
        bad("v2_extra_agent", "§9.5: the schema is closed; an unattributed fill names no agent", UNATTRIBUTED,
            [change(at("agent"), "agenta")], at("agent")),
        bad("v2_no_fee_config", "§9: `FillApplied` requires `fee_config`", UNATTRIBUTED,
            [delete("config_refs.fee_config")], "config_refs.fee_config", "missing_config_ref"),
        bad("v2_named_without_instrument_snapshot", f"{RULE_300}: a fill of an order of ours names its snapshot",
            ATTRIBUTED, [delete("config_refs.instrument_snapshot")], "config_refs.instrument_snapshot",
            "missing_config_ref"),
        bad("v1_without_instrument_snapshot", "§9: version 1 requires `instrument_snapshot`", VERSION_1,
            [delete("config_refs.instrument_snapshot")], "config_refs.instrument_snapshot", "missing_config_ref"),
        bad("late_without_instrument_snapshot", "§9: `LateFillApplied` requires `instrument_snapshot`", LATE,
            [delete("config_refs.instrument_snapshot")], "config_refs.instrument_snapshot", "missing_config_ref"),
        bad("v2_unattributed_without_trading_calendar", "§9.5: an unattributed fill still names both calendars",
            UNATTRIBUTED, [delete("config_refs.trading_calendar"), delete("config_refs.instrument_snapshot")],
            "config_refs.trading_calendar", "missing_config_ref"),
        bad("v2_unattributed_without_settlement_calendar", "§9.5: an unattributed fill still names both calendars",
            UNATTRIBUTED, [delete("config_refs.settlement_calendar")], "config_refs.settlement_calendar",
            "missing_config_ref"),
        bad("version_3", "§9.5: registered at `schema_version` 1 and 2 only", ATTRIBUTED,
            [change("schema_version", 3)], "payload", "unknown_schema"),
        bad("late_version_2", "§9.5: `LateFillApplied` stays at version 1", LATE, [change("schema_version", 2)],
            "payload", "unknown_schema"),
        bad("on_the_control_stream", "§9.5: an account-stream record", UNATTRIBUTED,
            [change("stream_id", f"ctl:{WORKSPACE}")], "event_type", "wrong_stream"),
    ]
    return out


def valid_drafts() -> list[dict]:
    """Every form of our id trading spec §2.3 states, the longest one, and what the rule might be
    misread to refuse: an unattributed sell, a fee, version 1's own id form, and the live
    environment."""

    def ok(name, clause, base, changes):
        return control_valid(name, clause, base, changes)

    def at(member: str) -> str:
        return f"payload.{member}"

    coid = at("client_order_id")
    forms = (
        ("replacement", "md-r-01JAE3A0A000000000000000R1"),
        ("watchdog_exit", "md-w-01JAE3A0A000000000000000W1"),
        ("kill_switch_flatten", "md-k-01JAE3A0A000000000000000K1-2"),
        ("protective_stop_leg", f"{OURS}-p01JAE3A0A000000000000000P1-sl"),
        ("ladder_rung", f"{OURS}-l2"),
        ("the_longest", "md-" + "A" * (LONGEST - 3)),
        ("lower_case_body", "md-x"),
    )
    out = [ok(f"v2_{name}", f"{RULE_299}: our id's forms (trading spec §2.3)", ATTRIBUTED, [change(coid, form)])
           for name, form in forms]
    out += [
        ok("v2_unattributed_sell", "§9.5: an unattributed fill on either side", UNATTRIBUTED,
           [change(at("side"), "sell"), change(at("qty_gross"), "0.5")]),
        ok("v2_unattributed_with_a_fee", "§9.5: an unattributed fill carries the broker's fees", UNATTRIBUTED,
           [change(at("fees"), [{"kind": "commission", "amount": "0.01", "asset": "USD", "status": "charged"}])]),
        ok("v1_another_form", "§9.5: version 1 is unchanged and binds no form", VERSION_1,
           [change(coid, "theirs-1")]),
        ok("late_another_form", "§9.5: `LateFillApplied` version 1 is unchanged", LATE, [change(coid, "theirs-1")]),
        ok("live", "§9.5: either environment", UNATTRIBUTED, [change("environment", "live")]),
        ok("v2_unattributed_without_instrument_snapshot",
           f"{RULE_300}: an unattributed fill in an instrument the platform holds no snapshot of", UNATTRIBUTED,
           [delete("config_refs.instrument_snapshot"), change("payload.instrument_id", "us_equity:OWNERPICK")]),
    ]
    return out


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = ("drafts.valid", "invalid_drafts", "valid_drafts", "forms.oracle")
BODY = frozenset(string.ascii_letters + string.digits + "-")


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def ours(named: str | None) -> bool:
    """Trading spec §2.3 read directly: nothing named, or `md-`, then a body of ASCII letters,
    digits and hyphens, at most 128 characters in all."""
    if named is None:
        return True
    body = named[3:]
    return named[:3] == "md-" and len(body) > 0 and len(named) <= LONGEST and all(c in BODY for c in body)


def candidates() -> list[str | None]:
    """Ids around every edge of the form: the prefix, each kind of character, and the length."""
    out: list[str | None] = [None]
    for prefix in ("md-", "MD-", "md", "md_", "m-", "", "xmd-"):
        for body in ("", "A", "a", "0", "-", "A-1", "A_1", "A 1", "A.1", "é", "A\n"):
            out.append(prefix + body)
    for length in (LONGEST - 1, LONGEST, LONGEST + 1):
        out.append("md-" + "A" * (length - 3))
    return out


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
    for named in candidates():
        draft = copy.deepcopy(section["drafts"][ATTRIBUTED])
        draft["payload"]["client_order_id"] = named
        broken = {v.path for v in violations(draft)}
        if ("payload.client_order_id" not in broken) != ours(named) and named != "":
            problems.append(found("forms.oracle", f"{named!r}: the rules and §2.3 disagree"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.299",
    "rule.300",
    "boundary.rule_299_length",
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
        ("the unattributed base names another system's order", "drafts.valid",
         mutated(lambda s: s["drafts"][UNATTRIBUTED]["payload"].update(client_order_id="theirs-1"))),
        ("an invalid draft's expectation differs", "invalid_drafts",
         mutated(lambda s: case(s, "invalid_drafts", "v2_prefix_only")["expect"].update(path="payload.fill_id"))),
        ("a valid draft names another system's order", "valid_drafts",
         mutated(lambda s: case(s, "valid_drafts", "v2_watchdog_exit")["changes"].append(
             change("payload.client_order_id", "w-01JAE3A0A000000000000000W1")))),
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
            escaped.append(f"fill applied validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if check.endswith(".oracle"):
            continue
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"fill applied check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"fill applied vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
