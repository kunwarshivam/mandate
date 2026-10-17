"""Journal spec v0.45 §9.18's reference vectors for the model gateway's records (DEC-996): the agent
stream's `ModelInvocationRecorded` and the meter stream's `StreamOpened`, `MeterReserved`, and
`MeterSettled` (inference spec §3.6, §7.3).

The schemas live in `control.py`, beside the control stream's `ModelInvocationRecorded` (§9.9), so
one validator judges every closed schema. This module builds the `model_gateway` section:

1. base drafts for the four records, an invalid draft for every member type and rule 143 to 152,
   and valid drafts for the cases a rule might be misread to refuse;
2. meter streams for rule 153, a stream rule that reads the stream's earlier records;
3. spend folds (§9.18's "Spend"), each recomputed by an accumulator of its own, independent of the
   fold this module states, with the admissions a cap gives.

Its oracles read the vectors directly, and every seeded bug, in the validator, the stream rule, the
fold, and the vectors, is shown caught by the check it is registered against.
"""

from __future__ import annotations

import copy
from decimal import Decimal

from common import Violation, apply_change, change, delete, digest_strings
from control import (
    AGENT_STREAM,
    CALL_REFUSALS,
    RUNTIME,
    SERVICES,
    WORKSPACE,
    check_of,
    reported,
    violations,
)

SPEC = "docs/specs/journal.md v0.45 §9.18 (DEC-996)"
METER = f"meter:{WORKSPACE}"
GATEWAY = {"kind": "system", "id": "model_gateway", "version": "0.1.0", "build": "sha256:" + "e" * 64}
AT = "2026-10-16T14:00:00.000000000Z"
DEADLINE = "2026-10-16T14:01:00.000000000Z"
DONE = "2026-10-16T14:00:41.250000000Z"
CALL = "01J9GW00A000000000000000C1"
CALL_B = "01J9GW00B000000000000000C2"
CALL_C = "01J9GW00C000000000000000C3"
INVOKED = "01J9GW10A000000000000000M1"
OPENED = "01J9GW20A000000000000000M2"
RESERVED = "01J9GW21A000000000000000M3"
SETTLED = "01J9GW22A000000000000000M4"
OBS_1 = "01J9GV00A000000000000000B1"
OBS_2 = "01J9GV00B000000000000000B2"
RUN = "01J9GT00A000000000000000R1"
CONTENT = "sha256:" + "a1" * 32
PROMPT = "sha256:" + "b2" * 32
RESPONSE = "sha256:" + "c3" * 32
DIGEST = "sha256:" + "d4" * 32
PRICES = "sha256:" + "e5" * 32
OTHER_REF = "sha256:" + "f6" * 32
MODEL = {"model_id": "llm.research_agent", "model_version": "0.1.0", "content_hash": CONTENT}
ENDPOINT = "openrouter:anthropic"
DAY = "2026-10-16"
BASE_CALL = "call"
BASE_OPENED = "opened"
BASE_RESERVED = "reserved"
BASE_SETTLED = "settled"
ZERO_TOKENS = {"input": 0, "output": 0, "cached": 0}


def envelope(event_id: str, stream: str, event_type: str, actor: dict, payload: dict, refs: dict) -> dict:
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": event_id,
        "stream_id": stream,
        "event_type": event_type,
        "schema_version": 1,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": None,
        "correlation_id": None,
        "actor": dict(actor),
        "config_refs": refs,
        "payload": payload,
        "artifact_refs": sorted(digest_strings(payload)),
        "pii_refs": [],
    }


def call_payload() -> dict:
    """A research call that returned two candidates: one handed to admission, one refused by the
    harness for citing an item the run did not retrieve."""
    return {
        "call_id": CALL,
        "purpose": "research",
        "model": dict(MODEL),
        "endpoint": ENDPOINT,
        "request_digest": DIGEST,
        "sampling": {"temperature": "0", "seed": 7},
        "prompt_ref": PROMPT,
        "response_ref": RESPONSE,
        "reported_identity": "anthropic/claude-sonnet-4.5",
        "provider_request_id": "gen-01",
        "outcome": "ok",
        "attempts": 1,
        "tokens": {"input": 1800, "output": 420, "cached": 0},
        "cost_usd": "0.0117",
        "price_table_ref": PRICES,
        "cache_hit": False,
        "deadline": DEADLINE,
        "completed_at": DONE,
        "reservation": RESERVED,
        "observations": [OBS_1, OBS_2],
        "verdicts": [{"candidate": 0, "result": "judged"}, {"candidate": 1, "result": "unknown_evidence"}],
    }


def reserved_payload(call_id: str = CALL, agent: str | None = "agent_a", cost: str = "0.048", day: str = DAY) -> dict:
    return {
        "call_id": call_id,
        "agent_id": agent,
        "purpose": "research" if agent is not None else "compiler",
        "model": dict(MODEL),
        "endpoint": ENDPOINT,
        "key_owner": "platform",
        "price_table_ref": PRICES,
        "max_cost_usd": cost,
        "risk_day": day,
    }


def settled_payload(call_id: str = CALL, reservation: str = RESERVED, cost: str = "0.0117") -> dict:
    return {
        "call_id": call_id,
        "reservation": reservation,
        "outcome": "ok",
        "attempts": 1,
        "tokens": {"input": 1800, "output": 420, "cached": 0},
        "cost_usd": cost,
    }


def base_drafts() -> dict[str, dict]:
    call = envelope(
        INVOKED, AGENT_STREAM, "ModelInvocationRecorded", RUNTIME, call_payload(), {"model_version": CONTENT}
    )
    call["correlation_id"] = RUN
    return {
        BASE_CALL: call,
        BASE_OPENED: envelope(OPENED, METER, "StreamOpened", GATEWAY, {"stream_type": "meter", "workspace_id": WORKSPACE}, {}),
        BASE_RESERVED: envelope(RESERVED, METER, "MeterReserved", GATEWAY, reserved_payload(), {}),
        BASE_SETTLED: envelope(SETTLED, METER, "MeterSettled", GATEWAY, settled_payload(), {}),
    }


DRAFTS = base_drafts()


def restated(base: str, changes: list[dict]) -> list[dict]:
    """The changes, then `artifact_refs` restated from the changed payload, so a draft breaks only
    the rule it names."""
    draft = copy.deepcopy(DRAFTS[base])
    for item in changes:
        apply_change(draft, item)
    refs = sorted(digest_strings(draft["payload"])) if isinstance(draft["payload"], dict) else []
    if refs != DRAFTS[base]["artifact_refs"]:
        return [*changes, change("artifact_refs", refs)]
    return changes


def invalid(name, clause, base, changes, reason, path):
    return {
        "name": name,
        "clause": clause,
        "base_draft": base,
        "changes": restated(base, changes),
        "expect": {"outcome": "Invalid", "reason": reason, "path": path},
    }


def valid(name, clause, base, changes):
    return {"name": name, "clause": clause, "base_draft": base, "changes": restated(base, changes), "expect": {"outcome": "Valid"}}


def refused(outcome: str) -> list[dict]:
    """A refusal as rules 73 and 146 require it: nothing sent, nothing reserved, no verdicts."""
    return [
        change("payload.outcome", outcome),
        change("payload.response_ref", None),
        change("payload.reported_identity", None),
        change("payload.provider_request_id", None),
        change("payload.attempts", 0),
        change("payload.tokens", dict(ZERO_TOKENS)),
        change("payload.cost_usd", "0"),
        change("payload.reservation", None),
        change("payload.verdicts", []),
    ]


def rejected() -> list[dict]:
    return [*refused("input_rejected"), change("payload.prompt_ref", None)]


def cache_hit() -> list[dict]:
    return [
        change("payload.cache_hit", True),
        change("payload.attempts", 0),
        change("payload.provider_request_id", None),
        change("payload.cost_usd", "0"),
        change("payload.reservation", None),
    ]


def failed(outcome: str) -> list[dict]:
    return [change("payload.outcome", outcome), change("payload.verdicts", [])]


# Each member of the agent-stream record: a value of the wrong JSON kind (`schema`) and, where a
# string has a form, a string of the wrong form (`non_canonical`).
CALL_MEMBERS = (
    ("call_id", 7, "not-a-ulid"),
    ("purpose", 7, "compiler"),
    ("model", "llm.research_agent", None),
    ("endpoint", 7, None),
    ("request_digest", 7, "sha256:short"),
    ("sampling", "0", None),
    ("prompt_ref", 7, "sha256:short"),
    ("response_ref", 7, "sha256:short"),
    ("reported_identity", 7, None),
    ("provider_request_id", 7, None),
    ("outcome", 7, "denied"),
    ("attempts", "1", None),
    ("tokens", 7, None),
    ("cost_usd", 7, "1.2.3"),
    ("price_table_ref", 7, "sha256:short"),
    ("cache_hit", "false", None),
    ("deadline", 7, "2026-10-16T14:01:00Z"),
    ("completed_at", 7, "2026-10-16T14:00:41Z"),
    ("reservation", 7, "not-a-ulid"),
    ("observations", OBS_1, None),
    ("verdicts", {"candidate": 0}, None),
)
CALL_NULLED = (
    "call_id", "purpose", "model", "endpoint", "request_digest", "sampling", "outcome", "attempts",
    "tokens", "cost_usd", "price_table_ref", "cache_hit", "deadline", "completed_at", "observations",
    "verdicts",
)
RESERVED_MEMBERS = (
    ("call_id", 7, "not-a-ulid"),
    ("agent_id", 7, "agent a"),
    ("purpose", 7, "admission"),
    ("model", 7, None),
    ("endpoint", 7, None),
    ("key_owner", 7, "workspace"),
    ("price_table_ref", 7, "sha256:short"),
    ("max_cost_usd", 7, "1.2.3"),
    ("risk_day", 7, "2026-10-16T00:00:00Z"),
)
SETTLED_MEMBERS = (
    ("call_id", 7, "not-a-ulid"),
    ("reservation", 7, "not-a-ulid"),
    ("outcome", 7, "budget_exhausted"),
    ("attempts", "1", None),
    ("tokens", 7, None),
    ("cost_usd", 7, "1.2.3"),
)


def member_drafts(base: str, members, nulled, clause: str) -> list[dict]:
    out = []
    for member, wrong_kind, wrong_form in members:
        path = f"payload.{member}"
        out.append(invalid(f"{base}.{member}.kind", clause, base, [change(path, wrong_kind)], "schema", path))
        if wrong_form is not None:
            out.append(invalid(f"{base}.{member}.form", clause, base, [change(path, wrong_form)], "non_canonical", path))
    for member in nulled:
        path = f"payload.{member}"
        out.append(invalid(f"{base}.{member}.null", clause, base, [change(path, None)], "schema", path))
    first, last = members[0][0], members[-1][0]
    out.append(invalid(f"{base}.missing", clause, base, [delete(f"payload.{last}")], "schema", f"payload.{last}"))
    out.append(invalid(f"{base}.extra", clause, base, [change("payload.zz_extra", True)], "schema", "payload.zz_extra"))
    assert first != last
    return out


def call_rule_drafts() -> list[dict]:
    c = BASE_CALL
    return [
        invalid("call.143.model", "rule 143", c, [change("payload.model.content_hash", OTHER_REF)], "schema", "payload.model.content_hash"),
        invalid("call.144.actor", "rule 144", c, [change("actor", dict(SERVICES))], "schema", "actor.kind"),
        invalid("call.145.rejected_kept", "rule 145: a refused prompt is never stored", c, refused("input_rejected"), "schema", "payload.prompt_ref"),
        invalid("call.145.prompt_dropped", "rule 145: every other call names its prompt", c, [change("payload.prompt_ref", None)], "schema", "payload.prompt_ref"),
        invalid(
            "call.146.refusal_reserved",
            "rule 146: a refusal reserved nothing",
            c,
            [*refused("budget_exhausted"), change("payload.reservation", RESERVED)],
            "schema",
            "payload.reservation",
        ),
        invalid(
            "call.146.hit_reserved",
            "rule 146: a cache hit reserved nothing",
            c,
            [*cache_hit(), change("payload.reservation", RESERVED)],
            "schema",
            "payload.reservation",
        ),
        invalid("call.146.sent_unreserved", "rule 146: a call that reached a provider reserved first", c, [change("payload.reservation", None)], "schema", "payload.reservation"),
        invalid("call.147.descending", "rule 147", c, [change("payload.observations", [OBS_2, OBS_1])], "non_canonical", "payload.observations"),
        invalid("call.147.twice", "rule 147: no item listed twice", c, [change("payload.observations", [OBS_1, OBS_1])], "non_canonical", "payload.observations"),
        invalid("call.148.signal_verdicts", "rule 148: only a research call has candidates", c, [change("payload.purpose", "llm_signal")], "schema", "payload.verdicts"),
        invalid("call.148.failed_verdicts", "rule 148: a failed call has no candidates", c, [change("payload.outcome", "schema_invalid")], "schema", "payload.verdicts"),
        invalid(
            "call.148.order",
            "rule 148: item i is candidate i",
            c,
            [change("payload.verdicts", [{"candidate": 0, "result": "judged"}, {"candidate": 2, "result": "judged"}])],
            "schema",
            "payload.verdicts[1].candidate",
        ),
        invalid(
            "call.148.repeated",
            "rule 148: every candidate exactly once",
            c,
            [change("payload.verdicts", [{"candidate": 0, "result": "judged"}, {"candidate": 0, "result": "judged"}])],
            "schema",
            "payload.verdicts[1].candidate",
        ),
        invalid("call.72.no_response", "rule 72 on the agent stream", c, [change("payload.response_ref", None)], "schema", "payload.response_ref"),
        invalid(
            "call.73.refusal_attempted",
            "rule 73 on the agent stream",
            c,
            [*refused("policy_denied"), change("payload.attempts", 1)],
            "schema",
            "payload.attempts",
        ),
        invalid("call.74.hit_cost", "rule 74 on the agent stream", c, [*cache_hit(), change("payload.cost_usd", "0.01")], "schema", "payload.cost_usd"),
        invalid("call.74.no_attempt", "rule 74 on the agent stream", c, [change("payload.attempts", 0)], "schema", "payload.attempts"),
        invalid("call.75.late", "rule 75 on the agent stream: a late ok", c, [change("payload.completed_at", "2026-10-16T14:01:00.000000001Z")], "schema", "payload.completed_at"),
        invalid("call.75.cost", "rule 75 on the agent stream", c, [change("payload.cost_usd", "-0.01")], "schema", "payload.cost_usd"),
        invalid("call.without_model", "§9 row: `mod`", c, [change("config_refs", {})], "missing_config_ref", "config_refs.model_version"),
        invalid("call.on_the_meter_stream", "§9: an agent-stream record", c, [change("stream_id", METER)], "wrong_stream", "event_type"),
        invalid("call.at_version_2", "§9.18: schema version 1 only", c, [change("schema_version", 2)], "unknown_schema", "payload"),
    ]


def meter_rule_drafts() -> list[dict]:
    o, r, s = BASE_OPENED, BASE_RESERVED, BASE_SETTLED
    return [
        invalid("opened.149", "rule 149", o, [change("payload.workspace_id", "ws_other")], "stream_mismatch", "stream_id"),
        invalid("opened.type", "§9.18 types", o, [change("payload.stream_type", "control")], "non_canonical", "payload.stream_type"),
        invalid("opened.segments", "§2: `meter:{workspace_id}`", o, [change("stream_id", f"{METER}:x")], "non_canonical", "stream_id"),
        invalid("reserved.150.actor", "rule 150", r, [change("actor", dict(RUNTIME))], "schema", "actor.kind"),
        invalid("settled.150.actor", "rule 150", s, [change("actor", dict(RUNTIME))], "schema", "actor.kind"),
        invalid("reserved.151.no_agent", "rule 151: an agent's call names it", r, [change("payload.agent_id", None)], "schema", "payload.agent_id"),
        invalid("reserved.151.compiler_agent", "rule 151: the compiler has no agent", r, [change("payload.purpose", "compiler")], "schema", "payload.agent_id"),
        invalid("reserved.151.cost", "rule 151", r, [change("payload.max_cost_usd", "-0.01")], "schema", "payload.max_cost_usd"),
        invalid("settled.152.cost", "rule 152", s, [change("payload.cost_usd", "-0.01")], "schema", "payload.cost_usd"),
        invalid("settled.152.attempts", "rule 152: an ok made an attempt", s, [change("payload.attempts", 0)], "schema", "payload.attempts"),
        invalid("reserved.on_the_agent_stream", "§9: a meter-stream record", r, [change("stream_id", AGENT_STREAM)], "wrong_stream", "event_type"),
        invalid("settled.at_version_2", "§9.18: schema version 1 only", s, [change("schema_version", 2)], "unknown_schema", "payload"),
    ]


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule; together they cover every member type and rule."""
    return [
        *member_drafts(BASE_CALL, CALL_MEMBERS, CALL_NULLED, "§9.18 types"),
        *member_drafts(BASE_RESERVED, RESERVED_MEMBERS, ("call_id", "purpose", "model", "endpoint", "key_owner", "price_table_ref", "max_cost_usd", "risk_day"), "§9.18 types"),
        *member_drafts(BASE_SETTLED, SETTLED_MEMBERS, ("call_id", "reservation", "outcome", "attempts", "tokens", "cost_usd"), "§9.18 types"),
        *call_rule_drafts(),
        *meter_rule_drafts(),
    ]


def valid_drafts() -> list[dict]:
    """The cases a rule might be misread to refuse; each must be accepted."""
    c, r, s = BASE_CALL, BASE_RESERVED, BASE_SETTLED
    return [
        *(
            valid(f"call.purpose.{purpose}", "§9.18: an LLM or fast signal model's call", c, [change("payload.purpose", purpose), change("payload.verdicts", []), change("payload.observations", [])])
            for purpose in ("llm_signal", "fast_signal")
        ),
        *(
            valid(f"call.refusal.{outcome}", "rules 73 and 146: a refusal names its prompt and reserves nothing", c, refused(outcome))
            for outcome in CALL_REFUSALS
            if outcome != "input_rejected"
        ),
        valid("call.input_rejected", "rule 145: the refused prompt is not stored", c, rejected()),
        valid("call.cache_hit", "rules 74 and 146: a hit costs nothing and reserves nothing", c, cache_hit()),
        valid("call.deadline_before_first_attempt", "rules 74 and 146: reserved, then the deadline passed", c, [*failed("deadline_exceeded"), change("payload.response_ref", None), change("payload.reported_identity", None), change("payload.attempts", 1), change("payload.cost_usd", "0")]),
        valid("call.identity_mismatch", "§9.18: a mismatch keeps its response and is metered", c, [*failed("identity_mismatch"), change("payload.reported_identity", "anthropic/claude-haiku")]),
        valid("call.at_the_deadline", "rule 75: completed exactly at the deadline", c, [change("payload.completed_at", DEADLINE)]),
        valid("call.no_candidates", "rule 148: a research call may return none", c, [change("payload.verdicts", [])]),
        valid("call.no_retrieval", "rule 147: a call with no retrieval plan", c, [change("payload.observations", [])]),
        valid("call.no_seed", "§9.18: a provider that takes no seed", c, [change("payload.sampling", {"temperature": "0", "seed": None})]),
        valid("call.every_harness_result", "rule 148: each harness result", c, [change("payload.verdicts", [{"candidate": i, "result": res} for i, res in enumerate(("judged", "unknown_instrument", "unknown_evidence", "duplicate_instrument", "forbidden_language", "revisions_not_enabled", "unknown_thesis"))])]),
        valid("reserved.compiler", "rule 151: the compiler's call", r, [change("payload.agent_id", None), change("payload.purpose", "compiler")]),
        valid("reserved.free", "rule 151: a local endpoint priced at zero", r, [change("payload.max_cost_usd", "0")]),
        valid("reserved.customer_key", "§9.18: a customer's key", r, [change("payload.key_owner", "customer")]),
        valid("settled.deadline_no_attempt", "rule 152: the deadline passed before the first attempt", s, [change("payload.outcome", "deadline_exceeded"), change("payload.attempts", 0), change("payload.tokens", dict(ZERO_TOKENS)), change("payload.cost_usd", "0")]),
        valid("settled.over_reservation", "§9.18: a settlement above its reservation is recorded as reported", s, [change("payload.cost_usd", "0.06")]),
        valid("settled.retried", "§9.18: a call retried after a transport failure", s, [change("payload.attempts", 3)]),
    ]


def draft_for(section: dict, case: dict) -> dict:
    draft = copy.deepcopy(section["drafts"][case["base_draft"]])
    for item in case["changes"]:
        apply_change(draft, item)
    return draft


# --------------------------------------------------------------------------- rule 153 and the fold


def stream_failure(drafts: list[dict], skip: frozenset[str] = frozenset()) -> dict | None:
    """Rule 153 over a meter stream: the first record that breaks it, with the member reported."""
    reserved: dict[str, str] = {}
    calls: set[str] = set()
    settled: set[str] = set()
    for i, d in enumerate(drafts):
        p = d["payload"]
        if d["event_type"] == "MeterReserved":
            if p["call_id"] in calls and "stream.153.once" not in skip:
                return {"index": i, "reason": "schema", "path": "payload.call_id"}
            calls.add(p["call_id"])
            reserved[d["event_id"]] = p["call_id"]
        if d["event_type"] == "MeterSettled":
            if p["reservation"] not in reserved and "stream.153.known" not in skip:
                return {"index": i, "reason": "schema", "path": "payload.reservation"}
            if reserved.get(p["reservation"], p["call_id"]) != p["call_id"] and "stream.153.same_call" not in skip:
                return {"index": i, "reason": "schema", "path": "payload.call_id"}
            if p["reservation"] in settled and "stream.153.settled_once" not in skip:
                return {"index": i, "reason": "schema", "path": "payload.reservation"}
            settled.add(p["reservation"])
    return None


def in_period(day: str, period: str) -> bool:
    kind, _, value = period.partition(":")
    return day == value if kind == "day" else day[:7] == value


def fold_spend(drafts: list[dict], scope: str, period: str, skip: frozenset[str] = frozenset()) -> Decimal:
    """§9.18's fold: a settled reservation at its settlement's cost, an open one at its maximum."""
    settlements = {d["payload"]["reservation"]: d["payload"] for d in drafts if d["event_type"] == "MeterSettled"}
    total = Decimal(0)
    for d in drafts:
        if d["event_type"] != "MeterReserved":
            continue
        p = d["payload"]
        if scope != "workspace" and p["agent_id"] != scope and "fold.agent_any" not in skip:
            continue
        day = p["risk_day"]
        within = in_period(day, period)
        if "fold.month_by_year" in skip and period.startswith("month:"):
            within = day[:4] == period[6:10]
        if not within:
            continue
        settled = settlements.get(d["event_id"])
        if settled is None:
            if "fold.open_free" not in skip:
                total += Decimal(p["max_cost_usd"])
        elif "fold.settled_at_max" in skip:
            total += Decimal(p["max_cost_usd"])
        else:
            total += Decimal(settled["cost_usd"])
    return total


def admits(drafts: list[dict], scope: str, period: str, cap: str, max_cost: str, skip: frozenset[str] = frozenset()) -> bool:
    """§9.18: admitted only when spend plus the call's maximum is at most the cap."""
    spend = fold_spend(drafts, scope, period, skip)
    if "admit.strict" in skip:
        return spend + Decimal(max_cost) < Decimal(cap)
    return spend + Decimal(max_cost) <= Decimal(cap)


def oracle_spend(drafts: list[dict], scope: str, period: str) -> Decimal:
    """An accumulator of its own: every reservation's maximum in the scope and period, less what each
    settlement released from it, so a reservation with no settlement keeps its whole maximum."""
    kind, _, value = period.partition(":")
    width = 10 if kind == "day" else 7
    maxima: dict[str, Decimal] = {}
    for d in drafts:
        p = d["payload"]
        if d["event_type"] == "MeterReserved" and (scope == "workspace" or p["agent_id"] == scope) and p["risk_day"][:width] == value:
            maxima[d["event_id"]] = Decimal(p["max_cost_usd"])
    total = sum(maxima.values(), Decimal(0))
    for d in drafts:
        if d["event_type"] == "MeterSettled" and d["payload"]["reservation"] in maxima:
            total -= maxima[d["payload"]["reservation"]] - Decimal(d["payload"]["cost_usd"])
    return total


def meter_record(event_type: str, event_id: str, payload: dict) -> dict:
    return envelope(event_id, METER, event_type, GATEWAY, payload, {})


def opened() -> dict:
    return copy.deepcopy(DRAFTS[BASE_OPENED])


def ulid(tag: str, n: int) -> str:
    return f"01J9GW3{tag}{n:018d}"


def reserve(n: int, agent: str | None = "agent_a", cost: str = "0.048", day: str = DAY) -> dict:
    return meter_record("MeterReserved", ulid("R", n), reserved_payload(ulid("C", n), agent, cost, day))


def settle(n: int, cost: str, reservation: int | None = None, call: int | None = None, event: int | None = None) -> dict:
    payload = settled_payload(ulid("C", call if call is not None else n), ulid("R", reservation if reservation is not None else n), cost)
    return meter_record("MeterSettled", ulid("S", event if event is not None else n), payload)


def stream_cases() -> list[dict]:
    def case(name, clause, records, expect):
        return {"name": name, "clause": clause, "records": [opened(), *records], "expect": expect}

    ok = {"outcome": "Valid"}
    return [
        case("interleaved", "rule 153: settlements in any order", [reserve(1), reserve(2), settle(2, "0.01"), settle(1, "0.02")], ok),
        case("open", "rule 153: a reservation never settled", [reserve(1)], ok),
        case("reserved_twice", "rule 153: one reservation per call", [reserve(1), meter_record("MeterReserved", ulid("R", 9), reserved_payload(ulid("C", 1)))], {"outcome": "Invalid", "index": 2, "reason": "schema", "path": "payload.call_id"}),
        case("unreserved", "rule 153: a settlement names an earlier reservation", [settle(1, "0.01")], {"outcome": "Invalid", "index": 1, "reason": "schema", "path": "payload.reservation"}),
        case("names_the_opening", "rule 153: the reservation is a `MeterReserved`", [reserve(1), meter_record("MeterSettled", ulid("S", 1), settled_payload(ulid("C", 1), OPENED))], {"outcome": "Invalid", "index": 2, "reason": "schema", "path": "payload.reservation"}),
        case("another_call", "rule 153: the same call", [reserve(1), reserve(2), settle(1, "0.01", reservation=2)], {"outcome": "Invalid", "index": 3, "reason": "schema", "path": "payload.call_id"}),
        case("settled_twice", "rule 153: one settlement per reservation", [reserve(1), settle(1, "0.01"), settle(1, "0.01", event=8)], {"outcome": "Invalid", "index": 3, "reason": "schema", "path": "payload.reservation"}),
        case("settled_before_reserved", "rule 153: the reservation is earlier", [settle(1, "0.01"), reserve(1)], {"outcome": "Invalid", "index": 1, "reason": "schema", "path": "payload.reservation"}),
    ]


def fold_cases() -> list[dict]:
    """Spend for scopes and periods, and what a cap admits next. The amounts are recomputed by
    `oracle_spend` and checked against what each case states."""
    month = [
        reserve(1, "agent_a", "0.048", "2026-09-30"),
        settle(1, "0.02"),
        reserve(2, "agent_a", "0.048", "2026-10-01"),
        settle(2, "0.01"),
        reserve(3, "agent_b", "0.5", "2026-10-16"),
        reserve(4, None, "0.03", "2026-10-16"),
        settle(4, "0.029"),
        reserve(5, "agent_a", "0.048", "2026-10-16"),
        settle(5, "0.06"),
    ]
    near = [reserve(n, "agent_a", "4.9", f"2026-10-{n + 1:02d}") for n in range(1, 11)]
    near += [settle(n, "4.9") for n in range(1, 11)]
    return [
        {
            "name": "a_month_of_calls",
            "clause": "§9.18 spend: settled at cost, open at maximum, by scope and period",
            "records": [opened(), *month],
            "expect": [
                {"scope": "workspace", "period": "month:2026-10", "spend_usd": "0.599"},
                {"scope": "workspace", "period": "month:2026-09", "spend_usd": "0.02"},
                {"scope": "workspace", "period": "day:2026-10-16", "spend_usd": "0.589"},
                {"scope": "agent_a", "period": "month:2026-10", "spend_usd": "0.07"},
                {"scope": "agent_b", "period": "day:2026-10-16", "spend_usd": "0.5"},
                {"scope": "agent_b", "period": "month:2026-09", "spend_usd": "0"},
            ],
            "admissions": [
                {"scope": "workspace", "period": "month:2026-10", "cap_usd": "50", "max_cost_usd": "49.401", "admitted": True},
                {"scope": "workspace", "period": "month:2026-10", "cap_usd": "50", "max_cost_usd": "49.4011", "admitted": False},
                {"scope": "agent_b", "period": "day:2026-10-16", "cap_usd": "0.50", "max_cost_usd": "0.0001", "admitted": False},
                {"scope": "agent_b", "period": "day:2026-10-16", "cap_usd": "0.50", "max_cost_usd": "0", "admitted": True},
            ],
        },
        {
            "name": "the_monthly_cap_reached",
            "clause": "DEC-997 item 3: $50 a month per workspace",
            "records": [opened(), *near],
            "expect": [{"scope": "workspace", "period": "month:2026-10", "spend_usd": "49"}],
            "admissions": [
                {"scope": "workspace", "period": "month:2026-10", "cap_usd": "50", "max_cost_usd": "1", "admitted": True},
                {"scope": "workspace", "period": "month:2026-10", "cap_usd": "50", "max_cost_usd": "1.0001", "admitted": False},
                {"scope": "workspace", "period": "month:2026-11", "cap_usd": "50", "max_cost_usd": "1.0001", "admitted": True},
            ],
        },
    ]


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = (
    "drafts.valid",
    "drafts.call",
    "invalid_drafts",
    "valid_drafts",
    "streams.records",
    "streams.rule",
    "folds.records",
    "folds.spend",
    "folds.oracle",
    "folds.admits",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def expected_failure(case: dict) -> dict | None:
    e = case["expect"]
    return None if e["outcome"] == "Valid" else {k: e[k] for k in ("index", "reason", "path")}


def check_section(section: dict, skip: frozenset[str] = frozenset()) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, draft in section["drafts"].items():
        got = violations(draft, skip)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    p = section["drafts"][BASE_CALL]["payload"]
    if p["outcome"] != "ok" or p["reservation"] != section["drafts"][BASE_RESERVED]["event_id"]:
        problems.append(found("drafts.call", "the base call is not the ok call the base reservation reserved"))
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case), skip)
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        got = violations(draft_for(section, case), skip)
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
    for case in section["streams"]:
        for i, record in enumerate(case["records"]):
            got = violations(record, skip)
            if got:
                problems.append(found("streams.records", f"{case['name']} record {i}: {got}"))
        got = stream_failure(case["records"], skip)
        if got != expected_failure(case):
            problems.append(found("streams.rule", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["folds"]:
        records = case["records"]
        for i, record in enumerate(records):
            if violations(record, skip) or stream_failure(records, skip) is not None:
                problems.append(found("folds.records", f"{case['name']} record {i} is not a valid meter stream"))
                break
        for e in case["expect"]:
            want = Decimal(e["spend_usd"])
            if fold_spend(records, e["scope"], e["period"], skip) != want:
                problems.append(found("folds.spend", f"{case['name']} {e['scope']} {e['period']}: fold differs from {want}"))
            if oracle_spend(records, e["scope"], e["period"]) != want:
                problems.append(found("folds.oracle", f"{case['name']} {e['scope']} {e['period']}: the accumulator differs from {want}"))
        for a in case["admissions"]:
            got = admits(records, a["scope"], a["period"], a["cap_usd"], a["max_cost_usd"], skip)
            if got != a["admitted"]:
                problems.append(found("folds.admits", f"{case['name']}: {a} admitted {got}"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "record.extra",
    "record.missing",
    "config_refs.required",
    *(f"rule.{n}" for n in ("143", "144", "145", "146", "147", "148.empty", "148.order", "149", "150")),
    "rule.151.agent",
    "rule.151.cost",
    "rule.152.cost",
    "rule.152.attempts",
    "rule.146.refusals_only",
    "stream.153.once",
    "stream.153.known",
    "stream.153.same_call",
    "stream.153.settled_once",
    "fold.agent_any",
    "fold.month_by_year",
    "fold.open_free",
    "fold.settled_at_max",
    "admit.strict",
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
        ("the base call names no model", "drafts.valid", mutated(lambda s: s["drafts"][BASE_CALL].update(config_refs={}))),
        ("the base call failed", "drafts.call", mutated(lambda s: s["drafts"][BASE_CALL]["payload"].update(reservation=RESERVED[:-1] + "9"))),
        ("an invalid draft's path differs", "invalid_drafts", mutated(lambda s: case(s, "invalid_drafts", "call.146.sent_unreserved")["expect"].update(path="payload.prompt_ref"))),
        ("a valid draft reserves a refusal", "valid_drafts", mutated(lambda s: case(s, "valid_drafts", "call.refusal.budget_exhausted")["changes"].append(change("payload.reservation", RESERVED)))),
        ("a stream record is written by the runtime", "streams.records", mutated(lambda s: case(s, "streams", "open")["records"][1].update(actor=dict(RUNTIME)))),
        ("a stream case expects the wrong record", "streams.rule", mutated(lambda s: case(s, "streams", "settled_twice")["expect"].update(index=2))),
        ("a fold's stream settles a call twice", "folds.records", mutated(lambda s: case(s, "folds", "a_month_of_calls")["records"].append(copy.deepcopy(case(s, "folds", "a_month_of_calls")["records"][2])))),
        ("a fold's stated spend is wrong", "folds.spend", mutated(lambda s: case(s, "folds", "a_month_of_calls")["expect"][0].update(spend_usd="0.549"))),
        ("an admission at the cap is refused", "folds.admits", mutated(lambda s: case(s, "folds", "the_monthly_cap_reached")["admissions"][0].update(admitted=False))),
    ]


def run_mutants(section: dict) -> list[str]:
    """Every seeded bug caught; a vector mutant only by the check it is registered against."""
    escaped = []
    for mutant in VALIDATOR_MUTANTS:
        if not check_section(section, frozenset([mutant])):
            escaped.append(f"gateway mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if check != "folds.oracle" and not any(c == check for _, c, _ in registered):
            escaped.append(f"gateway check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"gateway vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    seeded = copy.deepcopy(section)
    seeded["folds"][0]["expect"][0]["spend_usd"] = "0.549"
    if "folds.oracle" not in {check_of(p) for p in check_section(seeded)}:
        escaped.append("gateway oracle: the accumulator does not catch a wrong stated spend")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": copy.deepcopy(DRAFTS),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
        "streams": stream_cases(),
        "folds": fold_cases(),
    }


__all__ = ["Violation", "build_section", "check_section", "run_mutants", "vector_mutants"]
