"""Journal spec v0.62 §9.29's reference vectors for `StepUpConfirmed` (DEC-2104): the CLI's record
that the owner confirmed, on the host, one step-up challenge the workspace API issued for a paper
grant made in the web app, and from v0.82 for a paper OAuth connect (DEC-2125), in its own
`connection_drafts` list.

The schema and rule 242 live in `control.py`, beside §9.2's other control-stream records, so one
validator judges every closed schema. This module builds the `step_up_confirmations` section: a
base draft, an invalid draft for every member type and each clause of rule 242, valid drafts for
the members no rule reads, and control streams for rule 243. Its own oracle reads the drafts'
members directly, without the validator, and every seeded bug is shown caught.
"""

from __future__ import annotations

import copy

from common import change, delete, digest_strings
from control import (
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

SPEC = "docs/specs/journal.md v0.82 §9.29 (DEC-2104, DEC-2125)"
AT = "2026-10-18T22:30:05.000000000Z"
CONFIRMED_AT = 1792362605
EVENT = "01JAA2P4A000000000000000C1"
LATER = "01JAA2P4A000000000000000C2"
THIRD = "01JAA2P4A000000000000000C3"
CHALLENGE = "01JAA2P0A000000000000000H1"
OTHER_CHALLENGE = "01JAA2P0A000000000000000H2"
PRINCIPAL = "01JAA2N0A000000000000000P1"
OTHER_PRINCIPAL = "01JAA2N0A000000000000000P2"
CONTENT_HASH = "sha256:" + "4" * 64
# The digest of a connect's action object, which the CLI computes and stores (DEC-2125 item 3).
CONNECT_DIGEST = "sha256:" + "5" * 64
CODE = "7KXV6BGD"
BASE = "step_up_confirmed"


def confirmation(event_id: str = EVENT, challenge: str = CHALLENGE, principal: str = PRINCIPAL) -> dict:
    payload = {
        "challenge": challenge,
        "action_kind": "approve",
        "action_digest": CONTENT_HASH,
        "principal": principal,
        "code": CODE,
        "confirmed_at": CONFIRMED_AT,
    }
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": event_id,
        "stream_id": STREAM,
        "event_type": "StepUpConfirmed",
        "schema_version": 1,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": None,
        "correlation_id": None,
        "actor": {**USER, "id": principal},
        "config_refs": {},
        "payload": payload,
        "artifact_refs": sorted(digest_strings(payload)),
        "pii_refs": [],
    }


def base_drafts() -> dict[str, dict]:
    return {BASE: confirmation()}


def invalid(name, clause, changes, reason, path, also=()):
    return control_invalid(name, clause, BASE, changes, reason, path, also)


def valid(name, clause, changes):
    return control_valid(name, clause, BASE, changes)


# Each member, a value of the wrong JSON kind (`schema`) and a string of the wrong form
# (`non_canonical`). A lower-case code is the form the CLI normalizes away (DEC-2104 item 2).
MEMBER_CASES = (
    ("challenge", 7, "not-a-ulid"),
    ("action_digest", 7, "4" * 64),
    ("principal", 7, "user_owner_01"),
    ("code", 7, "7kxv6bgd"),
)
INT_MEMBERS = ("confirmed_at",)
RULE_242_MEMBERS = ("action_kind", "confirmed_at")
NULLED = ("challenge", "action_kind", "action_digest", "principal", "code", "confirmed_at")


# A changed `ref` member also leaves `artifact_refs` naming the digest it no longer holds.
REF_MEMBERS = ("action_digest",)


def also_for(member: str) -> tuple[tuple[str, str], ...]:
    return (("artifact_refs", "artifact_refs"),) if member in REF_MEMBERS else ()


def member_drafts() -> list[dict]:
    out = []
    for member, wrong_kind, wrong_form in MEMBER_CASES:
        path = f"payload.{member}"
        also = also_for(member)
        out.append(invalid(f"{member}.kind", "§9.29 types", [change(path, wrong_kind)], "schema", path, also))
        out.append(invalid(f"{member}.form", "§9.29 types", [change(path, wrong_form)], "non_canonical", path, also))
    for member in INT_MEMBERS:
        path = f"payload.{member}"
        out.append(invalid(f"{member}.kind", "§9.29 types", [change(path, "1792362605")], "schema", path))
    for member in NULLED:
        path = f"payload.{member}"
        out.append(invalid(f"{member}.null", "§9.29 types", [change(path, None)], "schema", path, also_for(member)))
    for code in ("7KXV6BG", "7KXV-6BGD", "7KXV6BGU"):
        out.append(invalid(f"code.shape.{code}", "§9.29 `confirm_code`", [change("payload.code", code)], "non_canonical", "payload.code"))
    out.append(invalid("missing", "§9.29 closed", [delete("payload.code")], "schema", "payload.code"))
    out.append(invalid("extra", "§9.29 closed", [change("payload.method", "cli_confirm")], "schema", "payload.method"))
    return out


def rule_drafts() -> list[dict]:
    return [
        invalid("another_kind", "rule 242: the kinds are `approve` and `connection`", [change("payload.action_kind", "confirm_version")], "schema", "payload.action_kind"),
        invalid("connect", "rule 242: `connection`, not the action's own word", [change("payload.action_kind", "connect")], "schema", "payload.action_kind"),
        invalid("connection_capitalized", "rule 242: the kind is lower case", [change("payload.action_kind", "Connection")], "schema", "payload.action_kind"),
        invalid("connection_padded", "rule 242: the kind exactly", [change("payload.action_kind", "connection ")], "schema", "payload.action_kind"),
        invalid("in_live", "rule 242: a `live` confirmation is never recorded", [change("environment", "live")], "schema", "environment"),
        invalid(
            "by_the_runtime",
            "rule 242: the actor is a user",
            [change("actor", {"kind": "system", "id": PRINCIPAL, "version": "1", "build": "sha256:" + "c" * 64})],
            "schema",
            "actor.kind",
        ),
        invalid("for_another_principal", "rule 242: the actor's id is the principal", [change("actor.id", OTHER_PRINCIPAL)], "schema", "actor.id"),
        invalid("backdated", "rule 242: `confirmed_at` is `event_time`'s second", [change("payload.confirmed_at", 0)], "schema", "payload.confirmed_at"),
        invalid(
            "a_second_early",
            "rule 242: not the second before",
            [change("payload.confirmed_at", CONFIRMED_AT - 1)],
            "schema",
            "payload.confirmed_at",
        ),
        invalid(
            "a_second_late",
            "rule 242: not the second after",
            [change("payload.confirmed_at", CONFIRMED_AT + 1)],
            "schema",
            "payload.confirmed_at",
        ),
    ]


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule; together they cover every member type and rule 242."""
    return [
        *member_drafts(),
        *rule_drafts(),
        invalid("at_version_2", "§9.29: closed at schema version 1 only", [change("schema_version", 2)], "unknown_schema", "payload"),
    ]


def valid_drafts() -> list[dict]:
    """The members no rule reads, and the cases a rule might be misread to refuse."""
    return [
        valid("another_challenge", "§9.29: any challenge the API issued", [change("payload.challenge", OTHER_CHALLENGE)]),
        valid(
            "another_principal_as_its_own_actor",
            "rule 242: a principal confirms for itself",
            [change("payload.principal", OTHER_PRINCIPAL), change("actor.id", OTHER_PRINCIPAL)],
        ),
        valid("caused", "§9.29: no rule reads the cause", [change("causation_id", OTHER_CHALLENGE)]),
        valid(
            "within_its_second",
            "rule 242: `confirmed_at` is `event_time`'s whole second, its fraction dropped",
            [change("event_time", "2026-10-18T22:30:05.999999999Z")],
        ),
        valid("all_digits", "§9.29 `confirm_code`: digits are Crockford characters", [change("payload.code", "01234567")]),
    ]


def connect(name, clause, changes=()):
    """A valid draft confirming a connect: kind `connection`, its digest the connect's action object,
    which `artifact_refs` names as it names every `ref`."""
    return control_valid(
        name,
        clause,
        BASE,
        [
            change("payload.action_kind", "connection"),
            change("payload.action_digest", CONNECT_DIGEST),
            change("artifact_refs", [CONNECT_DIGEST]),
            *changes,
        ],
    )


def connection_drafts() -> list[dict]:
    """v0.82 (DEC-2125): a confirmation of a paper OAuth connect, and the members no rule reads."""
    return [
        connect("connection", "rule 242: `connection` is the second kind"),
        connect("connection_by_another_principal", "rule 242: a principal confirms its own connect", [change("payload.principal", OTHER_PRINCIPAL), change("actor.id", OTHER_PRINCIPAL)]),
        connect("connection_within_its_second", "rule 242: `confirmed_at` is `event_time`'s whole second", [change("event_time", "2026-10-18T22:30:05.500000000Z")]),
    ]


# --------------------------------------------------------------------------- rule 243

def stream_failure(records: list[dict], skip: frozenset[str] = frozenset()) -> int | None:
    """The index of the first record that breaks rule 243, or None."""
    seen: set[str] = set()
    for i, record in enumerate(records):
        if record["event_type"] != "StepUpConfirmed":
            continue
        challenge = record["payload"]["challenge"]
        if challenge in seen and "stream.243" not in skip:
            return i
        seen.add(challenge)
    return None


def stream_cases() -> list[dict]:
    def case(name, clause, records, index):
        expect = {"outcome": "Valid"} if index is None else {"outcome": "Invalid", "rule": "243", "index": index}
        return {"name": name, "clause": clause, "records": records, "expect": expect}

    return [
        case(
            "two_challenges",
            "rule 243: one confirmation per challenge",
            [confirmation(), confirmation(LATER, OTHER_CHALLENGE)],
            None,
        ),
        case(
            "one_challenge_twice",
            "rule 243: a challenge is confirmed at most once",
            [confirmation(), confirmation(LATER)],
            1,
        ),
        case(
            "one_challenge_by_two_principals",
            "rule 243: whoever confirms it second",
            [confirmation(), confirmation(LATER, CHALLENGE, OTHER_PRINCIPAL)],
            1,
        ),
        case(
            "the_third_repeats_the_first",
            "rule 243: not only adjacent records",
            [confirmation(), confirmation(LATER, OTHER_CHALLENGE), confirmation(THIRD)],
            2,
        ),
    ]


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = (
    "drafts.valid",
    "drafts.confirmation",
    "invalid_drafts",
    "valid_drafts",
    "connection_drafts",
    "streams.records",
    "streams.rule",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def oracle_index(records: list[dict]) -> int | None:
    """Rule 243 counted apart from `stream_failure`: each challenge's count, at each record."""
    counts: dict[str, int] = {}
    for i, record in enumerate(records):
        key = record["payload"]["challenge"]
        counts[key] = counts.get(key, 0) + 1
        if counts[key] == 2:
            return i
    return None


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, draft in section["drafts"].items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    base = section["drafts"][BASE]
    own_second = base["event_time"] == AT and base["payload"]["confirmed_at"] == CONFIRMED_AT
    if base["actor"]["id"] != base["payload"]["principal"] or base["payload"]["action_kind"] != "approve" or not own_second:
        problems.append(found("drafts.confirmation", "the base StepUpConfirmed is not the principal's own approve"))
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case))
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        got = violations(draft_for(section, case))
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
    for case in section["connection_drafts"]:
        draft = draft_for(section, case)
        got = violations(draft)
        p = draft["payload"]
        own = p["action_kind"] == "connection" and draft["artifact_refs"] == [p["action_digest"]]
        if got or not own:
            problems.append(found("connection_drafts", f"{case['name']}: expected a valid connect's confirmation, got {got}"))
    for case in section["streams"]:
        for i, record in enumerate(case["records"]):
            got = violations(record)
            if got:
                problems.append(found("streams.records", f"{case['name']} record {i}: {got}"))
        want = case["expect"].get("index")
        if stream_failure(case["records"]) != want or oracle_index(case["records"]) != want:
            problems.append(found("streams.rule", f"{case['name']}: expected {case['expect']}"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "record.extra",
    "record.missing",
    *(f"loose.payload.{member}" for member, _, _ in MEMBER_CASES),
    # A null `action_kind` or `confirmed_at` is refused by rule 242 with the same reason and path
    # as by its type, so loosening its type alone changes no report: those mutants are equivalent
    # and are not seeded.
    *(f"nullable.payload.{member}" for member in NULLED if member not in RULE_242_MEMBERS),
    "types.confirm_code_case",
    "rule.242.kind",
    "rule.242.approve_only",
    "rule.242.paper",
    "rule.242.user",
    "rule.242.principal",
    "rule.242.time",
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
        ("the base draft is live", "drafts.valid", mutated(lambda s: s["drafts"][BASE].update(environment="live"))),
        (
            "the base draft confirms for another principal and is written by it",
            "drafts.confirmation",
            mutated(lambda s: s["drafts"][BASE]["actor"].update(id=OTHER_PRINCIPAL)),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "code.form")["expect"].update(path="payload.challenge")),
        ),
        (
            "a valid draft confirms another kind",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "caused")["changes"].append(change("payload.action_kind", "stop"))),
        ),
        (
            "a connect's confirmation names another kind",
            "connection_drafts",
            mutated(lambda s: case(s, "connection_drafts", "connection")["changes"].append(change("payload.action_kind", "approve"))),
        ),
        (
            "a stream record is live",
            "streams.records",
            mutated(lambda s: case(s, "streams", "two_challenges")["records"][1].update(environment="live")),
        ),
        (
            "a stream case expects no refusal of a repeated challenge",
            "streams.rule",
            mutated(lambda s: case(s, "streams", "one_challenge_twice").update(expect={"outcome": "Valid"})),
        ),
    ]


def run_mutants(section: dict) -> list[str]:
    """Every seeded bug caught; a vector mutant only by the check it is registered against."""
    escaped = []
    cases = [(copy.deepcopy(d), None) for d in section["drafts"].values()]
    cases += [(draft_for(section, c), None) for c in section["valid_drafts"]]
    cases += [(draft_for(section, c), None) for c in section["connection_drafts"]]
    cases += [(draft_for(section, c), c["expect"]) for c in section["invalid_drafts"]]
    for mutant in VALIDATOR_MUTANTS:
        skip = frozenset([mutant])
        caught = False
        for draft, want in cases:
            got = violations(draft, skip)
            caught |= bool(got) if want is None else not reported(got, want)
        if not caught:
            escaped.append(f"confirmation validator mutant {mutant}")
    stream_caught = any(
        stream_failure(c["records"], frozenset(["stream.243"])) != c["expect"].get("index") for c in section["streams"]
    )
    if not stream_caught:
        escaped.append("confirmation stream mutant stream.243")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"confirmation check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"confirmation vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
        "connection_drafts": connection_drafts(),
        "streams": stream_cases(),
    }
