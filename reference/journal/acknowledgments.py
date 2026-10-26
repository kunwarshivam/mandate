"""Journal spec v0.57 §9.28's reference vectors (DEC-2509): the owner's acknowledgment of a triggered
limit, `OwnerAcknowledged`, and its request, `OwnerAcknowledgmentRequested`, on the control stream,
with the executor's copies of both on the agent's account stream.

The schemas and rule 239 live in `control.py`, beside the other closed control-stream schemas, so one
validator judges every closed schema; so does rule 28's scoping of `OwnerCommandRefused`'s three new
reasons. This module builds the `acknowledgments` section:

1. base drafts of both events, their account-stream copies, and the executor's refusals for the
   three new reasons;
2. an invalid draft for every member type of both events, for every clause of rule 239, and for
   rule 28's new reasons off the account stream, and valid drafts for the cases a rule might be
   misread to refuse;
3. control streams for the stream rules 240 and 241, each record judged from the stream's start
   (its `append` first, then the stream rules), with the first failure typed by hand, and the first
   failure of rules 240 and 241 alone, so their clauses hold even where rule 239 was missed;
4. account streams for the executor's judgment at processing (§9.28 checks 1 to 4, in order), each
   with the overlay's independence requirement as an explicit input, the control stream its copies
   came from, and the outcome of every acknowledgment copy typed by hand: `lift`, or the reason of
   the `OwnerCommandRefused` it records;
5. a seeded fuzz over random orderings of requests, acknowledgments, triggers, lifts, and the owner
   commands and approval answers whose assertions a step-up may reuse, judged by the executor's
   forward fold and by an oracle written apart from it, which for each acknowledgment scans the
   streams' prefixes backwards; the two must agree on every outcome.

Every stream record passes every per-draft rule unless its case lists it as refused at append,
every expectation is typed from the rules rather than computed by the code under test, and every
seeded bug, in the validator, the stream rules, the executor's checks, the oracle, and the vectors,
is shown caught by the check it is registered against.
"""

from __future__ import annotations

import calendar
import copy
import random
import time

from common import apply_change, change, delete, digest_strings, is_ulid
from control import (
    ACCOUNT_STREAM_REF,
    AGENT_STREAM,
    EXECUTOR,
    RUNTIME,
    STREAM,
    WORKSPACE,
    check_of,
    reported,
    violations,
)

SPEC = "docs/specs/journal.md v0.57 §9.28 (DEC-2509)"
ACCOUNT = f"acct:{WORKSPACE}:{ACCOUNT_STREAM_REF}"
AGENT = "agent_a"
OTHER_AGENT = "agent_b"
REQUESTER = "user_owner_01"
ACKNOWLEDGER = "user_owner_02"
THIRD = "user_owner_03"
CLIENT = {"kind": "client", "id": "client_01", "version": "1", "build": None, "on_behalf_of": REQUESTER}
T0 = 1792339200
WINDOW = 300
METHODS = ("cli_confirm",)
LIMIT = "max_daily_loss"
OTHER_LIMIT = "drawdown_ladder[0]"
MANDATE_VERSION = "sha256:" + "7" * 64
REUSE_TYPES = ("ApprovalResponseSubmitted", "OwnerCommandIssued", "OwnerAcknowledgmentRequested", "OwnerAcknowledged")
REFUSALS = (
    "trigger_unknown",
    "trigger_superseded",
    "step_up_missing",
    "step_up_stale",
    "step_up_reused",
    "step_up_method",
    "not_independent",
    "request_invalid",
)
OUTCOMES = ("lift", *REFUSALS)
MATCHED = ("agent", "event", "requested_by", "independent_required")
TRIGGERED_MEMBERS = (
    "agent_id",
    "agent_equity",
    "high_water_mark",
    "drawdown",
    "day_start_equity",
    "capital_base",
    "inherited_loss",
    "net_contributed",
    "limit",
    "action",
    "reason",
    "breach_time_s",
    "metric",
    "threshold",
    "value",
    "risk_clock",
)
LIFTED_MEMBERS = (
    "agent_id",
    "agent_equity",
    "high_water_mark",
    "drawdown",
    "day_start_equity",
    "capital_base",
    "inherited_loss",
    "net_contributed",
    "limit",
    "action",
    "reason",
    "risk_clock",
)
SNAPSHOT = {
    "agent_equity": "9700",
    "high_water_mark": "10000",
    "drawdown": "0.03",
    "day_start_equity": "10000",
    "capital_base": "10000",
    "inherited_loss": "0",
    "net_contributed": "10000",
}


def ts(seconds: int) -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime(seconds)) + ".000000000Z"


def seconds_of(stamp: str) -> int:
    return calendar.timegm(time.strptime(stamp[:19], "%Y-%m-%dT%H:%M:%S"))


def uid(tag: str, n: int) -> str:
    """A ULID for the vectors: `tag` from Crockford's alphabet, then the number."""
    text = ("01JB" + tag + f"{n:05d}").ljust(26, "0")
    assert is_ulid(text), text
    return text


TRIGGER = uid("TG", 1)
REQUEST = uid("RQ", 1)
ACK = uid("AK", 1)
PLAIN_ACK = uid("AK", 2)


def user(name: str) -> dict:
    return {"kind": "user", "id": name, "version": "1", "build": None}


def envelope(event_id, event_type, payload, actor, stream=STREAM, causation=None, at=T0, refs=None) -> dict:
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": event_id,
        "stream_id": stream,
        "event_type": event_type,
        "schema_version": 1,
        "event_time": ts(at),
        "clock_source": "local",
        "causation_id": causation,
        "correlation_id": None,
        "actor": dict(actor),
        "config_refs": dict(refs or {}),
        "payload": payload,
        "artifact_refs": sorted(digest_strings(payload)),
        "pii_refs": [],
    }


def evidence(assertion: str, authenticated_at: int, method: str = "cli_confirm") -> dict:
    return {"assertion_id": assertion, "authenticated_at": authenticated_at, "method": method}


def requested(event_id=REQUEST, by=REQUESTER, agent=AGENT, event=TRIGGER, at=T0, step_up="fresh", independent=True):
    """A control-stream request; `step_up` is `"fresh"` for evidence ten seconds old."""
    if step_up == "fresh":
        step_up = evidence(f"assert_{event_id[4:11].lower()}", at - 10)
    payload = {
        "agent": agent,
        "event": event,
        "requested_by": by,
        "independent_required": independent,
        "submitted_at": at,
        "risk_clock": ts(at),
        "step_up": step_up,
    }
    return envelope(event_id, "OwnerAcknowledgmentRequested", payload, user(by), at=at)


def acknowledged(
    event_id=ACK,
    by=ACKNOWLEDGER,
    requested_by=REQUESTER,
    request=REQUEST,
    independent=True,
    agent=AGENT,
    event=TRIGGER,
    at=T0 + 60,
    step_up="fresh",
):
    """A control-stream acknowledgment; independent by default, naming the base request."""
    if step_up == "fresh":
        step_up = evidence(f"assert_{event_id[4:11].lower()}", at - 10)
    payload = {
        "agent": agent,
        "event": event,
        "user": by,
        "requested_by": requested_by,
        "independent_required": independent,
        "request": request,
        "submitted_at": at,
        "risk_clock": ts(at),
        "step_up": step_up,
    }
    return envelope(event_id, "OwnerAcknowledged", payload, user(by), at=at)


def plain(event_id=PLAIN_ACK, by=REQUESTER, **more):
    """An acknowledgment where no independent user is required: its own user requested it."""
    return acknowledged(event_id, by, by, None, False, **more)


def copied(original: dict, event_id: str) -> dict:
    """The executor's account-stream copy: the original's payload unchanged, caused by it."""
    at = original["payload"]["submitted_at"]
    p = copy.deepcopy(original["payload"])
    return envelope(event_id, original["event_type"], p, EXECUTOR, ACCOUNT, original["event_id"], at)


def triggered(event_id=TRIGGER, limit=LIMIT, agent=AGENT, reason=None, at=T0 - 600) -> dict:
    """§9.27's `RiskLimitTriggered` of a non-tripwire limit, written by the executor."""
    payload = {
        "agent_id": agent,
        **SNAPSHOT,
        "limit": limit,
        "action": "exits_only",
        "reason": reason,
        "breach_time_s": 0,
        "metric": None,
        "threshold": None,
        "value": None,
        "risk_clock": ts(at),
    }
    refs = {"mandate_version": MANDATE_VERSION}
    return envelope(event_id, "RiskLimitTriggered", payload, EXECUTOR, ACCOUNT, at=at, refs=refs)


def lifted(event_id, limit=LIMIT, agent=AGENT, reason=None, causation=None, at=T0 - 300) -> dict:
    """§9.27's `RiskLimitLifted`: by default the plain end of the condition, as at a rollover."""
    payload = {"agent_id": agent, **SNAPSHOT, "limit": limit, "action": None, "reason": reason, "risk_clock": ts(at)}
    refs = {"mandate_version": MANDATE_VERSION}
    return envelope(event_id, "RiskLimitLifted", payload, EXECUTOR, ACCOUNT, causation, at, refs)


def issued(event_id, assertion, at, command="resume") -> dict:
    """An owner command on the control stream whose assertion a later step-up may not reuse."""
    payload = {
        "agent": AGENT,
        "command": command,
        "scope": "agent",
        "subject": AGENT,
        "release": None,
        "warning_shown": None,
        "bid": None,
        "bid_size": None,
        "floor": None,
        "user": REQUESTER,
        "submitted_at": at,
        "step_up": evidence(assertion, at - 5),
    }
    return envelope(event_id, "OwnerCommandIssued", payload, user(REQUESTER), at=at)


def answered(event_id, assertion, at) -> dict:
    """§9.7's approval answer, whose assertion is spent like any owner command's."""
    payload = {
        "agent": AGENT,
        "approval": uid("AP", 1),
        "verdict": "approved",
        "content_hash": "sha256:" + "5" * 64,
        "submitted_at": at,
        "step_up": evidence(assertion, at - 5),
        "responder": REQUESTER,
        "role": "approver",
    }
    return envelope(event_id, "ApprovalResponseSubmitted", payload, user(REQUESTER), at=at)


def refused(event_id, reason, causation, at=T0 + 61) -> dict:
    payload = {"command": "acknowledge", "reason": reason, "effective_at": ts(at)}
    return envelope(event_id, "OwnerCommandRefused", payload, EXECUTOR, ACCOUNT, causation, at)


DRAFTS = {
    "requested": requested(),
    "acknowledged": acknowledged(),
    "acknowledged_plain": plain(),
    "requested_copy": copied(requested(), uid("CP", 1)),
    "acknowledged_copy": copied(acknowledged(), uid("CP", 2)),
    "refused_trigger_unknown": refused(uid("RF", 1), "trigger_unknown", ACK),
    "refused_trigger_superseded": refused(uid("RF", 2), "trigger_superseded", ACK),
    "refused_request_invalid": refused(uid("RF", 3), "request_invalid", ACK),
}
ORIGINALS = {"requested_copy": "requested", "acknowledged_copy": "acknowledged"}


# --------------------------------------------------------------------------- per-draft vectors


def invalid(name, clause, base, changes, reason, path):
    expect = {"outcome": "Invalid", "reason": reason, "path": path}
    return {"name": f"{base}.{name}", "clause": clause, "base_draft": base, "changes": changes, "expect": expect}


def valid(name, clause, base, changes):
    return {"name": f"{base}.{name}", "clause": clause, "base_draft": base, "changes": changes, "expect": {"outcome": "Valid"}}


def draft_for(section: dict, case: dict) -> dict:
    draft = copy.deepcopy(section["drafts"][case["base_draft"]])
    for item in case["changes"]:
        apply_change(draft, item)
    return draft


# Each member: a value of the wrong JSON kind (`schema`) and, where its type constrains a string, a
# string of the wrong form (`non_canonical`).
MEMBER_CASES = {
    "requested": (
        ("agent", 7, "agent:a"),
        ("event", 7, "not-a-ulid"),
        ("requested_by", 7, "user owner"),
        ("independent_required", "true", None),
        ("submitted_at", "1792339200", None),
        ("risk_clock", T0, "2026-10-17T00:00:00.5Z"),
        ("step_up", "cli_confirm", None),
    ),
    "acknowledged": (
        ("agent", 7, "agent:a"),
        ("event", 7, "not-a-ulid"),
        ("user", 7, "user owner"),
        ("requested_by", 7, "user owner"),
        ("independent_required", 1, None),
        ("request", 7, "01JBRQ"),
        ("submitted_at", "1792339260", None),
        ("risk_clock", T0 + 60, "2026-10-17T00:01:00.000000001Z"),
        ("step_up", ["cli_confirm"], None),
    ),
}
NULLED = {
    "requested": ("agent", "event", "requested_by", "independent_required", "submitted_at", "risk_clock"),
    "acknowledged": ("agent", "event", "user", "requested_by", "independent_required", "submitted_at", "risk_clock"),
}


def member_drafts() -> list[dict]:
    out = []
    for base, cases in MEMBER_CASES.items():
        for member, wrong_kind, wrong_form in cases:
            path = f"payload.{member}"
            out.append(invalid(f"{member}.kind", "§9.28 types", base, [change(path, wrong_kind)], "schema", path))
            if wrong_form is not None:
                out.append(invalid(f"{member}.form", "§9.28 types", base, [change(path, wrong_form)], "non_canonical", path))
        for member in NULLED[base]:
            path = f"payload.{member}"
            out.append(invalid(f"{member}.null", "§9.28: not nullable", base, [change(path, None)], "schema", path))
        for member, _, _ in cases:
            path = f"payload.{member}"
            out.append(invalid(f"{member}.missing", "§9.2: every member is present", base, [delete(path)], "schema", path))
        out.append(invalid("extra", "§9.28: closed", base, [change("payload.note", "x")], "schema", "payload.note"))
        step_up = "payload.step_up"
        out += [
            invalid("step_up.authenticated_at_as_a_timestamp", "§9.7's step-up: whole seconds, never a timestamp", base,
                    [change(f"{step_up}.authenticated_at", ts(T0))], "schema", f"{step_up}.authenticated_at"),
            invalid("step_up.assertion_id.kind", "§9.7's step-up types", base, [change(f"{step_up}.assertion_id", 7)], "schema", f"{step_up}.assertion_id"),
            invalid("step_up.method.empty", "§9.7's step-up: text is non-empty", base, [change(f"{step_up}.method", "")], "non_canonical", f"{step_up}.method"),
            invalid("step_up.extra", "§9.7's step-up is closed", base, [change(f"{step_up}.subject", REQUESTER)], "schema", f"{step_up}.subject"),
            invalid("step_up.missing_member", "§9.7's step-up: every member", base, [delete(f"{step_up}.method")], "schema", f"{step_up}.method"),
            invalid("at_version_3", "§9.28: schema versions 1 and 2 only (v0.100)", base, [change("schema_version", 3)], "unknown_schema", "payload"),
            invalid("on_an_agent_stream", "§9 rows: control and account streams only", base, [change("stream_id", AGENT_STREAM)], "wrong_stream", "event_type"),
            invalid("by_a_client", "rule 83: a client never acknowledges (identity ID-11)", base, [change("actor", dict(CLIENT))], "schema", "actor.kind"),
        ]
    return out


def rule_drafts() -> list[dict]:
    """Rule 239, clause by clause, reported once at the first offending member; rule 28's new reasons
    off the account stream."""
    req, ack, own = "requested", "acknowledged", "acknowledged_plain"
    out = [
        invalid("not_independent", "rule 239: a request exists only where an independent user must acknowledge", req,
                [change("payload.independent_required", False)], "schema", "payload.independent_required"),
        invalid("step_up_null", "rule 239: a request carries step-up evidence", req,
                [change("payload.step_up", None)], "schema", "payload.step_up"),
        invalid("not_independent_and_no_step_up", "rule 239: `independent_required` is reported first", req,
                [change("payload.independent_required", False), change("payload.step_up", None)], "schema", "payload.independent_required"),
        invalid("independent_without_request", "rule 239: an independent acknowledgment names its request", ack,
                [change("payload.request", None)], "schema", "payload.request"),
        invalid("independent_by_its_requester", "rule 239: a user never acknowledges their own request", ack,
                [change("payload.user", REQUESTER), change("actor.id", REQUESTER)], "schema", "payload.requested_by"),
        invalid("independent_without_request_by_its_requester", "rule 239: `request`, then `requested_by`", ack,
                [change("payload.request", None), change("payload.user", REQUESTER)], "schema", "payload.request"),
        invalid("plain_with_a_request", "rule 239: an acknowledgment needing no independent user names no request", own,
                [change("payload.request", REQUEST)], "schema", "payload.request"),
        invalid("plain_requested_by_another", "rule 239: with no independent user, the requester is the acknowledging user", own,
                [change("payload.requested_by", ACKNOWLEDGER)], "schema", "payload.requested_by"),
        invalid("plain_with_a_request_and_another_requester", "rule 239: `request`, then `requested_by`", own,
                [change("payload.request", REQUEST), change("payload.requested_by", ACKNOWLEDGER)], "schema", "payload.request"),
        invalid("copy_independent_without_request", "rule 239 judges the account-stream copy too", "acknowledged_copy",
                [change("payload.request", None)], "schema", "payload.request"),
        invalid("copy_request_not_independent", "rule 239 judges the account-stream copy too", "requested_copy",
                [change("payload.independent_required", False)], "schema", "payload.independent_required"),
    ]
    for reason in ("trigger_unknown", "trigger_superseded", "request_invalid"):
        base = f"refused_{reason}"
        stop = [change("stream_id", AGENT_STREAM), change("actor", dict(RUNTIME)), change("payload.command", "stop")]
        out.append(invalid("for_a_stop_on_the_agent_stream", "rule 28: only an acknowledgment, on the account stream",
                           base, stop, "stream_mismatch", "payload.reason"))
        lift = [*stop[:2], change("payload.command", "lift_hold"), change("schema_version", 2)]
        out.append(invalid("for_a_lift_on_the_agent_stream", "rule 28: a lift is refused only for its step-up",
                           base, lift, "stream_mismatch", "payload.reason"))
    out.append(invalid("not_independent_for_a_stop", "rule 28 keeps `not_independent` too", "refused_request_invalid",
                       [change("stream_id", AGENT_STREAM), change("actor", dict(RUNTIME)), change("payload.command", "stop"),
                        change("payload.reason", "not_independent")], "stream_mismatch", "payload.reason"))
    return out


def invalid_drafts() -> list[dict]:
    return [*member_drafts(), *rule_drafts()]


def valid_drafts() -> list[dict]:
    """The cases a rule might be misread to refuse; each must be accepted."""
    req, ack, own = "requested", "acknowledged", "acknowledged_plain"
    return [
        valid("unknown_request", "rule 240 is the stream's: `append` sees one record", ack, [change("payload.request", uid("RQ", 99))]),
        valid("requester_differs_from_actor", "§9.28: the writer sets the users; `append` compares no actor", ack, [change("actor.id", THIRD)]),
        valid("no_step_up", "§9.28: an acknowledgment's step-up is nullable; the executor refuses it as `step_up_missing`", ack, [change("payload.step_up", None)]),
        valid("plain_no_step_up", "§9.28: as for an independent acknowledgment", own, [change("payload.step_up", None)]),
        valid("stale_step_up", "§6.1's freshness is the executor's, at processing", req, [change("payload.step_up.authenticated_at", T0 - 3600)]),
        valid("other_method", "§6.1's methods are the executor's", ack, [change("payload.step_up.method", "passkey")]),
        valid("live", "§9.28: either environment", req, [change("environment", "live")]),
        valid("longest_ids", "§9.1: ids of 64 characters", ack, [change("payload.user", "u" * 64), change("payload.requested_by", "r" * 64)]),
        valid("submitted_at_zero", "§9.1 integer: 0 is an integer", own, [change("payload.submitted_at", 0)]),
        valid("copy_of_a_plain_acknowledgment", "§9.28: a copy is the original's payload", "acknowledged_copy",
              [change("payload.request", None), change("payload.independent_required", False), change("payload.user", REQUESTER)]),
        valid("copy_without_cause", "§9.28 names the original as `causation_id` but numbers no copy rule", "acknowledged_copy", [change("causation_id", None)]),
        valid("not_independent", "rule 28: an acknowledgment's own reason on the account stream", "refused_request_invalid", [change("payload.reason", "not_independent")]),
        valid("step_up_reused", "rule 28: the step-up reasons apply on either stream", "refused_request_invalid", [change("payload.reason", "step_up_reused")]),
    ]


# --------------------------------------------------------------------------- rules 240 and 241


def rule_number(name: str):
    parts = name.split(".")
    return int(parts[1]) if parts[0] == "rule" and parts[1].isdigit() else name


def stream_failure(records: list[dict], skip: frozenset[str] = frozenset(), rules_only: bool = False) -> dict | None:
    """Each control-stream record in order: its `append` (unless `rules_only`), then rules 240 and
    241. The first failure, or `None`."""
    requests: dict[str, dict] = {}
    if "stream.240.later" in skip:
        requests = {r["event_id"]: r["payload"] for r in records if r["event_type"] == "OwnerAcknowledgmentRequested"}
    named: set[str] = set()
    for i, d in enumerate(records):
        if not rules_only:
            got = violations(d, skip)
            if got:
                return {"index": i, "rule": rule_number(got[0].rule), "path": got[0].path}
        p = d["payload"]
        if d["event_type"] == "OwnerAcknowledgmentRequested":
            requests.setdefault(d["event_id"], p)
            continue
        if d["event_type"] != "OwnerAcknowledged":
            continue
        name = p["request"]
        if name is None and "stream.241.nulls" not in skip:
            continue
        if name is not None and "stream.240" not in skip:
            r = requests.get(name)
            if r is None and "stream.240.exists" not in skip:
                return {"index": i, "rule": 240, "path": "payload.request"}
            compared = [m for m in MATCHED if f"stream.240.{m}" not in skip]
            if r is not None and any(r[m] != p[m] for m in compared):
                return {"index": i, "rule": 240, "path": "payload.request"}
        if name in named and "stream.241" not in skip:
            return {"index": i, "rule": 241, "path": "payload.request"}
        named.add(name)
    return None


def stream_cases() -> list[dict]:
    """Control streams, each record its own `append`. `expect` is the first failure of `append` then
    the stream rules; `stream_rules` the first failure of rules 240 and 241 alone. Both typed."""
    ok = {"outcome": "Valid"}

    def at(index, rule=240, path="payload.request"):
        return {"outcome": "Invalid", "index": index, "rule": rule, "path": path}

    def case(name, clause, records, expect, rules=None):
        return {"name": name, "clause": clause, "records": records, "expect": expect, "stream_rules": rules or expect}

    second, third = uid("AK", 3), uid("AK", 4)
    other_request = uid("RQ", 2)
    return [
        case("request_then_acknowledgment", "rule 240: the request comes first and matches", [requested(), acknowledged()], ok),
        case("plain_acknowledgment", "rules 240 and 241 judge only an acknowledgment naming a request", [plain()], ok),
        case("two_plain_acknowledgments", "rule 241: only a non-null request is spent", [plain(), plain(second, at=T0 + 90)], ok),
        case("interleaved", "rule 240: each acknowledgment its own request",
             [requested(), requested(other_request, at=T0 + 5), acknowledged(second, request=other_request), acknowledged()], ok),
        case("retry_with_a_new_request", "rule 241: a retry needs a new request",
             [requested(), acknowledged(), requested(other_request, at=T0 + 120), acknowledged(second, request=other_request, at=T0 + 180)], ok),
        case("reordered", "rule 240: a reordered stream breaks it", [acknowledged(), requested()], at(0)),
        case("unknown_request", "rule 240: the request exists", [requested(), acknowledged(request=other_request)], at(1)),
        case("agent_differs", "rule 240: `agent` matches", [requested(agent=OTHER_AGENT), acknowledged()], at(1)),
        case("event_differs", "rule 240: `event` matches", [requested(event=uid("TG", 2)), acknowledged()], at(1)),
        case("requested_by_differs", "rule 240: `requested_by` matches", [requested(), acknowledged(requested_by=THIRD)], at(1)),
        case("requirement_differs", "rule 240: `independent_required` matches; rule 239 refuses the request first",
             [requested(independent=False), acknowledged()], at(0, 239, "payload.independent_required"), at(1)),
        case("second_acknowledgment_by_a_third_user", "rule 241: a request is spent by its first acknowledgment",
             [requested(), acknowledged(), acknowledged(second, by=THIRD, at=T0 + 90)], at(2, 241)),
        case("second_acknowledgment_by_the_same_user", "rule 241", [requested(), acknowledged(), acknowledged(second, at=T0 + 90)], at(2, 241)),
        case("self_acknowledgment", "rule 239: a user never acknowledges their own request",
             [requested(), acknowledged(by=REQUESTER)], at(1, 239, "payload.requested_by"), ok),
        case("self_acknowledgment_naming_another_requester", "rule 240: the requester is the request's",
             [requested(), acknowledged(by=REQUESTER, requested_by=ACKNOWLEDGER)], at(1)),
        case("acknowledgment_naming_an_acknowledgment", "rule 240: the named record is a request",
             [requested(), acknowledged(), acknowledged(third, by=THIRD, request=ACK, at=T0 + 90)], at(2)),
    ]


# --------------------------------------------------------------------------- the executor's judgment


def step_up_failure(step_up, at: int, prior: set[str], skip: frozenset[str], who: str, methods: tuple) -> str | None:
    """Mandate spec §6.1 at the second `at`, with `prior` the assertions spent before it and
    `methods` the methods the environment allows."""
    if step_up is None:
        return None if f"executor.3.missing.{who}" in skip else "step_up_missing"
    gap = at - step_up["authenticated_at"]
    if "executor.3.future" in skip:
        gap = abs(gap)
    window = WINDOW + (1 if "executor.3.window" in skip else 0)
    if not 0 <= gap <= window:
        return "step_up_stale"
    if step_up["assertion_id"] in prior and "executor.reuse" not in skip:
        return "step_up_reused"
    allowed = METHODS if "executor.3.live" in skip else methods
    if step_up["method"] not in allowed and "executor.3.method" not in skip:
        return "step_up_method"
    return None


def spent_before(control: list[dict], skip: frozenset[str]) -> dict[str, set[str]]:
    """For each control-stream record, the assertions on the records of §6.1's four types before it."""
    excluded = {
        "executor.reuse.requests": "OwnerAcknowledgmentRequested",
        "executor.reuse.commands": "OwnerCommandIssued",
        "executor.reuse.answers": "ApprovalResponseSubmitted",
        "executor.reuse.acknowledgments": "OwnerAcknowledged",
    }
    counted = set(REUSE_TYPES) - {t for bug, t in excluded.items() if bug in skip}
    seen: set[str] = set()
    out: dict[str, set[str]] = {}
    every = {
        r["payload"]["step_up"]["assertion_id"]
        for r in control
        if r["event_type"] in counted and r["payload"].get("step_up") is not None
    }
    for r in control:
        out[r["event_id"]] = set(every) if "executor.reuse.later" in skip else set(seen)
        step_up = r["payload"].get("step_up")
        if r["event_type"] in counted and step_up is not None:
            seen.add(step_up["assertion_id"])
    return out


def methods_for(environment: str, founder_connection: bool) -> tuple:
    """Mandate spec §6.1's methods: `cli_confirm` in `paper`, and in `live` only on the founder's
    own connection (DEC-529 item 3), which a case states as an input as it states the overlay."""
    return METHODS if environment == "paper" or founder_connection else ()


def judge(
    control: list[dict],
    account: list[dict],
    overlay: bool,
    skip: frozenset[str] = frozenset(),
    methods: tuple = METHODS,
) -> list[dict]:
    """The executor's fold of its account stream, forward: each acknowledgment copy judged by §9.28's
    checks 1 to 4 in order, its own lift folded back in. One outcome per acknowledgment copy."""
    spent = spent_before(control, skip)
    triggers: dict[str, dict] = {}
    standing: dict[str, bool] = {}
    requests: dict[str, dict] = {}
    named: set[str] = set()
    out = []
    records = list(enumerate(account))
    if "executor.copies_reordered" in skip:
        records.sort(key=lambda item: item[1]["event_type"] == "OwnerAcknowledged")
    if "executor.1.before" in skip:
        for r in account:
            if r["event_type"] == "RiskLimitTriggered":
                triggers.setdefault(r["event_id"], r["payload"])
                standing.setdefault(r["event_id"], True)

    def supersede(agent: str, limit: str, keep: str | None = None) -> None:
        for tid, t in triggers.items():
            same = t["limit"] == limit or "executor.2.any_limit" in skip
            if tid != keep and same and t["agent_id"] == agent:
                standing[tid] = False

    for i, r in records:
        kind, p = r["event_type"], r["payload"]
        if "executor.1.type" in skip and kind != "RiskLimitTriggered":
            triggers.setdefault(r["event_id"], {"agent_id": p.get("agent_id", p.get("agent")), "limit": p.get("limit", "")})
            standing.setdefault(r["event_id"], True)
        if kind == "RiskLimitTriggered":
            if "executor.2" not in skip:
                supersede(p["agent_id"], p["limit"], keep=r["event_id"])
            triggers[r["event_id"]] = p
            standing[r["event_id"]] = True
        elif kind == "RiskLimitLifted" and "executor.2" not in skip and "executor.2.lift" not in skip:
            supersede(p["agent_id"], p["limit"])
        elif kind == "OwnerAcknowledgmentRequested":
            requests.setdefault(r["causation_id"], p)
        elif kind == "OwnerAcknowledged":
            outcome = judged(r, triggers, standing, requests, named, spent, overlay, skip, methods)
            if p["request"] is not None:
                named.add(p["request"])
            out.append({"index": i, "outcome": outcome})
            trigger = triggers.get(p["event"])
            if outcome == "lift" and trigger is not None and "executor.2.own_lift" not in skip:
                supersede(trigger["agent_id"], trigger["limit"])
    out.sort(key=lambda o: o["index"])
    return out


def judged(r, triggers, standing, requests, named, spent, overlay, skip, methods) -> str:
    p = r["payload"]
    original = r["causation_id"]
    trigger = triggers.get(p["event"])
    unknown = trigger is None or (trigger["agent_id"] != p["agent"] and "executor.1.agent" not in skip)
    if unknown and "executor.1" not in skip:
        return "trigger_unknown"
    if trigger is not None and not standing.get(p["event"], True) and "executor.2" not in skip:
        return "trigger_superseded"
    if "executor.3" not in skip:
        at = p["submitted_at"] if "executor.3.at_submission" in skip else seconds_of(r["event_time"])
        failed = step_up_failure(p["step_up"], at, spent.get(original, set()), skip, "acknowledgment", methods)
        if failed:
            return failed
    flag = p["independent_required"] and "executor.4.copy_flag" not in skip
    if "executor.4" in skip or not (flag or (overlay and "executor.4.overlay" not in skip)):
        return "lift"
    if p["request"] is None or (p["user"] == p["requested_by"] and "executor.4.self" not in skip):
        return "not_independent"
    request = requests.get(p["request"])
    if request is None and "executor.4.order" in skip:
        request = next((c["payload"] for c in r.get("_later", []) if c["causation_id"] == p["request"]), None)
    if request is None:
        return "request_invalid"
    if any(request[m] != p[m] for m in MATCHED if f"executor.4.match.{m}" not in skip):
        return "request_invalid"
    request_failed = step_up_failure(
        request["step_up"], request["submitted_at"], spent.get(p["request"], set()), skip, "request", methods
    )
    if request_failed and "executor.4.request_step_up" not in skip:
        return "request_invalid"
    if p["request"] in named and "executor.4.spent" not in skip:
        return "request_invalid"
    return "lift"


def judge_streams(case: dict, skip: frozenset[str] = frozenset()) -> list[dict]:
    """`judge` over a case, with each acknowledgment copy shown the records after it only for the
    seeded bug that reads a request copied later."""
    account = case["account"]
    if "executor.4.order" in skip:
        account = [{**r, "_later": account[i + 1 :]} for i, r in enumerate(account)]
    methods = methods_for(case["environment"], case["founder_connection"])
    return judge(case["control"], account, case["overlay_independent"], skip, methods)


# --------------------------------------------------------------------------- the independent oracle


def oracle(
    control: list[dict],
    account: list[dict],
    overlay: bool,
    skip: frozenset[str] = frozenset(),
    environment: str = "paper",
    founder_connection: bool = False,
) -> list[dict]:
    """§9.28's judgment restated: for each acknowledgment copy, scan the account stream's prefix
    backwards for its trigger, anything since that ended it, its request copy and any earlier copy
    naming it, and the control stream's prefix backwards for a spent assertion. Earlier outcomes are
    computed first, since a lift that passed is a `RiskLimitLifted` the next one must see."""
    outcomes: dict[int, str] = {}

    def assertion_spent(original_id: str, assertion: str) -> bool:
        at = next(k for k, c in enumerate(control) if c["event_id"] == original_id)
        for c in reversed(control[:at]):
            evidence_ = c["payload"].get("step_up")
            if c["event_type"] in REUSE_TYPES and evidence_ is not None and evidence_["assertion_id"] == assertion:
                return True
        return False

    def evidence_fails(step_up, at: int, original_id: str):
        if not step_up:
            return "step_up_missing"
        if step_up["authenticated_at"] > at or at - step_up["authenticated_at"] > WINDOW:
            return "step_up_stale"
        if assertion_spent(original_id, step_up["assertion_id"]):
            return "step_up_reused"
        if step_up["method"] != "cli_confirm" or (environment == "live" and not founder_connection):
            return "step_up_method"
        return None

    def same_limit(payload: dict, agent: str, limit: str) -> bool:
        return payload.get("agent_id") == agent and payload.get("limit") == limit

    def outcome(k: int) -> str:
        p = account[k]["payload"]
        prefix = account[:k]
        found_at = None
        for j in range(k - 1, -1, -1):
            if prefix[j]["event_id"] == p["event"]:
                found_at = j
                break
        if found_at is None or prefix[found_at]["event_type"] != "RiskLimitTriggered":
            return "trigger_unknown"
        trigger = prefix[found_at]["payload"]
        if trigger["agent_id"] != p["agent"]:
            return "trigger_unknown"
        agent, limit = trigger["agent_id"], trigger["limit"]
        for j in range(k - 1, found_at, -1):
            between = prefix[j]
            if between["event_type"] in ("RiskLimitTriggered", "RiskLimitLifted") and same_limit(between["payload"], agent, limit):
                return "trigger_superseded"
            if between["event_type"] == "OwnerAcknowledged" and "oracle.own_lift" not in skip:
                earlier = triggers_of(j)
                if earlier is not None and same_limit(earlier, agent, limit) and outcomes[j] == "lift":
                    return "trigger_superseded"
        processed = seconds_of(account[k]["event_time"])
        failed = evidence_fails(p["step_up"], processed, account[k]["causation_id"])
        if failed:
            return failed
        if not (p["independent_required"] or overlay):
            return "lift"
        if p["request"] is None or p["user"] == p["requested_by"]:
            return "not_independent"
        request = None
        for j in range(k - 1, -1, -1):
            if prefix[j]["event_type"] == "OwnerAcknowledgmentRequested" and prefix[j]["causation_id"] == p["request"]:
                request = prefix[j]["payload"]
        if request is None:
            return "request_invalid"
        for member in ("agent", "event", "requested_by", "independent_required"):
            if request[member] != p[member]:
                return "request_invalid"
        if evidence_fails(request["step_up"], request["submitted_at"], p["request"]):
            return "request_invalid"
        named_before = any(c["event_type"] == "OwnerAcknowledged" and c["payload"]["request"] == p["request"] for c in reversed(prefix))
        if named_before and "oracle.spent" not in skip:
            return "request_invalid"
        return "lift"

    def triggers_of(j: int) -> dict | None:
        name = account[j]["payload"]["event"]
        return next((r["payload"] for r in reversed(account[:j]) if r["event_id"] == name and r["event_type"] == "RiskLimitTriggered"), None)

    for k, record in enumerate(account):
        if record["event_type"] == "OwnerAcknowledged":
            outcomes[k] = outcome(k)
    return [{"index": k, "outcome": o} for k, o in sorted(outcomes.items())]


ORACLE_MUTANTS = ("oracle.own_lift", "oracle.spent")


# --------------------------------------------------------------------------- executor cases


def executor_cases() -> list[dict]:
    """Account streams with the control stream their copies came from, the overlay's independence
    requirement at processing, and each acknowledgment copy's outcome, typed from §9.28's checks."""
    counter = iter(range(1, 10_000))

    def cp(original: dict, late: int = 0) -> dict:
        record = copied(original, uid("CP", next(counter)))
        record["event_time"] = ts(original["payload"]["submitted_at"] + late)
        return record

    def case(
        name, clause, control, account, outcomes, overlay=False, refused_at_append=(), environment="paper", founder=False
    ):
        control, account = copy.deepcopy(control), copy.deepcopy(account)
        for record in (*control, *account):
            record["environment"] = environment
        acks = [i for i, r in enumerate(account) if r["event_type"] == "OwnerAcknowledged"]
        assert len(acks) == len(outcomes), name
        expect = [{"index": i, "outcome": o} for i, o in zip(acks, outcomes, strict=True)]
        return {
            "name": name,
            "clause": clause,
            "overlay_independent": overlay,
            "environment": environment,
            "founder_connection": founder,
            "control": control,
            "account": account,
            "refused_at_append": [{"stream": s, "index": i, "rule": 239, "path": path} for s, i, path in refused_at_append],
            "expect": expect,
        }

    trig = triggered()
    req, ack, own = requested(), acknowledged(), plain()
    second, third = uid("AK", 3), uid("AK", 4)
    unknown = uid("TG", 90)
    other_trigger = uid("TG", 2)
    other_request = uid("RQ", 2)
    stale = evidence("assert_stale", T0 + 60 - 301)
    cases = [
        case("independent_lifts", "§9.28: all four checks pass", [req, ack], [trig, cp(req), cp(ack)], ["lift"]),
        case("plain_lifts", "check 4 does not apply when neither the copy nor the overlay requires independence", [own], [trig, cp(own)], ["lift"]),
        case("plain_under_overlay", "check 4: the stricter of the copy's requirement and the overlay's at processing",
             [own], [trig, cp(own)], ["not_independent"], overlay=True),
        case("independent_without_overlay", "check 4: the copy's own requirement holds when the overlay no longer asks",
             [req, ack], [trig, cp(req), cp(ack)], ["lift"], overlay=False),
        case("unknown_ulid", "check 1: an unknown ULID", [plain(event=unknown)], [trig, cp(plain(event=unknown))], ["trigger_unknown"]),
        case("another_agents_trigger", "check 1: another agent's trigger",
             [own], [triggered(agent=OTHER_AGENT), cp(own)], ["trigger_unknown"]),
        case("another_event_type", "check 1: the event named is not a `RiskLimitTriggered`",
             [plain(event=uid("XF", 1))], [trig, lifted(uid("XF", 1), OTHER_LIMIT), cp(plain(event=uid("XF", 1)))], ["trigger_unknown"]),
        case("trigger_after_the_copy", "check 1: the trigger comes before the acknowledgment copy", [own], [cp(own), trig], ["trigger_unknown"]),
        case("superseded_by_a_later_trigger", "check 2: a later `RiskLimitTriggered` of the same limit",
             [own], [trig, triggered(other_trigger, reason="new_day_breach", at=T0 - 60), cp(own)], ["trigger_superseded"]),
        case("the_later_trigger_stands", "check 2: the latest latch is acknowledged by naming it",
             [plain(event=other_trigger)], [trig, triggered(other_trigger, reason="new_day_breach", at=T0 - 60), cp(plain(event=other_trigger))], ["lift"]),
        case("superseded_by_a_lift", "check 2: a `RiskLimitLifted` of that limit",
             [own], [trig, lifted(uid("XF", 2)), cp(own)], ["trigger_superseded"]),
        case("later_trigger_of_another_limit", "check 2: only the same limit supersedes",
             [own], [trig, triggered(other_trigger, OTHER_LIMIT, at=T0 - 60), cp(own)], ["lift"]),
        case("lift_of_another_limit", "check 2: only a lift of that limit supersedes",
             [own], [trig, lifted(uid("XF", 3), OTHER_LIMIT), cp(own)], ["lift"]),
        case("another_agents_later_trigger", "check 2: the limit is this agent's risk state's",
             [own], [trig, triggered(other_trigger, agent=OTHER_AGENT, at=T0 - 60), cp(own)], ["lift"]),
        case("second_acknowledgment_after_a_lift", "check 2: the executor's own lift supersedes the trigger",
             [own, plain(second, at=T0 + 90)], [trig, cp(own), cp(plain(second, at=T0 + 90))], ["lift", "trigger_superseded"]),
        case("refused_acknowledgment_lifts_nothing", "§9.28: a refusal lifts nothing, so the trigger still stands",
             [plain(step_up=None), plain(second, at=T0 + 90)], [trig, cp(plain(step_up=None)), cp(plain(second, at=T0 + 90))],
             ["step_up_missing", "lift"]),
        case("request_copy_after_the_acknowledgment_copy", "check 4: the request copy comes first",
             [req, ack], [trig, cp(ack), cp(req)], ["request_invalid"]),
        case("request_never_copied", "check 4: a request copy names the request", [req, ack], [trig, cp(ack)], ["request_invalid"]),
        case("request_spent_by_a_refused_acknowledgment", "check 4: no earlier acknowledgment copy, refused or not, names it",
             [req, acknowledged(step_up=stale), acknowledged(second, by=THIRD, at=T0 + 90)],
             [trig, cp(req), cp(acknowledged(step_up=stale)), cp(acknowledged(second, by=THIRD, at=T0 + 90))],
             ["step_up_stale", "request_invalid"]),
        case("second_acknowledgment_of_a_lifted_request", "checks 2 then 4: the lift supersedes the trigger first",
             [req, ack, acknowledged(second, by=THIRD, at=T0 + 90)],
             [trig, cp(req), cp(ack), cp(acknowledged(second, by=THIRD, at=T0 + 90))], ["lift", "trigger_superseded"]),
        case("acknowledgment_processed_late", "check 3: an acknowledgment's step-up is judged when the executor processes its copy",
             [own], [trig, cp(own, late=291)], ["step_up_stale"]),
        case("acknowledgment_processed_at_the_window", "check 3: 300 s old at processing is valid",
             [own], [trig, cp(own, late=290)], ["lift"]),
        case("live_without_the_founder_connection", "check 3: `cli_confirm` is not a live method off the founder's connection",
             [own], [trig, cp(own)], ["step_up_method"], environment="live"),
        case("live_on_the_founder_connection", "check 3: `cli_confirm` on the founder's own live connection (DEC-529 item 3)",
             [own], [trig, cp(own)], ["lift"], environment="live", founder=True),
        case("request_step_up_stale", "check 4: the request's step-up, judged at its own `submitted_at`",
             [requested(step_up=evidence("assert_req_old", T0 - 301)), ack],
             [trig, cp(requested(step_up=evidence("assert_req_old", T0 - 301))), cp(ack)], ["request_invalid"]),
        case("request_step_up_at_the_window", "check 4: 300 s old at the request is valid",
             [requested(step_up=evidence("assert_req_edge", T0 - 300)), ack],
             [trig, cp(requested(step_up=evidence("assert_req_edge", T0 - 300))), cp(ack)], ["lift"]),
        case("request_step_up_after_the_request", "check 4: evidence authenticated after the request is stale",
             [requested(step_up=evidence("assert_req_next", T0 + 1)), ack],
             [trig, cp(requested(step_up=evidence("assert_req_next", T0 + 1))), cp(ack)], ["request_invalid"]),
        case("request_step_up_method", "check 4: the request's method",
             [requested(step_up=evidence("assert_req_pw", T0 - 10, "password")), ack],
             [trig, cp(requested(step_up=evidence("assert_req_pw", T0 - 10, "password"))), cp(ack)], ["request_invalid"]),
        case("request_without_step_up", "check 4: rule 239 refuses it at append; the executor fails closed all the same",
             [requested(step_up=None), ack], [trig, cp(requested(step_up=None)), cp(ack)], ["request_invalid"],
             refused_at_append=(("control", 0, "payload.step_up"), ("account", 1, "payload.step_up"))),
        case("request_reuses_a_commands_assertion", "check 4 and mandate spec §6.1: a request's assertion appears on no earlier command",
             [issued(uid("CM", 1), "assert_shared", T0 - 30), requested(step_up=evidence("assert_shared", T0 - 10)), ack],
             [trig, cp(requested(step_up=evidence("assert_shared", T0 - 10))), cp(ack)], ["request_invalid"]),
        case("acknowledgment_reuses_the_requests_assertion", "check 3: a request's assertion is spent",
             [req, acknowledged(step_up=evidence(req["payload"]["step_up"]["assertion_id"], T0 + 50))],
             [trig, cp(req), cp(acknowledged(step_up=evidence(req["payload"]["step_up"]["assertion_id"], T0 + 50)))], ["step_up_reused"]),
        case("acknowledgment_reuses_an_answers_assertion", "check 3: an approval answer's assertion is spent",
             [answered(uid("AN", 1), "assert_answer", T0), plain(step_up=evidence("assert_answer", T0 + 50))],
             [trig, cp(plain(step_up=evidence("assert_answer", T0 + 50)))], ["step_up_reused"]),
        case("acknowledgment_reuses_an_acknowledgments_assertion", "check 3: an earlier acknowledgment's assertion is spent, whatever its outcome",
             [plain(event=unknown, step_up=evidence("assert_twice", T0 + 50)), plain(second, at=T0 + 90, step_up=evidence("assert_twice", T0 + 85))],
             [trig, cp(plain(event=unknown, step_up=evidence("assert_twice", T0 + 50))),
              cp(plain(second, at=T0 + 90, step_up=evidence("assert_twice", T0 + 85)))], ["trigger_unknown", "step_up_reused"]),
        case("a_later_command_reuses_the_assertion", "check 3: only an earlier record spends an assertion",
             [plain(step_up=evidence("assert_first", T0 + 50)), issued(uid("CM", 2), "assert_first", T0 + 70)],
             [trig, cp(plain(step_up=evidence("assert_first", T0 + 50)))], ["lift"]),
        case("acknowledgment_step_up_missing", "check 3", [plain(step_up=None)], [trig, cp(plain(step_up=None))], ["step_up_missing"]),
        case("acknowledgment_step_up_stale", "check 3: more than 300 s",
             [plain(step_up=evidence("assert_old", T0 + 60 - 301))], [trig, cp(plain(step_up=evidence("assert_old", T0 + 60 - 301)))], ["step_up_stale"]),
        case("acknowledgment_step_up_at_the_window", "check 3: 300 s is valid",
             [plain(step_up=evidence("assert_edge", T0 + 60 - 300))], [trig, cp(plain(step_up=evidence("assert_edge", T0 + 60 - 300)))], ["lift"]),
        case("acknowledgment_step_up_after_it", "check 3: authenticated after the command is stale",
             [plain(step_up=evidence("assert_next", T0 + 61))], [trig, cp(plain(step_up=evidence("assert_next", T0 + 61)))], ["step_up_stale"]),
        case("acknowledgment_step_up_method", "check 3: the method is allowed for the environment",
             [plain(step_up=evidence("assert_pw", T0 + 50, "password"))], [trig, cp(plain(step_up=evidence("assert_pw", T0 + 50, "password")))],
             ["step_up_method"]),
        case("request_agent_differs", "check 4: the request copy's `agent`",
             [requested(agent=OTHER_AGENT), ack], [trig, cp(requested(agent=OTHER_AGENT)), cp(ack)], ["request_invalid"]),
        case("request_event_differs", "check 4: the request copy's `event`",
             [requested(event=other_trigger), ack], [trig, cp(requested(event=other_trigger)), cp(ack)], ["request_invalid"]),
        case("request_requested_by_differs", "check 4: the request copy's `requested_by`",
             [requested(by=THIRD), ack], [trig, cp(requested(by=THIRD)), cp(ack)], ["request_invalid"]),
        case("request_requirement_differs", "check 4: the request copy's `independent_required`; rule 239 refuses it at append",
             [requested(independent=False), ack], [trig, cp(requested(independent=False)), cp(ack)], ["request_invalid"],
             refused_at_append=(("control", 0, "payload.independent_required"), ("account", 1, "payload.independent_required"))),
        case("another_requests_copy", "check 4: the request copy whose `causation_id` is the request",
             [req, requested(other_request, at=T0 + 5), ack], [trig, cp(requested(other_request, at=T0 + 5)), cp(ack)], ["request_invalid"]),
        case("self_acknowledgment", "check 4: `user` differs from `requested_by`; rule 239 refuses it at append",
             [req, acknowledged(by=REQUESTER)], [trig, cp(req), cp(acknowledged(by=REQUESTER))], ["not_independent"],
             refused_at_append=(("control", 1, "payload.requested_by"), ("account", 2, "payload.requested_by"))),
        case("unknown_trigger_before_stale_step_up", "the order: check 1 before check 3",
             [plain(event=unknown, step_up=stale)], [trig, cp(plain(event=unknown, step_up=stale))], ["trigger_unknown"]),
        case("superseded_before_stale_step_up", "the order: check 2 before check 3",
             [plain(step_up=stale)], [trig, lifted(uid("XF", 4)), cp(plain(step_up=stale))], ["trigger_superseded"]),
        case("stale_step_up_before_independence", "the order: check 3 before check 4",
             [plain(step_up=stale)], [trig, cp(plain(step_up=stale))], ["step_up_stale"], overlay=True),
        case("independence_before_the_request", "the order: `not_independent` before `request_invalid`",
             [acknowledged(by=REQUESTER, request=other_request)], [trig, cp(acknowledged(by=REQUESTER, request=other_request))], ["not_independent"],
             refused_at_append=(("control", 0, "payload.requested_by"), ("account", 1, "payload.requested_by"))),
        case("two_agents_two_lifts", "§9.28: each acknowledgment its own agent's trigger",
             [own, plain(second, agent=OTHER_AGENT, event=other_trigger, at=T0 + 90)],
             [trig, triggered(other_trigger, agent=OTHER_AGENT, at=T0 - 60), cp(own),
              cp(plain(second, agent=OTHER_AGENT, event=other_trigger, at=T0 + 90))], ["lift", "lift"]),
        case("retry_with_a_new_request", "check 4: a retry after a refusal needs a new request, and lifts",
             [req, acknowledged(step_up=stale), requested(other_request, at=T0 + 120),
              acknowledged(third, request=other_request, at=T0 + 180)],
             [trig, cp(req), cp(acknowledged(step_up=stale)), cp(requested(other_request, at=T0 + 120)),
              cp(acknowledged(third, request=other_request, at=T0 + 180))], ["step_up_stale", "lift"]),
    ]
    return cases


# --------------------------------------------------------------------------- the fuzz

FUZZ = {"seed": 2509, "runs": 500}


def fuzz_streams(rng: random.Random, run: int) -> tuple[list[dict], list[dict], bool]:
    """One random workspace: triggers and lifts the executor wrote, requests and acknowledgments with
    their copies at random places, and commands and answers spending assertions."""
    serial = iter(range(1, 10_000))

    def fresh(tag: str) -> str:
        return uid(tag, run * 100 + next(serial))

    assertions = [f"assert_f{n}" for n in range(4)]
    users = [REQUESTER, ACKNOWLEDGER, THIRD]
    agents = [AGENT, OTHER_AGENT]
    limits = [LIMIT, OTHER_LIMIT]
    control: list[dict] = []
    account: list[dict] = []
    pending: list[dict] = []
    trigger_ids: list[str] = []
    trigger_agent: dict[str, str] = {}
    request_ids: list[str] = []
    clock = T0

    def target() -> tuple[str, str]:
        if trigger_ids and rng.random() < 0.85:
            tid = rng.choice(trigger_ids)
            return (trigger_agent[tid] if rng.random() < 0.9 else rng.choice(agents)), tid
        return rng.choice(agents), fresh("TG")
    named_requests: list[str] = []
    opening = ["trigger"] * rng.randint(1, 2)
    for n in range(rng.randint(4, 14)):
        clock += rng.randint(1, 40)
        kind = opening[n] if n < len(opening) else rng.choice(("trigger", "trigger", "lift", "request", "request", "ack", "ack", "ack", "command", "copy", "copy", "copy"))
        assertion = rng.choice(assertions) if rng.random() < 0.3 else f"assert_r{run}_{clock}"
        offset = rng.choice((0, 10, 10, 10, 300, 301, -1))
        step_up = evidence(assertion, clock - offset, "password" if rng.random() < 0.1 else "cli_confirm")
        if kind == "trigger":
            tid = fresh("TG")
            trigger_ids.append(tid)
            trigger_agent[tid] = rng.choice(agents)
            account.append(triggered(tid, rng.choice(limits), trigger_agent[tid], at=clock))
        elif kind == "lift":
            account.append(lifted(fresh("XF"), rng.choice(limits), rng.choice(agents), at=clock))
        elif kind == "command":
            control.append(rng.choice((issued, answered))(fresh("CM"), step_up["assertion_id"], clock))
        elif kind == "request":
            agent, event = target()
            r = requested(fresh("RQ"), rng.choice(users), agent, event, clock, step_up)
            control.append(r)
            request_ids.append(r["event_id"])
            pending.append(r)
        elif kind == "ack":
            agent, event = target()
            by = rng.choice(users)
            evidence_ = None if rng.random() < 0.1 else step_up
            if rng.random() < 0.5 or not request_ids:
                a = acknowledged(fresh("AK"), by, by, None, False, agent, event, clock, evidence_)
            else:
                again = named_requests and rng.random() < 0.3
                named = rng.choice(named_requests) if again else rng.choice([*request_ids, fresh("RQ")])
                named_requests.append(named)
                source = next((c for c in control if c["event_id"] == named), None)
                requester = source["payload"]["requested_by"] if source and rng.random() < 0.8 else rng.choice(users)
                if source and rng.random() < 0.7:
                    agent, event = source["payload"]["agent"], source["payload"]["event"]
                if requester == by:
                    by = next(u for u in users if u != requester)
                a = acknowledged(fresh("AK"), by, requester, named, True, agent, event, clock, evidence_)
            control.append(a)
            pending.append(a)
        elif pending:
            original = pending.pop(rng.randrange(len(pending)))
            account.append(copied(original, fresh("CP")))
    for original in pending:
        if rng.random() < 0.9:
            at = len(account) if rng.random() < 0.75 else rng.randint(0, len(account))
            account.insert(at, copied(original, fresh("CP")))
    return control, account, rng.random() < 0.5


def run_fuzz(fuzz: dict, skip: frozenset[str]) -> tuple[list[str], dict[str, int]]:
    rng = random.Random(fuzz["seed"])
    problems: list[str] = []
    counts = dict.fromkeys(OUTCOMES, 0)
    for run in range(fuzz["runs"]):
        control, account, overlay = fuzz_streams(rng, run)
        got = judge(control, account, overlay, skip)
        want = oracle(control, account, overlay)
        if "fuzz.oracle_drops_last" in skip:
            want = want[:-1]
        if "fuzz.oracle_reversed" in skip:
            want = want[::-1]
        if got != want:
            problems.append(f"run {run}: the executor's fold {got} differs from the oracle's {want}")
        for o in want:
            counts[o["outcome"]] += 1
    return problems, counts


def fuzz_section() -> dict:
    problems, counts = run_fuzz(FUZZ, frozenset())
    assert not problems, problems[:3]
    return {**FUZZ, "outcomes": counts}


# --------------------------------------------------------------------------- independent checks

# Checks a vector cannot break on its own, since they compare two readings of the same records; each
# is shown to catch seeded bugs in the reading instead (`INVARIANT_MUTANTS`).
INVARIANT_CHECKS = ("fuzz.agree",)
INVARIANT_MUTANTS = (
    ("fuzz.agree", "executor.2.own_lift"),
    ("fuzz.agree", "executor.copies_reordered"),
    ("fuzz.agree", "executor.4.spent"),
    ("fuzz.agree", "executor.reuse"),
    ("fuzz.agree", "fuzz.oracle_drops_last"),
    ("fuzz.agree", "fuzz.oracle_reversed"),
)
ORACLE_CHECKS = (
    "drafts.valid",
    "drafts.copies",
    "invalid_drafts",
    "valid_drafts",
    "streams.rule",
    "streams.rules_only",
    "executor.records",
    "executor.outcomes",
    "executor.oracle",
    "executor.refusals",
    "fuzz.agree",
    "fuzz.counts",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def expected_failure(expect: dict) -> dict | None:
    return None if expect["outcome"] == "Valid" else {k: v for k, v in expect.items() if k != "outcome"}


def record_problems(case: dict, skip: frozenset[str]) -> list[str]:
    """Every record of an executor case passes `append`, except those the case lists, which rule 239
    refuses at the path listed; §9.27's records, not registered here, carry its tables' members."""
    problems = []
    listed = {(e["stream"], e["index"]): e for e in case["refused_at_append"]}
    for stream in ("control", "account"):
        for i, record in enumerate(case[stream]):
            members = {"RiskLimitTriggered": TRIGGERED_MEMBERS, "RiskLimitLifted": LIFTED_MEMBERS}.get(record["event_type"])
            if members is not None:
                if sorted(record["payload"]) != sorted(members) or record["stream_id"] != ACCOUNT:
                    problems.append(f"{case['name']} {stream} {i}: not §9.27's {record['event_type']}")
                continue
            got = violations(record, skip)
            want = listed.get((stream, i))
            if want is None and got:
                problems.append(f"{case['name']} {stream} {i}: {got}")
            if want is not None and not reported(got, {"reason": "schema", "path": want["path"]}):
                problems.append(f"{case['name']} {stream} {i}: expected rule 239 at {want['path']}, got {got}")
            if stream == "account" and record["causation_id"] not in {c["event_id"] for c in case["control"]}:
                problems.append(f"{case['name']} account {i}: a copy of no control-stream record")
    return problems


def check_section(section: dict, skip: frozenset[str] = frozenset()) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    drafts = section["drafts"]
    for name, draft in drafts.items():
        got = violations(draft, skip)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    for name, original in ORIGINALS.items():
        c, o = drafts[name], drafts[original]
        if c["payload"] != o["payload"] or c["causation_id"] != o["event_id"] or c["stream_id"] != ACCOUNT:
            problems.append(found("drafts.copies", f"{name} is not {original}'s payload, copied with it as its cause"))
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case), skip)
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        got = violations(draft_for(section, case), skip)
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
    for case in section["streams"]:
        got = stream_failure(case["records"], skip)
        if got != expected_failure(case["expect"]):
            problems.append(found("streams.rule", f"{case['name']}: expected {case['expect']}, got {got}"))
        got = stream_failure(case["records"], skip, rules_only=True)
        if got != expected_failure(case["stream_rules"]):
            problems.append(found("streams.rules_only", f"{case['name']}: expected {case['stream_rules']}, got {got}"))
    for case in section["executor"]:
        problems += [found("executor.records", p) for p in record_problems(case, skip)]
        got = judge_streams(case, skip)
        if got != case["expect"]:
            problems.append(found("executor.outcomes", f"{case['name']}: expected {case['expect']}, got {got}"))
        got = oracle(
            case["control"],
            case["account"],
            case["overlay_independent"],
            skip,
            case["environment"],
            case["founder_connection"],
        )
        if got != case["expect"]:
            problems.append(found("executor.oracle", f"{case['name']}: expected {case['expect']}, got {got}"))
        for n, item in enumerate(case["expect"]):
            if item["outcome"] == "lift":
                continue
            ack = case["account"][item["index"]]
            record = refused(uid("RF", 50 + n), item["outcome"], ack["causation_id"], seconds_of(ack["event_time"]))
            got = violations(record, skip)
            if got:
                problems.append(found("executor.refusals", f"{case['name']}: its {item['outcome']} refusal is refused: {got}"))
    fuzz = section["fuzz"]
    disagreements, counts = run_fuzz(fuzz, skip)
    problems += [found("fuzz.agree", p) for p in disagreements[:3]]
    if counts != fuzz["outcomes"] or not all(counts.values()):
        problems.append(found("fuzz.counts", f"outcomes {counts}, not {fuzz['outcomes']}, or one never occurs"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "record.extra",
    "record.missing",
    "types.risk_clock",
    "types.risk_clock_whole",
    "types.str_nonempty",
    "rule.83",
    "rule.28.acknowledgment_reasons",
    "rule.239.requested",
    "rule.239.step_up",
    "rule.239.request",
    "rule.239.requester",
    "rule.239.request.independent",
    "rule.239.request.plain",
    "rule.239.requester.independent",
    "rule.239.requester.plain",
    "order.rule_239_requester_first",
    *(f"stream.240{n}" for n in ("", ".exists", ".later", *(f".{m}" for m in MATCHED))),
    "stream.241",
    "stream.241.nulls",
    "executor.1",
    "executor.1.agent",
    "executor.1.type",
    "executor.1.before",
    "executor.2",
    "executor.2.lift",
    "executor.2.any_limit",
    "executor.2.own_lift",
    "executor.3",
    "executor.3.missing.acknowledgment",
    "executor.3.missing.request",
    "executor.3.future",
    "executor.3.window",
    "executor.3.method",
    "executor.3.at_submission",
    "executor.3.live",
    "executor.reuse",
    "executor.reuse.requests",
    "executor.reuse.commands",
    "executor.reuse.answers",
    "executor.reuse.acknowledgments",
    "executor.reuse.later",
    "executor.4",
    "executor.4.overlay",
    "executor.4.copy_flag",
    "executor.4.self",
    "executor.4.order",
    *(f"executor.4.match.{m}" for m in MATCHED),
    "executor.4.request_step_up",
    "executor.4.spent",
    "executor.copies_reordered",
    *ORACLE_MUTANTS,
)
# Each seeded bug in a rule or a check, with the one check it must be caught by.
CAUGHT_BY = {
    **{m: "invalid_drafts" for m in VALIDATOR_MUTANTS if m.startswith(("rule.", "order.", "types.", "record."))},
    **{m: "streams.rules_only" for m in VALIDATOR_MUTANTS if m.startswith("stream.")},
    **{m: "executor.outcomes" for m in VALIDATOR_MUTANTS if m.startswith("executor.")},
    **{m: "executor.oracle" for m in ORACLE_MUTANTS},
}


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, kind, name):
        return next(c for c in s[kind] if c["name"] == name)

    def outcome(s, name, n, value):
        case(s, "executor", name)["expect"][n]["outcome"] = value

    def refuse_as(s, value):
        for c in s["executor"]:
            for item in c["expect"]:
                if item["outcome"] == "request_invalid":
                    item["outcome"] = value

    return [
        ("a base request names no requester", "drafts.valid", mutated(lambda s: s["drafts"]["requested"]["payload"].update(requested_by=None))),
        ("a copy's payload differs from its original's", "drafts.copies",
         mutated(lambda s: s["drafts"]["acknowledged_copy"]["payload"].update(user=THIRD))),
        ("an invalid draft's path differs", "invalid_drafts",
         mutated(lambda s: case(s, "invalid_drafts", "acknowledged.independent_by_its_requester")["expect"].update(path="payload.user"))),
        ("a valid draft drops its request", "valid_drafts",
         mutated(lambda s: case(s, "valid_drafts", "acknowledged.no_step_up")["changes"].append(change("payload.request", None)))),
        ("a stream case expects the wrong record", "streams.rule", mutated(lambda s: case(s, "streams", "second_acknowledgment_by_a_third_user")["expect"].update(index=1))),
        ("a self-acknowledgment listed as breaking rule 240", "streams.rules_only",
         mutated(lambda s: case(s, "streams", "self_acknowledgment").update(stream_rules={"outcome": "Invalid", "index": 1, "rule": 240, "path": "payload.request"}))),
        ("an executor record is refused at append unlisted", "executor.records",
         mutated(lambda s: case(s, "executor", "plain_lifts")["control"][0]["payload"].update(request=REQUEST))),
        ("a superseded trigger is listed as lifted", "executor.outcomes", mutated(lambda s: outcome(s, "superseded_by_a_lift", 0, "lift"))),
        ("a refusal for a reason no check gives", "executor.oracle", mutated(lambda s: outcome(s, "request_never_copied", 0, "not_independent"))),
        ("a refusal no `OwnerCommandRefused` can carry", "executor.refusals", mutated(lambda s: refuse_as(s, "request_unknown"))),
        ("the fuzz's stated counts are wrong", "fuzz.counts", mutated(lambda s: s["fuzz"]["outcomes"].update(lift=s["fuzz"]["outcomes"]["lift"] + 1))),
    ]


def run_mutants(section: dict) -> list[str]:
    """Every seeded bug caught by the check it is registered against; a vector mutant only so."""
    escaped = []
    for mutant in VALIDATOR_MUTANTS:
        caught_by = {check_of(problem) for problem in check_section(section, frozenset([mutant]))}
        if CAUGHT_BY[mutant] not in caught_by:
            escaped.append(f"acknowledgments mutant {mutant} (not caught by {CAUGHT_BY[mutant]}; caught by {sorted(caught_by)})")
    for check, mutant in INVARIANT_MUTANTS:
        if check not in {check_of(problem) for problem in check_section(section, frozenset([mutant]))}:
            escaped.append(f"acknowledgments invariant {check} does not catch {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if check in INVARIANT_CHECKS:
            continue
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"acknowledgments check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"acknowledgments vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "stream_id": STREAM,
        "account_stream_id": ACCOUNT,
        "step_up_window_s": WINDOW,
        "methods": list(METHODS),
        "drafts": copy.deepcopy(DRAFTS),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
        "streams": stream_cases(),
        "executor": executor_cases(),
        "fuzz": fuzz_section(),
    }


__all__ = ["build_section", "check_section", "run_mutants", "vector_mutants"]
