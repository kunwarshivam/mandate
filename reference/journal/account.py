"""Journal spec v0.13 §9.5's reference vectors (DEC-446, DEC-447): the account stream's executor records.

The schemas and consistency rules 39 to 44 live in `control.py`, beside §9.2's, §9.3's and §9.4's,
so one validator judges every closed schema; the batch rule 45 is checked here against the same
drafts. This module builds the `account_stream` section: a hash-chained account stream from
`StreamOpened` at seq 1, with the `intended` record in its intent's batch, the companion and its
order in the next, a `placed` and an `unprotected_start` for replay, the fold oracle the chain
replays to — the restored protection and the request the companion rebuilds — an invalid draft for
every rule and each member-typing case, valid drafts for the cases a rule might be misread to
refuse, and rule 45's valid and invalid batches. It checks the section with its own oracles,
including an independent re-derivation of the fold oracle from the chain payloads alone, and shows
every seeded bug caught. It also builds the `protection_shapes` section of v0.33 (DEC-859): the
`ProtectionChanged` records rules 41 and 44 now admit, and the near misses they still refuse.
"""

from __future__ import annotations

import copy

from common import (
    canon,
    change,
    delete,
    hash_chain,
    is_ulid,
    sha256_hex,
)
from control import (
    ACCOUNT_STREAM_REF_ALT,
    AGENT,
    EXECUTOR,
    WORKSPACE,
    draft_for,
    reported,
    violations,
)

SPEC = "docs/specs/journal.md v0.13 §9.5 (DEC-446, DEC-447)"
STREAM = f"acct:{WORKSPACE}:{ACCOUNT_STREAM_REF_ALT}"
INSTRUMENT = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415"
INTENT = "01J8ZT90A000000000000000P1"
PROPOSAL = "01J8ZT80A000000000000000D1"
ENTRY = f"md-{INTENT}"
LEG = f"{ENTRY}-p1"
assert all(is_ulid(u) for u in (INTENT, PROPOSAL)), "ulids (§3, §9.1)"

CHAIN = (
    ("opened", "01J8ZQA0A000000000000000E1"),
    ("intent", "01J8ZQA1A000000000000000E2"),
    ("intended", "01J8ZQA2A000000000000000E3"),
    ("gate", "01J8ZQA3A000000000000000E4"),
    ("request", "01J8ZQA4A000000000000000E5"),
    ("submitted", "01J8ZQA5A000000000000000E6"),
    ("placed", "01J8ZQA6A000000000000000E7"),
    ("start", "01J8ZQA7A000000000000000E8"),
)
assert all(is_ulid(i) for _, i in CHAIN), "event IDs (§3)"
SEQ = {name: seq for seq, (name, _) in enumerate(CHAIN, start=1)}
ID = dict(CHAIN)

# The append batches the chain was written in: the intended record in its intent's batch, the
# companion and its order in the next (§9.5, rule 45).
BATCHES = [[1], [2, 3], [4], [5, 6], [7], [8]]

CLOCK_BASE = "2026-09-21T14:00"
TIMES = {
    name: f"{CLOCK_BASE}:{(seq - 1) * 5:02d}.000000000Z"
    for seq, (name, _) in enumerate(CHAIN, start=1)
}


def clock(name: str) -> str:
    return TIMES[name]


def event(name: str, event_type: str, version: int, payload: dict, causation: str | None) -> dict:
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": ID[name],
        "stream_id": STREAM,
        "seq": SEQ[name],
        "event_type": event_type,
        "schema_version": version,
        "event_time": clock(name),
        "recorded_at": clock(name)[:-4] + "400Z",
        "clock_source": "local",
        "causation_id": causation,
        "correlation_id": None,
        "actor": dict(EXECUTOR),
        "config_refs": {},
        "payload": payload,
        "artifact_refs": sorted(
            v for v in payload.values() if isinstance(v, str) and v.startswith("sha256:")
        ),
        "pii_refs": [],
        "prev_hash": None,
    }


def chain_bodies() -> list[dict]:
    """One chain event per closed schema, in the order the executor writes them."""
    opened = event("opened", "StreamOpened", 1, {"stream_type": "account", "workspace_id": WORKSPACE, "broker": "alpaca", "account_ref": ACCOUNT_STREAM_REF_ALT}, None)
    opened["config_refs"] = {}
    intent = event(
        "intent",
        "IntentReceived",
        2,
        {
            "intent_id": INTENT,
            "agent_id": AGENT,
            "instrument_id": INSTRUMENT,
            "side": "buy",
            "type": "limit",
            "tif": "day",
            "qty": "10",
            "limit_price": "150",
            "purpose": "open",
            "risk_clock": clock("intent"),
        },
        PROPOSAL,
    )
    intent["config_refs"] = {"mandate_version": "sha256:" + "5" * 64}
    intended = event(
        "intended",
        "ProtectionChanged",
        1,
        {
            "instrument_id": INSTRUMENT,
            "action": "intended",
            "orders": [],
            "awaiting": [],
            "qty": None,
            "stop": "139",
            "take_profit": "159",
            "intent_id": INTENT,
            "bracket": None,
            "entry": None,
            "agent_id": None,
            "replacing": None,
            "created_on": None,
            "sent": None,
            "uncovered": None,
            "acknowledged": None,
            "risk_clock": clock("intended"),
        },
        ID["intent"],
    )
    gate = event(
        "gate",
        "GateDecided",
        2,
        {
            "intent_id": INTENT,
            "verdict": "allow",
            "reason_code": None,
            "data_profile": "iex",
            "quotes_used": [
                {"instrument_id": INSTRUMENT, "bid": "149.98", "ask": "150.02", "as_of": clock("intent"), "feed": "iex"}
            ],
            "marks_used": [
                {"instrument_id": INSTRUMENT, "price": "149.98", "source": "quote", "kind": "risk"}
            ],
            "checks": [
                {"id": "buying_power", "result": "pass", "inputs": {"buying_power": "10000"}, "computed": {"order_cost": "1500"}},
                {"id": "day_trade_budget", "result": "not_applicable", "inputs": {"regime": "intraday_margin"}, "computed": {}},
            ],
            "risk_clock": clock("gate"),
        },
        ID["intent"],
    )
    gate["config_refs"] = {
        "fee_config": "sha256:" + "1" * 64,
        "trading_calendar": "sha256:" + "7" * 64,
        "instrument_snapshot": "sha256:" + "4" * 64,
        "rule_set": "sha256:" + "2" * 64,
        "mandate_version": "sha256:" + "5" * 64,
    }
    request = event(
        "request",
        "OrderRequestRecorded",
        1,
        {
            "agent_id": AGENT,
            "intent_id": INTENT,
            "purpose": "open",
            "extended_hours": False,
            "stop_price": None,
            "order_class": "bracket",
            "take_profit": "159",
            "stop": "139",
            "rung": None,
            "at_floor": None,
            "risk_clock": clock("request"),
        },
        None,
    )
    submitted = event(
        "submitted",
        "OrderSubmitted",
        2,
        {
            "client_order_id": ENTRY,
            "attempt": 1,
            "instrument_id": INSTRUMENT,
            "side": "buy",
            "type": "limit",
            "tif": "gtc",
            "qty": "10",
            "limit_price": "150",
            "risk_clock": clock("submitted"),
        },
        ID["request"],
    )
    placed = event(
        "placed",
        "ProtectionChanged",
        1,
        {
            "instrument_id": INSTRUMENT,
            "action": "placed",
            "orders": [LEG],
            "awaiting": [],
            "qty": "10",
            "stop": "139",
            "take_profit": "159",
            "intent_id": None,
            "bracket": None,
            "entry": None,
            "agent_id": None,
            "replacing": None,
            "created_on": "2026-09-21",
            "sent": None,
            "uncovered": None,
            "acknowledged": None,
            "risk_clock": clock("placed"),
        },
        None,
    )
    start = event(
        "start",
        "ProtectionChanged",
        1,
        {
            "instrument_id": INSTRUMENT,
            "action": "unprotected_start",
            "orders": [LEG],
            "awaiting": [],
            "qty": None,
            "stop": "139",
            "take_profit": "159",
            "intent_id": INTENT,
            "bracket": None,
            "entry": ENTRY,
            "agent_id": AGENT,
            "replacing": True,
            "created_on": None,
            "sent": None,
            "uncovered": None,
            "acknowledged": None,
            "risk_clock": clock("start"),
        },
        None,
    )
    return [opened, intent, intended, gate, request, submitted, placed, start]


# --------------------------------------------------------------------------- rule 45's batches


def batch_case(name: str, clause: str, members: list[dict], expect: dict) -> dict:
    return {"name": name, "clause": clause, "drafts": members, "expect": expect}


def invalid(name: str, clause: str, base: int, changes: list[dict], reason: str, path: str, also=()):
    """`also` lists, in report order, the later rules a draft breaks on purpose: the journal
    reports only the first, and the generator checks the whole order."""
    expect = {"outcome": "Invalid", "reason": reason, "path": path}
    if also:
        expect["also"] = [{"reason": r, "path": at} for r, at in also]
    return {"name": name, "clause": clause, "base_seq": base, "changes": changes, "expect": expect}


def valid(name: str, clause: str, base: int, changes: list[dict]):
    return {"name": name, "clause": clause, "base_seq": base, "changes": changes, "expect": {"outcome": "Valid"}}


def valid_batches() -> list[dict]:
    return [
        batch_case(
            "the_companion_and_its_order_in_one_batch",
            "rule 45: one batch writes both",
            [{"base_seq": SEQ["request"], "changes": []}, {"base_seq": SEQ["submitted"], "changes": []}],
            {"outcome": "Committed"},
        ),
    ]


def invalid_batches() -> list[dict]:
    return [
        batch_case(
            "a_submission_without_its_companion",
            "rule 45: no OrderRequestRecorded immediately before it",
            [
                {"base_seq": SEQ["gate"], "changes": []},
                {"base_seq": SEQ["submitted"], "changes": []},
            ],
            {"outcome": "Invalid", "draft_index": 1, "reason": "schema", "path": "event_type"},
        ),
        batch_case(
            "two_companions_before_a_submission",
            "rule 45: more than one in the run of records immediately before it",
            [
                {"base_seq": SEQ["request"], "changes": []},
                {"base_seq": SEQ["request"], "changes": [change("event_id", "01J8ZQA8A000000000000000E9")]},
                {"base_seq": SEQ["submitted"], "changes": []},
            ],
            {"outcome": "Invalid", "draft_index": 2, "reason": "schema", "path": "event_type"},
        ),
        batch_case(
            "a_submission_naming_another_event",
            "rule 45: the causation_id does not name the companion",
            [
                {"base_seq": SEQ["request"], "changes": []},
                {"base_seq": SEQ["submitted"], "changes": [change("causation_id", ID["gate"])]},
            ],
            {"outcome": "Invalid", "draft_index": 1, "reason": "schema", "path": "causation_id"},
        ),
        batch_case(
            "a_companion_no_submission_names",
            "rule 45: an orphan OrderRequestRecorded",
            [{"base_seq": SEQ["request"], "changes": []}],
            {"outcome": "Invalid", "draft_index": 0, "reason": "schema", "path": "event_type"},
        ),
        batch_case(
            "the_out_of_order_pair",
            "rule 45: the submission cannot precede its companion",
            [
                {"base_seq": SEQ["submitted"], "changes": []},
                {"base_seq": SEQ["request"], "changes": []},
            ],
            {"outcome": "Invalid", "draft_index": 0, "reason": "schema", "path": "event_type"},
        ),
        batch_case(
            "an_orphan_after_a_paired_submission",
            "rule 45: a companion no submission names is refused, even beside a named one",
            [
                {"base_seq": SEQ["request"], "changes": []},
                {"base_seq": SEQ["submitted"], "changes": []},
                {"base_seq": SEQ["request"], "changes": [change("event_id", "01J8ZQA8A000000000000000E9")]},
            ],
            {"outcome": "Invalid", "draft_index": 2, "reason": "schema", "path": "event_type"},
        ),
    ]


def rule45(drafts: list[dict], skip: frozenset[str] = frozenset()) -> list[tuple[str, str, int]]:
    """The batch rule 45 (§9.5, DEC-446 item 3), the reference reading of the journal's order:
    submissions in batch order first, then the orphans. Answers every failure as
    `(reason, path, draft_index)`, in report order."""
    out: list[tuple[str, str, int]] = []
    names = [(d.get("causation_id") or "") for d in drafts]
    for i, draft in enumerate(drafts):
        if draft["event_type"] != "OrderSubmitted" or draft["schema_version"] != 2:
            continue
        run = 0
        j = i
        while j > 0 and drafts[j - 1]["event_type"] == "OrderRequestRecorded":
            run += 1
            j -= 1
        if run != 1 and "rule.45.run" not in skip:
            out.append(("schema", "event_type", i))
        elif run == 1 and names[i] != drafts[i - 1]["event_id"] and "rule.45.causation" not in skip:
            out.append(("schema", "causation_id", i))
    for i, draft in enumerate(drafts):
        if draft["event_type"] == "OrderRequestRecorded" and "rule.45.orphan" not in skip:
            if not any(
                d["event_type"] == "OrderSubmitted" and names[k] == draft["event_id"]
                for k, d in enumerate(drafts)
            ):
                out.append(("schema", "event_type", i))
    return out


# --------------------------------------------------------------------------- drafts


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule, or the rules its `also` lists; together they cover every
    §9.5 member type and every clause of rules 39 to 44."""
    s = SEQ
    return [
        invalid(
            "companion_class_without_take_profit",
            "rule 39",
            s["request"],
            [change("payload.take_profit", None)],
            "schema",
            "payload.order_class",
        ),
        invalid(
            "companion_take_profit_without_stop",
            "rule 39",
            s["request"],
            [change("payload.stop", None)],
            "schema",
            "payload.take_profit",
        ),
        invalid(
            "companion_stop_without_class_or_take_profit",
            "rule 39: the legs travel together, at the first offending member",
            s["request"],
            [change("payload.take_profit", None), change("payload.order_class", None)],
            "schema",
            "payload.take_profit",
        ),
        invalid(
            "companion_rung_without_floor",
            "rule 40",
            s["request"],
            [change("payload.rung", 1), change("payload.at_floor", None)],
            "schema",
            "payload.at_floor",
        ),
        invalid(
            "companion_floor_without_rung",
            "rule 40",
            s["request"],
            [change("payload.at_floor", True)],
            "schema",
            "payload.at_floor",
        ),
        invalid(
            "companion_rung_as_text",
            "§9.5 integer",
            s["request"],
            [change("payload.rung", "1"), change("payload.at_floor", False)],
            "schema",
            "payload.rung",
        ),
        invalid(
            "companion_agent_not_an_id",
            "§9.1 id",
            s["request"],
            [change("payload.agent_id", "agent a")],
            "non_canonical",
            "payload.agent_id",
        ),
        invalid(
            "companion_intent_not_a_ulid",
            "§9.1 ulid",
            s["request"],
            [change("payload.intent_id", "intent 01")],
            "non_canonical",
            "payload.intent_id",
        ),
        invalid(
            "companion_carries_a_laddered_flag",
            "§9.5 closed schema: there is no laddered member (DEC-446 item 2)",
            s["request"],
            [change("payload.laddered", True)],
            "schema",
            "payload.laddered",
        ),
        invalid(
            "companion_without_extended_hours",
            "§4.2 absent member",
            s["request"],
            [delete("payload.extended_hours")],
            "schema",
            "payload.extended_hours",
        ),
        invalid(
            "companion_risk_clock_off_the_second",
            "§9.2 risk_clock",
            s["request"],
            [change("payload.risk_clock", "2026-09-21T14:15:00.500000000Z")],
            "non_canonical",
            "payload.risk_clock",
        ),
        invalid(
            "companion_without_risk_clock",
            "§9.1 absent member",
            s["request"],
            [delete("payload.risk_clock")],
            "schema",
            "payload.risk_clock",
        ),
        invalid(
            "intended_names_an_order",
            "rule 41",
            s["intended"],
            [change("payload.orders", [LEG])],
            "schema",
            "payload.orders",
        ),
        invalid(
            "placed_names_no_order",
            "rule 41",
            s["placed"],
            [change("payload.orders", [])],
            "schema",
            "payload.orders",
        ),
        invalid(
            "placed_awaits_a_cancel",
            "rule 41",
            s["placed"],
            [change("payload.awaiting", [LEG])],
            "schema",
            "payload.awaiting",
        ),
        invalid(
            "intended_carries_a_qty",
            "rule 42",
            s["intended"],
            [change("payload.qty", "10")],
            "schema",
            "payload.qty",
        ),
        invalid(
            "placed_without_qty",
            "rule 42",
            s["placed"],
            [change("payload.qty", None)],
            "schema",
            "payload.qty",
        ),
        invalid(
            "intended_take_profit_without_stop",
            "rule 43: take_profit is null when stop is",
            s["intended"],
            [change("payload.stop", None)],
            "schema",
            "payload.take_profit",
        ),
        invalid(
            "watchdog_carrying_prices",
            "rule 43: the prices are null on every action but the four",
            s["intended"],
            [change("payload.action", "watchdog")],
            "schema",
            "payload.stop",
            also=[("schema", "payload.intent_id")],
        ),
        invalid(
            "watchdog_take_profit_alone",
            "rule 43: a price without its stop is still a price off the four",
            s["intended"],
            [change("payload.action", "watchdog"), change("payload.stop", None)],
            "schema",
            "payload.stop",
            also=[("schema", "payload.take_profit"), ("schema", "payload.intent_id")],
        ),
        invalid(
            "intended_names_an_entry",
            "rule 44: entry only in a start's trio",
            s["intended"],
            [change("payload.entry", ENTRY)],
            "schema",
            "payload.entry",
        ),
        invalid(
            "intended_names_an_agent",
            "rule 44: agent_id only in a start's trio",
            s["intended"],
            [change("payload.agent_id", AGENT)],
            "schema",
            "payload.agent_id",
        ),
        invalid(
            "rung_short_without_intent",
            "rule 44",
            s["intended"],
            [change("payload.action", "rung_short"), change("payload.stop", None), change("payload.take_profit", None), change("payload.intent_id", None)],
            "schema",
            "payload.intent_id",
        ),
        invalid(
            "a_start_whose_trio_disagrees",
            "rule 44: an exit sequence names all three, at the first that disagrees",
            s["start"],
            [change("payload.entry", None)],
            "schema",
            "payload.entry",
        ),
        invalid(
            "placed_replaces",
            "rule 44: replacing only on the two starts",
            s["placed"],
            [change("payload.replacing", True)],
            "schema",
            "payload.replacing",
        ),
        invalid(
            "watchdog_names_a_bracket",
            "rule 44: bracket only on placed, the two starts, and unprotected_end",
            s["intended"],
            [change("payload.action", "watchdog"), change("payload.stop", None), change("payload.take_profit", None), change("payload.intent_id", None), change("payload.bracket", ENTRY)],
            "schema",
            "payload.bracket",
        ),
        invalid(
            "a_start_created_on",
            "rule 44: created_on only on placed",
            s["start"],
            [change("payload.created_on", "2026-09-21")],
            "schema",
            "payload.created_on",
        ),
        invalid(
            "watchdog_sent_a_quantity",
            "rule 44: sent only on rung_short",
            s["intended"],
            [change("payload.action", "watchdog"), change("payload.stop", None), change("payload.take_profit", None), change("payload.intent_id", None), change("payload.sent", "3")],
            "schema",
            "payload.sent",
        ),
        invalid(
            "watchdog_uncovered",
            "rule 44: uncovered only on interval_limit and unprotected_end",
            s["intended"],
            [change("payload.action", "watchdog"), change("payload.stop", None), change("payload.take_profit", None), change("payload.intent_id", None), change("payload.uncovered", True)],
            "schema",
            "payload.uncovered",
        ),
        invalid(
            "a_start_acknowledged",
            "rule 44: acknowledged only on unprotected_end",
            s["start"],
            [change("payload.acknowledged", True)],
            "schema",
            "payload.acknowledged",
        ),
        invalid(
            "orders_as_text",
            "§9.5 list",
            s["placed"],
            [change("payload.orders", LEG)],
            "schema",
            "payload.orders",
        ),
        invalid(
            "placed_without_instrument",
            "§4.2 absent member",
            s["placed"],
            [delete("payload.instrument_id")],
            "schema",
            "payload.instrument_id",
        ),
        invalid(
            "an_unknown_action",
            "§9.5 action vocabulary",
            s["placed"],
            [change("payload.action", "replaced")],
            "non_canonical",
            "payload.action",
        ),
        invalid(
            "created_on_not_a_date",
            "§9.1 date",
            s["placed"],
            [change("payload.created_on", "09/21/2026")],
            "non_canonical",
            "payload.created_on",
        ),
        invalid(
            "intent_v2_without_risk_clock",
            "§9.1 absent member: version 2 carries the clock",
            s["intent"],
            [delete("payload.risk_clock")],
            "schema",
            "payload.risk_clock",
        ),
        invalid(
            "intent_v2_carries_protective_prices",
            "§9.5 closed schema: never a member, at either version (DEC-446 item 5)",
            s["intent"],
            [change("payload.stop", "139")],
            "schema",
            "payload.stop",
        ),
        invalid(
            "intent_v2_carries_the_legacy_kind",
            "§9.5 closed schema: the legacy member is refused",
            s["intent"],
            [change("payload.kind", "order")],
            "schema",
            "payload.kind",
        ),
        invalid(
            "intent_v2_agent_not_an_id",
            "§9.1 id",
            s["intent"],
            [change("payload.agent_id", "agent a")],
            "non_canonical",
            "payload.agent_id",
        ),
        invalid(
            "gate_v2_without_risk_clock",
            "§9.1 absent member",
            s["gate"],
            [delete("payload.risk_clock")],
            "schema",
            "payload.risk_clock",
        ),
        invalid(
            "gate_v2_carries_the_legacy_evaluation",
            "§9.5 closed schema",
            s["gate"],
            [change("payload.evaluation", "account_stream_only")],
            "schema",
            "payload.evaluation",
        ),
        invalid(
            "submitted_v2_carries_the_agent",
            "§9.5 closed schema: the executor-only members live on the companion (DEC-360 item 2)",
            s["submitted"],
            [change("payload.agent", AGENT)],
            "schema",
            "payload.agent",
        ),
        invalid(
            "submitted_v2_without_risk_clock",
            "§9.1 absent member",
            s["submitted"],
            [delete("payload.risk_clock")],
            "schema",
            "payload.risk_clock",
        ),
        invalid(
            "submitted_v2_at_version_3",
            "§9.5: the closed versions are 1 and 2",
            s["submitted"],
            [change("schema_version", 3)],
            "unknown_schema",
            "payload",
        ),
        invalid(
            "companion_on_the_agent_stream",
            "§9 stream",
            s["request"],
            [change("stream_id", f"agent:{WORKSPACE}:{AGENT}")],
            "wrong_stream",
            "event_type",
        ),
    ]


def valid_drafts() -> list[dict]:
    """Each asserts a case a rule might be misread to refuse."""
    s = SEQ
    return [
        valid(
            "v1_intent_received_still_appends",
            "§8: version 1 is never edited and stays registered",
            s["intent"],
            [change("schema_version", 1), delete("payload.risk_clock")],
        ),
        valid(
            "intended_without_take_profit",
            "rule 43: take_profit is null where the sequence has none (crypto's)",
            s["intended"],
            [change("payload.take_profit", None)],
        ),
        valid(
            "cancelled_with_qty",
            "rule 42: cancelled carries the quantity it uncovered, where journaled",
            s["placed"],
            [change("payload.action", "cancelled"), change("payload.stop", None), change("payload.take_profit", None), change("payload.created_on", None)],
        ),
        valid(
            "cancelled_without_qty",
            "rule 42",
            s["placed"],
            [change("payload.action", "cancelled"), change("payload.qty", None), change("payload.stop", None), change("payload.take_profit", None), change("payload.created_on", None)],
        ),
        valid(
            "placed_names_its_bracket",
            "rule 44: bracket may be non-null on placed",
            s["placed"],
            [change("payload.bracket", ENTRY)],
        ),
        valid(
            "a_start_without_its_trio",
            "rule 44: the three are null together",
            s["start"],
            [change("payload.intent_id", None), change("payload.entry", None), change("payload.agent_id", None), change("payload.replacing", None)],
        ),
        valid(
            "companion_first_rung_zero",
            "rule 40: a lone ladder's first rung writes rung 0 (DEC-446 item 2)",
            s["request"],
            [change("payload.rung", 0), change("payload.at_floor", False)],
        ),
        valid(
            "interval_limit_with_awaiting",
            "rules 41 to 44: the interval's bound, awaiting its confirmed cancels",
            s["intended"],
            [
                change("payload.action", "interval_limit"),
                change("payload.stop", None),
                change("payload.take_profit", None),
                change("payload.intent_id", None),
                change("payload.awaiting", [LEG]),
                change("payload.uncovered", True),
            ],
        ),
        valid(
            "watchdog_minimal",
            "rules 41 to 44: a watchdog with nothing else to say",
            s["intended"],
            [change("payload.action", "watchdog"), change("payload.stop", None), change("payload.take_profit", None), change("payload.intent_id", None)],
        ),
        valid(
            "unprotected_end_with_awaiting",
            "rules 41 to 44: the interval ends awaiting a confirmed cancel, uncovered",
            s["start"],
            [
                change("payload.action", "unprotected_end"),
                change("payload.orders", []),
                change("payload.stop", None),
                change("payload.take_profit", None),
                change("payload.intent_id", None),
                change("payload.entry", None),
                change("payload.agent_id", None),
                change("payload.replacing", None),
                change("payload.awaiting", [LEG]),
                change("payload.uncovered", True),
                change("payload.acknowledged", False),
            ],
        ),
        valid(
            "rung_short_with_sent",
            "rule 44: the short rung names the quantity it sent",
            s["intended"],
            [change("payload.action", "rung_short"), change("payload.stop", None), change("payload.take_profit", None), change("payload.sent", "0")],
        ),
    ]


# --------------------------------------------------------------------------- the fold oracle


def fold_oracle() -> dict:
    """What the executor's fold derives from the chain alone: the restored protection, the request
    the companion rebuilds, and the resubmission it makes possible (§5.7)."""
    return {
        "intent": {
            "intent_id": INTENT,
            "agent_id": AGENT,
            "outcome": "submitted",
            "protection": {"stop": "139", "take_profit": "159"},
        },
        "request": {
            "client_order_id": ENTRY,
            "intent_id": INTENT,
            "agent_id": AGENT,
            "instrument_id": INSTRUMENT,
            "side": "buy",
            "type": "limit",
            "tif": "gtc",
            "qty": "10",
            "limit_price": "150",
            "stop_price": None,
            "order_class": "bracket",
            "take_profit": "159",
            "stop": "139",
            "extended_hours": False,
            "purpose": "open",
        },
        "protection": {
            "instrument_id": INSTRUMENT,
            "resting": [LEG],
            "covered_qty": "10",
            "stop": "139",
            "take_profit": "159",
        },
        "unprotected_open": True,
        "resubmission": {
            "client_order_id": ENTRY,
            "attempt": 2,
            "changes_nothing_else": True,
        },
    }


def recompute_oracle(section: dict) -> dict:
    """The oracle recomputed from the chain's payloads alone, an independent path through the same
    reads the executor's fold makes: the intent's record and restored prices, the order set, the
    rebuilt request, and the resting protection."""
    by_type = {e["body"]["event_type"]: e["body"]["payload"] for e in section["chain"]}
    intent = by_type["IntentReceived"]
    intended = next(p for p in section["chain"] if p["body"]["payload"].get("action") == "intended")["body"]["payload"]
    request = by_type["OrderRequestRecorded"]
    submitted = by_type["OrderSubmitted"]
    placed = next(p for p in section["chain"] if p["body"]["payload"].get("action") == "placed")["body"]["payload"]
    starts = [p["body"]["payload"] for p in section["chain"] if p["body"]["payload"].get("action") == "unprotected_start"]
    protection = {
        "instrument_id": placed["instrument_id"],
        "resting": list(placed["orders"]),
        "covered_qty": placed["qty"],
        "stop": placed["stop"],
        "take_profit": placed["take_profit"],
    }
    for start in starts:
        protection["resting"] = sorted(set(protection["resting"]) | set(start["orders"]))
    oracle = {
        "intent": {
            "intent_id": intent["intent_id"],
            "agent_id": intent["agent_id"],
            "outcome": "submitted",
            "protection": {
                "stop": intended["stop"],
                "take_profit": intended["take_profit"],
            },
        },
        "request": {
            "client_order_id": submitted["client_order_id"],
            "intent_id": request["intent_id"],
            "agent_id": request["agent_id"],
            "instrument_id": submitted["instrument_id"],
            "side": submitted["side"],
            "type": submitted["type"],
            "tif": submitted["tif"],
            "qty": submitted["qty"],
            "limit_price": submitted["limit_price"],
            "stop_price": request["stop_price"],
            "order_class": request["order_class"],
            "take_profit": request["take_profit"],
            "stop": request["stop"],
            "extended_hours": request["extended_hours"],
            "purpose": request["purpose"],
        },
        "protection": protection,
        "unprotected_open": any(starts),
        "resubmission": {
            "client_order_id": submitted["client_order_id"],
            "attempt": submitted["attempt"] + 1,
            "changes_nothing_else": True,
        },
    }
    return oracle


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = (
    "chain",
    "chain.batches",
    "intent.protected_never",
    "invalid_drafts",
    "valid_drafts",
    "batches.valid",
    "batches.invalid",
    "fold_oracle.recompute",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def draft_of_body(body: dict) -> dict:
    """A chain body as a writer's draft: without the journal-assigned fields."""
    draft = copy.deepcopy(body)
    for field in ("seq", "prev_hash", "recorded_at"):
        draft.pop(field, None)
    return draft


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems: list[str] = []
    chain = section["chain"]
    for i, entry in enumerate(chain):
        seq = i + 1
        if entry["seq"] != seq:
            problems.append(found("chain", f"chain position {i} carries seq {entry['seq']}"))
        if entry["body"]["prev_hash"] != (chain[i - 1]["hash"] if i else section["genesis_prev_hash"]):
            problems.append(found("chain", f"seq {seq} does not chain"))
        if canon(entry["body"]) != entry["canonical"] or sha256_hex(entry["canonical"].encode()) != entry["hash"]:
            problems.append(found("chain", f"seq {seq} does not re-hash"))
        got = violations(draft_of_body(entry["body"]))
        if got:
            problems.append(found("chain", f"seq {entry['seq']}: {got}"))
    flat = [seq for batch in section["batches"] for seq in batch]
    if flat != list(range(1, len(chain) + 1)) or section["batches"] != [list(b) for b in BATCHES]:
        problems.append(found("chain.batches", "the batches are not the chain's append batches"))
    intended_payload = next(e["body"]["payload"] for e in chain if e["body"]["event_type"] == "IntentReceived")
    if "stop" in intended_payload or "take_profit" in intended_payload:
        problems.append(found("intent.protected_never", "the intent carries a protective price"))
    if any(
        member in intended_payload
        for member in ("agent", "kind", "instrument", "limit", "order_type")
    ):
        problems.append(found("intent.protected_never", "the intent carries a legacy member"))
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case))
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        got = violations(draft_for(section, case))
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
    for case in section["valid_batches"]:
        drafts = [draft_for(section, m) for m in case["drafts"]]
        per_draft = [violations(d) for d in drafts]
        if any(per_draft):
            problems.append(found("batches.valid", f"{case['name']}: a draft is invalid: {per_draft}"))
        got = rule45(drafts)
        if got:
            problems.append(found("batches.valid", f"{case['name']}: rule 45 refused it: {got}"))
    for case in section["invalid_batches"]:
        drafts = [draft_for(section, m) for m in case["drafts"]]
        want = case["expect"]
        got = rule45(drafts)
        listed = (want["reason"], want["path"], want["draft_index"])
        if listed not in got:
            problems.append(found("batches.invalid", f"{case['name']}: expected {listed}, got {got}"))
        elif got.index(listed) != 0:
            problems.append(found("batches.invalid", f"{case['name']}: {listed} is not the first failure in {got}"))
    if recompute_oracle(section) != section["fold_oracle"]:
        problems.append(found("fold_oracle.recompute", "the listed oracle is not what the chain replays to"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.39",
    "order.rule_39_last",
    "rule.40",
    "rule.41",
    "rule.41.awaiting",
    "rule.42",
    "rule.43",
    "rule.43.tp",
    "rule.44.intent",
    "rule.44.entry",
    "rule.44.agent_id",
    "rule.44.trio",
    "rule.44.replacing",
    "rule.44.bracket",
    "rule.44.created_on",
    "rule.44.sent",
    "rule.44.uncovered",
    "rule.44.acknowledged",
    "rule.45.run",
    "rule.45.causation",
    "rule.45.orphan",
    "loose.payload.agent_id",
    "loose.payload.intent_id",
    "loose.payload.rung",
    "loose.payload.at_floor",
    "loose.payload.stop",
    "loose.payload.take_profit",
    "loose.payload.qty",
    "loose.payload.orders",
    "loose.payload.created_on",
    "loose.payload.risk_clock",
    "loose.payload.action",
    "loose.payload.instrument_id",
    "loose.payload.laddered",
)

VECTOR_MUTANTS = (
    ("the oracle's restored stop differs", "fold_oracle.recompute",
     lambda s: s["fold_oracle"]["intent"]["protection"].update(stop="140")),
    ("the oracle's rebuilt request loses its bracket", "fold_oracle.recompute",
     lambda s: s["fold_oracle"]["request"].update(order_class=None, take_profit=None, stop=None)),
    ("the oracle's resting orders drop the leg", "fold_oracle.recompute",
     lambda s: s["fold_oracle"]["protection"].update(resting=[])),
    ("the companion's legs disagree with the chain's record", "fold_oracle.recompute",
     lambda s: s["chain"][SEQ["request"] - 1]["body"]["payload"].update(take_profit="160")),
    ("an invalid draft's expectation names another member", "invalid_drafts",
     lambda s: s["invalid_drafts"][0]["expect"].update(path="payload.stop")),
    ("a valid draft breaks rule 40", "valid_drafts",
     lambda s: _edit_valid(s, "companion_first_rung_zero", [change("payload.rung", 0), change("payload.at_floor", None)])),
    ("a chain event stops chaining", "chain",
     lambda s: s["chain"][4]["body"]["payload"].update(extended_hours=True)),
    ("the intended record names an order", "chain",
     lambda s: s["chain"][SEQ["intended"] - 1]["body"]["payload"].update(orders=[LEG])),
    ("the intent carries a protective price", "intent.protected_never",
     lambda s: s["chain"][SEQ["intent"] - 1]["body"]["payload"].update(stop="139")),
    ("the batches stop matching the appends", "chain.batches",
     lambda s: s.update(batches=[[1], [2, 3], [4], [5], [6], [7], [8]])),
    ("an invalid batch's expectation moves", "batches.invalid",
     lambda s: s["invalid_batches"][0]["expect"].update(draft_index=0)),
    ("a valid batch's drafts stop parsing", "batches.valid",
     lambda s: s["valid_batches"][0]["drafts"][0]["changes"].append(change("payload.order_class", None))),
)


def _edit_valid(section: dict, name: str, changes: list[dict]) -> None:
    case = next(c for c in section["valid_drafts"] if c["name"] == name)
    case["changes"] = changes


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""
    out = []
    for name, check, fn in VECTOR_MUTANTS:
        mutated = copy.deepcopy(section)
        fn(mutated)
        out.append((name, check, mutated))
    return out


def found_of(problem: str) -> str:
    return problem.partition(": ")[0]


def run_mutants(section: dict) -> list[str]:
    """Every seeded bug caught; a vector mutant only by the check it is registered against, with no
    other check of that check's family also catching it."""
    escaped: list[str] = []
    cases = [(copy.deepcopy(d), None) for d in section["chain"]]
    cases += [(draft_for(section, c), None) for c in section["valid_drafts"]]
    cases += [(draft_for(section, c), c["expect"]) for c in section["invalid_drafts"]]
    batch_cases = [(m, None) for c in section["valid_batches"] for m in c["drafts"]]
    batch_cases += [(m, None) for c in section["invalid_batches"] for m in c["drafts"]]
    for mutant in VALIDATOR_MUTANTS:
        skip = frozenset([mutant])
        caught = False
        for draft, want in cases:
            got = violations(draft, skip)
            caught |= bool(got) if want is None else not reported(got, want)
        for member, _ in batch_cases:
            drafts = [draft_for(section, m) for m in ([member] if isinstance(member, dict) else member)]
            caught |= bool(rule45(drafts, skip))
        if not caught:
            escaped.append(f"account-stream validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"account-stream check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {found_of(p) for p in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"account-stream vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section(genesis: str) -> dict:
    return {
        "spec": SPEC,
        "stream_id": STREAM,
        "genesis_prev_hash": genesis,
        "chain": hash_chain(chain_bodies(), genesis),
        "batches": [list(b) for b in BATCHES],
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
        "valid_batches": valid_batches(),
        "invalid_batches": invalid_batches(),
        "fold_oracle": fold_oracle(),
    }


# --------------------------------------------------------------------------- rules 41 and 44 (DEC-859)

# The `protection_shapes` section: the `ProtectionChanged` records v0.33 admits (DEC-859 item 1's
# table) and their near misses. It is a section of its own so `mandate-journal`'s tests, which read
# `account_stream` and count its cases, keep their answers until the code change that accepts these
# shapes reads this one (ES-22).
SHAPES_SPEC = "docs/specs/journal.md v0.33 §9.5 rules 41 and 44 (DEC-859)"
SHAPE_BASES = ("intended", "start")
NO_PRICES = (change("payload.stop", None), change("payload.take_profit", None))
NO_TRIO = (change("payload.intent_id", None), change("payload.entry", None), change("payload.agent_id", None))
ENDS = (*NO_PRICES, *NO_TRIO, change("payload.action", "unprotected_end"), change("payload.orders", []),
        change("payload.replacing", None))
SHAPE_VALIDATOR_MUTANTS = (
    "rule.41",
    "rule.41.awaiting",
    "rule.44.trio",
    "rule.44.replacing_only",
    "rule.44.pair",
)
# Seeded bugs that put back a clause v0.33 removed: each must refuse one of the valid drafts.
SHAPE_TIGHTEN_MUTANTS = ("tighten.41.starts", "tighten.41.awaiting", "tighten.44.trio")
SHAPE_CHECKS = ("drafts.valid", "invalid_drafts", "valid_drafts", "valid_drafts.table")
# DEC-859 item 1's table, one valid draft or more per row, by row.
TABLE_ROWS = {
    "bracket_first_fill_start": 1,
    "bracket_filled_end": 2,
    "nothing_to_cover_end": 3,
    "acknowledged_end": 3,
    "uncovered_end": 3,
    "interval_limit_awaiting_nothing": 4,
    "replacement_before_expiry": 5,
    "passive_replacement_before_expiry": 5,
    "handed_on_passive_start": 6,
    "passive_wait_start": 6,
    "lost_protection_start": 6,
}


def shape_drafts() -> dict[str, dict]:
    bodies = {body["event_id"]: body for body in chain_bodies()}
    return {name: draft_of_body(bodies[ID[name]]) for name in SHAPE_BASES}


def shape(name: str, clause: str, base: str, changes: list) -> dict:
    return {"name": name, "clause": clause, "base_draft": base, "changes": copy.deepcopy(list(changes)), "expect": {"outcome": "Valid"}}


def shape_invalid(name: str, clause: str, base: str, changes: list, path: str) -> dict:
    expect = {"outcome": "Invalid", "reason": "schema", "path": path}
    return {"name": name, "clause": clause, "base_draft": base, "changes": copy.deepcopy(list(changes)), "expect": expect}


def shape_valid_drafts() -> list[dict]:
    """DEC-859 item 1's table: each record a writer in `protection.rs` needs, now valid."""
    return [
        shape("bracket_first_fill_start", "rule 41: a bracket entry's first fill starts an interval naming only its bracket",
              "start", [*NO_PRICES, *NO_TRIO, change("payload.orders", []), change("payload.replacing", None),
                        change("payload.bracket", ENTRY)]),
        shape("bracket_filled_end", "rule 41: the bracket filled after it ends the interval awaiting nothing",
              "start", [*ENDS, change("payload.bracket", ENTRY)]),
        shape("nothing_to_cover_end", "rule 41: an interval with nothing to cover ends awaiting nothing", "start", [*ENDS]),
        shape("acknowledged_end", "rule 41: the owner's acknowledgment ends an interval awaiting nothing",
              "start", [*ENDS, change("payload.acknowledged", True)]),
        shape("uncovered_end", "rule 41: an interval ends uncovered, awaiting nothing",
              "start", [*ENDS, change("payload.uncovered", True)]),
        shape("interval_limit_awaiting_nothing", "rule 41: the interval's bound, awaiting no cancel",
              "intended", [*NO_PRICES, change("payload.intent_id", None), change("payload.action", "interval_limit")]),
        shape("replacement_before_expiry", "rule 44: a re-placement names its entry and owner with no intent",
              "start", [change("payload.intent_id", None)]),
        shape("passive_replacement_before_expiry", "rule 44: the same on a passive start",
              "start", [change("payload.action", "passive_start"), change("payload.intent_id", None)]),
        shape("handed_on_passive_start", "rule 41: a handed-on passive sequence starts naming no order",
              "start", [change("payload.action", "passive_start"), change("payload.orders", []), change("payload.replacing", None)]),
        shape("passive_wait_start", "rule 41: a passive wait starts naming no order and no exit sequence",
              "start", [change("payload.action", "passive_start"), change("payload.orders", []), *NO_PRICES, *NO_TRIO,
                        change("payload.replacing", None)]),
        shape("lost_protection_start", "rule 41: lost protection starts an interval naming no order",
              "start", [change("payload.orders", []), *NO_PRICES, *NO_TRIO, change("payload.replacing", None)]),
    ]


def shape_invalid_drafts() -> list[dict]:
    """The near misses: every clause v0.33 keeps still refuses."""
    return [
        shape_invalid("placed_still_names_an_order", "rule 41: placed names at least one order", "start",
                      [change("payload.action", "placed"), change("payload.orders", []), change("payload.qty", "10"),
                       *NO_TRIO, change("payload.replacing", None)], "payload.orders"),
        shape_invalid("cancelled_still_names_an_order", "rule 41: cancelled names at least one order", "start",
                      [change("payload.action", "cancelled"), change("payload.orders", []), *NO_PRICES, *NO_TRIO,
                       change("payload.replacing", None)], "payload.orders"),
        shape_invalid("an_end_names_an_order", "rule 41: orders is empty on unprotected_end", "start",
                      [*ENDS, change("payload.orders", [LEG])], "payload.orders"),
        shape_invalid("a_start_awaits_a_cancel", "rule 41: awaiting is empty on a start", "start",
                      [change("payload.awaiting", [LEG])], "payload.awaiting"),
        shape_invalid("a_passive_start_awaits_a_cancel", "rule 41: awaiting is empty on a start", "start",
                      [change("payload.action", "passive_start"), change("payload.orders", []), change("payload.awaiting", [LEG])],
                      "payload.awaiting"),
        shape_invalid("watchdog_awaits_a_cancel", "rule 41: awaiting is empty but on the two that await", "intended",
                      [*NO_PRICES, change("payload.intent_id", None), change("payload.action", "watchdog"),
                       change("payload.awaiting", [LEG])], "payload.awaiting"),
        shape_invalid("entry_without_intent_not_replacing", "rule 44: replacing false keeps the three together", "start",
                      [change("payload.intent_id", None), change("payload.replacing", False)], "payload.entry"),
        shape_invalid("entry_without_intent_replacing_null", "rule 44: a start that replaces nothing keeps the three together",
                      "start", [change("payload.intent_id", None), change("payload.replacing", None)], "payload.entry"),
        shape_invalid("passive_entry_without_intent_not_replacing", "rule 44: the same on a passive start", "start",
                      [change("payload.action", "passive_start"), change("payload.intent_id", None),
                       change("payload.replacing", False)], "payload.entry"),
        shape_invalid("replacement_without_its_owner", "rule 44: a re-placement names entry and agent_id together", "start",
                      [change("payload.intent_id", None), change("payload.agent_id", None)], "payload.agent_id"),
        shape_invalid("replacement_owner_without_entry", "rule 44: a re-placement names entry and agent_id together", "start",
                      [change("payload.intent_id", None), change("payload.entry", None)], "payload.agent_id"),
        shape_invalid("replacing_exit_without_its_entry", "rule 44: with an intent, an exit sequence names all three", "start",
                      [change("payload.entry", None)], "payload.entry"),
        shape_invalid("an_end_names_an_entry", "rule 44: entry only on the two starts", "start",
                      [*ENDS, change("payload.entry", ENTRY)], "payload.entry"),
        shape_invalid("an_end_replaces", "rule 44: replacing only on the two starts", "start",
                      [*ENDS, change("payload.replacing", True)], "payload.replacing"),
    ]


def build_shape_section() -> dict:
    return {
        "spec": SHAPES_SPEC,
        "drafts": shape_drafts(),
        "invalid_drafts": shape_invalid_drafts(),
        "valid_drafts": shape_valid_drafts(),
    }


def shape_found(check: str, message: str) -> str:
    if check not in SHAPE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def check_shape_section(section: dict) -> list[str]:
    """Every failure: the bases append, each invalid draft is refused only as listed, each valid
    draft appends, and every row of DEC-859 item 1's table has a valid draft."""
    problems: list[str] = []
    for name, draft in section["drafts"].items():
        got = violations(copy.deepcopy(draft))
        if got:
            problems.append(shape_found("drafts.valid", f"{name}: {got}"))
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case))
        if not reported(got, case["expect"]):
            problems.append(shape_found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        got = violations(draft_for(section, case))
        if got:
            problems.append(shape_found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
    rows = {TABLE_ROWS.get(case["name"]) for case in section["valid_drafts"]}
    if not set(range(1, 7)) <= rows:
        problems.append(shape_found("valid_drafts.table", f"rows {sorted(set(range(1, 7)) - rows)} have no valid draft"))
    return problems


SHAPE_VECTOR_MUTANTS = (
    ("a base draft breaks rule 41", "drafts.valid",
     lambda s: s["drafts"]["start"]["payload"].update(awaiting=[LEG])),
    ("an invalid draft's expectation names another member", "invalid_drafts",
     lambda s: s["invalid_drafts"][0]["expect"].update(path="payload.awaiting")),
    ("a valid draft names an order on its end", "valid_drafts",
     lambda s: s["valid_drafts"][1]["changes"].append(change("payload.orders", [LEG]))),
    ("the re-placement rows go missing", "valid_drafts.table",
     lambda s: s.update(valid_drafts=[c for c in s["valid_drafts"] if TABLE_ROWS[c["name"]] != 5])),
)


def run_shape_mutants(section: dict) -> list[str]:
    """Every seeded bug caught: each dropped clause by an invalid draft, each clause put back by a
    valid draft, and each vector mutant by the check it is registered against."""
    escaped: list[str] = []
    invalid_cases = [(draft_for(section, c), c["expect"]) for c in section["invalid_drafts"]]
    valid_cases = [draft_for(section, c) for c in section["valid_drafts"]]
    for mutant in SHAPE_VALIDATOR_MUTANTS:
        skip = frozenset([mutant])
        if not any(not reported(violations(d, skip), want) for d, want in invalid_cases):
            escaped.append(f"protection_shapes validator mutant {mutant}")
    for mutant in SHAPE_TIGHTEN_MUTANTS:
        skip = frozenset([mutant])
        if not any(violations(d, skip) for d in valid_cases):
            escaped.append(f"protection_shapes tightening mutant {mutant}: no valid draft is refused")
    for check in SHAPE_CHECKS:
        if not any(c == check for _, c, _ in SHAPE_VECTOR_MUTANTS):
            escaped.append(f"protection_shapes check {check} has no vector mutant registered against it")
    for name, check, fn in SHAPE_VECTOR_MUTANTS:
        mutated = copy.deepcopy(section)
        fn(mutated)
        caught_by = {found_of(p) for p in check_shape_section(mutated)}
        if check not in caught_by:
            escaped.append(f"protection_shapes vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped
