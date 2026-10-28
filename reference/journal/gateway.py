"""Journal spec v0.46 §9.19's reference vectors for the model gateway's records (DEC-998): the agent
stream's `ModelInvocationRecorded` and the meter stream's `StreamOpened`, `MeterReserved`, and
`MeterSettled` (inference spec §3.6, §7.3).

The schemas live in `control.py`, beside the control stream's `ModelInvocationRecorded` (§9.9), so
one validator judges every closed schema. This module builds the `model_gateway` section:

1. base drafts for the four records, an invalid draft for every member type and rules 146 to 157,
   and valid drafts for the cases a rule might be misread to refuse;
2. agent streams for rule 153 and meter streams for rule 158, stream rules that read the stream's
   earlier records;
3. spend folds (§9.19's "Spend"), each recomputed by an accumulator of its own, independent of the
   fold this module states, with the admissions a cap gives;
4. a seeded fuzz: random sequences of reservations under caps, settlements at or below the
   reservation, crashes that leave a reservation open, and jumps across risk days and months. An
   independent running accumulator asserts that no cap is ever exceeded and that the fold agrees.

Its oracles read the vectors directly, and every seeded bug, in the validator, the stream rules, the
fold, and the vectors, is shown caught by the check it is registered against.
"""

from __future__ import annotations

import copy
import random
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

SPEC = "docs/specs/journal.md v0.46 §9.19 (DEC-998)"
METER = f"meter:{WORKSPACE}"
GATEWAY = {"kind": "system", "id": "model_gateway", "version": "0.1.0", "build": "sha256:" + "e" * 64}
AT = "2026-10-16T14:00:00.000000000Z"
DEADLINE = "2026-10-16T14:01:00.000000000Z"
DONE = "2026-10-16T14:00:41.250000000Z"
CALL = "01J9GW00A000000000000000C1"
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
RESULTS = (
    ("thesis", "judged"),
    ("thesis", "unknown_instrument"),
    ("thesis", "unknown_evidence"),
    ("thesis", "duplicate_instrument"),
    ("thesis", "forbidden_language"),
    ("thesis", "revisions_not_enabled"),
    ("invalidation", "judged"),
    ("invalidation", "forbidden_language"),
    ("invalidation", "unknown_thesis"),
)


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


def verdict(i: int, kind: str, result: str) -> dict:
    return {"candidate": i, "kind": kind, "result": result}


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
        "guard_rule": None,
        "response_ref": RESPONSE,
        "reported_identity": "anthropic/claude-sonnet-4.5@Anthropic",
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
        "verdicts": [verdict(0, "thesis", "judged"), verdict(1, "thesis", "unknown_evidence")],
    }


def reserved_payload(
    call_id: str = CALL, agent: str | None = "agent_a", cost: str = "0.048", day: str = DAY, key: str = "platform"
) -> dict:
    return {
        "call_id": call_id,
        "agent_id": agent,
        "purpose": "research" if agent is not None else "compiler",
        "model": dict(MODEL),
        "endpoint": ENDPOINT,
        "key_owner": key,
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


def unsent() -> list[dict]:
    """Nothing sent: no response, no tokens, no cost, no verdicts (rules 73 and 152)."""
    return [
        change("payload.response_ref", None),
        change("payload.reported_identity", None),
        change("payload.provider_request_id", None),
        change("payload.attempts", 0),
        change("payload.tokens", dict(ZERO_TOKENS)),
        change("payload.cost_usd", "0"),
        change("payload.verdicts", []),
    ]


def refused(outcome: str) -> list[dict]:
    """A refusal as rules 73 and 149 require it: nothing sent, nothing reserved."""
    return [change("payload.outcome", outcome), *unsent(), change("payload.reservation", None)]


def rejected(rule: str = "credential") -> list[dict]:
    """An `input_rejected` as rule 148 requires it: no prompt, no request digest, the guard's rule."""
    return [
        *refused("input_rejected"),
        change("payload.prompt_ref", None),
        change("payload.request_digest", None),
        change("payload.guard_rule", rule),
    ]


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


def never_sent(outcome: str) -> list[dict]:
    """Rule 152's exception: reserved, then nothing left (no time, or no key)."""
    return [change("payload.outcome", outcome), *unsent()]


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
    ("guard_rule", 7, "secret"),
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
    "call_id", "purpose", "model", "endpoint", "sampling", "outcome", "attempts", "tokens", "cost_usd",
    "price_table_ref", "cache_hit", "deadline", "completed_at", "observations", "verdicts",
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
    last = members[-1][0]
    out.append(invalid(f"{base}.missing", clause, base, [delete(f"payload.{last}")], "schema", f"payload.{last}"))
    out.append(invalid(f"{base}.extra", clause, base, [change("payload.zz_extra", True)], "schema", "payload.zz_extra"))
    return out


def verdict_drafts() -> list[dict]:
    c = BASE_CALL
    return [
        invalid("call.verdict.kind.kind", "§9.19 types", c, [change("payload.verdicts", [{"candidate": 0, "kind": 7, "result": "judged"}])], "schema", "payload.verdicts[0].kind"),
        invalid("call.verdict.kind.form", "§9.19 types", c, [change("payload.verdicts", [verdict(0, "proposal", "judged")])], "non_canonical", "payload.verdicts[0].kind"),
        invalid("call.verdict.result.form", "§9.19 types", c, [change("payload.verdicts", [verdict(0, "thesis", "admitted")])], "non_canonical", "payload.verdicts[0].result"),
        invalid("call.verdict.missing", "§9.19 types", c, [change("payload.verdicts", [{"candidate": 0, "result": "judged"}])], "schema", "payload.verdicts[0].kind"),
    ]


def call_rule_drafts() -> list[dict]:
    c = BASE_CALL
    return [
        invalid("call.146.model", "rule 146", c, [change("payload.model.content_hash", OTHER_REF)], "schema", "payload.model.content_hash"),
        invalid("call.147.actor", "rule 147", c, [change("actor", dict(SERVICES))], "schema", "actor.kind"),
        invalid("call.148.prompt_kept", "rule 148: a refused prompt is never stored", c, [*rejected(), change("payload.prompt_ref", PROMPT)], "schema", "payload.prompt_ref"),
        invalid("call.148.request_kept", "rule 148: nor the canonical request that holds it", c, [*rejected(), change("payload.request_digest", DIGEST)], "schema", "payload.request_digest"),
        invalid("call.148.rule_missing", "rule 148: a refused prompt names its guard rule", c, [*rejected(), change("payload.guard_rule", None)], "schema", "payload.guard_rule"),
        invalid("call.148.prompt_dropped", "rule 148: every other call names its prompt", c, [change("payload.prompt_ref", None)], "schema", "payload.prompt_ref"),
        invalid("call.148.request_dropped", "rule 148: and its request", c, [change("payload.request_digest", None)], "schema", "payload.request_digest"),
        invalid("call.148.rule_named", "rule 148: no guard rule on a prompt the guard passed", c, [change("payload.guard_rule", "credential")], "schema", "payload.guard_rule"),
        invalid("call.149.refusal_reserved", "rule 149: a refusal reserved nothing", c, [*refused("budget_exhausted"), change("payload.reservation", RESERVED)], "schema", "payload.reservation"),
        invalid("call.149.hit_reserved", "rule 149: a cache hit reserved nothing", c, [*cache_hit(), change("payload.reservation", RESERVED)], "schema", "payload.reservation"),
        invalid("call.149.sent_unreserved", "rule 149: a call that may cost reserved first", c, [change("payload.reservation", None)], "schema", "payload.reservation"),
        invalid("call.150.descending", "rule 150", c, [change("payload.observations", [OBS_2, OBS_1])], "non_canonical", "payload.observations"),
        invalid("call.150.twice", "rule 150: no item listed twice", c, [change("payload.observations", [OBS_1, OBS_1])], "non_canonical", "payload.observations"),
        invalid("call.151.signal_verdicts", "rule 151: only a research call has candidates", c, [change("payload.purpose", "llm_signal")], "schema", "payload.verdicts"),
        invalid("call.151.failed_verdicts", "rule 151: a failed call has no candidates", c, [change("payload.outcome", "schema_invalid")], "schema", "payload.verdicts"),
        invalid("call.151.order", "rule 151: item i is candidate i", c, [change("payload.verdicts", [verdict(0, "thesis", "judged"), verdict(2, "thesis", "judged")])], "schema", "payload.verdicts[1].candidate"),
        invalid("call.151.repeated", "rule 151: every candidate exactly once", c, [change("payload.verdicts", [verdict(0, "thesis", "judged"), verdict(0, "thesis", "judged")])], "schema", "payload.verdicts[1].candidate"),
        invalid("call.151.kind_order", "rule 151: theses first, then invalidations", c, [change("payload.verdicts", [verdict(0, "invalidation", "judged"), verdict(1, "thesis", "judged")])], "schema", "payload.verdicts[1].kind"),
        invalid("call.151.thesis_result", "rule 151: `unknown_thesis` only for an invalidation", c, [change("payload.verdicts", [verdict(0, "thesis", "unknown_thesis")])], "schema", "payload.verdicts[0].result"),
        *(
            invalid(f"call.151.invalidation_{result}", "rule 151: a thesis check only for a thesis", c, [change("payload.verdicts", [verdict(0, "invalidation", result)])], "schema", "payload.verdicts[0].result")
            for result in ("unknown_instrument", "unknown_evidence", "duplicate_instrument", "revisions_not_enabled")
        ),
        invalid("call.151.first_failure", "rule 151: the first item that fails, then its first clause", c, [change("payload.verdicts", [verdict(0, "thesis", "unknown_thesis"), verdict(2, "thesis", "judged")])], "schema", "payload.verdicts[0].result"),
        invalid("call.152.other_outcome", "rule 152: only an unsent deadline or key may record no attempt", c, [*never_sent("provider_unavailable")], "schema", "payload.attempts"),
        invalid("call.152.response", "rule 152: a call with no attempt kept no response", c, [*never_sent("deadline_exceeded"), change("payload.response_ref", RESPONSE)], "schema", "payload.response_ref"),
        invalid("call.152.identity", "rule 152", c, [*never_sent("credential_invalid"), change("payload.reported_identity", "x@y")], "schema", "payload.reported_identity"),
        invalid("call.152.request_id", "rule 152", c, [*never_sent("deadline_exceeded"), change("payload.provider_request_id", "gen-1")], "schema", "payload.provider_request_id"),
        invalid("call.152.tokens", "rule 152", c, [*never_sent("deadline_exceeded"), change("payload.tokens", {"input": 1, "output": 0, "cached": 0})], "schema", "payload.tokens"),
        invalid("call.152.cost", "rule 152", c, [*never_sent("credential_invalid"), change("payload.cost_usd", "0.01")], "schema", "payload.cost_usd"),
        invalid("call.72.no_response", "rule 72 on the agent stream", c, [change("payload.response_ref", None)], "schema", "payload.response_ref"),
        invalid("call.73.refusal_attempted", "rule 73 on the agent stream", c, [*refused("policy_denied"), change("payload.attempts", 1)], "schema", "payload.attempts"),
        invalid("call.74.hit_cost", "rule 74 on the agent stream", c, [*cache_hit(), change("payload.cost_usd", "0.01")], "schema", "payload.cost_usd"),
        invalid("call.74.no_attempt", "rules 74 and 152: an ok made an attempt", c, [change("payload.attempts", 0)], "schema", "payload.attempts"),
        invalid("call.75.late", "rule 75 on the agent stream: a late ok", c, [change("payload.completed_at", "2026-10-16T14:01:00.000000001Z")], "schema", "payload.completed_at"),
        invalid("call.75.cost", "rule 75 on the agent stream", c, [change("payload.cost_usd", "-0.01")], "schema", "payload.cost_usd"),
        invalid("call.without_model", "§9 row: `mod`", c, [change("config_refs", {})], "missing_config_ref", "config_refs.model_version"),
        invalid("call.on_the_meter_stream", "§9: an agent-stream record", c, [change("stream_id", METER)], "wrong_stream", "event_type"),
        invalid("call.at_version_2", "§9.19: schema version 1 only", c, [change("schema_version", 2)], "unknown_schema", "payload"),
    ]


def meter_rule_drafts() -> list[dict]:
    o, r, s = BASE_OPENED, BASE_RESERVED, BASE_SETTLED
    zero = [change("payload.attempts", 0), change("payload.tokens", dict(ZERO_TOKENS)), change("payload.cost_usd", "0")]
    return [
        invalid("opened.154", "rule 154", o, [change("payload.workspace_id", "ws_other")], "stream_mismatch", "stream_id"),
        invalid("opened.type", "§9.19 types", o, [change("payload.stream_type", "control")], "non_canonical", "payload.stream_type"),
        invalid("opened.segments", "§2: `meter:{workspace_id}`", o, [change("stream_id", f"{METER}:x")], "non_canonical", "stream_id"),
        invalid("reserved.155.actor", "rule 155", r, [change("actor", dict(RUNTIME))], "schema", "actor.kind"),
        invalid("settled.155.actor", "rule 155", s, [change("actor", dict(RUNTIME))], "schema", "actor.kind"),
        invalid("reserved.156.no_agent", "rule 156: an agent's call names it", r, [change("payload.agent_id", None)], "schema", "payload.agent_id"),
        invalid("reserved.156.compiler_agent", "rule 156: the compiler has no agent", r, [change("payload.purpose", "compiler")], "schema", "payload.agent_id"),
        invalid("reserved.156.evaluation_agent", "rule 156: an evaluation has no agent (DEC-2216)", r, [change("payload.purpose", "evaluation")], "schema", "payload.agent_id"),
        invalid("reserved.156.cost", "rule 156", r, [change("payload.max_cost_usd", "-0.01")], "schema", "payload.max_cost_usd"),
        invalid("reserved.156.both", "rule 156: the first that fails", r, [change("payload.agent_id", None), change("payload.max_cost_usd", "-0.01")], "schema", "payload.agent_id"),
        invalid("settled.157.cost", "rule 157", s, [change("payload.cost_usd", "-0.01")], "schema", "payload.cost_usd"),
        invalid("settled.157.ok_unsent", "rule 157: an ok made an attempt", s, zero, "schema", "payload.attempts"),
        invalid("settled.157.tokens", "rule 157: no attempt, no tokens", s, [change("payload.outcome", "deadline_exceeded"), *zero, change("payload.tokens", {"input": 0, "output": 3, "cached": 0})], "schema", "payload.tokens"),
        invalid("settled.157.free", "rule 157: no attempt, no cost", s, [change("payload.outcome", "credential_invalid"), *zero, change("payload.cost_usd", "0.01")], "schema", "payload.cost_usd"),
        invalid("reserved.on_the_agent_stream", "§9: a meter-stream record", r, [change("stream_id", AGENT_STREAM)], "wrong_stream", "event_type"),
        invalid("settled.at_version_2", "§9.19: schema version 1 only", s, [change("schema_version", 2)], "unknown_schema", "payload"),
    ]


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule; together they cover every member type and rule."""
    return [
        *member_drafts(BASE_CALL, CALL_MEMBERS, CALL_NULLED, "§9.19 types"),
        *verdict_drafts(),
        *member_drafts(BASE_RESERVED, RESERVED_MEMBERS, ("call_id", "purpose", "model", "endpoint", "key_owner", "price_table_ref", "max_cost_usd", "risk_day"), "§9.19 types"),
        *member_drafts(BASE_SETTLED, SETTLED_MEMBERS, ("call_id", "reservation", "outcome", "attempts", "tokens", "cost_usd"), "§9.19 types"),
        *call_rule_drafts(),
        *meter_rule_drafts(),
    ]


def valid_drafts() -> list[dict]:
    """The cases a rule might be misread to refuse; each must be accepted."""
    c, r, s = BASE_CALL, BASE_RESERVED, BASE_SETTLED
    return [
        *(
            valid(f"call.purpose.{purpose}", "§9.19: an LLM or fast signal model's call", c, [change("payload.purpose", purpose), change("payload.verdicts", []), change("payload.observations", [])])
            for purpose in ("llm_signal", "fast_signal")
        ),
        *(
            valid(f"call.refusal.{outcome}", "rules 73 and 149: a refusal names its prompt and reserves nothing", c, refused(outcome))
            for outcome in CALL_REFUSALS
            if outcome != "input_rejected"
        ),
        *(
            valid(f"call.input_rejected.{rule}", "rule 148: neither the prompt nor its request is stored", c, rejected(rule))
            for rule in ("credential", "account_identifier", "personal_data", "foreign_workspace", "input_contract")
        ),
        valid("call.cache_hit", "rules 74 and 149: a hit costs nothing and reserves nothing", c, cache_hit()),
        valid("call.deadline_before_first_attempt", "rule 152: reserved, then the deadline passed before any attempt", c, never_sent("deadline_exceeded")),
        valid("call.no_key", "rule 152: reserved, then the vault held no key", c, never_sent("credential_invalid")),
        valid("call.deadline_after_attempts", "rule 74: a deadline passed after an attempt", c, [*failed("deadline_exceeded"), change("payload.response_ref", None), change("payload.attempts", 2)]),
        valid("call.identity_mismatch", "§9.19: a mismatch keeps its response and is metered", c, [*failed("identity_mismatch"), change("payload.reported_identity", "anthropic/claude-haiku@Anthropic")]),
        valid("call.identity_missing", "rule 72 asks an identity only of an ok: a 200 that reports none is a metered mismatch", c, [*failed("identity_mismatch"), change("payload.reported_identity", None)]),
        valid("call.at_the_deadline", "rule 75: completed exactly at the deadline", c, [change("payload.completed_at", DEADLINE)]),
        valid("call.no_candidates", "rule 151: a research call may return none", c, [change("payload.verdicts", [])]),
        valid("call.no_retrieval", "rule 150: a call with no retrieval plan", c, [change("payload.observations", [])]),
        valid("call.no_seed", "§9.19: a provider that takes no seed", c, [change("payload.sampling", {"temperature": "0", "seed": None})]),
        valid("call.every_harness_result", "rule 151: each result with each kind it fits, theses first", c, [change("payload.verdicts", [verdict(i, kind, result) for i, (kind, result) in enumerate(RESULTS)])]),
        valid("call.invalidations_only", "rule 151: a response of invalidations alone", c, [change("payload.verdicts", [verdict(0, "invalidation", "judged"), verdict(1, "invalidation", "unknown_thesis")])]),
        valid("reserved.compiler", "rule 156: the compiler's call", r, [change("payload.agent_id", None), change("payload.purpose", "compiler")]),
        valid("reserved.evaluation", "rule 156: an evaluation's call (DEC-2216)", r, [change("payload.agent_id", None), change("payload.purpose", "evaluation")]),
        valid("reserved.free", "rule 156: a local endpoint priced at zero", r, [change("payload.max_cost_usd", "0")]),
        valid("reserved.customer_key", "§9.19: a customer's key", r, [change("payload.key_owner", "customer")]),
        *(
            valid(f"settled.unsent.{outcome}", "rule 157: no attempt left", s, [change("payload.outcome", outcome), change("payload.attempts", 0), change("payload.tokens", dict(ZERO_TOKENS)), change("payload.cost_usd", "0")])
            for outcome in ("deadline_exceeded", "credential_invalid")
        ),
        valid("settled.over_reservation", "§9.19: a settlement above its reservation is recorded as reported", s, [change("payload.cost_usd", "0.06")]),
        valid("settled.retried", "§9.19: a call retried after a transport failure", s, [change("payload.attempts", 3)]),
        valid("settled.failed_with_cost", "rule 157: a failure that reached a provider may cost", s, [change("payload.outcome", "schema_invalid")]),
    ]


def draft_for(section: dict, case: dict) -> dict:
    draft = copy.deepcopy(section["drafts"][case["base_draft"]])
    for item in case["changes"]:
        apply_change(draft, item)
    return draft


# --------------------------------------------------------------------------- rules 153 and 158, the fold


def agent_stream_failure(drafts: list[dict], skip: frozenset[str] = frozenset()) -> dict | None:
    """Rule 153 over an agent stream: the first `ModelInvocationRecorded` of a call already recorded."""
    calls: set[str] = set()
    for i, d in enumerate(drafts):
        if d["event_type"] != "ModelInvocationRecorded":
            continue
        if d["payload"]["call_id"] in calls and "stream.153.once" not in skip:
            return {"index": i, "reason": "schema", "path": "payload.call_id"}
        calls.add(d["payload"]["call_id"])
    return None


def stream_failure(drafts: list[dict], skip: frozenset[str] = frozenset()) -> dict | None:
    """Rule 158 over a meter stream: the first record that breaks it, with the member reported."""
    reserved: dict[str, str] = {}
    calls: set[str] = set()
    settled: set[str] = set()
    for i, d in enumerate(drafts):
        p = d["payload"]
        if d["event_type"] == "MeterReserved":
            if p["call_id"] in calls and "stream.158.once" not in skip:
                return {"index": i, "reason": "schema", "path": "payload.call_id"}
            calls.add(p["call_id"])
            reserved[d["event_id"]] = p["call_id"]
        if d["event_type"] == "MeterSettled":
            if p["reservation"] not in reserved and "stream.158.known" not in skip:
                return {"index": i, "reason": "schema", "path": "payload.reservation"}
            if reserved.get(p["reservation"], p["call_id"]) != p["call_id"] and "stream.158.same_call" not in skip:
                return {"index": i, "reason": "schema", "path": "payload.call_id"}
            if p["reservation"] in settled and "stream.158.settled_once" not in skip:
                return {"index": i, "reason": "schema", "path": "payload.reservation"}
            settled.add(p["reservation"])
    return None


def in_period(day: str, period: str) -> bool:
    kind, _, value = period.partition(":")
    return day == value if kind == "day" else day[:7] == value


def fold_spend(drafts: list[dict], scope: str, period: str, skip: frozenset[str] = frozenset()) -> Decimal:
    """§9.19's fold: a settled reservation at its settlement's cost, an open one at its maximum."""
    settlements = {d["payload"]["reservation"]: d["payload"] for d in drafts if d["event_type"] == "MeterSettled"}
    total = Decimal(0)
    for d in drafts:
        if d["event_type"] != "MeterReserved":
            continue
        p = d["payload"]
        if scope != "workspace" and p["agent_id"] != scope and "fold.agent_any" not in skip:
            continue
        if p["key_owner"] == "customer" and "fold.platform_only" in skip:
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
        elif "fold.settled_clamped" in skip:
            total += min(Decimal(settled["cost_usd"]), Decimal(p["max_cost_usd"]))
        else:
            total += Decimal(settled["cost_usd"])
    return total


def admits(drafts: list[dict], scope: str, period: str, cap: str, max_cost: str, skip: frozenset[str] = frozenset()) -> bool:
    """§9.19: admitted only when spend plus the call's maximum is at most the cap."""
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


def reserve(n: int, agent: str | None = "agent_a", cost: str = "0.048", day: str = DAY, key: str = "platform") -> dict:
    return meter_record("MeterReserved", ulid("R", n), reserved_payload(ulid("C", n), agent, cost, day, key))


def settle(n: int, cost: str, reservation: int | None = None, call: int | None = None, event: int | None = None) -> dict:
    payload = settled_payload(ulid("C", call if call is not None else n), ulid("R", reservation if reservation is not None else n), cost)
    return meter_record("MeterSettled", ulid("S", event if event is not None else n), payload)


def stream_cases() -> list[dict]:
    def case(name, clause, records, expect):
        return {"name": name, "clause": clause, "records": [opened(), *records], "expect": expect}

    ok = {"outcome": "Valid"}
    return [
        case("interleaved", "rule 158: settlements in any order", [reserve(1), reserve(2), settle(2, "0.01"), settle(1, "0.02")], ok),
        case("open", "rule 158: a reservation never settled", [reserve(1)], ok),
        case("reserved_twice", "rule 158: one reservation per call", [reserve(1), meter_record("MeterReserved", ulid("R", 9), reserved_payload(ulid("C", 1)))], {"outcome": "Invalid", "index": 2, "reason": "schema", "path": "payload.call_id"}),
        case("unreserved", "rule 158: a settlement names an earlier reservation", [settle(1, "0.01")], {"outcome": "Invalid", "index": 1, "reason": "schema", "path": "payload.reservation"}),
        case("names_the_opening", "rule 158: the reservation is a `MeterReserved`", [reserve(1), meter_record("MeterSettled", ulid("S", 1), settled_payload(ulid("C", 1), OPENED))], {"outcome": "Invalid", "index": 2, "reason": "schema", "path": "payload.reservation"}),
        case("another_call", "rule 158: the same call", [reserve(1), reserve(2), settle(1, "0.01", reservation=2)], {"outcome": "Invalid", "index": 3, "reason": "schema", "path": "payload.call_id"}),
        case("settled_twice", "rule 158: one settlement per reservation", [reserve(1), settle(1, "0.01"), settle(1, "0.01", event=8)], {"outcome": "Invalid", "index": 3, "reason": "schema", "path": "payload.reservation"}),
        case("settled_before_reserved", "rule 158: the reservation is earlier", [settle(1, "0.01"), reserve(1)], {"outcome": "Invalid", "index": 1, "reason": "schema", "path": "payload.reservation"}),
    ]


def call_record(n: int, call: str, changes: list[dict]) -> dict:
    draft = copy.deepcopy(DRAFTS[BASE_CALL])
    draft["event_id"] = ulid("M", n)
    draft["payload"]["call_id"] = call
    for item in restated(BASE_CALL, changes):
        apply_change(draft, item)
    return draft


def agent_stream_cases() -> list[dict]:
    """Rule 153: one `ModelInvocationRecorded` per call on the agent stream."""
    late = [*failed("deadline_exceeded"), change("payload.response_ref", None)]
    return [
        {
            "name": "two_calls",
            "clause": "rule 153: each call recorded once",
            "records": [call_record(1, ulid("C", 1), []), call_record(2, ulid("C", 2), late)],
            "expect": {"outcome": "Valid"},
        },
        {
            "name": "an_ok_after_its_deadline",
            "clause": "rule 153: no later record brings a late output in",
            "records": [call_record(1, ulid("C", 1), late), call_record(2, ulid("C", 1), [])],
            "expect": {"outcome": "Invalid", "index": 1, "reason": "schema", "path": "payload.call_id"},
        },
        {
            "name": "recorded_twice",
            "clause": "rule 153: the same call twice",
            "records": [call_record(1, ulid("C", 3), []), call_record(2, ulid("C", 4), []), call_record(3, ulid("C", 3), [])],
            "expect": {"outcome": "Invalid", "index": 2, "reason": "schema", "path": "payload.call_id"},
        },
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
        reserve(6, "agent_b", "0.1", "2026-10-17", key="customer"),
        settle(6, "0.05"),
    ]
    near = [reserve(n, "agent_a", "4.9", f"2026-10-{n + 1:02d}") for n in range(1, 11)]
    near += [settle(n, "4.9") for n in range(1, 11)]
    return [
        {
            "name": "a_month_of_calls",
            "clause": "§9.19 spend: settled at cost (above its reservation too), open at maximum, a customer's key counted, by scope and period",
            "records": [opened(), *month],
            "expect": [
                {"scope": "workspace", "period": "month:2026-10", "spend_usd": "0.649"},
                {"scope": "workspace", "period": "month:2026-09", "spend_usd": "0.02"},
                {"scope": "workspace", "period": "day:2026-10-16", "spend_usd": "0.589"},
                {"scope": "agent_a", "period": "month:2026-10", "spend_usd": "0.07"},
                {"scope": "agent_b", "period": "day:2026-10-16", "spend_usd": "0.5"},
                {"scope": "agent_b", "period": "month:2026-10", "spend_usd": "0.55"},
                {"scope": "agent_b", "period": "month:2026-09", "spend_usd": "0"},
            ],
            "admissions": [
                {"scope": "workspace", "period": "month:2026-10", "cap_usd": "50", "max_cost_usd": "49.351", "admitted": True},
                {"scope": "workspace", "period": "month:2026-10", "cap_usd": "50", "max_cost_usd": "49.3511", "admitted": False},
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


# --------------------------------------------------------------------------- the seeded fuzz

FUZZ = {
    "seed": 20261017,
    "runs": 120,
    "steps": 40,
    "caps": {"workspace_monthly": "0.6", "agent_monthly": "0.35", "research_daily": "0.12"},
}
FUZZ_AGENTS = ("agent_a", "agent_b", None)
FUZZ_DAYS = ("2026-09-29", "2026-09-30", "2026-10-01", "2026-10-02", "2026-10-31", "2026-11-01")


def fuzz_caps(agent: str | None, purpose: str, day: str) -> list[tuple[str, str, str]]:
    """The caps a reservation is admitted under, as the gateway applies them (inference spec §7.3)."""
    caps = FUZZ["caps"]
    out = [("workspace", f"month:{day[:7]}", caps["workspace_monthly"])]
    if agent is not None:
        out.append((agent, f"month:{day[:7]}", caps["agent_monthly"]))
        if purpose == "research":
            out.append((agent, f"day:{day}", caps["research_daily"]))
    return out


def fuzz_stream(rng: random.Random, skip: frozenset[str]) -> tuple[list[dict], int]:
    """One random meter stream written as the gateway writes it: each reservation admitted by
    `admits` under every cap, each settlement at or below its reservation, some calls left open by a
    crash, the risk day moving forward and back across a month's end."""
    records = [opened()]
    open_ids: list[int] = []
    refused = 0
    for n in range(1, FUZZ["steps"] + 1):
        if open_ids and rng.random() < 0.4:
            k = open_ids.pop(rng.randrange(len(open_ids)))
            reserved = next(r for r in records if r["event_id"] == ulid("R", k))
            cap = Decimal(reserved["payload"]["max_cost_usd"])
            records.append(settle(k, str(cap * rng.randint(0, 100) / 100)))
            continue
        if open_ids and rng.random() < 0.1:
            open_ids.pop(rng.randrange(len(open_ids)))
            continue
        agent = rng.choice(FUZZ_AGENTS)
        day = rng.choice(FUZZ_DAYS)
        cost = str(Decimal(rng.randint(0, 150)) / 1000)
        key = rng.choice(("platform", "customer"))
        record = reserve(n, agent, cost, day, key)
        purpose = record["payload"]["purpose"]
        if all(admits(records, scope, period, cap, cost, skip) for scope, period, cap in fuzz_caps(agent, purpose, day)):
            records.append(record)
            open_ids.append(n)
        else:
            refused += 1
    return records, refused


def fuzz_oracle(records: list[dict], skip: frozenset[str]) -> list[str]:
    """An independent running accumulator over the stream in append order. After each reservation,
    every cap that applied to it must hold; at the end, every total must equal the fold."""
    problems = []
    totals: dict[tuple[str, str], Decimal] = {}
    open_keys: dict[str, tuple[Decimal, list[tuple[str, str]]]] = {}
    for r in records:
        p = r["payload"]
        if r["event_type"] == "MeterReserved":
            day = p["risk_day"]
            keys = [("workspace", f"month:{day[:7]}")]
            if p["agent_id"] is not None:
                keys += [(p["agent_id"], f"month:{day[:7]}"), (p["agent_id"], f"day:{day}")]
            maximum = Decimal(p["max_cost_usd"])
            for key in keys:
                totals[key] = totals.get(key, Decimal(0)) + maximum
            for scope, period, cap in fuzz_caps(p["agent_id"], p["purpose"], day):
                if totals[(scope, period)] > Decimal(cap):
                    problems.append(f"{scope} {period} reached {totals[(scope, period)]} over {cap}")
            open_keys[r["event_id"]] = (maximum, keys)
        elif r["event_type"] == "MeterSettled":
            maximum, keys = open_keys.pop(p["reservation"])
            for key in keys:
                totals[key] -= maximum - Decimal(p["cost_usd"])
    for (scope, period), total in totals.items():
        if fold_spend(records, scope, period, skip) != total:
            problems.append(f"the fold of {scope} {period} differs from the accumulator's {total}")
    return problems


def run_fuzz(fuzz: dict, skip: frozenset[str]) -> tuple[list[str], int, int]:
    rng = random.Random(fuzz["seed"])
    problems: list[str] = []
    admitted = refused = 0
    for _ in range(fuzz["runs"]):
        records, refusals = fuzz_stream(rng, skip)
        refused += refusals
        admitted += sum(1 for r in records if r["event_type"] == "MeterReserved")
        problems += fuzz_oracle(records, skip)
    return problems, admitted, refused


def fuzz_section() -> dict:
    _, admitted, refused = run_fuzz(FUZZ, frozenset())
    return {**FUZZ, "admitted": admitted, "refused": refused}


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = (
    "drafts.valid",
    "drafts.call",
    "invalid_drafts",
    "valid_drafts",
    "agent_streams.records",
    "agent_streams.rule",
    "streams.records",
    "streams.rule",
    "folds.records",
    "folds.spend",
    "folds.oracle",
    "folds.admits",
    "fuzz.caps",
    "fuzz.counts",
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
    for case in section["agent_streams"]:
        for i, record in enumerate(case["records"]):
            got = violations(record, skip)
            if got:
                problems.append(found("agent_streams.records", f"{case['name']} record {i}: {got}"))
        got = agent_stream_failure(case["records"], skip)
        if got != expected_failure(case):
            problems.append(found("agent_streams.rule", f"{case['name']}: expected {case['expect']}, got {got}"))
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
    fuzz = section["fuzz"]
    caps_broken, admitted, refused = run_fuzz(fuzz, skip)
    problems += [found("fuzz.caps", p) for p in caps_broken[:3]]
    if (admitted, refused) != (fuzz["admitted"], fuzz["refused"]) or not (admitted and refused):
        problems.append(found("fuzz.counts", f"admitted {admitted} and refused {refused}, not {fuzz['admitted']} and {fuzz['refused']}"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "record.extra",
    "record.missing",
    "config_refs.required",
    *(f"rule.{n}" for n in ("146", "147", "148.prompt", "148.request", "148.guard", "149", "150")),
    *(f"rule.151.{n}" for n in ("empty", "candidate", "kind_order", "result")),
    "rule.152",
    "rule.152.any_outcome",
    "rule.154",
    "rule.155",
    "rule.156.agent",
    "rule.156.evaluation",
    "rule.156.cost",
    *(f"rule.157.{n}" for n in ("cost", "unsent", "tokens", "free", "any_outcome")),
    "rule.149.refusals_only",
    "stream.153.once",
    "stream.158.once",
    "stream.158.known",
    "stream.158.same_call",
    "stream.158.settled_once",
    "fold.agent_any",
    "fold.platform_only",
    "fold.month_by_year",
    "fold.open_free",
    "fold.settled_at_max",
    "fold.settled_clamped",
    "admit.strict",
)
FUZZ_CAUGHT = ("fold.open_free", "fold.platform_only", "fold.month_by_year", "fold.agent_any")


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
        ("an invalid draft's path differs", "invalid_drafts", mutated(lambda s: case(s, "invalid_drafts", "call.149.sent_unreserved")["expect"].update(path="payload.prompt_ref"))),
        ("a valid draft reserves a refusal", "valid_drafts", mutated(lambda s: case(s, "valid_drafts", "call.refusal.budget_exhausted")["changes"].append(change("payload.reservation", RESERVED)))),
        ("an agent stream record is the gateway's", "agent_streams.records", mutated(lambda s: case(s, "agent_streams", "two_calls")["records"][0].update(actor=dict(GATEWAY)))),
        ("an agent stream case expects the wrong record", "agent_streams.rule", mutated(lambda s: case(s, "agent_streams", "recorded_twice")["expect"].update(index=1))),
        ("a stream record is written by the runtime", "streams.records", mutated(lambda s: case(s, "streams", "open")["records"][1].update(actor=dict(RUNTIME)))),
        ("a stream case expects the wrong record", "streams.rule", mutated(lambda s: case(s, "streams", "settled_twice")["expect"].update(index=2))),
        ("a fold's stream settles a call twice", "folds.records", mutated(lambda s: case(s, "folds", "a_month_of_calls")["records"].append(copy.deepcopy(case(s, "folds", "a_month_of_calls")["records"][2])))),
        ("a fold's stated spend is wrong", "folds.spend", mutated(lambda s: case(s, "folds", "a_month_of_calls")["expect"][0].update(spend_usd="0.599"))),
        ("an admission at the cap is refused", "folds.admits", mutated(lambda s: case(s, "folds", "the_monthly_cap_reached")["admissions"][0].update(admitted=False))),
        ("the fuzz's stated counts are wrong", "fuzz.counts", mutated(lambda s: s["fuzz"].update(admitted=s["fuzz"]["admitted"] + 1))),
    ]


def run_mutants(section: dict) -> list[str]:
    """Every seeded bug caught; a vector mutant only by the check it is registered against; and the
    fuzz's own oracle catches every fold bug that lets spend past a cap."""
    escaped = []
    for mutant in VALIDATOR_MUTANTS:
        if not check_section(section, frozenset([mutant])):
            escaped.append(f"gateway mutant {mutant}")
    for mutant in FUZZ_CAUGHT:
        if "fuzz.caps" not in {check_of(p) for p in check_section(section, frozenset([mutant]))}:
            escaped.append(f"gateway fuzz oracle misses {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if check not in ("folds.oracle", "fuzz.caps") and not any(c == check for _, c, _ in registered):
            escaped.append(f"gateway check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"gateway vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    seeded = copy.deepcopy(section)
    seeded["folds"][0]["expect"][0]["spend_usd"] = "0.599"
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
        "agent_streams": agent_stream_cases(),
        "streams": stream_cases(),
        "folds": fold_cases(),
        "fuzz": fuzz_section(),
    }


__all__ = ["Violation", "build_section", "check_section", "run_mutants", "vector_mutants"]
