"""Journal spec v0.21 §9.9's reference vectors (DEC-670): the control-stream records the workspace API
commits. They are a saved mandate draft (`MandateDraftSaved`), the compiler's model call
(`ModelInvocationRecorded` on the control stream), a confirmation that names the agent it is for
(`MandateConfirmed` version 2), and an owner's request (`OwnerRequestSubmitted`).

The schemas and rules 69 to 80 live in `control.py`, beside §9.2's to §9.7's, so one validator
judges every closed schema. This module builds the `workspace_api` section: a base draft of each
record, an invalid draft for every member type and rule, and valid drafts for the cases a rule might
be misread to refuse. It checks the section with three oracles of its own: the drafts' lineage (a
compile reads one save and its result names both), what a call that made no attempt can have
spent or received, and the human each confirmation and request names. Every seeded bug is shown
caught.
"""

from __future__ import annotations

import copy
from datetime import datetime
from decimal import Decimal

from common import change, delete, digest_strings
from control import (
    AGENT,
    AGENT_STREAM,
    SERVICES,
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

SPEC = "docs/specs/journal.md v0.21 §9.9 (DEC-670)"
AT = "2026-10-08T14:00:00.000000000Z"
IDS = {
    "created": "01J8Z4W0A000000000000000W1",
    "compile": "01J8Z4W1A000000000000000W2",
    "compiled": "01J8Z4W2A000000000000000W3",
    "confirmed": "01J8Z4W3A000000000000000W4",
    "request": "01J8Z4W4A000000000000000W5",
}
CALL_ID = "01J8Z4W1B000000000000000W6"
DRAFT_ID = "draft_01"
DRAFT_V1 = "sha256:" + "4" * 64
DRAFT_V2 = "sha256:" + "5" * 64
COMPILER = "sha256:" + "6" * 64
BASE_VERSION = "sha256:" + "1" * 64
NEW_VERSION = "sha256:" + "2" * 64
RECORD = "sha256:" + "7" * 64
REQUEST_DIGEST = "sha256:" + "8" * 64
PROMPT = "sha256:" + "9" * 64
RESPONSE = "sha256:" + "a" * 64
PRICE_TABLE = "sha256:" + "b" * 64
ASSET = "0f9e7d3c-5b1a-4c2e-8f6d-9a8b7c6d5e4f"
DEADLINE = "2026-10-08T14:00:30.000000000Z"
COMPLETED = "2026-10-08T14:00:12.500000000Z"
AGENT_ACTOR = {"kind": "agent", "id": AGENT, "version": "0.1.0", "build": "sha256:" + "c" * 64}
BROKER = {"kind": "broker", "id": "broker_01", "version": "1", "build": None}
OPERATOR = {"kind": "platform_operator", "id": "op_01", "version": "1", "build": None}
OTHER_ACTORS = {"agent": AGENT_ACTOR, "broker": BROKER, "platform_operator": OPERATOR}
# Each actor rule, the base it is judged on, and the kinds besides those its own cases already cover.
ACTOR_RULES = (
    ("70", "draft_created", ("agent", "broker", "platform_operator")),
    ("76", "compile_ok", ("agent", "broker", "platform_operator")),
    ("78", "confirmed_v2", ("agent", "broker", "platform_operator")),
    ("79", "request_owner", ("agent", "broker", "platform_operator")),
)
NOTHING_SENT = {
    "response_ref": None,
    "reported_identity": None,
    "provider_request_id": None,
    "attempts": 0,
    "tokens": {"input": 0, "output": 0, "cached": 0},
    "cost_usd": "0",
}


def envelope(name, event_type, actor, payload, causation=None, refs=None, version=1) -> dict:
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": IDS[name],
        "stream_id": STREAM,
        "event_type": event_type,
        "schema_version": version,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": causation,
        "correlation_id": None,
        "actor": dict(actor),
        "config_refs": refs or {},
        "payload": payload,
        "artifact_refs": sorted(digest_strings(payload)),
        "pii_refs": [],
    }


def base_drafts() -> dict[str, dict]:
    created = {"draft_id": DRAFT_ID, "draft": DRAFT_V1, "origin": "description", "base_draft": None, "base_version": None}
    compile_ok = {
        "call_id": CALL_ID,
        "purpose": "compiler",
        "draft_id": DRAFT_ID,
        "draft": DRAFT_V1,
        "model": {"model_id": "compiler", "model_version": "1", "content_hash": COMPILER},
        "endpoint": "gateway_default",
        "request_digest": REQUEST_DIGEST,
        "sampling": {"temperature": "0", "seed": 7},
        "prompt_ref": PROMPT,
        "response_ref": RESPONSE,
        "reported_identity": "compiler-1",
        "provider_request_id": "req_01",
        "outcome": "ok",
        "attempts": 1,
        "tokens": {"input": 1200, "output": 400, "cached": 0},
        "cost_usd": "0.0042",
        "price_table_ref": PRICE_TABLE,
        "cache_hit": False,
        "deadline": DEADLINE,
        "completed_at": COMPLETED,
    }
    compiled = {"draft_id": DRAFT_ID, "draft": DRAFT_V2, "origin": "compile", "base_draft": DRAFT_V1, "base_version": None}
    confirmed = {
        "mandate_version": NEW_VERSION,
        "confirmed_paths": ["/capital", "/risk"],
        "record_ref": RECORD,
        "agent_id": AGENT,
        "base_version": BASE_VERSION,
    }
    request = {
        "agent_id": AGENT,
        "instrument_id": ASSET,
        "side": "buy",
        "quantity": "10",
        "requested_by": "owner",
        "client_id": None,
    }
    return {
        "draft_created": envelope("created", "MandateDraftSaved", USER, created),
        "compile_ok": envelope(
            "compile", "ModelInvocationRecorded", SERVICES, compile_ok, refs={"model_version": COMPILER}
        ),
        "draft_compiled": envelope("compiled", "MandateDraftSaved", USER, compiled, IDS["compile"]),
        "confirmed_v2": envelope("confirmed", "MandateConfirmed", USER, confirmed, version=2),
        "request_owner": envelope("request", "OwnerRequestSubmitted", USER, request),
    }


BASES = base_drafts()


def draft_change(draft: dict, item: dict) -> None:
    *parents, last = item["path"].split(".")
    node = draft
    for name in parents:
        node = node[name]
    if item.get("delete"):
        del node[last]
    else:
        node[last] = copy.deepcopy(item["value"])


def refs_kept(base: str, changes: list[dict]) -> list[dict]:
    """`changes`, then `artifact_refs` set to the changed payload's digests (§3), so a case that edits
    a reference member breaks only the rule it names."""
    draft = copy.deepcopy(BASES[base])
    for item in changes:
        draft_change(draft, item)
    return [*changes, change("artifact_refs", sorted(digest_strings(draft["payload"])))]


def invalid(name, clause, base, changes, reason, path, also=()):
    return control_invalid(name, clause, base, refs_kept(base, changes), reason, path, also)


def valid(name, clause, base, changes):
    return control_valid(name, clause, base, refs_kept(base, changes))


def refused(outcome: str) -> list[dict]:
    """A refusal outcome with everything §3.3 says a refusal never sends or spends at zero."""
    return [change("payload.outcome", outcome), *(change(f"payload.{m}", v) for m, v in NOTHING_SENT.items())]


# Each member, a value of the wrong JSON kind (`schema`) and, where its type constrains a string, a
# string of the wrong form (`non_canonical`).
MEMBER_CASES = {
    "draft_created": (
        ("draft_id", 7, "draft 01"),
        ("draft", 7, "draft_v1"),
        ("origin", 7, "chat"),
        ("base_draft", 7, "sha256:short"),
        ("base_version", 7, "v1"),
    ),
    "compile_ok": (
        ("call_id", 7, "not-a-ulid"),
        ("purpose", 7, "research"),
        ("draft_id", 7, "draft/01"),
        ("draft", 7, "sha256:" + "G" * 64),
        ("model", "compiler", None),
        ("endpoint", 7, ""),
        ("request_digest", 7, "digest"),
        ("sampling", 0, None),
        ("prompt_ref", 7, "prompt.txt"),
        ("response_ref", 7, "response.txt"),
        ("reported_identity", 7, ""),
        ("provider_request_id", 7, ""),
        ("outcome", 7, "compile_failed"),
        ("attempts", "1", None),
        ("tokens", [], None),
        ("cost_usd", 42, "4.2e-3x"),
        ("price_table_ref", 7, "prices_v1"),
        ("cache_hit", "false", None),
        ("deadline", 1791468030, "2026-10-08T14:00:30Z"),
        ("completed_at", 1791468012, "2026-10-08 14:00:12.500000000Z"),
    ),
    "confirmed_v2": (
        ("agent_id", 7, "agent:a"),
        ("base_version", 7, "v1"),
    ),
    "request_owner": (
        ("agent_id", 7, "agent a"),
        ("instrument_id", 7, "AAPL"),
        ("side", 7, "short"),
        ("quantity", 10, "ten"),
        ("requested_by", 7, "agent"),
        ("client_id", 7, "client 1"),
    ),
}
# Nested members, each a wrong kind or form at its own path.
NESTED_CASES = (
    ("compile_ok", "model.content_hash", "sha256:" + "A" * 64, "non_canonical"),
    ("compile_ok", "model.model_id", 7, "schema"),
    ("compile_ok", "sampling.temperature", 0, "schema"),
    ("compile_ok", "sampling.seed", "7", "schema"),
    ("compile_ok", "tokens.input", "1200", "schema"),
    ("compile_ok", "tokens.cached", None, "schema"),
)
# `requested_by`'s value is rule 79's, at the member's own path with the same reason, so a `null` is
# refused identically whether or not its type is checked first.
RULE_TYPED = ("requested_by",)
# Non-nullable members a `null` must not satisfy, each its own case.
NULLED = {
    "draft_created": ("draft_id", "draft", "origin"),
    "compile_ok": (
        "call_id",
        "purpose",
        "draft",
        "model",
        "request_digest",
        "prompt_ref",
        "outcome",
        "attempts",
        "cost_usd",
        "price_table_ref",
        "cache_hit",
        "deadline",
        "completed_at",
    ),
    "confirmed_v2": ("mandate_version", "record_ref"),
    "request_owner": ("agent_id", "instrument_id", "side", "requested_by"),
}


def member_drafts() -> list[dict]:
    out = []
    for base, cases in MEMBER_CASES.items():
        for member, wrong_kind, wrong_form in cases:
            path = f"payload.{member}"
            out.append(invalid(f"{base}.{member}.kind", "§9.9 types", base, [change(path, wrong_kind)], "schema", path))
            if wrong_form is not None:
                form = [change(path, wrong_form)]
                out.append(invalid(f"{base}.{member}.form", "§9.9 types", base, form, "non_canonical", path))
        for member in NULLED[base]:
            path = f"payload.{member}"
            out.append(invalid(f"{base}.{member}.null", "§9.9 types", base, [change(path, None)], "schema", path))
        some = MEMBER_CASES[base][0][0]
        out.append(invalid(f"{base}.missing", "§9.9 closed", base, [delete(f"payload.{some}")], "schema", f"payload.{some}"))
        out.append(invalid(f"{base}.extra", "§9.9 closed", base, [change("payload.note", "x")], "schema", "payload.note"))
    for base, member, value, reason in NESTED_CASES:
        path = f"payload.{member}"
        out.append(invalid(f"{base}.{member}", "§9.9 types", base, [change(path, value)], reason, path))
    return out


def actor_drafts() -> list[dict]:
    """Every actor kind a rule refuses, not only `system` and `user`, so a rule read as "not the
    system" or "not a user" is caught."""
    return [
        invalid(
            f"rule_{number}_refuses_{kind}",
            f"rule {number}: a {kind} actor",
            base,
            [change("actor", dict(OTHER_ACTORS[kind]))],
            "schema",
            "actor.kind",
        )
        for number, base, kinds in ACTOR_RULES
        for kind in kinds
    ]


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule, or the rules its `also` lists; together they cover every
    §9.9 member type and rule."""
    call = "compile_ok"
    save = "draft_created"
    conf = "confirmed_v2"
    req = "request_owner"
    return [
        *member_drafts(),
        *actor_drafts(),
        invalid(
            "draft_wrong_stream",
            "§9.9: a draft is a control-stream record",
            save,
            [change("stream_id", AGENT_STREAM)],
            "wrong_stream",
            "event_type",
        ),
        invalid(
            "compile_without_its_model",
            "§9 row: `ModelInvocationRecorded` names `model_version`",
            call,
            [change("config_refs", {})],
            "missing_config_ref",
            "config_refs.model_version",
        ),
        invalid(
            "draft_refs_not_listed",
            "§3: `artifact_refs` lists the payload's references",
            save,
            [],
            "artifact_refs",
            "artifact_refs",
        )
        | {"changes": [change("artifact_refs", [])]},
        invalid(
            "confirmed_v1_with_the_agent_link",
            "§9.9: version 1 stays closed; the agent link is version 2's",
            conf,
            [change("schema_version", 1)],
            "schema",
            "payload.agent_id",
            also=[("schema", "payload.base_version")],
        ),
        invalid(
            "confirmed_unknown_version",
            "§9.9: versions 1 and 2 are registered",
            conf,
            [change("schema_version", 3)],
            "unknown_schema",
            "payload",
        ),
        invalid(
            "new_draft_with_a_base",
            "rule 69: a new draft replaces no save",
            save,
            [change("payload.base_draft", DRAFT_V2)],
            "schema",
            "payload.base_draft",
        ),
        invalid(
            "edit_without_its_base",
            "rule 69: an edit names the save it replaces",
            save,
            [change("payload.origin", "edit")],
            "schema",
            "payload.base_draft",
        ),
        invalid(
            "draft_from_a_version_without_it",
            "rule 69: origin `version` names the version",
            save,
            [change("payload.origin", "version")],
            "schema",
            "payload.base_version",
        ),
        invalid(
            "draft_from_a_description_with_a_version",
            "rule 69",
            save,
            [change("payload.base_version", BASE_VERSION)],
            "schema",
            "payload.base_version",
        ),
        invalid(
            "compiled_draft_without_its_call",
            "rule 69: a compiled save names the call it came from",
            "draft_compiled",
            [change("causation_id", None)],
            "schema",
            "causation_id",
        ),
        invalid(
            "draft_reports_its_first_failure",
            "rule 69: in order, the base draft before the version",
            save,
            [change("payload.origin", "edit"), change("payload.base_version", BASE_VERSION)],
            "schema",
            "payload.base_draft",
        ),
        invalid(
            "draft_saved_by_the_system",
            "rule 70: a draft is a person's",
            save,
            [change("actor", dict(SERVICES))],
            "schema",
            "actor.kind",
        ),
        invalid(
            "compile_names_another_model",
            "rule 71: the call binds the registered model it names",
            call,
            [change("payload.model.content_hash", BASE_VERSION)],
            "schema",
            "payload.model.content_hash",
        ),
        invalid(
            "ok_without_a_response",
            "rule 72",
            call,
            [change("payload.response_ref", None)],
            "schema",
            "payload.response_ref",
        ),
        invalid(
            "ok_without_its_identity",
            "rule 72",
            call,
            [change("payload.reported_identity", None)],
            "schema",
            "payload.reported_identity",
        ),
        invalid(
            "refusal_with_an_attempt",
            "rule 73: a refusal sends nothing",
            call,
            [*refused("budget_exhausted"), change("payload.attempts", 1)],
            "schema",
            "payload.attempts",
        ),
        invalid(
            "refusal_with_a_cost",
            "rule 73",
            call,
            [*refused("meter_unavailable"), change("payload.cost_usd", "0.001")],
            "schema",
            "payload.cost_usd",
        ),
        invalid(
            "refusal_with_tokens",
            "rule 73",
            call,
            [*refused("input_rejected"), change("payload.tokens.input", 10)],
            "schema",
            "payload.tokens",
        ),
        invalid(
            "refusal_with_a_provider_request",
            "rule 73",
            call,
            [*refused("policy_denied"), change("payload.provider_request_id", "req_01")],
            "schema",
            "payload.provider_request_id",
        ),
        invalid(
            "refusal_reports_its_first_failure",
            "rule 73: in order, the response before the cost",
            call,
            [*refused("model_withdrawn"), change("payload.response_ref", RESPONSE), change("payload.cost_usd", "1")],
            "schema",
            "payload.response_ref",
        ),
        invalid(
            "cache_hit_on_a_failure",
            "rule 74: only a complete, valid response is cached",
            call,
            [change("payload.outcome", "schema_invalid"), change("payload.cache_hit", True)],
            "schema",
            "payload.cache_hit",
        ),
        invalid(
            "cache_hit_on_a_refusal",
            "rule 74: a refusal served nothing, from the cache either",
            call,
            [*refused("budget_exhausted"), change("payload.cache_hit", True)],
            "schema",
            "payload.cache_hit",
        ),
        invalid(
            "cache_hit_with_an_attempt",
            "rule 74: a hit sends nothing",
            call,
            [change("payload.cache_hit", True), change("payload.provider_request_id", None), change("payload.cost_usd", "0")],
            "schema",
            "payload.attempts",
        ),
        invalid(
            "cache_hit_with_a_cost",
            "rule 74: a hit costs nothing",
            call,
            [change("payload.cache_hit", True), change("payload.attempts", 0), change("payload.provider_request_id", None)],
            "schema",
            "payload.cost_usd",
        ),
        invalid(
            "cache_hit_with_a_provider_request",
            "rule 74: a hit makes no provider request",
            call,
            [change("payload.cache_hit", True), change("payload.attempts", 0), change("payload.cost_usd", "0")],
            "schema",
            "payload.provider_request_id",
        ),
        invalid(
            "sent_without_an_attempt",
            "rule 74: a call that reached the provider made an attempt",
            call,
            [change("payload.outcome", "provider_unavailable"), change("payload.attempts", 0)],
            "schema",
            "payload.attempts",
        ),
        invalid(
            "negative_cost",
            "rule 75",
            call,
            [change("payload.outcome", "schema_invalid"), change("payload.cost_usd", "-0.01")],
            "schema",
            "payload.cost_usd",
        ),
        invalid(
            "ok_after_its_deadline",
            "rule 75: one nanosecond late is late (INF-3)",
            call,
            [change("payload.completed_at", "2026-10-08T14:00:30.000000001Z")],
            "schema",
            "payload.completed_at",
        ),
        invalid(
            "compile_by_a_user",
            "rule 76: the services that called the gateway write it",
            call,
            [change("actor", dict(USER))],
            "schema",
            "actor.kind",
        ),
        invalid(
            "confirm_agent_without_its_base",
            "rule 77: a version for a deployed agent names its base",
            conf,
            [change("payload.base_version", None)],
            "schema",
            "payload.base_version",
        ),
        invalid(
            "confirm_base_without_an_agent",
            "rule 77",
            conf,
            [change("payload.agent_id", None)],
            "schema",
            "payload.base_version",
        ),
        invalid(
            "confirm_the_version_in_force",
            "rule 77: a confirmation moves the agent off its base",
            conf,
            [change("payload.base_version", NEW_VERSION)],
            "schema",
            "payload.base_version",
        ),
        invalid(
            "confirm_by_the_system",
            "rule 78: only a user confirms",
            conf,
            [change("actor", dict(SERVICES))],
            "schema",
            "actor.kind",
        ),
        invalid(
            "request_by_the_system",
            "rule 79: a request is a person's",
            req,
            [change("actor", dict(SERVICES))],
            "schema",
            "actor.kind",
        ),
        invalid(
            "request_claims_a_client",
            "rule 79: `requested_by` comes from the actor, never the body",
            req,
            [change("payload.requested_by", "client"), change("payload.client_id", "client_01")],
            "schema",
            "payload.requested_by",
        ),
        invalid(
            "owner_request_names_a_client",
            "rule 79",
            req,
            [change("payload.client_id", "client_01")],
            "schema",
            "payload.client_id",
        ),
        invalid("request_zero_size", "rule 80", req, [change("payload.quantity", "0")], "schema", "payload.quantity"),
        invalid("request_negative_size", "rule 80", req, [change("payload.quantity", "-5")], "schema", "payload.quantity"),
    ]


def valid_drafts() -> list[dict]:
    """Cases a rule might be misread to refuse; each must be accepted."""
    save = "draft_created"
    call = "compile_ok"
    conf = "confirmed_v2"
    return [
        valid("draft_from_goal_answers", "rule 69: a new draft", save, [change("payload.origin", "goal_answers")]),
        valid("draft_from_a_template", "rule 69", save, [change("payload.origin", "template")]),
        valid(
            "draft_from_a_version",
            "rule 69: the version it started from",
            save,
            [change("payload.origin", "version"), change("payload.base_version", BASE_VERSION)],
        ),
        valid(
            "draft_edited",
            "rule 69: an edit names its base and needs no cause",
            save,
            [change("payload.origin", "edit"), change("payload.base_draft", DRAFT_V2)],
        ),
        valid("call_refused", "rules 73 and 74: a refusal sent and spent nothing", call, refused("budget_exhausted")),
        valid(
            "call_cache_hit",
            "rule 74: a hit has its response, no attempt, and no cost",
            call,
            [
                change("payload.cache_hit", True),
                change("payload.attempts", 0),
                change("payload.provider_request_id", None),
                change("payload.cost_usd", "0"),
            ],
        ),
        valid(
            "call_schema_invalid",
            "rules 72 and 74: the `compile_failed` call keeps its response and its cost",
            call,
            [change("payload.outcome", "schema_invalid")],
        ),
        valid(
            "call_late",
            "rule 75: a late response is `deadline_exceeded`, not `ok`, and may complete after the deadline",
            call,
            [
                change("payload.outcome", "deadline_exceeded"),
                change("payload.completed_at", "2026-10-08T14:00:31.000000000Z"),
                change("payload.response_ref", None),
                change("payload.reported_identity", None),
                change("payload.attempts", 2),
            ],
        ),
        valid(
            "call_at_its_deadline",
            "rule 75: completing at the deadline is in time",
            call,
            [change("payload.completed_at", DEADLINE)],
        ),
        valid("call_without_a_seed", "§9.9: a provider without seeds", call, [change("payload.sampling.seed", None)]),
        valid(
            "confirm_new_mandate",
            "rule 77: a new mandate names no agent and no base",
            conf,
            [change("payload.agent_id", None), change("payload.base_version", None)],
        ),
        valid(
            "confirmed_v1_unchanged",
            "§9.9: version 1 stays registered",
            conf,
            [change("schema_version", 1), delete("payload.agent_id"), delete("payload.base_version")],
        ),
        valid(
            "request_sell_unsized",
            "rule 80: no size lets the builder size it",
            "request_owner",
            [change("payload.side", "sell"), change("payload.quantity", None)],
        ),
    ]


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = ("drafts.valid", "drafts.lineage", "drafts.sent", "drafts.human", "invalid_drafts", "valid_drafts")


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def instant(text: str) -> tuple[datetime, int]:
    return datetime.strptime(text[:19], "%Y-%m-%dT%H:%M:%S"), int(text[20:29])


def sent_problems(name: str, draft: dict) -> list[str]:
    """What a call that made no attempt can show: no provider request, no tokens or cost unless
    served from cache, and an `ok` only in time. Computed from inference spec §3.2 to §3.5, not from
    rules 72 to 75's outcome lists."""
    p = draft["payload"]
    out = []
    if p["attempts"] == 0:
        spent = Decimal(p["cost_usd"]) != 0 or p["provider_request_id"] is not None
        if not p["cache_hit"]:
            spent = spent or p["response_ref"] is not None or sum(p["tokens"].values()) != 0
        if spent:
            out.append(found("drafts.sent", f"{name}: no attempt, yet something was sent or spent"))
    if p["outcome"] == "ok" and instant(p["completed_at"]) > instant(p["deadline"]):
        out.append(found("drafts.sent", f"{name}: an ok completed after its deadline"))
    return out


def human_problems(name: str, draft: dict) -> list[str]:
    """Only a user confirms a version (workspace API §5.1), and an owner's request is a user's with
    no client named (workspace API §4.6)."""
    kind = draft["actor"]["kind"]
    p = draft["payload"]
    if draft["event_type"] == "MandateConfirmed" and draft["schema_version"] == 2 and kind != "user":
        return [found("drafts.human", f"{name}: confirmed by a {kind}")]
    if draft["event_type"] == "OwnerRequestSubmitted" and p["requested_by"] == "owner":
        if kind != "user" or p["client_id"] is not None:
            return [found("drafts.human", f"{name}: an owner request not a user's alone")]
    return []


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    drafts = section["drafts"]
    for name, draft in drafts.items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    created, call, compiled = drafts["draft_created"], drafts["compile_ok"], drafts["draft_compiled"]
    ids = {created["payload"]["draft_id"], call["payload"]["draft_id"], compiled["payload"]["draft_id"]}
    if (
        len(ids) != 1
        or call["payload"]["draft"] != created["payload"]["draft"]
        or compiled["payload"]["base_draft"] != created["payload"]["draft"]
        or compiled["causation_id"] != call["event_id"]
    ):
        problems.append(found("drafts.lineage", "the compiled save does not follow the call that read its base"))
    accepted = [(name, draft) for name, draft in drafts.items()]
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case))
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        draft = draft_for(section, case)
        got = violations(draft)
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
        accepted.append((case["name"], draft))
    for name, draft in accepted:
        if draft["event_type"] == "ModelInvocationRecorded":
            problems += sent_problems(name, draft)
        problems += human_problems(name, draft)
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.69.base_draft",
    "rule.69.base_version",
    "rule.69.compile",
    "rule.70",
    "rule.71",
    "rule.72",
    "rule.73",
    "rule.73.tokens",
    "rule.74.hit",
    "rule.74.free",
    "rule.74.attempts",
    "boundary.rule_74_attempts",
    "rule.75.cost",
    "rule.75.late",
    "boundary.rule_75_strict",
    "rule.76",
    "rule.77.paired",
    "rule.77.moved",
    "rule.78",
    "rule.79.actor",
    "rule.70.not_system_only",
    "rule.76.not_user_only",
    "rule.78.not_system_only",
    "rule.79.not_system_only",
    "rule.79.requested_by",
    "rule.79.client_id",
    "rule.80",
    "boundary.rule_80_zero",
    "record.extra",
    "record.missing",
    "config_refs.required",
    "artifact_refs",
    *sorted({f"loose.payload.{member}" for cases in MEMBER_CASES.values() for member, _, _ in cases}),
    *sorted({f"loose.payload.{member.rsplit('.', 1)[1]}" for _, member, _, _ in NESTED_CASES}),
    *sorted(
        {f"nullable.payload.{member}" for members in NULLED.values() for member in members}
        - {f"nullable.payload.{member}" for member in RULE_TYPED}
    ),
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
        (
            "a base draft breaks rule 69",
            "drafts.valid",
            mutated(lambda s: s["drafts"]["draft_created"]["payload"].update(base_draft=DRAFT_V2)),
        ),
        (
            "the compiled save names another base than the call read",
            "drafts.lineage",
            mutated(lambda s: s["drafts"]["draft_compiled"]["payload"].update(base_draft=DRAFT_V2)),
        ),
        (
            "a valid hit is written as sent by a provider while rule 73 reads only refusals",
            "drafts.sent",
            mutated(
                lambda s: case(s, "valid_drafts", "call_cache_hit")["changes"].append(
                    change("payload.provider_request_id", "req_01")
                )
            ),
        ),
        (
            "the base confirmation is the services' while rule 78 is read loosely",
            "drafts.human",
            mutated(lambda s: s["drafts"]["confirmed_v2"].update(actor=dict(SERVICES))),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "new_draft_with_a_base")["expect"].update(path="payload.origin")),
        ),
        (
            "a valid draft breaks a rule",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "draft_edited")["changes"].append(change("payload.base_draft", None))),
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
            escaped.append(f"workspace validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"workspace check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"workspace vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
