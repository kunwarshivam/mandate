"""Journal spec v0.67 rules 247 and 248's reference vectors (DEC-2040): §9.5's
`OwnerConfirmationReceived`, the owner's confirmed bid journaled beside the `IntentReceived` of an
owner's exit.

Rule 247 lives in `control.py`, beside §9.5's other account-stream records, so one validator judges
every closed schema. Rule 248 is a batch rule and lives here. This module builds the
`owner_confirmations` section: the base intent and its confirmation, an invalid draft for each
clause of rule 247 and for the closed schema, valid drafts a rule might be misread to refuse, and
batches for each clause of rule 248. Its own oracles read the members directly, without the
validator, and every seeded bug is shown caught.
"""

from __future__ import annotations

import copy
from decimal import Decimal

from common import change, delete
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

SPEC = "docs/specs/journal.md v0.67 §9.5 rules 247 and 248 (DEC-2040)"
ACCOUNT = f"acct:{WORKSPACE}:{ACCOUNT_STREAM_REF}"
AT = "2026-10-19T21:30:00.250000000Z"
RISK_CLOCK = "2026-10-19T21:30:01.000000000Z"
INTENT = "01JAB1N0P000000000000000X1"
OTHER_INTENT = "01JAB1N0P000000000000000X2"
REQUEST = "01JAB1N0R000000000000000Q1"
FLOOR = "150"
INTENT_DRAFT = "intent"
CONFIRMATION = "confirmation"
EVENT_IDS = {INTENT_DRAFT: "01JAB1N0A000000000000000E1", CONFIRMATION: "01JAB1N0A000000000000000E2"}
PRICES = ("qty", "bid", "bid_size", "floor")
EXPIRES_AT = 1_790_089_200
RULE_247 = "rule 247"
RULE_248 = "rule 248"


def envelope(name: str, event_type: str, payload: dict) -> dict:
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": EVENT_IDS[name],
        "stream_id": ACCOUNT,
        "event_type": event_type,
        "schema_version": 2 if event_type == "IntentReceived" else 1,
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
    """An owner's exit at the market, received as proposed, and the confirmation beside it."""
    intent = {
        "intent_id": INTENT,
        "agent_id": AGENT,
        "instrument_id": "AAPL",
        "side": "sell",
        "type": "market",
        "tif": "day",
        "qty": "10",
        "limit_price": None,
        "purpose": "owner_exit",
        "risk_clock": RISK_CLOCK,
    }
    confirmation = {
        "intent_id": INTENT,
        "instrument_id": "AAPL",
        "qty": "10",
        "bid": "152.10",
        "bid_size": "300",
        "floor": FLOOR,
        "expires_at": EXPIRES_AT,
        "copied_from": REQUEST,
        "risk_clock": RISK_CLOCK,
    }
    return {
        INTENT_DRAFT: envelope(INTENT_DRAFT, "IntentReceived", intent),
        CONFIRMATION: envelope(CONFIRMATION, "OwnerConfirmationReceived", confirmation),
    }


def invalid_drafts() -> list[dict]:
    """Each draft breaks rule 247 or the closed schema once, at the member it names."""

    def bad(name, clause, changes, reason, path):
        return control_invalid(name, clause, CONFIRMATION, changes, reason, path)

    out = [
        bad(f"zero_{member}", f"{RULE_247}: `{member}` is positive", [change(f"payload.{member}", "0")], "schema", f"payload.{member}")
        for member in PRICES
    ]
    out += [
        bad(f"negative_{member}", f"{RULE_247}: `{member}` is positive", [change(f"payload.{member}", "-1")], "schema", f"payload.{member}")
        for member in PRICES
    ]
    out += [
        bad("zero_expires_at", f"{RULE_247}: `expires_at` is positive", [change("payload.expires_at", 0)], "schema", "payload.expires_at"),
        bad(
            "zero_bid_and_floor",
            f"{RULE_247}: reported at the first that fails, `bid` before `floor`",
            [change("payload.bid", "0"), change("payload.floor", "0")],
            "schema",
            "payload.bid",
        ),
        bad("null_floor", "§9.5: `floor` is a decimal, never null", [change("payload.floor", None)], "schema", "payload.floor"),
        bad("no_copied_from", "§9.5: every member present", [delete("payload.copied_from")], "schema", "payload.copied_from"),
        bad("extra_limit", "§9.5: the schema is closed", [change("payload.limit_price", FLOOR)], "schema", "payload.limit_price"),
    ]
    return out


def valid_drafts() -> list[dict]:
    """What rule 247 might be misread to refuse."""
    return [
        control_valid("floor_above_bid", f"{RULE_247}: the floor is the owner's, not bound to the bid", CONFIRMATION, [change("payload.floor", "160")]),
        control_valid("tiny_prices", f"{RULE_247}: positive, however small", CONFIRMATION, [change(f"payload.{m}", "0.0001") for m in PRICES]),
        control_valid("the_intent", "§9.5: the owner's exit at the market it confirms", INTENT_DRAFT, []),
    ]


# --------------------------------------------------------------------------- rule 248


def batch_failure(drafts: list[dict], skip: frozenset[str] = frozenset()) -> dict | None:
    """Each draft's own rules in draft order, then rule 248 on each confirmation in batch order:
    the record immediately before it is an `IntentReceived` (`event_type`), of the same intent
    (`payload.intent_id`), whose purpose is `owner_exit` (its `payload.purpose`), and whose limit is
    null or not below the floor (its `payload.limit_price`)."""
    for i, draft in enumerate(drafts):
        got = violations(draft)
        if got:
            return {"index": i, "reason": got[0].reason, "path": got[0].path}
    for i, draft in enumerate(drafts):
        if draft["event_type"] != "OwnerConfirmationReceived":
            continue
        p = draft["payload"]
        earlier = [d for d in drafts[:i] if d["event_type"] == "IntentReceived"]
        if "batch.248.adjacent" in skip:
            before = earlier[-1] if earlier else None
        else:
            before = drafts[i - 1] if i > 0 and drafts[i - 1]["event_type"] == "IntentReceived" else None
        if before is None:
            if "batch.248.present" in skip:
                continue
            return {"index": i, "reason": "schema", "path": "event_type"}
        q = before["payload"]
        at = drafts.index(before)
        if q["intent_id"] != p["intent_id"] and "batch.248.intent" not in skip:
            return {"index": i, "reason": "schema", "path": "payload.intent_id"}
        if q["instrument_id"] != p["instrument_id"] and "batch.248.instrument" not in skip:
            return {"index": i, "reason": "schema", "path": "payload.instrument_id"}
        if Decimal(q["qty"]) != Decimal(p["qty"]) and "batch.248.qty" not in skip:
            return {"index": i, "reason": "schema", "path": "payload.qty"}
        if q["purpose"] != "owner_exit" and "batch.248.purpose" not in skip:
            return {"index": at, "reason": "schema", "path": "payload.purpose"}
        limit = q["limit_price"]
        if limit is not None and "batch.248.limit" not in skip:
            below = Decimal(limit) <= Decimal(p["floor"]) if "boundary.batch_248_equal" in skip else Decimal(limit) < Decimal(p["floor"])
            if below:
                return {"index": at, "reason": "schema", "path": "payload.limit_price"}
    return None


def with_id(draft: dict, event_id: str, changes: list[dict]) -> dict:
    out = copy.deepcopy(draft)
    out["event_id"] = event_id
    for item in changes:
        out["payload"][item["path"].removeprefix("payload.")] = item["value"]
    return out


def batch_cases() -> list[dict]:
    drafts = base_drafts()
    intent, confirmation = drafts[INTENT_DRAFT], drafts[CONFIRMATION]

    def case(name, clause, members, expect):
        return {"name": name, "clause": clause, "drafts": members, "expect": expect}

    ok = {"outcome": "Valid"}

    def at(index, path):
        return {"outcome": "Invalid", "index": index, "reason": "schema", "path": path}

    def limited(price: str) -> dict:
        return with_id(intent, intent["event_id"], [change("payload.type", "limit"), change("payload.limit_price", price)])

    other = with_id(intent, "01JAB1N0A000000000000000E3", [change("payload.intent_id", OTHER_INTENT)])
    second = with_id(confirmation, "01JAB1N0A000000000000000E4", [])
    risk_exit = with_id(intent, intent["event_id"], [change("payload.purpose", "risk_exit")])
    larger = with_id(intent, intent["event_id"], [change("payload.qty", "100")])
    elsewhere = with_id(intent, intent["event_id"], [change("payload.instrument_id", "MSFT")])
    same_qty = with_id(intent, intent["event_id"], [change("payload.qty", "10.0")])
    return [
        case("the_intent_then_its_confirmation", f"{RULE_248}: one batch, the intent first", [intent, confirmation], ok),
        case("a_limit_at_the_floor", f"{RULE_248}: a limit not below the floor", [limited(FLOOR), confirmation], ok),
        case("a_limit_above_the_floor", f"{RULE_248}: a limit above the floor", [limited("151"), confirmation], ok),
        case("an_unconfirmed_owner_exit", f"{RULE_248}: an owner exit needs no confirmation", [intent], ok),
        case("then_another_intent", f"{RULE_248}: a later intent needs none", [intent, confirmation, other], ok),
        case("alone", f"{RULE_248}: the intent is in the batch", [confirmation], at(0, "event_type")),
        case("before_its_intent", f"{RULE_248}: the intent is immediately before it", [confirmation, intent], at(0, "event_type")),
        case("twice", f"{RULE_248}: one confirmation per intent", [intent, confirmation, second], at(2, "event_type")),
        case("after_another_intent", f"{RULE_248}: immediately after its own intent", [intent, other, confirmation], at(2, "payload.intent_id")),
        case("of_another_intent", f"{RULE_248}: the same intent", [other, confirmation], at(1, "payload.intent_id")),
        case("of_another_instrument", f"{RULE_248}: the instrument the owner confirmed", [elsewhere, confirmation], at(1, "payload.instrument_id")),
        case("a_larger_intent", f"{RULE_248}: never more than the quantity confirmed", [larger, confirmation], at(1, "payload.qty")),
        case("the_same_quantity_written_otherwise", f"{RULE_248}: quantities compared as decimals", [same_qty, confirmation], ok),
        case("of_a_risk_exit", f"{RULE_248}: only an owner's exit is confirmed", [risk_exit, confirmation], at(0, "payload.purpose")),
        case("a_limit_below_the_floor", f"{RULE_248}: never a limit below the floor", [limited("149.99"), confirmation], at(0, "payload.limit_price")),
        case("a_bad_confirmation", "each draft's own rules first", [intent, with_id(confirmation, confirmation["event_id"], [change("payload.floor", "0")])], at(1, "payload.floor")),
    ]


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = (
    "drafts.valid",
    "drafts.confirmed",
    "invalid_drafts",
    "invalid_drafts.oracle",
    "valid_drafts",
    "valid_drafts.oracle",
    "batches.rule",
    "batches.oracle",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def oracle_247(draft: dict) -> str | None:
    """Rule 247 from the members alone: the first price that is not positive, or None."""
    if draft["event_type"] != "OwnerConfirmationReceived":
        return None
    for member in (*PRICES, "expires_at"):
        value = draft["payload"].get(member)
        if isinstance(value, (str, int)) and not isinstance(value, bool) and Decimal(value) <= 0:
            return f"payload.{member}"
    return None


def oracle_248(drafts: list[dict]) -> tuple[int, str] | None:
    """Rule 248 as a walk: the last record seen, and what each confirmation finds there."""
    last: tuple[int, dict] | None = None
    for i, draft in enumerate(drafts):
        if draft["event_type"] == "OwnerConfirmationReceived":
            if last is None or last[1]["event_type"] != "IntentReceived":
                return (i, "event_type")
            j, before = last
            if before["payload"]["intent_id"] != draft["payload"]["intent_id"]:
                return (i, "payload.intent_id")
            for member in ("instrument_id", "qty"):
                mine, theirs = draft["payload"][member], before["payload"][member]
                if (Decimal(mine) != Decimal(theirs)) if member == "qty" else mine != theirs:
                    return (i, f"payload.{member}")
            if before["payload"]["purpose"] != "owner_exit":
                return (j, "payload.purpose")
            limit = before["payload"]["limit_price"]
            if limit is not None and Decimal(limit) < Decimal(draft["payload"]["floor"]):
                return (j, "payload.limit_price")
        last = (i, draft)
    return None


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, draft in section["drafts"].items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    intent, confirmation = section["drafts"].get(INTENT_DRAFT), section["drafts"].get(CONFIRMATION)
    if not (
        intent
        and confirmation
        and intent["payload"]["purpose"] == "owner_exit"
        and intent["payload"]["type"] == "market"
        and confirmation["payload"]["intent_id"] == intent["payload"]["intent_id"]
    ):
        problems.append(found("drafts.confirmed", "the base confirmation does not confirm the base owner exit at the market"))
    for case in section["invalid_drafts"]:
        draft = draft_for(section, case)
        got = violations(draft)
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
        if case["clause"].startswith(RULE_247) and oracle_247(draft) != case["expect"]["path"]:
            problems.append(found("invalid_drafts.oracle", f"{case['name']}: the oracle disagrees on rule 247"))
    for case in section["valid_drafts"]:
        draft = draft_for(section, case)
        got = violations(draft)
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
        if oracle_247(draft) is not None:
            problems.append(found("valid_drafts.oracle", f"{case['name']}: the oracle refuses it by rule 247"))
    for case in section["batches"]:
        got = batch_failure(case["drafts"])
        want = None if case["expect"]["outcome"] == "Valid" else {k: case["expect"][k] for k in ("index", "reason", "path")}
        if got != want:
            problems.append(found("batches.rule", f"{case['name']}: expected {want}, got {got}"))
        if case["clause"].startswith(RULE_248):
            expected = None if want is None else (want["index"], want["path"])
            if oracle_248(case["drafts"]) != expected:
                problems.append(found("batches.oracle", f"{case['name']}: the oracle disagrees on rule 248"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.247",
    "rule.247.bid",
    "rule.247.bid_size",
    "rule.247.floor",
    "rule.247.qty",
    "rule.247.expires_at",
    "boundary.rule_247_zero",
    "order.rule_247_floor_first",
)
BATCH_MUTANTS = (
    "batch.248.present",
    "batch.248.adjacent",
    "batch.248.intent",
    "batch.248.instrument",
    "batch.248.qty",
    "batch.248.purpose",
    "batch.248.limit",
    "boundary.batch_248_equal",
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
        ("the base confirmation has a zero bid", "drafts.valid", mutated(lambda s: s["drafts"][CONFIRMATION]["payload"].update(bid="0"))),
        (
            "the base intent is a risk exit",
            "drafts.confirmed",
            mutated(lambda s: s["drafts"][INTENT_DRAFT]["payload"].update(purpose="risk_exit")),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "zero_floor")["expect"].update(path="payload.bid")),
        ),
        (
            "the ordering draft breaks the floor only",
            "invalid_drafts.oracle",
            mutated(lambda s: case(s, "invalid_drafts", "zero_bid_and_floor")["changes"].__setitem__(0, change("payload.bid", "1"))),
        ),
        (
            "a valid draft has a zero bid size",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "floor_above_bid")["changes"].append(change("payload.bid_size", "0"))),
        ),
        (
            "a valid draft is relabelled onto the intent with a negative bid",
            "valid_drafts.oracle",
            mutated(lambda s: case(s, "valid_drafts", "the_intent").update(base_draft=CONFIRMATION, changes=[change("payload.bid", "-1")])),
        ),
        (
            "a batch's expectation differs",
            "batches.rule",
            mutated(lambda s: case(s, "batches", "twice")["expect"].update(index=1)),
        ),
        (
            "a rule-248 batch is relabelled to the draft rules",
            "batches.oracle",
            mutated(lambda s: case(s, "batches", "a_bad_confirmation").update(clause=f"{RULE_248}: relabelled")),
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
            escaped.append(f"owner-confirmation validator mutant {mutant}")
    for mutant in BATCH_MUTANTS:
        skip = frozenset([mutant])
        caught = False
        for case in section["batches"]:
            want = None if case["expect"]["outcome"] == "Valid" else {k: case["expect"][k] for k in ("index", "reason", "path")}
            caught |= batch_failure(case["drafts"], skip) != want
        if not caught:
            escaped.append(f"owner-confirmation batch mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"owner-confirmation check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"owner-confirmation vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
        "batches": batch_cases(),
    }
