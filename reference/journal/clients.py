"""Journal spec v0.20's reference vectors (DEC-671): the `client` actor of §3 (rules 66 to 68), and
§9.9's `ConnectionRevoked` version 2 and client records (rules 69 to 74).

The rules live in `control.py`, beside §9.2's to §9.8's, so one validator judges every closed
schema. This module builds the `client_actor` section: a base draft of each record, an invalid draft
for every member type and rule, and valid drafts for the cases a rule might be misread to refuse,
including the client branches of §9.8's rules 55 and 64, which v0.19 could only refuse. It checks
the section with two oracles of its own: the human behind every accepted record, computed from
identity spec ID-6 and ID-11 rather than from the rules, and the compromised revocation's link to
its kill switch. Every seeded bug is shown caught.
"""

from __future__ import annotations

import copy

from common import change, delete, digest_strings
from control import (
    AGENT,
    AGENT_STREAM,
    OWNER,
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

SPEC = "docs/specs/journal.md v0.20 §3, §9.9 (DEC-671)"
AT = "2026-10-08T15:00:00.000000000Z"
IDS = {
    "draft": "01J8Z5C0A000000000000000C1",
    "request": "01J8Z5C1A000000000000000C2",
    "revoked": "01J8Z5C2A000000000000000C3",
    "connected": "01J8Z5C3A000000000000000C4",
    "client_revoked": "01J8Z5C4A000000000000000C5",
}
KILL_SWITCH = "01J8Z5C2B000000000000000C6"
CLIENT_ID = "client_01"
CLIENT = {"kind": "client", "id": CLIENT_ID, "version": "1", "build": None, "on_behalf_of": OWNER}
BASE_VERSION = "sha256:" + "1" * 64
DRAFT = "sha256:" + "4" * 64
ASSET = "0f9e7d3c-5b1a-4c2e-8f6d-9a8b7c6d5e4f"
STEP_UP = {"assertion_id": "assert_owner_07", "authenticated_at": "2026-10-08T14:59:50.000000000Z", "method": "cli_confirm"}
# Identity spec ID-11: what a client never does, by the record that would show it.
NEVER_A_CLIENTS = (
    "MandateConfirmed",
    "ApprovalResponseSubmitted",
    "OwnerAcknowledged",
    "OwnerCommandIssued",
    "ConnectionEstablished",
    "ConnectionRevoked",
    "ClientConnected",
    "ClientRevoked",
    "DisclosureAccepted",
    "AgentDeployed",
)


def envelope(name, event_type, actor, payload, causation=None, version=1, stream=STREAM) -> dict:
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": IDS[name],
        "stream_id": stream,
        "event_type": event_type,
        "schema_version": version,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": causation,
        "correlation_id": None,
        "actor": dict(actor),
        "config_refs": {},
        "payload": payload,
        "artifact_refs": sorted(digest_strings(payload)),
        "pii_refs": [],
    }


def base_drafts() -> dict[str, dict]:
    draft = {"draft_id": "draft_02", "draft": DRAFT, "origin": "version", "base_draft": None, "base_version": BASE_VERSION}
    request = {
        "agent_id": AGENT,
        "instrument_id": ASSET,
        "side": "buy",
        "quantity": None,
        "requested_by": "client",
        "client_id": CLIENT_ID,
    }
    revoked = {"connection_id": "conn_01", "reason": "compromised", "step_up": dict(STEP_UP)}
    connected = {
        "client_id": CLIENT_ID,
        "user": OWNER,
        "scopes": ["read", "request"],
        "agents": [AGENT, "agent_b"],
        "step_up": dict(STEP_UP),
    }
    return {
        "client_draft": envelope("draft", "MandateDraftSaved", CLIENT, draft),
        "client_request": envelope("request", "OwnerRequestSubmitted", CLIENT, request),
        "revoked_compromised": envelope("revoked", "ConnectionRevoked", USER, revoked, KILL_SWITCH, version=2),
        "client_connected": envelope("connected", "ClientConnected", USER, connected),
        "client_revoked": envelope("client_revoked", "ClientRevoked", USER, {"client_id": CLIENT_ID, "user": OWNER}),
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
    """`changes`, then `artifact_refs` set to the changed payload's digests (§3)."""
    draft = copy.deepcopy(BASES[base])
    for item in changes:
        draft_change(draft, item)
    return [*changes, change("artifact_refs", sorted(digest_strings(draft["payload"])))]


def invalid(name, clause, base, changes, reason, path, also=()):
    return control_invalid(name, clause, base, refs_kept(base, changes), reason, path, also)


def valid(name, clause, base, changes):
    return control_valid(name, clause, base, refs_kept(base, changes))


def as_client(event_type: str, version: int = 1) -> list[dict]:
    """The base request turned into another record a client might try to write; rule 68 refuses it
    before its payload is read."""
    return [change("event_type", event_type), change("schema_version", version)]


# Each member, a value of the wrong JSON kind (`schema`) and, where its type constrains a string, a
# string of the wrong form (`non_canonical`).
MEMBER_CASES = {
    "revoked_compromised": (
        ("connection_id", 7, "conn 01"),
        ("reason", 7, "leaked"),
        ("step_up", "cli_confirm", None),
    ),
    "client_connected": (
        ("client_id", 7, "client 01"),
        ("user", 7, ""),
        ("scopes", "read", None),
        ("agents", AGENT, None),
        ("step_up", [], None),
    ),
    "client_revoked": (
        ("client_id", 7, "client/01"),
        ("user", 7, ""),
    ),
}
# A `null` `scopes` or `agents` is refused by rule 71, at the member's own path with the same reason,
# so it is refused identically whether or not its type is checked first.
RULE_TYPED = ("scopes", "agents")
NULLED = {
    "revoked_compromised": ("connection_id", "reason", "step_up"),
    "client_connected": ("client_id", "user", "scopes", "agents", "step_up"),
    "client_revoked": ("client_id", "user"),
}


def member_drafts() -> list[dict]:
    out = []
    for base, cases in MEMBER_CASES.items():
        for member, wrong_kind, wrong_form in cases:
            path = f"payload.{member}"
            out.append(invalid(f"{base}.{member}.kind", "§9.9 types", base, [change(path, wrong_kind)], "schema", path))
            if wrong_form is not None:
                out.append(
                    invalid(f"{base}.{member}.form", "§9.9 types", base, [change(path, wrong_form)], "non_canonical", path)
                )
        for member in NULLED[base]:
            path = f"payload.{member}"
            out.append(invalid(f"{base}.{member}.null", "§9.9 types", base, [change(path, None)], "schema", path))
        some = MEMBER_CASES[base][0][0]
        out.append(invalid(f"{base}.missing", "§9.9 closed", base, [delete(f"payload.{some}")], "schema", f"payload.{some}"))
        out.append(invalid(f"{base}.extra", "§9.9 closed", base, [change("payload.note", "x")], "schema", "payload.note"))
    return out


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule, or the rules its `also` lists."""
    req = "client_request"
    rev = "revoked_compromised"
    con = "client_connected"
    return [
        *member_drafts(),
        invalid(
            "client_scope_unknown",
            "§9.9: the scopes are workspace API §3.8's closed list",
            con,
            [change("payload.scopes", ["read", "trade"])],
            "non_canonical",
            "payload.scopes[1]",
        ),
        invalid(
            "client_agent_not_an_id",
            "§9.9 types",
            con,
            [change("payload.agents", [AGENT, "agent b"])],
            "non_canonical",
            "payload.agents[1]",
        ),
        invalid(
            "revoked_v1_with_a_reason",
            "§9.9: version 1 stays closed",
            rev,
            [change("schema_version", 1)],
            "schema",
            "payload.reason",
            also=[("schema", "payload.step_up")],
        ),
        invalid(
            "revoked_unknown_version",
            "§9.9: versions 1 and 2 are registered",
            rev,
            [change("schema_version", 3)],
            "unknown_schema",
            "payload",
        ),
        invalid(
            "user_acting_for_someone",
            "rule 66: only a client acts on behalf of a user",
            "client_revoked",
            [change("actor.on_behalf_of", OWNER)],
            "schema",
            "actor.on_behalf_of",
        ),
        invalid(
            "client_for_no_one",
            "rule 66: a client names its user",
            req,
            [delete("actor.on_behalf_of")],
            "schema",
            "actor.on_behalf_of",
        ),
        invalid(
            "client_for_a_non_id",
            "rule 66: the user is an opaque id",
            req,
            [change("actor.on_behalf_of", "user 01")],
            "non_canonical",
            "actor.on_behalf_of",
        ),
        invalid(
            "client_for_a_number",
            "rule 66",
            req,
            [change("actor.on_behalf_of", 7)],
            "schema",
            "actor.on_behalf_of",
        ),
        invalid(
            "client_with_a_build",
            "rule 67: a client is external, so it has no build digest",
            req,
            [change("actor.build", "sha256:" + "d" * 64)],
            "schema",
            "actor.build",
        ),
        invalid(
            "client_confirms",
            "rule 68: a client never confirms a version (identity ID-11)",
            req,
            as_client("MandateConfirmed", 2),
            "schema",
            "actor.kind",
        ),
        invalid(
            "client_answers_an_approval",
            "rule 68: refused at append; mandate spec §6.4 check 3 refuses it again",
            req,
            as_client("ApprovalResponseSubmitted"),
            "schema",
            "actor.kind",
        ),
        invalid(
            "client_revokes_a_connection",
            "rule 68",
            req,
            as_client("ConnectionRevoked", 2),
            "schema",
            "actor.kind",
        ),
        invalid(
            "client_connects_a_client",
            "rule 68",
            con,
            [change("actor", dict(CLIENT))],
            "schema",
            "actor.kind",
        ),
        invalid(
            "client_on_an_agent_stream",
            "rule 68: a client writes only to the control stream",
            req,
            [*as_client("ApprovalResponded"), change("stream_id", AGENT_STREAM)],
            "schema",
            "actor.kind",
        ),
        invalid(
            "client_starts_a_draft_from_a_description",
            "rule 55: a client's draft starts from a base version",
            "client_draft",
            [change("payload.origin", "description"), change("payload.base_version", None)],
            "schema",
            "actor.kind",
        ),
        invalid(
            "client_edits_a_draft",
            "rule 55",
            "client_draft",
            [change("payload.origin", "edit"), change("payload.base_draft", DRAFT), change("payload.base_version", None)],
            "schema",
            "actor.kind",
        ),
        invalid(
            "client_request_claims_the_owner",
            "rule 64: `requested_by` follows the actor",
            req,
            [change("payload.requested_by", "owner"), change("payload.client_id", None)],
            "schema",
            "payload.requested_by",
        ),
        invalid(
            "client_request_names_another_client",
            "rule 64: `client_id` is the actor's",
            req,
            [change("payload.client_id", "client_02")],
            "schema",
            "payload.client_id",
        ),
        invalid(
            "client_request_names_no_client",
            "rule 64",
            req,
            [change("payload.client_id", None)],
            "schema",
            "payload.client_id",
        ),
        invalid(
            "compromised_without_its_kill_switch",
            "rule 69: the revocation follows its kill switch (workspace API §5.6)",
            rev,
            [change("causation_id", None)],
            "schema",
            "causation_id",
        ),
        invalid(
            "ordinary_revoke_with_a_cause",
            "rule 69",
            rev,
            [change("payload.reason", "owner")],
            "schema",
            "causation_id",
        ),
        invalid(
            "revoked_by_the_system",
            "rule 70: the owner revokes",
            rev,
            [change("actor", dict(SERVICES))],
            "schema",
            "actor.kind",
        ),
        invalid(
            "client_with_no_scope",
            "rule 71",
            con,
            [change("payload.scopes", [])],
            "schema",
            "payload.scopes",
        ),
        invalid(
            "client_with_no_agent",
            "rule 71",
            con,
            [change("payload.agents", [])],
            "schema",
            "payload.agents",
        ),
        invalid(
            "client_scopes_unsorted",
            "rule 72",
            con,
            [change("payload.scopes", ["request", "read"])],
            "non_canonical",
            "payload.scopes",
        ),
        invalid(
            "client_agents_repeated",
            "rule 72",
            con,
            [change("payload.agents", [AGENT, AGENT])],
            "non_canonical",
            "payload.agents",
        ),
        invalid(
            "client_connected_by_the_system",
            "rule 73: a user connects a client",
            con,
            [change("actor", dict(SERVICES))],
            "schema",
            "actor.kind",
        ),
        invalid(
            "client_connected_for_another_user",
            "rule 73: a user connects their own client",
            con,
            [change("payload.user", "user_owner_02")],
            "schema",
            "payload.user",
        ),
        invalid(
            "client_revoked_by_an_operator",
            "rule 74",
            "client_revoked",
            [change("actor", {"kind": "platform_operator", "id": "op_01", "version": "1", "build": None})],
            "schema",
            "actor.kind",
        ),
    ]


def valid_drafts() -> list[dict]:
    """Cases a rule might be misread to refuse; each must be accepted."""
    rev = "revoked_compromised"
    return [
        valid(
            "client_request_sized_sell",
            "rule 64: a client's request, any side and size",
            "client_request",
            [change("payload.side", "sell"), change("payload.quantity", "3")],
        ),
        valid(
            "revoked_by_the_owner",
            "rule 69: an ordinary revoke has no cause",
            rev,
            [change("payload.reason", "owner"), change("causation_id", None)],
        ),
        valid(
            "revoked_v1_unchanged",
            "§9.9: version 1 stays registered",
            rev,
            [change("schema_version", 1), delete("payload.reason"), delete("payload.step_up"), change("causation_id", None)],
        ),
        valid(
            "client_one_scope_one_agent",
            "rules 71 and 72",
            "client_connected",
            [change("payload.scopes", ["hold"]), change("payload.agents", [AGENT])],
        ),
        valid(
            "client_revoked_on_deprovisioning",
            "rule 74: the system revokes a deprovisioned user's clients (identity §11.1)",
            "client_revoked",
            [change("actor", dict(SERVICES))],
        ),
        valid(
            "client_revoked_by_an_admin",
            "rule 74: an admin may revoke another member's client",
            "client_revoked",
            [change("actor.id", "user_admin_01")],
        ),
    ]


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = ("drafts.valid", "drafts.human", "drafts.kill_switch", "invalid_drafts", "valid_drafts")


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def human_problems(name: str, draft: dict) -> list[str]:
    """Identity spec ID-6 and ID-11 on an accepted record: a client is never the human and never
    does what ID-11 lists, and a request or connection names the human the actor stands for."""
    actor = draft["actor"]
    is_client = actor["kind"] == "client"
    person = actor.get("on_behalf_of") if is_client else actor["id"]
    p = draft["payload"]
    out = []
    if is_client and draft["event_type"] in NEVER_A_CLIENTS:
        out.append(f"{name}: a client wrote {draft['event_type']}")
    if not is_client and "on_behalf_of" in actor:
        out.append(f"{name}: a {actor['kind']} acting for someone")
    if draft["event_type"] == "OwnerRequestSubmitted" and is_client:
        if p["requested_by"] != "client" or p["client_id"] != actor["id"] or person != OWNER:
            out.append(f"{name}: a client's request not attributed to it and its user")
    if draft["event_type"] == "ClientConnected" and p["user"] != person:
        out.append(f"{name}: a client connected for someone other than the user who connected it")
    return [found("drafts.human", problem) for problem in out]


def check_section(section: dict) -> list[str]:
    problems = []
    drafts = section["drafts"]
    accepted = []
    for name, draft in drafts.items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
        accepted.append((name, draft))
    revoked = drafts["revoked_compromised"]
    if revoked["payload"]["reason"] != "compromised" or revoked["causation_id"] != KILL_SWITCH:
        problems.append(found("drafts.kill_switch", "the compromised revocation does not follow its kill switch"))
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
        problems += human_problems(name, draft)
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.55",
    "rule.64.requested_by",
    "rule.64.client_id",
    "rule.66.extra",
    "rule.66.missing",
    "loose.actor.on_behalf_of",
    "rule.67",
    "rule.68",
    "rule.69",
    "rule.70",
    "rule.71",
    "rule.72.scopes",
    "rule.72.agents",
    "rule.73.actor",
    "rule.73.user",
    "rule.74",
    "record.extra",
    "record.missing",
    *sorted({f"loose.payload.{member}" for cases in MEMBER_CASES.values() for member, _, _ in cases}),
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
            mutated(lambda s: s["drafts"]["revoked_compromised"].update(causation_id=None)),
        ),
        (
            "the compromised revocation is written as an ordinary one",
            "drafts.kill_switch",
            mutated(
                lambda s: s["drafts"]["revoked_compromised"]["payload"].update(reason="owner")
                or s["drafts"]["revoked_compromised"].update(causation_id=None)
            ),
        ),
        (
            "a valid client request is attributed to another user while rule 64 is read loosely",
            "drafts.human",
            mutated(
                lambda s: case(s, "valid_drafts", "client_request_sized_sell")["changes"].append(
                    change("actor.on_behalf_of", "user_owner_02")
                )
            ),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "client_with_a_build")["expect"].update(path="actor.kind")),
        ),
        (
            "a valid draft breaks a rule",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "revoked_by_the_owner")["changes"].append(change("causation_id", KILL_SWITCH))),
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
            escaped.append(f"client validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"client check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"client vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
